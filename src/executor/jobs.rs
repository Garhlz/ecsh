//! Job control：进程组、终端前台切换，以及前后台作业的状态收集。
//!
//! 这里的核心边界只有三件事：
//! - shell 启动时把自己放到独立进程组，并拿回终端前台。
//! - 启动 job 后，决定它是进入前台等待还是登记为后台作业。
//! - 用 waitpid 更新每个进程的状态，再聚合成整条 job 的状态。

use crate::signals::init_interactive_shell_signals;
use crate::types::{CommandStatus, Job, JobStatus, ProcessState, ShellResult, ShellState};
use nix::errno::Errno;
use nix::sys::signal::{Signal, killpg};
use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use nix::unistd::{Pid, getpgrp, getpid, setpgid, tcgetpgrp, tcsetpgrp};
use std::os::fd::BorrowedFd;

const REAP_FLAGS: WaitPidFlag = WaitPidFlag::WNOHANG
    .union(WaitPidFlag::WUNTRACED)
    .union(WaitPidFlag::WCONTINUED);
const FOREGROUND_WAIT_FLAGS: WaitPidFlag = WaitPidFlag::WUNTRACED.union(WaitPidFlag::WCONTINUED);

/// 交互模式下初始化 shell 自己的进程组和终端控制权。
pub fn init_shell_job_control(state: &mut ShellState) -> ShellResult<()> {
    if !state.interactive {
        return Ok(());
    }

    let shell_pid = getpid();

    if getpgrp() != shell_pid {
        setpgid(shell_pid, shell_pid)?;
    }

    let terminal = unsafe { BorrowedFd::borrow_raw(0) };
    if tcgetpgrp(terminal)? != shell_pid {
        tcsetpgrp(terminal, shell_pid)?;
    }

    init_interactive_shell_signals()?;
    state.shell_pgid = Some(shell_pid);
    state.shell_terminal_fd = Some(0);
    Ok(())
}

/// 主循环轮询后台 job 的状态变化。
pub fn reap_background_jobs(state: &mut ShellState) -> ShellResult<()> {
    loop {
        let result = waitpid(Pid::from_raw(-1), Some(REAP_FLAGS));

        match result {
            Ok(WaitStatus::StillAlive) => break,
            Ok(status) => handle_waitpid_status(state, status),
            Err(Errno::ECHILD) => break,
            Err(err) => return Err(err.into()),
        }
    }

    Ok(())
}

/// 在命令启动后接管作业生命周期。
pub(crate) fn finalize_launched_job(
    state: &mut ShellState,
    job: &mut Job,
    background: bool,
) -> ShellResult<CommandStatus> {
    if background {
        register_background_job(state, job);
        return Ok(CommandStatus::success());
    }

    wait_for_foreground_job(state, job)?;
    store_stopped_job(state, job);
    Ok(status_from_job(job))
}

/// 把一个 job 置为前台并等待到结束或暂停。
fn wait_for_foreground_job(state: &mut ShellState, job: &mut Job) -> ShellResult<()> {
    give_terminal_to_job(state, job.pgid)?;
    let wait_result = collect_child_statuses(job);
    let reclaim_result = reclaim_terminal_for_shell(state);
    wait_result?;
    reclaim_result
}

/// 阻塞等待指定前台进程组的状态变化。
fn collect_child_statuses(job: &mut Job) -> ShellResult<()> {
    loop {
        let result = waitpid(
            Pid::from_raw(-job.pgid.as_raw()),
            Some(FOREGROUND_WAIT_FLAGS),
        );

        match result {
            Ok(status) => {
                if let Some(pid) = wait_status_pid(&status) {
                    update_job_process(job, pid, status);
                }

                if is_job_wait_complete(job) {
                    return Ok(());
                }
            }
            Err(Errno::ECHILD) => {
                recompute_job_status(job);
                return Ok(());
            }
            Err(err) => return Err(err.into()),
        }
    }
}

/// 让一个已存在 job 在前台继续运行。
pub(crate) fn resume_job_in_foreground(state: &mut ShellState, job: &mut Job) -> ShellResult<()> {
    mark_job_as_running(job);
    killpg(job.pgid, Signal::SIGCONT)?;
    wait_for_foreground_job(state, job)
}

/// 让一个已停止 job 在后台继续运行。
pub(crate) fn continue_job(job: &mut Job) -> ShellResult<()> {
    mark_job_as_running(job);
    killpg(job.pgid, Signal::SIGCONT)?;
    Ok(())
}

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

/// fork 后父子两边都可能竞争 setpgid，EACCES 视为成功。
pub(crate) fn set_child_pgid(pid: Pid, pgid: Pid) -> nix::Result<()> {
    match setpgid(pid, pgid) {
        Ok(()) => Ok(()),
        Err(Errno::EACCES) => Ok(()),
        Err(err) => Err(err),
    }
}

fn mark_job_as_running(job: &mut Job) {
    for p in &mut job.processes {
        if p.state == ProcessState::Stopped {
            p.state = ProcessState::Running;
        }
    }
    job.status = JobStatus::Running;
}

fn register_background_job(state: &mut ShellState, job: &Job) {
    println!("[{}] {}", job.id, job.pgid);
    state.jobs.push(job.clone());
}

fn store_stopped_job(state: &mut ShellState, job: &Job) {
    if job.status == JobStatus::Stopped {
        state.jobs.push(job.clone());
    }
}

fn is_job_wait_complete(job: &Job) -> bool {
    matches!(job.status, JobStatus::Stopped | JobStatus::Done(_))
}

fn handle_waitpid_status(state: &mut ShellState, status: WaitStatus) {
    let Some(pid) = wait_status_pid(&status) else {
        return;
    };

    let Some(job) = state
        .jobs
        .iter_mut()
        .find(|job| job.processes.iter().any(|p| p.pid == pid))
    else {
        return;
    };

    update_job_process(job, pid, status);
}

fn give_terminal_to_job(state: &mut ShellState, pgid: Pid) -> ShellResult<()> {
    if !state.interactive {
        return Ok(());
    }
    let terminal = unsafe { BorrowedFd::borrow_raw(0) };
    tcsetpgrp(terminal, pgid)?;
    state.current_fg_pgid = Some(pgid);
    Ok(())
}

fn reclaim_terminal_for_shell(state: &mut ShellState) -> ShellResult<()> {
    if !state.interactive {
        state.current_fg_pgid = None;
        return Ok(());
    }

    let shell_pid = getpid();
    let terminal = unsafe { BorrowedFd::borrow_raw(0) };
    tcsetpgrp(terminal, shell_pid)?;
    state.current_fg_pgid = None;
    Ok(())
}

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
        _ => {}
    }

    recompute_job_status(job);
}

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

fn wait_status_pid(status: &WaitStatus) -> Option<Pid> {
    match *status {
        WaitStatus::Exited(pid, _)
        | WaitStatus::Signaled(pid, _, _)
        | WaitStatus::Stopped(pid, _)
        | WaitStatus::Continued(pid) => Some(pid),
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
            script_env: std::rc::Rc::new(crate::ecscript::env::Environment::new()),
            aliases: std::collections::HashMap::new(),
            traps: std::collections::HashMap::new(),
            command_history: Vec::new(),
            extensions: crate::extensions::new_extensions(),
            module_loader: None,
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
