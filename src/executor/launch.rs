//! 进程启动：fork → 建管道 → 接重定向 → exec。
//!
//! 本模块涉及的核心 POSIX 调用一览：
//!
//! ┌──────────────────┬─────────────────────────────────────────────────┐
//! │ 系统调用          │ 功能                                             │
//! ├──────────────────┼─────────────────────────────────────────────────┤
//! │ fork()           │ 克隆当前进程。调用一次，返回两次（父→子PID, 子→0） │
//! │ pipe()           │ 创建一个匿名管道，返回 (读端fd, 写端fd)             │
//! │ dup2_stdin(fd)   │ 让 stdin(0) 指向 fd，即把 fd 复制到 stdin 位置     │
//! │ dup2_stdout(fd)  │ 让 stdout(1) 指向 fd                             │
//! │ execvp(file,argv)│ 用新程序镜像替换当前进程。成功永不返回，失败返回-1    │
//! └──────────────────┴─────────────────────────────────────────────────┘
//!
//! fork 内存模型要点：
//!   - fork 之后，父子进程的内存是独立的副本（copy-on-write）
//!   - 父进程修改变量不影响子进程，反之亦然
//!   - 子进程继承父进程的所有 fd、信号处理表、环境变量
//!   - 子进程从 fork 调用点之后继续执行（和父进程执行同一份代码）
//!
//! execvp 语义：
//!   - 用新程序替换当前进程的镜像、内存、栈
//!   - 成功时：execvp 不返回（! 返回类型），原进程"变成"了新程序
//!   - 失败时：返回 -1，当前进程继续执行。常见失败原因：命令不存在（ENOENT）
//!   - fd 默认保留（除非设置了 FD_CLOEXEC），所以重定向在 exec 后仍然有效

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

/// 启动一条外部命令（可能是在前台，也可能是后台）。
///
/// 总流程：
///   fork ──┬── Parent: 记账 → finalize_launched_job()（后台立刻返回，前台阻塞等待）
///          │
///          └── Child:  setpgid → restore_signals → handle_redirection
///                      → execvp("ls")（进程变成 ls，永不返回）
///
/// 如果 execvp 失败（如命令不存在），子进程打印错误后 exit(127)。
/// 按 POSIX shell 约定，127 表示"命令未找到"。
pub(crate) fn launch_command_job(
    command: &Command,
    state: &mut ShellState,
    background: bool,
    command_line: &str,
) -> ShellResult<CommandStatus> {
    // fork 将当前进程一分为二：
    //   父进程的 fork 返回 ForkResult::Parent { child } → child 是子进程 PID
    //   子进程的 fork 返回 ForkResult::Child → 它看不到 child PID
    match unsafe { fork()? } {
        ForkResult::Parent { child } => {
            // 父进程侧：set_child_pgid(child, child) 尝试将子进程放入以自己为组长的新进程组。
            // 如果子进程已经先 exec 并设好了，这里返回 EACCES（被防竞态封装吞掉）。
            set_child_pgid(child, child)?;

            // 组装 Job 对象：当前只有一个进程成员。
            let mut job = Job {
                id: next_job_id(state),
                pgid: child,
                command_line: command_line.to_string(),
                status: JobStatus::Running,
                last_pid: child,
                processes: vec![JobProcess {
                    pid: child,
                    state: ProcessState::Running,
                    last_status: None,
                }],
            };
            // 后台 → 放入作业表立刻返回
            // 前台 → 阻塞等待子进程结束/停止
            finalize_launched_job(state, &mut job, background)
        }
        ForkResult::Child => {
            // 子进程侧：Pid::from_raw(0) 在 setpgid 中表示"我自己"。
            // 第一个参数 0 = 我的 PID，第二个参数 0 = 创建以我为组长的新进程组。
            set_child_pgid(Pid::from_raw(0), Pid::from_raw(0))?;

            // fork 继承了 shell 的信号忽略策略。
            // 用户程序必须恢复默认信号行为，否则 Ctrl-C 对它无效。
            restore_child_interactive_signals()?;

            // 把 stdin/stdout 重定向到命令指定的文件（如果命令带了 < 或 > 的话）。
            // 失败时直接 exit(127)，不在子进程中把错误传回父进程的 Rust 控制流。
            handle_redirection_or_exit(command);

            // execvp 把当前子进程替换成外部程序。
            // 成功后不返回；失败时内部执行 exit(127)。
            exec_external_or_exit(command);
        }
    }
}

