//! 需要访问 job 表和终端控制的特殊内置命令：jobs / fg / bg。
//!
//! 这些命令不能放在 `builtin.rs` 的 `run_builtin` 里处理，
//! 因为它们需要访问 `ShellState.jobs` 和终端前台控制，而这些是 executor 层的职责。
//! `run_builtin` 返回的 `BuiltinKind::Jobs/Fg/Bg` 返回 `None`，
//! 由本模块通过 `run_special_builtin` 接管执行。

use crate::builtin::{BuiltinKind, builtin_kind, run_builtin};
use crate::diagnostics::print_error;
use crate::executor::jobs::{
    continue_job, job_status_text, resume_job_in_foreground, status_from_job,
};
use crate::redirection::{apply_redirection_in_shell, flush_standard_streams, restore_redirection};
use crate::types::{Command, CommandFlow, CommandStatus, JobStatus, ShellResult, ShellState};

/// 执行入口：判断当前命令是不是 jobs/fg/bg，是则执行，不是则返回 None。
///
/// 返回 `Option<CommandFlow>`：
///   - Some(flow) → 这个内置命令已被处理，调用者不需要再走外部命令 fork 路径
///   - None       → 不是特殊内置命令，调用者继续走原来的 run_command 流程
///
/// 这个函数在前台命令路径的早期被调用（见 main.rs run_parsed_line），
/// 确保 `jobs` / `fg` / `bg` 在 shell 进程内执行，不会 fork 到子进程。
pub fn run_special_builtin(
    command: &Command,
    state: &mut ShellState,
) -> ShellResult<Option<CommandFlow>> {
    let Some(kind) = builtin_kind(command) else {
        return Ok(None);
    };

    let flow = match kind {
        BuiltinKind::Jobs => Some(run_jobs_builtin_with_redirection(command, state)?),
        BuiltinKind::Fg => Some(run_fg_builtin_with_redirection(command, state)?),
        BuiltinKind::Bg => Some(run_bg_builtin_with_redirection(command, state)?),
        _ => None,
    };

    Ok(flow)
}

/// 对普通 builtin（cd/export/unset 等）应用重定向后执行。
///
/// 为什么 builtin 的重定向由 executor 层处理，不是 builtin 层自己处理？
///   因为 builtin 在 shell 进程内运行，重定向会改变进程的 stdin/stdout，
///   必须 save → apply → run → flush → restore 五步走。
///   外部命令不需要这么麻烦——fork 后子进程直接 dup2 就行，子进程退出后自动恢复。
pub(crate) fn run_builtin_with_redirection(
    command: &Command,
    state: &mut ShellState,
) -> ShellResult<CommandFlow> {
    // 1. 保存当前 stdin/stdout，然后 dup2 到重定向目标。
    let saved = apply_redirection_in_shell(command)?;
    // 2. 执行内置命令（此时 println! 会写到重定向目标）。
    let result = run_builtin(command, state).expect("builtin command should have a builtin result");
    // 3. flush 缓冲区，确保输出已经写入重定向目标。
    flush_standard_streams()?;
    // 4. 把 stdin/stdout 恢复为原来的 fd。
    restore_redirection(saved)?;
    // 5. 返回内置命令的结果。

    Ok(result)
}

/// `jobs` 内置命令：列出所有后台和已暂停的 job。
///
/// 输出格式：`[job_id] Status command_line`
/// 例如：
///   [1] Running    sleep 100 &
///   [2] Stopped    cat
///
/// 输出完成后，清理所有已完成的 job（Done 状态的从作业表中移除）。
/// 这样 `jobs` 命令天然也有"回收已完成 job"的功能。
fn run_jobs_builtin_with_redirection(
    command: &Command,
    state: &mut ShellState,
) -> ShellResult<CommandFlow> {
    let saved = apply_redirection_in_shell(command)?;

    if !command.args.is_empty() {
        print_error("jobs: usage: jobs");
        flush_standard_streams()?;
        restore_redirection(saved)?;
        return Ok(CommandFlow::Continue(CommandStatus::failure()));
    }

    for job in &state.jobs {
        println!(
            "[{}] {} {}",
            job.id,
            job_status_text(job.status),
            job.command_line
        );
    }
    // 清理 Done 状态的 job。这些 job 已经全部进程退出，
    // 保留在表中没有意义（fg/bg 只对 Stopped/Running 有意义）。
    state
        .jobs
        .retain(|job| !matches!(job.status, JobStatus::Done(_)));

    flush_standard_streams()?;
    restore_redirection(saved)?;
    Ok(CommandFlow::Continue(CommandStatus::success()))
}

