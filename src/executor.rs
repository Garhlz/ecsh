use crate::builtin::{BuiltinResult, builtin_kind, is_builtin_allowed_in_pipeline, run_builtin};
use crate::diagnostics::print_error;
use crate::types::{Command, CommandFlow, CommandStatus, OutputRedirection, Pipeline};
use nix::fcntl::{OFlag, open};
use nix::sys::stat::Mode;
use nix::sys::wait::{WaitStatus, waitpid};
use nix::unistd::{ForkResult, dup, dup2_stdin, dup2_stdout, execvp, fork, pipe};
use std::ffi::CString;
use std::io::{self, Write};
use std::os::fd::{AsRawFd, OwnedFd};
use std::process;

pub type ShellResult<T> = Result<T, Box<dyn std::error::Error>>;

pub fn run_command(command: &Command) -> ShellResult<CommandFlow> {
    let print_lifecycle = command.program != "clear";

    if print_lifecycle {
        println!("{} starting...", &command.program);
    }

    let flow = if builtin_kind(command).is_some() {
        // 内置命令在 shell 进程中执行，需要使用专门的重定向恢复逻辑。
        match run_builtin_with_redirection(command) {
            Ok(BuiltinResult::Continue) => CommandFlow::Continue(CommandStatus::success()),
            Ok(BuiltinResult::Exit) => CommandFlow::Exit(CommandStatus::success()),
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

struct SavedRedirection {
    stdin: Option<OwnedFd>,
    stdout: Option<OwnedFd>,
}

fn run_builtin_with_redirection(command: &Command) -> ShellResult<BuiltinResult> {
    let saved = apply_redirection_in_shell(command)?;
    // 这里已经确认是内置命令；若返回 None，说明调用路径存在内部错误。
    let result = run_builtin(command).expect("builtin command should have a builtin result");

    // builtin 在当前 shell 进程中执行，恢复 fd 前先刷新缓冲区。
    io::stdout().flush()?;
    io::stderr().flush()?;
    restore_redirection(saved)?;

    Ok(result)
}

// 对内置命令应用临时重定向，并保存原始标准输入输出用于恢复。
fn apply_redirection_in_shell(command: &Command) -> ShellResult<SavedRedirection> {
    let mut saved = SavedRedirection {
        stdin: None,
        stdout: None,
    };

    let result = (|| -> ShellResult<()> {
        if let Some(path) = &command.redirection.stdin {
            // 保存标准输入 fd 的副本，避免重定向永久影响 shell 进程。
            let original_stdin = dup(io::stdin())?;
            let fd = open_input_redirection(path)?;
            dup2_stdin(&fd)?;
            drop(fd);
            saved.stdin = Some(original_stdin);
        }

        if let Some(output_redirection) = &command.redirection.stdout {
            let original_stdout = dup(io::stdout())?;
            let fd = open_output_redirection(output_redirection)?;
            dup2_stdout(&fd)?;
            drop(fd);
            saved.stdout = Some(original_stdout);
        }

        Ok(())
    })();
    // 这里不能直接用 `?` 返回，因为出错时仍需恢复已经修改过的 fd。
    if let Err(err) = result {
        let _ = restore_redirection(saved);
        return Err(err);
    }

    Ok(saved)
}

fn restore_redirection(saved: SavedRedirection) -> ShellResult<()> {
    if let Some(stdin) = saved.stdin {
        dup2_stdin(&stdin)?;
    }

    if let Some(stdout) = saved.stdout {
        dup2_stdout(&stdout)?;
    }

    Ok(())
}

// 打开输入重定向文件，并补充 shell 风格的错误上下文。
fn open_input_redirection(path: &str) -> ShellResult<OwnedFd> {
    open(path, OFlag::O_RDONLY, Mode::empty())
        .map_err(|err| format!("{}: cannot open for input: {}", path, err).into())
}

fn open_output_redirection(output_redirection: &OutputRedirection) -> ShellResult<OwnedFd> {
    match output_redirection {
        OutputRedirection::Truncate(path) => open(
            path.as_str(),
            OFlag::O_CREAT | OFlag::O_WRONLY | OFlag::O_TRUNC,
            Mode::from_bits_truncate(0o644),
        )
        .map_err(|err| format!("{}: cannot open for output: {}", path, err).into()),
        OutputRedirection::Append(path) => open(
            path.as_str(),
            OFlag::O_CREAT | OFlag::O_WRONLY | OFlag::O_APPEND,
            Mode::from_bits_truncate(0o644),
        )
        .map_err(|err| format!("{}: cannot open for output: {}", path, err).into()),
    }
}

// 关闭子进程继承到的原始 pipe fd，避免它们被 execvp 后的新程序继续持有。
fn close_pipeline_fds_in_child(pipes: &[(OwnedFd, OwnedFd)]) {
    for (read_fd, write_fd) in pipes {
        unsafe {
            nix::libc::close(read_fd.as_raw_fd());
            nix::libc::close(write_fd.as_raw_fd());
        }
    }
}

pub fn run_pipeline(pipeline: &Pipeline) -> ShellResult<CommandStatus> {
    println!("pipeline starting...");

    // 当前仅允许纯输出型内置命令进入管道。
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

    for (i, command) in pipeline.commands.iter().enumerate() {
        match unsafe { fork()? } {
            ForkResult::Parent { child } => {
                children_pids.push(child);
            }
            ForkResult::Child => {
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

                close_pipeline_fds_in_child(&pipes);

                // 允许出现在管道中的内置命令在子进程中直接执行并退出。
                if let Some(kind) = builtin_kind(command) {
                    if is_builtin_allowed_in_pipeline(kind) {
                        run_builtin(command);
                        process::exit(0);
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
    drop(pipes);

    let mut last_status = CommandStatus::success();
    for child in children_pids {
        last_status = wait_status_to_command_status(waitpid(child, None)?);
    }
    println!("pipeline ending.");

    Ok(last_status)
}

// 把waitpid返回的状态码转换成自定义的
fn wait_status_to_command_status(status: WaitStatus) -> CommandStatus {
    match status {
        WaitStatus::Exited(_, code) => CommandStatus::new(code),
        // 程序不是正常退出，而是被信号终止。shell 通常把这种情况编码成 128 + signal_number
        WaitStatus::Signaled(_, signal, _) => CommandStatus::new(128 + signal as i32),
        // TODO 其他情况统一当错误处理，目前还没有job control
        _ => CommandStatus::failure(),
    }
}

fn build_c_argv(command: &Command) -> ShellResult<Vec<CString>> {
    let mut argv = Vec::new();
    argv.push(command.program.clone());
    argv.extend(command.args.clone());

    argv.iter()
        .map(|arg| CString::new(arg.as_str()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| err.into())
}

// 构造 c_argv 并调用 execvp。如果 execvp 成功，当前进程不会返回。
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

// 子进程不返回错误，执行失败也直接退出，执行成功也不会返回
pub fn handle_redirection_or_exit(command: &Command) {
    if let Some(path) = &command.redirection.stdin {
        // 先打开文件描述符
        let fd = match open_input_redirection(path) {
            Ok(fd) => fd,
            Err(err) => {
                print_error(err);
                process::exit(127);
            }
        };

        // 然后把stdin指向刚才的文件
        if let Err(err) = dup2_stdin(&fd) {
            print_error(format!("{}: dup2 stdin failed: {}", path, err));
            process::exit(127);
        }
        drop(fd);
    };

    if let Some(output_redirection) = &command.redirection.stdout {
        let fd = match open_output_redirection(output_redirection) {
            Ok(fd) => fd,
            Err(err) => {
                print_error(err);
                process::exit(127);
            }
        };

        if let Err(err) = dup2_stdout(&fd) {
            print_error(format!("dup2 stdout failed: {}", err));
            process::exit(127);
        }
        drop(fd);
    }
}

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

fn run_external(command: &Command) -> ShellResult<CommandStatus> {
    match unsafe { fork()? } {
        ForkResult::Parent { child } => {
            let wait_status = waitpid(child, None)?;
            // waitpid会返回各种不同的信号
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
