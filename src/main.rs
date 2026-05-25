//! ecsh 入口：主循环、续行读取和顶层执行分派。

use ecsh::diagnostics::print_error;
use ecsh::ecscript::env::Environment;
use ecsh::executor::{init_shell_job_control, reap_background_jobs, run_command, run_pipeline};
use ecsh::input::{InputLine, ShellInput};
use ecsh::parser::parse_line;
use ecsh::prompt::build_prompt;
use ecsh::shell_error::format_shell_parse_error;
use ecsh::types::{CommandFlow, CommandStatus, ParsedLine, ShellState};
use std::collections::HashMap;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    main_loop()
}

/// 初始化输入与全局状态，然后进入交互主循环。
fn main_loop() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = ShellInput::new()?;
    let mut state = new_shell_state(input.is_interactive());
    init_shell_job_control(&mut state)?;
    input.print_welcome();

    loop {
        reap_background_jobs(&mut state)?;

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

        if line.is_empty() {
            state.last_status = CommandStatus::success();
            continue;
        }
        state.command_history.push(line.to_string());
        input.add_history_entry(line);

        let parsed = match parse_line(line, &state) {
            Ok(parsed) => parsed,
            Err(err) => {
                print_error(format_shell_parse_error(line, &err));
                state.last_status = CommandStatus::failure();
                continue;
            }
        };

        match run_parsed_line(
            &parsed.line,
            parsed.background,
            &parsed.command_line,
            &mut state,
        )? {
            CommandFlow::Continue(current_status) => {
                state.last_status = current_status;
            }
            CommandFlow::Exit(_current_status) => break,
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

/// 构造 shell 运行时的初始状态。
fn new_shell_state(interactive: bool) -> ShellState {
    ShellState {
        last_status: CommandStatus::success(),
        interactive,
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

/// 读取一条完整命令。
///
/// 当 lexer/parser 报 incomplete 时，继续用 `... ` 提示符读续行；
/// 当输入在 EOF 结束时，保留当前缓冲并交给后续解析阶段统一报错。
fn read_complete_command(
    input: &mut ShellInput,
    state: &ShellState,
) -> Result<ReadCommand, Box<dyn std::error::Error>> {
    let prompt = build_prompt(state)?;
    let mut buffer = String::new();

    loop {
        // 第一行使用完整 prompt，续行统一切到简短提示符。
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

        // 续行按真实换行拼回去，保持后续 parse/error 的源码位置一致。
        if !buffer.is_empty() {
            buffer.push('\n');
        }
        buffer.push_str(line.trim_end_matches('\n'));

        match parse_line(&buffer, state) {
            Ok(_) => return Ok(ReadCommand::Line(buffer)),
            // incomplete 表示当前行还可能合法，继续读下一行。
            Err(err) if err.incomplete => {}
            // 普通 parse error 留给主循环统一格式化和输出。
            Err(_) => return Ok(ReadCommand::Line(buffer)),
        }
    }
}

/// 执行指定 trap 中保存的一段 shell 命令。
///
/// trap 命令仍然走正常的 parse/execute 路径，只是输入来源变成了 trap 表。
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
    let flow = run_parsed_line(&parsed.line, parsed.background, &parsed.command_line, state)?;
    Ok(Some(flow))
}

#[cfg(test)]
mod tests {
    use super::{ReadCommand, read_complete_command};
    use ecsh::input::{InputLine, ShellInput};
    use ecsh::types::ShellState;

    fn state() -> ShellState {
        super::new_shell_state(true)
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

/// 根据顶层 AST 递归分派执行。
///
/// 最外层的 `background` 只作用于完整命令或完整管道；
/// 递归进入 `&& / || / ;` 的左右子树时，不继承这层 `&` 语义。
fn run_parsed_line(
    line: &ParsedLine,
    background: bool,
    command_line: &str,
    state: &mut ShellState,
) -> Result<CommandFlow, Box<dyn std::error::Error>> {
    match line {
        // 单条命令交给 executor 做展开、builtin 判定和外部命令启动。
        ParsedLine::Command(command) => run_command(command, state, background, command_line),
        // 管道是单独的执行路径，但返回值仍然折叠成顶层 CommandFlow。
        ParsedLine::Pipeline(pipeline) => Ok(CommandFlow::Continue(run_pipeline(
            pipeline,
            state,
            background,
            command_line,
        )?)),
        ParsedLine::AndThen(left, right) => {
            run_with_condition(left, right, command_line, state, |status| status.code == 0)
        }
        ParsedLine::OrElse(left, right) => {
            run_with_condition(left, right, command_line, state, |status| status.code != 0)
        }
        ParsedLine::Sequence(left, right) => run_sequence(left, right, command_line, state),
    }
}

/// 复用 `&&` / `||` 的条件分派骨架。
fn run_with_condition(
    left: &ParsedLine,
    right: &ParsedLine,
    command_line: &str,
    state: &mut ShellState,
    predicate: impl Fn(CommandStatus) -> bool,
) -> Result<CommandFlow, Box<dyn std::error::Error>> {
    match run_parsed_line(left, false, command_line, state)? {
        CommandFlow::Exit(status) => Ok(CommandFlow::Exit(status)),
        CommandFlow::Continue(status) => {
            if predicate(status) {
                state.last_status = status;
                run_parsed_line(right, false, command_line, state)
            } else {
                Ok(CommandFlow::Continue(status))
            }
        }
    }
}

/// 执行 `left ; right`。
///
/// 左侧只要不是 `exit`，右侧都会继续执行。
fn run_sequence(
    left: &ParsedLine,
    right: &ParsedLine,
    command_line: &str,
    state: &mut ShellState,
) -> Result<CommandFlow, Box<dyn std::error::Error>> {
    match run_parsed_line(left, false, command_line, state)? {
        CommandFlow::Exit(status) => Ok(CommandFlow::Exit(status)),
        CommandFlow::Continue(status) => {
            state.last_status = status;
            run_parsed_line(right, false, command_line, state)
        }
    }
}
