use crate::builtin::{builtin_kind, is_builtin_allowed_in_pipeline, run_builtin};
use crate::diagnostics::print_error;
use crate::redirection::{
    apply_redirection_in_shell, flush_standard_streams, handle_redirection_or_exit,
    restore_redirection,
};
use crate::types::{Command, CommandFlow, CommandStatus, Pipeline, ShellResult, ShellState};
use nix::sys::wait::{WaitStatus, waitpid};
use nix::unistd::{ForkResult, dup2_stdin, dup2_stdout, execvp, fork, pipe};
use std::ffi::CString;
use std::process;

// ===== 入口层 =====

pub fn run_command(command: &Command, state: &mut ShellState) -> ShellResult<CommandFlow> {
    let print_lifecycle = command.program != "clear";
    // 实验要求每条命令打印 starting/ending。`clear` 是交互式清屏命令，
    // 如果清屏后立刻打印 ending，用户体验会比较奇怪，因此单独跳过。
    if print_lifecycle {
        println!("{} starting...", &command.program);
    }

    let flow = if builtin_kind(command).is_some() {
        // 内置命令在 shell 进程中执行，需要使用专门的重定向恢复逻辑。
        match run_builtin_with_redirection(command, state) {
            Ok(flow) => flow,
            Err(err) => {
                print_error(format!("{}: {}", command.program, err));
                CommandFlow::Continue(CommandStatus::failure())
            }
        }
    } else {
        // 非内置命令在子进程中处理重定向，避免修改 shell 进程自身的 fd。
        let status = match run_external(command) {
            Ok(status) => status,
            Err(err) => {
                print_error(format!("{}: {}", command.program, err));
                CommandStatus::failure()
            }
        };
        CommandFlow::Continue(status)
    };

    if print_lifecycle {
        println!("{} ending.", &command.program);
    }

    Ok(flow)
}

pub fn run_pipeline(pipeline: &Pipeline, state: &mut ShellState) -> ShellResult<CommandStatus> {
    println!("pipeline starting...");

    // 当前仅允许纯输出型内置命令进入管道。
    // 会修改 shell 自身状态的 builtin，例如 cd/export/unset，放进子进程后
    // 只会影响子进程，不能影响父进程中的 shell 状态，因此暂时禁止。
    for command in &pipeline.commands {
        if let Some(kind) = builtin_kind(command) {
            if !is_builtin_allowed_in_pipeline(kind) {
                print_error(format!(
                    "pipeline: built-in command is not supported in pipelines: {}",
                    command.program
                ));
                return Ok(CommandStatus::failure());
            }
        }
    }

    // 管道中的重定向仅允许出现在边界命令上。
    if let Err(err) = validate_pipeline_redirection(pipeline) {
        print_error(format!("pipeline: {}", err));
        println!("pipeline ending.");
        return Ok(CommandStatus::failure());
    }

    // 空指令，直接返回成功，主循环继续执行
    let n = pipeline.commands.len();
    if n == 0 {
        return Ok(CommandStatus::success());
    }

    // n 个命令只需要 n - 1 个匿名管道。
    let mut pipes = Vec::new();
    for _ in 0..n - 1 {
        let (read_fd, write_fd) = pipe()?;
        pipes.push((read_fd, write_fd));
    }

    let mut children_pids = Vec::new();

    // pipeline 中的所有命令都要先 fork 出来。父进程负责记录 pid 并等待；
    // 子进程负责绑定自己的 stdin/stdout，然后执行 builtin 或 exec 外部程序。
    for (i, command) in pipeline.commands.iter().enumerate() {
        match unsafe { fork()? } {
            ForkResult::Parent { child } => {
                children_pids.push(child);
            }
            ForkResult::Child => {
                // 对子进程中的指令绑定管道的fd
                // 非首条命令的 stdin 来自前一个管道的读端。
                if i != 0 {
                    if let Err(err) = dup2_stdin(&pipes[i - 1].0) {
                        print_error(format!("pipeline: dup2 stdin failed: {}", err));
                        process::exit(127);
                    }
                }

                // 非末条命令的 stdout 指向当前管道的写端。
                if i != n - 1 {
                    if let Err(err) = dup2_stdout(&pipes[i].1) {
                        print_error(format!("pipeline: dup2 stdout failed: {}", err));
                        process::exit(127);
                    }
                }

                // 管道 fd 绑定完成后，再处理命令自身的边界重定向。
                handle_redirection_or_exit(command);

                // 关闭子进程继承到的原始 pipe fd，避免它们被 execvp 后的新程序继续持有。
                // 这里直接 drop Vec<OwnedFd>，让 Rust 按所有权自动 close fd。
                // 不要先用 raw fd 手动 close 再让 OwnedFd 析构，否则会触发重复关闭。
                drop(pipes);

                // 允许出现在管道中的内置命令在子进程中直接执行并退出。
                if let Some(kind) = builtin_kind(command) {
                    if is_builtin_allowed_in_pipeline(kind) {
                        let flow = run_builtin(command, state)
                            .expect("pipeline built-in should have a builtin result");
                        match flow {
                            CommandFlow::Continue(status) | CommandFlow::Exit(status) => {
                                process::exit(status.code)
                            }
                        }
                    }

                    print_error(format!(
                        "pipeline: built-in command is not supported: {}",
                        command.program
                    ));
                    process::exit(127);
                }

                // 非内置命令通过 execvp 替换子进程镜像。
                exec_external_or_exit(command);
            }
        }
    }

    // 父进程在等待前释放自己持有的 pipe fd，避免管道另一端迟迟收不到 EOF。
    // 例如 `producer | consumer` 中，如果父进程还持有写端，consumer 可能一直等不到 EOF。
    drop(pipes);

    let mut last_status = CommandStatus::success();
    for child in children_pids {
        last_status = wait_status_to_command_status(waitpid(child, None)?);
    }
    println!("pipeline ending.");

    Ok(last_status)
}

