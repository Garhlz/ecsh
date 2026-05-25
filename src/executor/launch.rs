//! 启动外部命令和管道。
//!
//! 这里负责 fork、管道接线、子进程初始化，以及把已启动进程交给 job control。

use crate::builtin::{builtin_kind, is_builtin_allowed_in_pipeline, run_builtin};
use crate::diagnostics::print_error;
use crate::executor::jobs::{finalize_launched_job, next_job_id, set_child_pgid};
use crate::redirection::handle_redirection_or_exit;
use crate::signals::restore_child_interactive_signals;
use crate::types::{
    Command, CommandFlow, CommandStatus, Job, JobProcess, JobStatus, Pipeline, ProcessState,
    ShellResult, ShellState,
};
use nix::unistd::{ForkResult, Pid, dup2_stdin, dup2_stdout, execvp, fork, getpid, pipe};
use std::collections::HashMap;
use std::process;

/// 启动一条外部命令。
pub(crate) fn launch_command_job(
    command: &Command,
    state: &mut ShellState,
    background: bool,
    command_line: &str,
) -> ShellResult<CommandStatus> {
    match unsafe { fork()? } {
        ForkResult::Parent { child } => {
            // 父进程负责记账并把新子进程纳入 job control。
            set_child_pgid(child, child)?;
            let mut job = single_process_job(state, child, command_line);
            finalize_launched_job(state, &mut job, background)
        }
        ForkResult::Child => {
            // 子进程在 exec 前只做最小初始化：进程组、信号和重定向。
            set_child_pgid(Pid::from_raw(0), Pid::from_raw(0))?;
            restore_child_interactive_signals()?;
            handle_redirection_or_exit(command);
            exec_external_or_exit(command);
        }
    }
}

/// 启动一条管道中的所有命令。
pub(crate) fn launch_pipeline_job(
    pipeline: &Pipeline,
    state: &mut ShellState,
    background: bool,
    command_line: &str,
) -> ShellResult<CommandStatus> {
    let n = pipeline.commands.len();
    if n == 0 {
        return Ok(CommandStatus::success());
    }

    // n 条命令需要 n-1 根匿名管道。
    let mut pipes = Vec::new();
    for _ in 0..n - 1 {
        let (read_fd, write_fd) = pipe()?;
        pipes.push((read_fd, write_fd));
    }

    let mut processes = Vec::new();
    let mut pgid = None;

    for (i, command) in pipeline.commands.iter().enumerate() {
        match unsafe { fork()? } {
            ForkResult::Parent { child } => {
                // 第一条子进程成为整个 pipeline 的进程组组长。
                let group = pgid.unwrap_or(child);
                set_child_pgid(child, group)?;
                pgid = Some(group);
                processes.push(JobProcess {
                    pid: child,
                    state: ProcessState::Running,
                    last_status: None,
                });
            }
            ForkResult::Child => {
                // 管道中的所有子进程都加入同一个进程组，便于统一前后台控制。
                let group = pgid.unwrap_or(getpid());
                set_child_pgid(Pid::from_raw(0), group)?;
                restore_child_interactive_signals()?;

                // 非首条命令从上一根管道的读端读入。
                if i != 0 {
                    if let Err(err) = dup2_stdin(&pipes[i - 1].0) {
                        print_error(format!("pipeline: dup2 stdin failed: {}", err));
                        process::exit(127);
                    }
                }

                // 非末条命令把 stdout 接到下一根管道的写端。
                if i != n - 1 {
                    if let Err(err) = dup2_stdout(&pipes[i].1) {
                        print_error(format!("pipeline: dup2 stdout failed: {}", err));
                        process::exit(127);
                    }
                }

                // 命令自身的重定向优先级高于默认的管道接线。
                handle_redirection_or_exit(command);

                // 子进程完成 dup2 后，原始 pipe fd 不再需要继续保留。
                drop(pipes);

                if let Some(kind) = builtin_kind(command) {
                    if is_builtin_allowed_in_pipeline(kind) {
                        let mut child_state = pipeline_child_state(state.last_status);

                        let flow = run_builtin(command, &mut child_state)
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

                exec_external_or_exit(command);
            }
        }
    }

    // 父进程也必须及时关闭自己的 pipe fd，避免影响 EOF 传播。
    drop(pipes);

    let group = pgid.expect("pipeline with commands should have a process group");
    let mut job = Job {
        id: next_job_id(state),
        pgid: group,
        command_line: command_line.to_string(),
        status: JobStatus::Running,
        last_pid: processes
            .last()
            .map(|process| process.pid)
            .expect("pipeline should contain at least one process"),
        processes,
    };

    finalize_launched_job(state, &mut job, background)
}

/// 将 argv 转成 C 字符串后调用 execvp。
///
/// 按 shell 约定，exec 失败统一退出 127。
fn exec_external_or_exit(command: &Command) -> ! {
    let mut argv = Vec::new();
    argv.push(command.program.to_string());
    for arg in &command.args {
        argv.push(arg.to_string());
    }

    let c_argv = match argv
        .iter()
        .map(|arg| std::ffi::CString::new(arg.as_str()))
        .collect::<Result<Vec<_>, _>>()
    {
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

/// 为单个外部命令构造 job 记录。
fn single_process_job(state: &mut ShellState, pid: Pid, command_line: &str) -> Job {
    Job {
        id: next_job_id(state),
        pgid: pid,
        command_line: command_line.to_string(),
        status: JobStatus::Running,
        last_pid: pid,
        processes: vec![JobProcess {
            pid,
            state: ProcessState::Running,
            last_status: None,
        }],
    }
}

/// 为 pipeline 中允许执行的 builtin 准备一个最小子进程状态。
fn pipeline_child_state(last_status: CommandStatus) -> ShellState {
    ShellState {
        last_status,
        interactive: false,
        shell_pgid: None,
        shell_terminal_fd: None,
        jobs: Vec::new(),
        next_job_id: 1,
        current_fg_pgid: None,
        script_env: crate::ecscript::env::Environment::new(),
        aliases: HashMap::new(),
        traps: HashMap::new(),
        command_history: Vec::new(),
    }
}
