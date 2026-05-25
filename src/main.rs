//! ecsh 入口：主循环和 AST 执行调度。
//!
//! 主循环的每一轮迭代：
//!   1. reap_background_jobs  — 非阻塞回收后台子进程
//!   2. build_prompt          — 生成带颜色的提示符
//!   3. read_complete_command — 等待用户输入，并在 incomplete 时继续续行
//!   4. parse_line            — 词法 + 语法分析
//!   5. run_parsed_line       — 根据 AST 类型分派执行

use ecsh::diagnostics::print_error;
use ecsh::ecscript::env::Environment;
use ecsh::executor::{init_shell_job_control, reap_background_jobs, run_command, run_pipeline};
use ecsh::input::{InputLine, ShellInput};
use ecsh::parser::parse_line;
use ecsh::prompt::build_prompt;
use ecsh::shell_error::format_shell_parse_error;
use ecsh::types::{CommandFlow, CommandStatus, ParsedJob, ParsedLine, ShellState};
use std::collections::HashMap;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    main_loop()
}

/// shell 主循环。
///
/// 初始化 → 进入 REPL (Read-Eval-Print Loop) → 退出时保存历史。
fn main_loop() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = ShellInput::new()?;
    let mut state = ShellState {
        last_status: CommandStatus::success(),
        interactive: input.is_interactive(),
        shell_pgid: None,
        shell_terminal_fd: None,
        jobs: Vec::new(),
        next_job_id: 1,
        current_fg_pgid: None,
        script_env: Environment::new(),
        aliases: HashMap::new(),
        traps: HashMap::new(),
        command_history: Vec::new(),
    };
    // 交互模式下初始化 job control（设进程组、抢终端、忽略信号）。
    init_shell_job_control(&mut state)?;
    input.print_welcome();

    loop {
        // 1. 非阻塞回收已结束的后台子进程。
        reap_background_jobs(&mut state)?;

        // 2. 读取一条完整命令；若 parser/lexer 判定 incomplete，则继续读续行。
        let line = match read_complete_command(&mut input, &state)? {
            ReadCommand::Line(line) => line,
            ReadCommand::Interrupted => {
                if let Some(flow) = run_named_trap("INT", &mut state)? {
                    if matches!(flow, CommandFlow::Exit(_)) {
                        break;
                    }
                }
                state.last_status = CommandStatus::new(130);
                continue;
            }
            ReadCommand::Eof => break,
        };
        let line = line.trim();

        // 3. 空输入，重置状态码后直接下一轮。
        if line.is_empty() {
            state.last_status = CommandStatus::success();
            continue;
        }
        state.command_history.push(line.to_string());
        input.add_history_entry(line);

        // 4. 词法 + 语法分析。
        let parsed = match parse_line(line, &state) {
            Ok(parsed) => parsed,
            Err(err) => {
                print_error(format_shell_parse_error(line, &err));
                state.last_status = CommandStatus::failure();
                continue;
            }
        };

        // 5. 根据 AST 类型分派执行。
        match run_parsed_line(&parsed, &mut state)? {
            CommandFlow::Continue(current_status) => {
                state.last_status = current_status;
            }
            CommandFlow::Exit(_current_status) => {
                break; // exit 命令
            }
        }
    }

    let _ = run_named_trap("EXIT", &mut state)?;
    input.save_history();
    Ok(())
}

enum ReadCommand {
    Line(String),
    Interrupted,
    Eof,
}

fn read_complete_command(
    input: &mut ShellInput,
    state: &ShellState,
) -> Result<ReadCommand, Box<dyn std::error::Error>> {
    let prompt = build_prompt(state)?;
    let mut buffer = String::new();

    loop {
        let current_prompt = if buffer.is_empty() { &prompt } else { "... " };
        let line = match input.read_line(current_prompt)? {
            InputLine::Line(line) => line,
            InputLine::Interrupted => return Ok(ReadCommand::Interrupted),
            InputLine::Eof => {
                return Ok(if buffer.is_empty() {
                    ReadCommand::Eof
                } else {
                    ReadCommand::Line(buffer)
                });
            }
        };

        if !buffer.is_empty() {
            buffer.push('\n');
        }
        buffer.push_str(line.trim_end_matches('\n'));

        match parse_line(&buffer, state) {
            Ok(_) => return Ok(ReadCommand::Line(buffer)),
            Err(err) if err.incomplete => {}
            Err(_) => return Ok(ReadCommand::Line(buffer)),
        }
    }
}

fn run_named_trap(
    name: &str,
    state: &mut ShellState,
) -> Result<Option<CommandFlow>, Box<dyn std::error::Error>> {
    let Some(command_line) = state.traps.get(name).cloned() else {
        return Ok(None);
    };
    let parsed = match parse_line(&command_line, state) {
        Ok(parsed) => parsed,
        Err(err) => {
            print_error(format_shell_parse_error(&command_line, &err));
            state.last_status = CommandStatus::failure();
            return Ok(Some(CommandFlow::Continue(CommandStatus::failure())));
        }
    };
    let flow = run_parsed_line(&parsed, state)?;
    Ok(Some(flow))
}

