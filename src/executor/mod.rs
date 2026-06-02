//! 执行入口：路由命令到 builtin / 外部命令 / pipeline。
//!
//! 这是 executor 模块的对外接口层，职责是"决策"，不是"执行"：
//!   - 判断命令是 builtin 还是外部命令
//!   - 判断 pipeline 是否合法
//!   - 把具体的 fork / waitpid / job 管理委托给子模块

mod builtins;
pub mod command_value;
mod expand;
mod jobs;
mod launch;

use crate::builtin::{BuiltinKind, builtin_kind, is_builtin_allowed_in_pipeline};
use crate::diagnostics::print_error;
use crate::extensions::{has_registered_command, run_registered_command};
use crate::types::{Command, CommandFlow, CommandStatus, Pipeline, ShellResult, ShellState};

pub use builtins::run_special_builtin;
pub use command_value::{capture_command_invocation, run_command_invocation};
pub use jobs::{init_shell_job_control, reap_background_jobs};

/// 执行一条命令（非管道）。
///
/// 路由逻辑：
///   - 是 builtin 且不是后台 → shell 进程内执行（如 cd / export），支持重定向
///   - 是 builtin 但是后台 → 报错（builtin 不能后台运行，因为它需要修改 shell 状态）
///   - 是外部命令 → fork 子进程，execvp 执行
pub fn run_command(
    command: &Command,
    state: &mut ShellState,
    background: bool,
    command_line: &str,
) -> ShellResult<CommandFlow> {
    let command = match expand::expand_command(command, state) {
        Ok(command) => command,
        Err(err) => {
            print_error(format!("expand: {}", err));
            return Ok(CommandFlow::Continue(CommandStatus::failure()));
        }
    };

    let flow = if let Some(kind) = builtin_kind(&command) {
        if background {
            // builtin 在后台子进程里执行无意义：
            // cd /tmp & → 子进程 cd 到 /tmp 后退出，shell 的工作目录不变
            // export FOO=bar & → 子进程设的环境变量父进程看不到
            print_error(format!(
                "{}: built-in command cannot run in the background",
                command.program
            ));
            CommandFlow::Continue(CommandStatus::failure())
        } else {
            match kind {
                BuiltinKind::Jobs | BuiltinKind::Fg | BuiltinKind::Bg => {
                    match builtins::run_special_builtin(&command, state)? {
                        Some(flow) => flow,
                        None => unreachable!("special builtin should have been handled"),
                    }
                }
                _ => match builtins::run_builtin_with_redirection(&command, state) {
                    Ok(flow) => flow,
                    Err(err) => {
                        print_error(format!("{}: {}", command.program, err));
                        CommandFlow::Continue(CommandStatus::failure())
                    }
                },
            }
        }
    } else if has_registered_command(state, command.program.as_lit_str().unwrap_or("")) {
        if background {
            print_error(format!(
                "{}: ecscript shell command cannot run in the background",
                command.program
            ));
            CommandFlow::Continue(CommandStatus::failure())
        } else if command.redirection.stdin.is_some() || command.redirection.stdout.is_some() {
            print_error(format!(
                "{}: redirection is not supported for ecscript shell commands",
                command.program
            ));
            CommandFlow::Continue(CommandStatus::failure())
        } else {
            let name = command.program.as_lit_str().unwrap_or("");
            let args = command
                .args
                .iter()
                .map(|arg| arg.as_lit_str().unwrap_or("").to_string())
                .collect();
            match run_registered_command(state, name, args) {
                Ok(Some(status)) => CommandFlow::Continue(status),
                Ok(None) => unreachable!("registered command disappeared during execution"),
                Err(err) => {
                    print_error(err.format_with_source(""));
                    CommandFlow::Continue(CommandStatus::failure())
                }
            }
        }
    } else {
        // 外部命令：fork → execvp，由 launch_command_job 处理前后台逻辑
        let status = match launch::launch_command_job(&command, state, background, command_line) {
            Ok(status) => status,
            Err(err) => {
                print_error(format!("{}: {}", command.program, err));
                CommandStatus::failure()
            }
        };
        CommandFlow::Continue(status)
    };

    Ok(flow)
}

/// 执行一条管道。
///
/// 在执行前做两次校验：
///   1. 管道内的 builtin 是否被允许（只允许 help/pwd/env/status 这类纯输出型）
///   2. 重定向位置是否合法（stdin 重定向只能在第一条命令，stdout 重定向只能在最后一条）
pub fn run_pipeline(
    pipeline: &Pipeline,
    state: &mut ShellState,
    background: bool,
    command_line: &str,
) -> ShellResult<CommandStatus> {
    if let Err(err) = validate_pipeline_redirection(pipeline) {
        print_error(format!("pipeline: {}", err));
        return Ok(CommandStatus::failure());
    }

    let expanded_pipeline = match pipeline
        .commands
        .iter()
        .map(|command| expand::expand_command(command, state))
        .collect::<ShellResult<Vec<_>>>()
    {
        Ok(commands) => Pipeline { commands },
        Err(err) => {
            print_error(format!("pipeline: expand: {}", err));
            return Ok(CommandStatus::failure());
        }
    };

    for command in &expanded_pipeline.commands {
        if has_registered_command(state, command.program.as_lit_str().unwrap_or("")) {
            print_error(format!(
                "pipeline: ecscript shell command is not supported in pipelines: {}",
                command.program
            ));
            return Ok(CommandStatus::failure());
        }
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

    let status = launch::launch_pipeline_job(&expanded_pipeline, state, background, command_line)?;
    Ok(status)
}

/// 校验管道重定向规则：
///   - stdin 重定向（<）只能在第一条命令上
///   - stdout 重定向（> / >>）只能在最后一条命令上
///
/// 中间命令的 stdin/stdout 已经被管道连接，再加重定向会互相覆盖，语义混乱。
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