/// `fg %N` 内置命令：将 job N 拉到前台。
///
/// 步骤：
///   1. 解析 `%N` 参数，找到对应 job
///   2. 从作业表中移除该 job
///   3. 调用 resume_job_in_foreground：tcsetpgrp → SIGCONT → waitpid 等待
///      （如果 job 是 Running 态，resume_job_in_foreground 也会正确等待）
///   4. 如果 job 最终又是 Stopped（又按了一次 Ctrl-Z），放回作业表
///
/// 不传 `%N` 参数时默认取"当前 job"（最后被暂停的那个），但当前实现未支持。
fn run_fg_builtin_with_redirection(
    command: &Command,
    state: &mut ShellState,
) -> ShellResult<CommandFlow> {
    let saved = apply_redirection_in_shell(command)?;

    let status = match parse_job_spec(command, "fg") {
        Ok(job_id) => match state.jobs.iter().position(|job| job.id == job_id) {
            Some(index) => {
                let mut job = state.jobs.remove(index);
                // 将终端控制权交给 job 的进程组，发 SIGCONT，进入前台等待循环。
                resume_job_in_foreground(state, &mut job)?;
                let status = status_from_job(&job);
                // 如果 job 在前台又被 Ctrl-Z 暂停了，放回作业表。
                if matches!(job.status, JobStatus::Stopped) {
                    state.jobs.push(job);
                }
                status
            }
            None => {
                print_error(format!("fg: no such job: %{}", job_id));
                CommandStatus::failure()
            }
        },
        Err(err) => {
            print_error(err);
            CommandStatus::failure()
        }
    };

    flush_standard_streams()?;
    restore_redirection(saved)?;
    Ok(CommandFlow::Continue(status))
}

/// `bg %N` 内置命令：将已停止的 job N 放到后台继续执行。
///
/// 只对 Stopped 状态的 job 有效（Running 的 job 本来就在后台跑）。
///
/// 和 fg 的关键区别：
///   - fg：tcsetpgrp 拿走终端控制权 → SIGCONT → 阻塞 waitpid
///   - bg：直接 SIGCONT → 打印确认信息 → 返回（不碰终端，不等待）
fn run_bg_builtin_with_redirection(
    command: &Command,
    state: &mut ShellState,
) -> ShellResult<CommandFlow> {
    let saved = apply_redirection_in_shell(command)?;

    let status = match parse_job_spec(command, "bg") {
        Ok(job_id) => match state.jobs.iter_mut().find(|job| job.id == job_id) {
            Some(job) => {
                if !matches!(job.status, JobStatus::Stopped) {
                    print_error(format!("bg: job %{} is not stopped", job_id));
                    CommandStatus::failure()
                } else {
                    // continue_job：本地状态切 Running → killpg(SIGCONT) → 返回。
                    // shell 不等待，后续主循环通过 reap_background_jobs 异步回收。
                    continue_job(job)?;
                    println!("[{}] {}", job.id, job.command_line);
                    CommandStatus::success()
                }
            }
            None => {
                print_error(format!("bg: no such job: %{}", job_id));
                CommandStatus::failure()
            }
        },
        Err(err) => {
            print_error(err);
            CommandStatus::failure()
        }
    };

    flush_standard_streams()?;
    restore_redirection(saved)?;
    Ok(CommandFlow::Continue(status))
}

/// 解析 `fg %N` / `bg %N` 中的 `%N` 部分。
///
/// 格式要求：`fg %1` 或 `bg %2`，参数必须以 `%` 开头后跟数字。
fn parse_job_spec(command: &Command, builtin_name: &str) -> Result<usize, String> {
    if command.args.len() != 1 {
        return Err(format!("{}: usage: {} %N", builtin_name, builtin_name));
    }

    let arg = &command.args[0];
    let arg_str = arg.as_lit_str().unwrap_or("");
    let Some(job_id) = arg_str.strip_prefix('%') else {
        return Err(format!("{}: usage: {} %N", builtin_name, builtin_name));
    };

    job_id
        .parse::<usize>()
        .map_err(|_| format!("{}: invalid job specifier: {}", builtin_name, arg))
}

// ── 测试 ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Redirection, ShellWord, WordFragment};

    /// 构造一个只含程序名和参数的 Command，用于测试 parse_job_spec。
    fn cmd(program: &str, args: &[&str]) -> Command {
        Command {
            program: ShellWord {
                fragments: vec![WordFragment::Lit(program.to_string())],
            },
            args: args
                .iter()
                .map(|a| ShellWord {
                    fragments: vec![WordFragment::Lit(a.to_string())],
                })
                .collect(),
            redirection: Redirection::default(),
        }
    }

    #[test]
    fn parse_job_spec_valid() {
        assert_eq!(parse_job_spec(&cmd("fg", &["%1"]), "fg"), Ok(1));
        assert_eq!(parse_job_spec(&cmd("bg", &["%42"]), "bg"), Ok(42));
    }

    #[test]
    fn parse_job_spec_missing_percent() {
        assert!(parse_job_spec(&cmd("fg", &["1"]), "fg").is_err());
        assert!(parse_job_spec(&cmd("bg", &["abc"]), "bg").is_err());
    }

    #[test]
    fn parse_job_spec_no_args() {
        assert!(parse_job_spec(&cmd("fg", &[]), "fg").is_err());
        assert!(parse_job_spec(&cmd("bg", &[]), "bg").is_err());
    }

    #[test]
    fn parse_job_spec_too_many_args() {
        assert!(parse_job_spec(&cmd("fg", &["%1", "%2"]), "fg").is_err());
    }

    #[test]
    fn parse_job_spec_non_numeric() {
        assert!(parse_job_spec(&cmd("fg", &["%abc"]), "fg").is_err());
        assert!(parse_job_spec(&cmd("fg", &["%"]), "fg").is_err());
    }
}