#[cfg(test)]
mod tests {
    use super::{ReadCommand, read_complete_command};
    use ecsh::ecscript::env::Environment;
    use ecsh::input::{InputLine, ShellInput};
    use ecsh::types::{CommandStatus, ShellState};
    use std::collections::HashMap;

    fn state() -> ShellState {
        ShellState {
            last_status: CommandStatus::success(),
            interactive: true,
            shell_pgid: None,
            shell_terminal_fd: None,
            jobs: Vec::new(),
            next_job_id: 1,
            current_fg_pgid: None,
            script_env: Environment::new(),
            aliases: HashMap::new(),
            traps: HashMap::new(),
            command_history: Vec::new(),
        }
    }

    #[test]
    fn read_complete_command_requests_continuation_for_braced_envvar() {
        let mut input =
            ShellInput::scripted([InputLine::Line("echo ${HOME".into()), InputLine::Eof]);
        let result = read_complete_command(&mut input, &state()).unwrap();

        match result {
            ReadCommand::Line(buffer) => assert_eq!(buffer, "echo ${HOME"),
            _ => panic!("expected buffered line on EOF after incomplete envvar"),
        }

        assert_eq!(input.recorded_prompts().len(), 2);
        assert_eq!(input.recorded_prompts()[1], "... ");
    }

    #[test]
    fn read_complete_command_interrupts_during_continuation() {
        let mut input = ShellInput::scripted([
            InputLine::Line("echo \"unterminated".into()),
            InputLine::Interrupted,
        ]);
        let result = read_complete_command(&mut input, &state()).unwrap();

        match result {
            ReadCommand::Interrupted => {}
            _ => panic!("expected interruption during continuation"),
        }

        assert_eq!(input.recorded_prompts().len(), 2);
        assert_eq!(input.recorded_prompts()[1], "... ");
    }
}

/// 根据解析出的语法结构分派到不同的执行路径。
///
/// 递归处理控制流操作符（&& / || / ;）：
///   - `&&`：左侧执行；成功(code=0)才执行右侧
///   - `||`：左侧执行；失败(code≠0)才执行右侧
///   - `;`：左侧执行；无论成败都执行右侧（exit 除外）
///
/// 每个操作符递归时会构造一个新的 ParsedJob（强制 foreground=false），
/// 这是因为 `&&` / `||` 的左右侧不应该继承最外层的 `&` 语义。
fn run_parsed_line(
    parsed: &ParsedJob,
    state: &mut ShellState,
) -> Result<CommandFlow, Box<dyn std::error::Error>> {
    match &parsed.line {
        // ── 单条命令：交给 executor 统一做展开 / builtin / external 分派 ──
        ParsedLine::Command(command) => {
            run_command(command, state, parsed.background, &parsed.command_line)
        }

        // ── 管道：直接派发 ──
        ParsedLine::Pipeline(pipeline) => Ok(CommandFlow::Continue(run_pipeline(
            pipeline,
            state,
            parsed.background,
            &parsed.command_line,
        )?)),

        // ── `&&`：左侧成功才执行右侧；左侧 exit 则透传 ──
        ParsedLine::AndThen(left, right) => match run_parsed_line(
            &ParsedJob {
                line: left.as_ref().clone(),
                background: false,
                command_line: parsed.command_line.clone(),
            },
            state,
        )? {
            CommandFlow::Exit(status) => Ok(CommandFlow::Exit(status)),
            CommandFlow::Continue(status) => {
                if status.code == 0 {
                    state.last_status = status;
                    run_parsed_line(
                        &ParsedJob {
                            line: right.as_ref().clone(),
                            background: false,
                            command_line: parsed.command_line.clone(),
                        },
                        state,
                    )
                } else {
                    Ok(CommandFlow::Continue(status))
                }
            }
        },

        // ── `||`：左侧失败才执行右侧；左侧 exit 则透传 ──
        ParsedLine::OrElse(left, right) => match run_parsed_line(
            &ParsedJob {
                line: left.as_ref().clone(),
                background: false,
                command_line: parsed.command_line.clone(),
            },
            state,
        )? {
            CommandFlow::Exit(status) => Ok(CommandFlow::Exit(status)),
            CommandFlow::Continue(status) => {
                if status.code != 0 {
                    state.last_status = status;
                    run_parsed_line(
                        &ParsedJob {
                            line: right.as_ref().clone(),
                            background: false,
                            command_line: parsed.command_line.clone(),
                        },
                        state,
                    )
                } else {
                    Ok(CommandFlow::Continue(status))
                }
            }
        },

        // ── `;`：左侧执行完始终执行右侧（exit 除外）──
        ParsedLine::Sequence(left, right) => match run_parsed_line(
            &ParsedJob {
                line: left.as_ref().clone(),
                background: false,
                command_line: parsed.command_line.clone(),
            },
            state,
        )? {
            CommandFlow::Exit(status) => Ok(CommandFlow::Exit(status)),
            CommandFlow::Continue(status) => {
                state.last_status = status;
                run_parsed_line(
                    &ParsedJob {
                        line: right.as_ref().clone(),
                        background: false,
                        command_line: parsed.command_line.clone(),
                    },
                    state,
                )
            }
        },
    }
}
