//! `jobs` / `fg` / `bg` 这类依赖 job table 和终端控制的特殊内置命令。

use crate::builtin::{BuiltinKind, builtin_kind, run_builtin};
use crate::diagnostics::print_error;
use crate::executor::jobs::{
    continue_job, job_status_text, resume_job_in_foreground, status_from_job,
};
use crate::redirection::{apply_redirection_in_shell, flush_standard_streams, restore_redirection};
use crate::types::{Command, CommandFlow, CommandStatus, JobStatus, ShellResult, ShellState};

/// `jobs` / `fg` / `bg` 在 shell 进程内执行，外层不再进入 fork 路径。
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

/// 对 shell 内执行的 builtin 临时套用重定向。
fn with_builtin_redirection<T>(
    command: &Command,
    f: impl FnOnce() -> ShellResult<T>,
) -> ShellResult<T> {
    let saved = apply_redirection_in_shell(command)?;
    let result = f();
    let flush_result = flush_standard_streams();
    let restore_result = restore_redirection(saved);

    flush_result?;
    restore_result?;
    result
}

/// 对普通 builtin（cd/export/unset 等）应用重定向后执行。
pub(crate) fn run_builtin_with_redirection(
    command: &Command,
    state: &mut ShellState,
) -> ShellResult<CommandFlow> {
    with_builtin_redirection(command, || {
        Ok(run_builtin(command, state).expect("builtin command should have a builtin result"))
    })
}

fn run_jobs_builtin_with_redirection(
    command: &Command,
    state: &mut ShellState,
) -> ShellResult<CommandFlow> {
    with_builtin_redirection(command, || {
        if !command.args.is_empty() {
            print_error("jobs: usage: jobs");
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
        state
            .jobs
            .retain(|job| !matches!(job.status, JobStatus::Done(_)));
        Ok(CommandFlow::Continue(CommandStatus::success()))
    })
}

fn run_fg_builtin_with_redirection(
    command: &Command,
    state: &mut ShellState,
) -> ShellResult<CommandFlow> {
    with_builtin_redirection(command, || {
        let status = match parse_job_spec(command, "fg") {
            Ok(job_id) => match state.jobs.iter().position(|job| job.id == job_id) {
                Some(index) => {
                    let mut job = state.jobs.remove(index);
                    resume_job_in_foreground(state, &mut job)?;
                    let status = status_from_job(&job);
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

        Ok(CommandFlow::Continue(status))
    })
}

fn run_bg_builtin_with_redirection(
    command: &Command,
    state: &mut ShellState,
) -> ShellResult<CommandFlow> {
    with_builtin_redirection(command, || {
        let status = match parse_job_spec(command, "bg") {
            Ok(job_id) => match state.jobs.iter_mut().find(|job| job.id == job_id) {
                Some(job) => {
                    if !matches!(job.status, JobStatus::Stopped) {
                        print_error(format!("bg: job %{} is not stopped", job_id));
                        CommandStatus::failure()
                    } else {
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

        Ok(CommandFlow::Continue(status))
    })
}

/// 解析 `fg %N` / `bg %N` 的 `%N` 参数。
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
