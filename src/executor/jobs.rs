//! Job 状态机：进程组管理、终端控制权切换、前后台等待与回收。
//!
//! 本模块涉及的核心 POSIX 调用一览：
//!
//! ┌─────────────────────┬──────────────────────────────────────────────────┐
//! │ 系统调用             │ 功能                                              │
//! ├─────────────────────┼──────────────────────────────────────────────────┤
//! │ getpid()            │ 获取当前进程的 PID                                 │
//! │ getpgrp()           │ 获取当前进程所在的进程组 PGID                       │
//! │ setpgid(pid, pgid)  │ 将进程 pid 放入进程组 pgid；pgid=pid 表示新建进程组 │
//! │ tcgetpgrp(fd)       │ 查询终端 fd 的当前前台进程组 PGID                   │
//! │ tcsetpgrp(fd, pgid) │ 将终端 fd 的前台权交给进程组 pgid                   │
//! │ waitpid(pid, opts)  │ 等待子进程状态变化；pid<0 时等待该进程组内任意成员   │
//! │ killpg(pgid, sig)   │ 向进程组 pgid 的所有成员发送信号 sig                │
//! └─────────────────────┴──────────────────────────────────────────────────┘
//!
//! 交互式 shell 用到的信号：
//!
//! ┌──────────┬───────────┬──────────────────────────────────────────────┐
//! │ 信号      │ 常见来源   │ 默认行为                                      │
//! ├──────────┼───────────┼──────────────────────────────────────────────┤
//! │ SIGINT   │ Ctrl-C    │ 终止进程                                      │
//! │ SIGQUIT  │ Ctrl-\    │ 终止进程 + core dump                          │
//! │ SIGTSTP  │ Ctrl-Z    │ 暂停进程（可通过 SIGCONT 恢复）                  │
//! │ SIGCONT  │ fg / bg   │ 恢复之前被暂停的进程                            │
//! │ SIGTTIN  │ 内核自动   │ 后台进程读终端时，内核发送；默认暂停进程           │
//! │ SIGTTOU  │ 内核自动   │ 后台进程写终端或 tcsetpgrp 时，内核发送；默认暂停  │
//! │ SIGCHLD  │ 内核自动   │ 子进程状态变化时，内核发给父进程；默认忽略         │
//! └──────────┴───────────┴──────────────────────────────────────────────┘

use crate::signals::init_interactive_shell_signals;
use crate::types::{CommandStatus, Job, JobStatus, ProcessState, ShellResult, ShellState};
use nix::errno::Errno;
use nix::sys::signal::{Signal, killpg};
use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use nix::unistd::{Pid, getpgrp, getpid, setpgid, tcgetpgrp, tcsetpgrp};
use std::os::fd::BorrowedFd;

// ══════════════════════════════════════════════════════════════════════
// 对外入口
// ══════════════════════════════════════════════════════════════════════

/// shell 启动时调用一次：初始化 job control 基础设施。
///
/// 做的事情：
///   1. 让 shell 成为自己进程组的组长（setpgid）
///   2. 从当前终端前台抢回控制权（tcgetpgrp → tcsetpgrp）
///   3. 设置信号忽略策略（sigaction）
///   4. 记录 shell_pgid / shell_terminal_fd，供后续 give/reclaim 使用
pub fn init_shell_job_control(state: &mut ShellState) -> ShellResult<()> {
    if !state.interactive {
        return Ok(());
    }

    let shell_pid = getpid();

    // getpgrp() 返回我当前在哪个进程组。
    // 如果我不是组长（常见于从另一个 shell 启动 ecsh 时），
    // 用 setpgid 创建以自己为组长的新进程组。
    if getpgrp() != shell_pid {
        setpgid(shell_pid, shell_pid)?;
    }

    // fd=0(stdin)、fd=1(stdout)、fd=2(stderr) 都指向同一个终端设备。
    // 借用 fd=0 作为操作终端的"句柄"。
    let terminal = unsafe { BorrowedFd::borrow_raw(0) };

    // 检查：终端当前的前台进程组是不是我？如果不是，抢回来。
    if tcgetpgrp(terminal)? != shell_pid {
        tcsetpgrp(terminal, shell_pid)?;
    }

    // 让 shell 忽略 Ctrl-C / Ctrl-Z（这些信号应该只影响前台 job）。
    init_interactive_shell_signals()?;
    state.shell_pgid = Some(shell_pid);
    state.shell_terminal_fd = Some(0);
    Ok(())
}