// ===== 执行层 =====

fn run_builtin_with_redirection(
    command: &Command,
    state: &mut ShellState,
) -> ShellResult<CommandFlow> {
    let saved = apply_redirection_in_shell(command)?;
    // 这里已经确认是内置命令；若返回 None，说明调用路径存在内部错误。
    let result = run_builtin(command, state).expect("builtin command should have a builtin result");

    // builtin 在当前 shell 进程中执行，恢复 fd 前先刷新缓冲区。
    flush_standard_streams()?;
    restore_redirection(saved)?;

    Ok(result)
}

fn run_external(command: &Command) -> ShellResult<CommandStatus> {
    // fork 之后同一段代码会在父子两个进程中继续执行，ForkResult 用来区分当前分支。
    match unsafe { fork()? } {
        ForkResult::Parent { child } => {
            // 父进程不能 exec；它要保留 shell 主循环，并等待子进程结束。
            let wait_status = waitpid(child, None)?;
            Ok(wait_status_to_command_status(wait_status))
        }

        ForkResult::Child => {
            // 外部命令在子进程中处理重定向，不会影响 shell 进程自身。
            handle_redirection_or_exit(command);
            // execvp 成功后不会返回；失败时在子进程中直接退出。
            exec_external_or_exit(command);
        }
    }
}

// 构造 c_argv 并调用 execvp。如果 execvp 成功，当前进程不会返回。
// 返回类型是 !，专门表达“子进程到这里不会返回”
fn exec_external_or_exit(command: &Command) -> ! {
    let c_argv = match build_c_argv(command) {
        Ok(c_argv) => c_argv,
        Err(err) => {
            print_error(format!("{}: {}", command.program, err));
            process::exit(127);
        }
    };

    match execvp(&c_argv[0], &c_argv) {
        Ok(_) => unreachable!("execvp should not return on success"),
        Err(err) => {
            print_error(format!("{}: execvp failed: {}", command.program, err));
            process::exit(127);
        }
    }
}

// ===== 资源层 =====

// pipeline 暂时只允许首条命令重定向输入、末条命令重定向输出。
// 中间命令若包含重定向，直接作为解析后的执行错误处理。
fn validate_pipeline_redirection(pipeline: &Pipeline) -> Result<(), String> {
    let n = pipeline.commands.len();

    for (i, command) in pipeline.commands.iter().enumerate() {
        if command.redirection.stdin.is_some() && i != 0 {
            return Err(format!(
                "{}: stdin redirection is only supported on the first pipeline command",
                command
            ));
        }

        if command.redirection.stdout.is_some() && i != n - 1 {
            return Err(format!(
                "{}: stdout redirection is only supported on the last pipeline command",
                command
            ));
        }
    }
    Ok(())
}

fn wait_status_to_command_status(status: WaitStatus) -> CommandStatus {
    match status {
        // 子进程正常调用 exit(code) 或 main 返回时，状态码就是 code。
        WaitStatus::Exited(_, code) => CommandStatus::new(code),
        // 程序不是正常退出，而是被信号终止。shell 通常把这种情况编码成 128 + signal_number。
        WaitStatus::Signaled(_, signal, _) => CommandStatus::new(128 + signal as i32),
        // TODO 其他情况统一当错误处理，目前还没有 job control。
        _ => CommandStatus::failure(),
    }
}

fn build_c_argv(command: &Command) -> ShellResult<Vec<CString>> {
    let mut argv = Vec::new();
    // Unix 约定 argv[0] 是程序名本身，因此需要把 program 放回参数数组开头。
    argv.push(command.program.clone());
    argv.extend(command.args.clone());

    // execvp 需要 C 字符串。CString::new 会拒绝内部包含 NUL 字节的字符串，
    // 因为 C API 会把 NUL 视为字符串结束。
    argv.iter()
        .map(|arg| CString::new(arg.as_str()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| err.into())
}