/// 启动一条管道中的所有命令。
///
/// 对于 `cmd0 | cmd1 | cmd2` 这种 3 条命令的管道：
///
///   第 0 步: piperead,pipewrite = pipe() × 2  → 创建 2 根管道 (n-1 根)
///   第 1 步: fork cmd0 → child 把 stdout dup2 到 pipe0 写端
///   第 2 步: fork cmd1 → child 把 stdin dup2 到 pipe0 读端, stdout dup2 到 pipe1 写端
///   第 3 步: fork cmd2 → child 把 stdin dup2 到 pipe1 读端
///   第 4 步: parent 关闭所有 pipe fd（否则读端收不到 EOF）
///   第 5 步: 组装 Job → finalize_launched_job
///
/// 为什么要 parent 关闭 pipe fd？
///   pipe 的读端只有在"所有写端都关闭"之后才会返回 EOF。
///   父进程 fork 完所有 child 后如果还持有写端 fd，
///   `cat` 之类读管道的进程就永远等不到 EOF，一直挂在那里。
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

    // 为 n 条命令创建 n-1 根匿名管道。
    // 每根管道 pipe() 返回 (读端 OwnedFd, 写端 OwnedFd)。
    // OwnedFd 离开作用域时自动 close。
    let mut pipes = Vec::new();
    for _ in 0..n - 1 {
        let (read_fd, write_fd) = pipe()?;
        pipes.push((read_fd, write_fd));
    }

    let mut processes = Vec::new();
    let mut pgid = None;

    // 逐个 fork 管道里的每条命令。
    for (i, command) in pipeline.commands.iter().enumerate() {
        match unsafe { fork()? } {
            ForkResult::Parent { child } => {
                // 第一条命令成为整个 pipeline 的进程组组长
                // （pgid = 第一个 child 的 pid）。
                // 后续所有子进程都加入同一个进程组。
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
                // 第一条命令用 getpid() 作为进程组 pgid（它就是组长）。
                // 后面命令用父进程 fork 前记录的 pgid。
                let group = pgid.unwrap_or(getpid());
                set_child_pgid(Pid::from_raw(0), group)?;
                restore_child_interactive_signals()?;

                // 接管道数据流：
                //   - 第 i 条（非首条）：stdin ← pipe[i-1] 的读端
                //   - 第 i 条（非末条）：stdout → pipe[i] 的写端
                if i != 0 {
                    // dup2_stdin(fd)：让 stdin(0) 指向 fd，原 stdin 被覆盖。
                    // 之后这条命令从管道读端读取上一条命令的输出。
                    if let Err(err) = dup2_stdin(&pipes[i - 1].0) {
                        print_error(format!("pipeline: dup2 stdin failed: {}", err));
                        process::exit(127);
                    }
                }

                if i != n - 1 {
                    // dup2_stdout(fd)：让 stdout(1) 指向 fd，原 stdout 被覆盖。
                    // 之后这条命令的输出会写入管道，被下一条命令读到。
                    if let Err(err) = dup2_stdout(&pipes[i].1) {
                        print_error(format!("pipeline: dup2 stdout failed: {}", err));
                        process::exit(127);
                    }
                }

                // 接着应用命令自己的重定向（例如 `> out.txt`）。
                // 这可以覆盖管道的默认数据流方向。
                handle_redirection_or_exit(command);

                // 子进程不再需要管道的任何一端（已经 dup2 到 stdin/stdout），
                // 关闭原始 pipe fd，避免泄漏给 exec 后的新程序。
                drop(pipes);

                // pipeline 中允许纯输出型 builtin（help/pwd/env/status）在子进程直接运行。
                if let Some(kind) = builtin_kind(command) {
                    if is_builtin_allowed_in_pipeline(kind) {
                        let mut child_state = ShellState {
                            last_status: state.last_status,
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
                        };

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

    // 父进程关闭所有管道 fd。
    // 因为每次 fork 会复制 fd，父进程和每个子进程各持有一份。
    // 子进程已经 drop 了它们的那份，父进程也必须 drop，
    // 这样管道的读写端总数正确，EOF 才能正常传播。
    drop(pipes);

    let group = pgid.expect("pipeline with commands should have a process group");

    // 所有子进程都 fork 完后，才知道完整的进程成员列表，此时组装 Job。
    let mut job = Job {
        id: next_job_id(state),
        pgid: group,
        command_line: command_line.to_string(),
        status: JobStatus::Running,
        // 管道退出码取最后一条命令的 PID。
        last_pid: processes
            .last()
            .map(|process| process.pid)
            .expect("pipeline should contain at least one process"),
        processes,
    };

    finalize_launched_job(state, &mut job, background)
}

/// 将外部命令的 argv 转换成 C 字符串，然后调用 execvp。
///
/// execvp(file, argv)：
///   - file：要执行的程序名。如果包含 /，当作路径；否则在 PATH 中搜索。
///   - argv：参数列表，argv[0] 是程序名（shell 约定）。
///   - "vp" 后缀含义：v = vector（数组传参），p = PATH 搜索。
///   - 成功：当前进程镜像被替换，此函数不返回（返回类型 `!` 表示发散函数）。
///   - 失败：返回 Err，一般是 ENOENT（命令不存在）或 EACCES（无执行权限）。
///
/// 按 POSIX shell 约定，执行失败返回退出码 127（command not found）。
fn exec_external_or_exit(command: &Command) -> ! {
    let mut argv = Vec::new();
    // 对于简单的字面量 ShellWord，Display 回显出原文；
    // 对于含 $ 展开的，调用方应在 fork 之前先调 expand_argv 展开。
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