/// 主循环每次迭代调用：非阻塞地回收所有后台子进程的状态变化。
///
/// 用 waitpid(-1, WNOHANG|WUNTRACED|WCONTINUED) 一次收一个变化，
/// 收完立刻更新对应 job 的状态；没有更多变化时返回。
///
/// 为什么不用异步 SIGCHLD handler？
///   同步轮询让所有状态修改都在同一个调用栈里，推理和调试都更简单。
pub fn reap_background_jobs(state: &mut ShellState) -> ShellResult<()> {
    // 不断询问："还有哪个子进程发生状态变化了？"
    loop {
        let result = waitpid(
            Pid::from_raw(-1), // -1 = 等我名下任意子进程
            Some(WaitPidFlag::WNOHANG | WaitPidFlag::WUNTRACED | WaitPidFlag::WCONTINUED),
            // WNOHANG    → 非阻塞，没有就立刻返回 StillAlive
            // WUNTRACED  → 也要报告"被暂停"的变化（Ctrl-Z）
            // WCONTINUED → 也要报告"被恢复"的变化（SIGCONT）
        );

        match result {
            Ok(WaitStatus::StillAlive) => break, // 暂时没有更多变化了
            Ok(status) => {
                // 有变化！找到这个 pid 属于哪个 job，更新它的状态。
                handle_waitpid_status(state, status);
                // 回到循环开头继续收，可能还有更多变化。
            }
            Err(Errno::ECHILD) => break, // 一个子进程都没有了
            Err(err) => return Err(err.into()),
        }
    }

    Ok(())
}

// ══════════════════════════════════════════════════════════════════════
// 前台/后台分叉
// ══════════════════════════════════════════════════════════════════════

/// fork 完子进程后调用：把组装好的 Job 推向"前台等待"或"后台记账"。
///
///   后台路径：把 job 塞进作业表，打印 [job_id] pgid，立刻返回
///   前台路径：阻塞等待整个 job 结束，把终端还给 shell
///
/// 返回：这个 job 的退出码。后台 job 返回 success() 表示"已提交"。
pub(crate) fn finalize_launched_job(
    state: &mut ShellState,
    job: &mut Job,
    background: bool,
) -> ShellResult<CommandStatus> {
    if background {
        println!("[{}] {}", job.id, job.pgid);
        state.jobs.push(job.clone());
        return Ok(CommandStatus::success());
    }

    // 前台：把终端给对方 → 阻塞等待 → 把终端拿回来
    wait_for_foreground_job(state, job)?;

    // 如果前台 job 是被 Ctrl-Z 暂停的，放进作业表，以便后续 fg/bg
    if job.status == JobStatus::Stopped {
        state.jobs.push(job.clone());
    }
    Ok(status_from_job(job))
}

/// 前台 job 的完整生命周期：交出终端 → 阻塞等待 → 收回终端。
///
/// 交出终端后，键盘输入和 Ctrl-C/Ctrl-Z 都由内核路由给 job。
/// 等待循环内只做一件事：调用 waitpid(-pgid) 收集子进程状态变化。
fn wait_for_foreground_job(state: &mut ShellState, job: &mut Job) -> ShellResult<()> {
    give_terminal_to_job(state, job.pgid)?;

    // 阻塞等待：卡在这里，直到 job 结束或全部被暂停。
    collect_child_statuses(job, state.interactive)?;

    reclaim_terminal_for_shell(state)?;
    Ok(())
}

/// 循环收集子进程状态变化，直到整条 job 结束或全部被暂停。
///
/// 每次 waitpid 收到变化，就更新对应 JobProcess 的状态，
/// 然后重新计算整条 job 的状态（recompute_job_status）。
fn collect_child_statuses(job: &mut Job, interactive: bool) -> ShellResult<()> {
    loop {
        let result = waitpid(
            Pid::from_raw(-job.pgid.as_raw()), // 负数 pid → 等进程组 pgid 里的任意成员
            Some(WaitPidFlag::WUNTRACED | WaitPidFlag::WCONTINUED),
            // 没有 WNOHANG → 阻塞等待，直到有变化
            // WUNTRACED   → 报告"被暂停"（Ctrl-Z）
            // WCONTINUED  → 报告"被恢复"（SIGCONT）
        );

        match result {
            Ok(status) => {
                if let Some(pid) = wait_status_pid(&status) {
                    update_job_process(job, pid, status);
                }

                // job 状态刚被 recompute_job_status 推导过，检查能否结束等待
                let job_done = matches!(job.status, JobStatus::Stopped | JobStatus::Done(_));
                if job_done {
                    return Ok(());
                }
                // 否则继续等下一个状态变化
            }
            Err(Errno::ECHILD) => {
                // 该进程组里所有子进程都已被回收
                recompute_job_status(job);
                return Ok(());
            }
            Err(err) => {
                reclaim_terminal_for_shell_interactive(interactive)?;
                return Err(err.into());
            }
        }
    }
}

