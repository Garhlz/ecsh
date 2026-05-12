use crate::builtin::{BuiltinResult, builtin_kind, is_builtin_allowed_in_pipeline, run_builtin};
use crate::diagnostics::print_error;
use crate::types::{Command, CommandFlow, CommandStatus, Pipeline};
use nix::sys::wait::{WaitStatus, waitpid};
use nix::unistd::{ForkResult, dup2_stdin, dup2_stdout, execvp, fork, pipe};
use std::ffi::CString;
use std::os::fd::{AsRawFd, OwnedFd};
use std::process;

pub type ShellResult<T> = Result<T, Box<dyn std::error::Error>>;

pub fn run_command(command: &Command) -> ShellResult<CommandFlow> {
    let print_lifecycle = command.program != "clear";

    if print_lifecycle {
        println!("{} starting...", &command.program);
    }

    let flow = match run_builtin(command) {
        Some(BuiltinResult::Continue) => CommandFlow::Continue(CommandStatus::success()),
        Some(BuiltinResult::Exit) => CommandFlow::Exit(CommandStatus::success()),
        None => {
            // run_builtin匹配失败会返回None
            let status = match run_external(command) {
                Ok(status) => status,
                Err(err) => {
                    print_error(format!("{}: {}", command.program, err));
                    CommandStatus::failure()
                }
            };
            CommandFlow::Continue(status)
        }
    };

    if print_lifecycle {
        println!("{} ending.", &command.program);
    }

    Ok(flow)
}

// 关闭子进程继承到的原始 pipe fd，避免它们被 `execvp` 后的新程序继续持有。
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

fn wait_status_to_command_status(status: WaitStatus) -> CommandStatus {
    match status {
        WaitStatus::Exited(_, code) => CommandStatus::new(code),
        WaitStatus::Signaled(_, signal, _) => CommandStatus::new(128 + signal as i32),
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

fn run_external(command: &Command) -> ShellResult<CommandStatus> {
    match unsafe { fork()? } {
        ForkResult::Parent { child } => {
            let wait_status = waitpid(child, None)?;
            // waitpid会返回各种不同的信号
            Ok(wait_status_to_command_status(wait_status))
        }

        ForkResult::Child => {
            // `execvp` 成功后不会返回；失败时在子进程中直接退出。
            exec_external_or_exit(command);
        }
    }
}