// ══════════════════════════════════════════════════════════════════════
// fg / bg 命令实现
// ══════════════════════════════════════════════════════════════════════

/// `fg %N`：把一个 job 拉到前台。
///
///   mark_as_running → give terminal → SIGCONT → wait_for_foreground_job
///
/// 如果 job 在前台又被 Ctrl-Z 了，放回作业表。
pub(crate) fn resume_job_in_foreground(state: &mut ShellState, job: &mut Job) -> ShellResult<()> {
    mark_job_as_running(job);

    if state.interactive {
        give_terminal_to_job(state, job.pgid)?;
    }
    // killpg：向 job 的进程组全体发送 SIGCONT，让暂停的进程恢复执行
    killpg(job.pgid, Signal::SIGCONT)?;

    wait_for_foreground_job(state, job)
}

/// `bg %N`：让一个 Stopped job 在后台恢复运行。
///
///   mark_as_running → SIGCONT → 返回
///
/// 不碰终端，不等它跑完。后续 reap_background_jobs 会异步回收。
pub(crate) fn continue_job(job: &mut Job) -> ShellResult<()> {
    mark_job_as_running(job);
    killpg(job.pgid, Signal::SIGCONT)?;
    Ok(())
}

// ══════════════════════════════════════════════════════════════════════
// 小型工具函数
// ══════════════════════════════════════════════════════════════════════

pub(crate) fn status_from_job(job: &Job) -> CommandStatus {
    match job.status {
        JobStatus::Running => CommandStatus::success(),
        JobStatus::Stopped => CommandStatus::new(128 + Signal::SIGTSTP as i32),
        JobStatus::Done(s) => s,
    }
}

pub(crate) fn job_status_text(status: JobStatus) -> &'static str {
    match status {
        JobStatus::Running => "Running",
        JobStatus::Stopped => "Stopped",
        JobStatus::Done(_) => "Done",
    }
}

pub(crate) fn next_job_id(state: &mut ShellState) -> usize {
    let id = state.next_job_id;
    state.next_job_id += 1;
    id
}

/// 防竞态版本的 setpgid 封装。
///
/// fork 后父子两边都调 setpgid——谁快谁成功，慢的收到 EACCES（对方已设好）。
/// 所以 EACCES 不算错误。
pub(crate) fn set_child_pgid(pid: Pid, pgid: Pid) -> nix::Result<()> {
    match setpgid(pid, pgid) {
        Ok(()) => Ok(()),
        Err(Errno::EACCES) => Ok(()),
        Err(err) => Err(err),
    }
}

// ══════════════════════════════════════════════════════════════════════
// 内部辅助函数
// ══════════════════════════════════════════════════════════════════════

/// 将一个 Stopped job 的所有进程标记为 Running。
/// 在 fg 和 bg 恢复 job 时都会被用到。
fn mark_job_as_running(job: &mut Job) {
    for p in &mut job.processes {
        if p.state == ProcessState::Stopped {
            p.state = ProcessState::Running;
        }
    }
    job.status = JobStatus::Running;
}

/// 收到 waitpid 状态变化后，找到这个 pid 所属的 job 并更新。
/// 被 reap_background_jobs 调用。
fn handle_waitpid_status(state: &mut ShellState, status: WaitStatus) {
    let Some(pid) = wait_status_pid(&status) else {
        return;
    };

    // 在作业表中找到包含此 pid 的 job
    let Some(job) = state
        .jobs
        .iter_mut()
        .find(|job| job.processes.iter().any(|p| p.pid == pid))
    else {
        return;
    };

    update_job_process(job, pid, status);
}

/// 把终端前台权交给 job.pgid。
fn give_terminal_to_job(state: &mut ShellState, pgid: Pid) -> ShellResult<()> {
    if !state.interactive {
        return Ok(());
    }
    let terminal = unsafe { BorrowedFd::borrow_raw(0) };
    tcsetpgrp(terminal, pgid)?;
    state.current_fg_pgid = Some(pgid);
    Ok(())
}

/// 把终端前台权收回给 shell。
fn reclaim_terminal_for_shell(state: &mut ShellState) -> ShellResult<()> {
    reclaim_terminal_for_shell_interactive(state.interactive)?;
    state.current_fg_pgid = None;
    Ok(())
}

/// 终端回收的内部实现。
fn reclaim_terminal_for_shell_interactive(interactive: bool) -> ShellResult<()> {
    if !interactive {
        return Ok(());
    }
    let shell_pid = getpid();
    let terminal = unsafe { BorrowedFd::borrow_raw(0) };
    tcsetpgrp(terminal, shell_pid)?;
    Ok(())
}

// ══════════════════════════════════════════════════════════════════════
// 进程状态更新
// ══════════════════════════════════════════════════════════════════════

/// 收到某个子进程的状态变化后，更新它在 Job 中的状态。
///
/// 四种变化对应的处理：
///
///   Exited(pid, code)          → 进程正常退出。状态标记 Completed，记下 code
///   Signaled(pid, sig, core)   → 进程被信号杀死。Completed，退出码 = 128 + sig
///   Stopped(pid, sig)          → 进程被暂停（Ctrl-Z）。状态标记 Stopped
///   Continued(pid)             → 进程被恢复（SIGCONT）。状态标记 Running
///
/// 128+signal 是 bash/zsh 的通用约定：$? = 128+N 表示"被信号 N 终止"。
fn update_job_process(job: &mut Job, pid: Pid, status: WaitStatus) {
    let Some(process) = job.processes.iter_mut().find(|p| p.pid == pid) else {
        return;
    };

    match status {
        WaitStatus::Exited(_, code) => {
            process.state = ProcessState::Completed;
            process.last_status = Some(CommandStatus::new(code));
        }
        WaitStatus::Signaled(_, sig, _) => {
            process.state = ProcessState::Completed;
            process.last_status = Some(CommandStatus::new(128 + sig as i32));
        }
        WaitStatus::Stopped(_, sig) => {
            process.state = ProcessState::Stopped;
            process.last_status = Some(CommandStatus::new(128 + sig as i32));
        }
        WaitStatus::Continued(_) => {
            process.state = ProcessState::Running;
        }
        _ => { /* StillAlive 等不携带进程信息的变体，忽略 */ }
    }

    // 一个进程变了，整个 job 的聚合状态也必须重新推导。
    recompute_job_status(job);
}

/// 从 job 中所有进程的状态推导整个 job 的状态。
///
/// 规则很简单：
///   1. 全部 Completed                     → Done
///   2. 没人 Running（但没全部 Completed）   → Stopped
///   3. 至少有一个 Running                  → Running
fn recompute_job_status(job: &mut Job) {
    let all_completed = job
        .processes
        .iter()
        .all(|p| p.state == ProcessState::Completed);
    let any_running = job
        .processes
        .iter()
        .any(|p| p.state == ProcessState::Running);

    if all_completed {
        // 退出码取 last_pid（管道最后一条命令）的结果
        let code = job
            .processes
            .iter()
            .find(|p| p.pid == job.last_pid)
            .and_then(|p| p.last_status)
            .unwrap_or_else(CommandStatus::success);
        job.status = JobStatus::Done(code);
    } else if !any_running {
        job.status = JobStatus::Stopped;
    } else {
        job.status = JobStatus::Running;
    }
}

/// 从 WaitStatus 中提取 pid（仅对有 pid 的变体）。
fn wait_status_pid(status: &WaitStatus) -> Option<Pid> {
    match *status {
        WaitStatus::Exited(pid, _)
        | WaitStatus::Signaled(pid, _, _)
        | WaitStatus::Stopped(pid, _)
        | WaitStatus::Continued(pid) => Some(pid),
        // StillAlive 和 PtraceEvent/PtraceSyscall 不携带 pid
        _ => None,
    }
}

// ══════════════════════════════════════════════════════════════════════
// 测试
// ══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{JobProcess, ShellState as ShellStateType};
    use nix::sys::signal::Signal;

    fn single_process_job(pid_raw: i32) -> Job {
        let pid = Pid::from_raw(pid_raw);
        Job {
            id: 1,
            pgid: pid,
            command_line: "sleep 100".to_string(),
            status: JobStatus::Running,
            last_pid: pid,
            processes: vec![JobProcess {
                pid,
                state: ProcessState::Running,
                last_status: None,
            }],
        }
    }

    fn pipeline_job(pid1: i32, pid2: i32) -> Job {
        let pid1 = Pid::from_raw(pid1);
        let pid2 = Pid::from_raw(pid2);
        Job {
            id: 2,
            pgid: pid1,
            command_line: "cat | grep hi".to_string(),
            status: JobStatus::Running,
            last_pid: pid2,
            processes: vec![
                JobProcess {
                    pid: pid1,
                    state: ProcessState::Running,
                    last_status: None,
                },
                JobProcess {
                    pid: pid2,
                    state: ProcessState::Running,
                    last_status: None,
                },
            ],
        }
    }

    // ── recompute_job_status ─────────────────────────────────────

    #[test]
    fn all_running_stays_running() {
        let mut job = single_process_job(100);
        recompute_job_status(&mut job);
        assert_eq!(job.status, JobStatus::Running);
    }

    #[test]
    fn single_completed_is_done() {
        let mut job = single_process_job(100);
        job.processes[0].state = ProcessState::Completed;
        job.processes[0].last_status = Some(CommandStatus::new(42));
        recompute_job_status(&mut job);
        assert_eq!(job.status, JobStatus::Done(CommandStatus::new(42)));
    }

    #[test]
    fn single_stopped_is_stopped() {
        let mut job = single_process_job(100);
        job.processes[0].state = ProcessState::Stopped;
        recompute_job_status(&mut job);
        assert_eq!(job.status, JobStatus::Stopped);
    }

    #[test]
    fn pipeline_all_completed_takes_last_pid_code() {
        let mut job = pipeline_job(100, 200);
        job.processes[0].state = ProcessState::Completed;
        job.processes[0].last_status = Some(CommandStatus::new(0));
        job.processes[1].state = ProcessState::Completed;
        job.processes[1].last_status = Some(CommandStatus::new(1));
        recompute_job_status(&mut job);
        assert_eq!(job.status, JobStatus::Done(CommandStatus::new(1)));
    }

    #[test]
    fn pipeline_one_running_one_completed_stays_running() {
        let mut job = pipeline_job(100, 200);
        job.processes[0].state = ProcessState::Completed;
        recompute_job_status(&mut job);
        assert_eq!(job.status, JobStatus::Running);
    }

    #[test]
    fn pipeline_one_completed_one_stopped_is_stopped() {
        let mut job = pipeline_job(100, 200);
        job.processes[0].state = ProcessState::Completed;
        job.processes[1].state = ProcessState::Stopped;
        recompute_job_status(&mut job);
        assert_eq!(job.status, JobStatus::Stopped);
    }

    #[test]
    fn pipeline_all_stopped_is_stopped() {
        let mut job = pipeline_job(100, 200);
        job.processes[0].state = ProcessState::Stopped;
        job.processes[1].state = ProcessState::Stopped;
        recompute_job_status(&mut job);
        assert_eq!(job.status, JobStatus::Stopped);
    }

    // ── status_from_job ──────────────────────────────────────────

    #[test]
    fn status_running_is_0() {
        let mut job = single_process_job(100);
        job.status = JobStatus::Running;
        assert_eq!(status_from_job(&job), CommandStatus::success());
    }

    #[test]
    fn status_stopped_is_128_plus_sigtstp() {
        let mut job = single_process_job(100);
        job.status = JobStatus::Stopped;
        assert_eq!(
            status_from_job(&job),
            CommandStatus::new(128 + Signal::SIGTSTP as i32)
        );
    }

    #[test]
    fn status_done_passthrough() {
        let mut job = single_process_job(100);
        job.status = JobStatus::Done(CommandStatus::new(5));
        assert_eq!(status_from_job(&job), CommandStatus::new(5));
    }

    // ── job_status_text ──────────────────────────────────────────

    #[test]
    fn job_status_text_labels() {
        assert_eq!(job_status_text(JobStatus::Running), "Running");
        assert_eq!(job_status_text(JobStatus::Stopped), "Stopped");
        assert_eq!(
            job_status_text(JobStatus::Done(CommandStatus::success())),
            "Done"
        );
    }

    // ── next_job_id ──────────────────────────────────────────────

    #[test]
    fn next_job_id_increments() {
        let mut state = ShellStateType {
            last_status: CommandStatus::success(),
            interactive: false,
            shell_pgid: None,
            shell_terminal_fd: None,
            jobs: Vec::new(),
            next_job_id: 5,
            current_fg_pgid: None,
            script_env: crate::ecscript::env::Environment::new(),
        };
        assert_eq!(next_job_id(&mut state), 5);
        assert_eq!(next_job_id(&mut state), 6);
        assert_eq!(next_job_id(&mut state), 7);
    }

    // ── wait_status_pid ──────────────────────────────────────────

    #[test]
    fn wait_status_extracts_pid() {
        let pid = Pid::from_raw(42);
        assert_eq!(wait_status_pid(&WaitStatus::Exited(pid, 0)), Some(pid));
        assert_eq!(
            wait_status_pid(&WaitStatus::Signaled(pid, Signal::SIGTERM, false)),
            Some(pid)
        );
        assert_eq!(
            wait_status_pid(&WaitStatus::Stopped(pid, Signal::SIGTSTP)),
            Some(pid)
        );
        assert_eq!(wait_status_pid(&WaitStatus::Continued(pid)), Some(pid));
    }

    #[test]
    fn wait_status_still_alive_returns_none() {
        assert_eq!(wait_status_pid(&WaitStatus::StillAlive), None);
    }

    // ── update_job_process ───────────────────────────────────────

    #[test]
    fn update_exited_sets_completed() {
        let mut job = single_process_job(100);
        update_job_process(
            &mut job,
            Pid::from_raw(100),
            WaitStatus::Exited(Pid::from_raw(100), 7),
        );
        assert_eq!(job.processes[0].state, ProcessState::Completed);
        assert_eq!(job.processes[0].last_status, Some(CommandStatus::new(7)));
        assert_eq!(job.status, JobStatus::Done(CommandStatus::new(7)));
    }

    #[test]
    fn update_signaled_sets_completed_128_plus_sig() {
        let mut job = single_process_job(100);
        update_job_process(
            &mut job,
            Pid::from_raw(100),
            WaitStatus::Signaled(Pid::from_raw(100), Signal::SIGTERM, false),
        );
        assert_eq!(job.processes[0].state, ProcessState::Completed);
        assert_eq!(
            job.processes[0].last_status,
            Some(CommandStatus::new(128 + Signal::SIGTERM as i32))
        );
    }

    #[test]
    fn update_stopped_sets_stopped() {
        let mut job = single_process_job(100);
        update_job_process(
            &mut job,
            Pid::from_raw(100),
            WaitStatus::Stopped(Pid::from_raw(100), Signal::SIGTSTP),
        );
        assert_eq!(job.processes[0].state, ProcessState::Stopped);
        assert_eq!(job.status, JobStatus::Stopped);
    }

    #[test]
    fn update_continued_sets_running() {
        let mut job = single_process_job(100);
        job.processes[0].state = ProcessState::Stopped;
        job.status = JobStatus::Stopped;
        update_job_process(
            &mut job,
            Pid::from_raw(100),
            WaitStatus::Continued(Pid::from_raw(100)),
        );
        assert_eq!(job.processes[0].state, ProcessState::Running);
        assert_eq!(job.status, JobStatus::Running);
    }

    #[test]
    fn pipeline_gradually_goes_done() {
        let mut job = pipeline_job(100, 200);
        update_job_process(
            &mut job,
            Pid::from_raw(100),
            WaitStatus::Exited(Pid::from_raw(100), 0),
        );
        assert_eq!(job.status, JobStatus::Running);
        update_job_process(
            &mut job,
            Pid::from_raw(200),
            WaitStatus::Exited(Pid::from_raw(200), 1),
        );
        assert_eq!(job.status, JobStatus::Done(CommandStatus::new(1)));
    }

    #[test]
    fn update_unknown_pid_is_noop() {
        let mut job = single_process_job(100);
        update_job_process(
            &mut job,
            Pid::from_raw(999),
            WaitStatus::Exited(Pid::from_raw(999), 0),
        );
        assert_eq!(job.processes[0].state, ProcessState::Running);
    }
}
