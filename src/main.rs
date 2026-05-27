//! ecsh 入口：主循环、续行读取和顶层执行分派。

use ecsh::diagnostics::print_error;
use ecsh::ecscript::{
    Environment, ParseError, RuntimeError, RuntimeErrorKind, Stmt, Value, display_value,
    eval_top_level_script_with_ctx, parse_top_level_script, repl_output_needs_newline,
    reset_repl_output_state, run_script_file as run_ecscript_file,
};
use ecsh::executor::{init_shell_job_control, reap_background_jobs, run_command, run_pipeline};
use ecsh::input::{InputLine, ShellInput};
use ecsh::parser::parse_line;
use ecsh::prompt::build_prompt;
use ecsh::shell_error::format_shell_parse_error;
use ecsh::types::{CommandFlow, CommandStatus, ParsedJob, ParsedLine, ShellState};
use std::collections::HashMap;
use std::{env, path::Path};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    match script_file_arg()? {
        Some(path) => run_script_file(&path),
        None => main_loop(),
    }
}

fn script_file_arg() -> Result<Option<String>, Box<dyn std::error::Error>> {
    let mut args = env::args();
    let _program = args.next();
    let Some(path) = args.next() else {
        return Ok(None);
    };
    if let Some(extra) = args.next() {
        return Err(format!(
            "unexpected extra argument '{}'; ecsh currently accepts at most one script path",
            extra
        )
        .into());
    }
    Ok(Some(path))
}

fn run_script_file(path: impl AsRef<Path>) -> Result<(), Box<dyn std::error::Error>> {
    let env = Environment::new();

    match run_ecscript_file(path, &env) {
        Ok(()) => Ok(()),
        Err(err) => {
            print_error(err.format_for_user());
            Ok(())
        }
    }
}

/// 初始化输入与全局状态，然后进入交互主循环。
fn main_loop() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = ShellInput::new()?;
    let mut state = new_shell_state(input.is_interactive());
    init_shell_job_control(&mut state)?;
    load_startup_rc(&mut state);
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

        let input = match dispatch_input(line, &state) {
            Ok(input) => input,
            Err(TopLevelError::Shell(err)) => {
                print_error(format_shell_parse_error(line, &err));
                state.last_status = CommandStatus::failure();
                continue;
            }
            Err(TopLevelError::EcscriptParse(err)) => {
                print_error(err.format_with_source(line));
                state.last_status = CommandStatus::failure();
                continue;
            }
        };

        match run_top_level_input(input, &mut state) {
            Ok(CommandFlow::Continue(current_status)) => {
                state.last_status = current_status;
            }
            Ok(CommandFlow::Exit(_current_status)) => break,
            Err(err) => {
                if repl_output_needs_newline() {
                    println!();
                }
                print_error(err.format_with_source(line));
                state.last_status = CommandStatus::failure();
                continue;
            }
        }
    }

    let _ = run_named_trap("EXIT", &mut state)?;
    input.save_history();
    Ok(())
}

fn load_startup_rc(state: &mut ShellState) {
    if !state.interactive {
        return;
    }

    let Some(home) = env::var_os("HOME") else {
        return;
    };
    let path = Path::new(&home).join(".ecshrc");
    if !path.exists() {
        return;
    }

    reset_repl_output_state();
    match run_ecscript_file(&path, &state.script_env) {
        Ok(()) => {
            if repl_output_needs_newline() {
                println!();
            }
        }
        Err(err) => {
            if repl_output_needs_newline() {
                println!();
            }
            print_error(err.format_for_user());
            state.last_status = CommandStatus::failure();
        }
    }
}

enum ReadCommand {
    Line(String),
    Interrupted,
    Eof,
}

enum TopLevelInput {
    Shell(ParsedJob),
    Ecscript(Vec<Stmt>),
}

enum TopLevelError {
    Shell(ParseError),
    EcscriptParse(ParseError),
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

        match dispatch_input(&buffer, state) {
            Ok(_) => return Ok(ReadCommand::Line(buffer)),
            // incomplete 表示当前行还可能合法，继续读下一行。
            Err(TopLevelError::Shell(err)) if err.incomplete => {}
            Err(TopLevelError::EcscriptParse(err)) if err.incomplete => {}
            // 顶层脚本块里已经出现 parse error 时，若 `{}` 仍未闭合，继续把后续行并入同一段源码，
            // 避免把孤立的 `}` 当成下一条 shell 命令执行。
            Err(TopLevelError::EcscriptParse(_)) if ecscript_block_still_open(&buffer) => {}
            // 普通 parse error 留给主循环统一格式化和输出。
            Err(_) => return Ok(ReadCommand::Line(buffer)),
        }
    }
}

fn ecscript_block_still_open(src: &str) -> bool {
    enum ScanState {
        Normal,
        SingleQuoted,
        DoubleQuoted,
    }

    let mut state = ScanState::Normal;
    let mut brace_depth = 0usize;
    let mut chars = src.chars();

    while let Some(ch) = chars.next() {
        match state {
            ScanState::Normal => match ch {
                '{' => brace_depth += 1,
                '}' => brace_depth = brace_depth.saturating_sub(1),
                '\'' => state = ScanState::SingleQuoted,
                '"' => state = ScanState::DoubleQuoted,
                '\\' => {
                    let _ = chars.next();
                }
                _ => {}
            },
            ScanState::SingleQuoted => {
                if ch == '\'' {
                    state = ScanState::Normal;
                }
            }
            ScanState::DoubleQuoted => match ch {
                '"' => state = ScanState::Normal,
                '\\' => {
                    let _ = chars.next();
                }
                _ => {}
            },
        }
    }

    brace_depth > 0
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

/// 把读取的一行字符串分配到ecscript或者ecsh解析
fn dispatch_input(line: &str, state: &ShellState) -> Result<TopLevelInput, TopLevelError> {
    if let Some(result) = parse_top_level_script(line) {
        let stmts = result.map_err(TopLevelError::EcscriptParse)?;
        Ok(TopLevelInput::Ecscript(stmts))
    } else {
        let parsed = parse_line(line, state).map_err(TopLevelError::Shell)?;
        Ok(TopLevelInput::Shell(parsed))
    }
}

/// 分派到两种路径中运行
fn run_top_level_input(
    input: TopLevelInput,
    state: &mut ShellState,
) -> Result<CommandFlow, RuntimeError> {
    match input {
        TopLevelInput::Shell(parsed) => {
            run_parsed_line(&parsed.line, parsed.background, &parsed.command_line, state)
                .map_err(|err| RuntimeError::new(0, RuntimeErrorKind::IoError, err.to_string()))
        }
        TopLevelInput::Ecscript(stmts) => {
            reset_repl_output_state();
            if let Some(value) =
                eval_top_level_script_with_ctx(&stmts, &state.script_env, Some(&*state))?
                && !matches!(value, Value::Nil)
            {
                println!("{}", display_value(&value));
            } else if repl_output_needs_newline() {
                println!();
            }
            Ok(CommandFlow::Continue(CommandStatus::success()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ReadCommand, load_startup_rc, read_complete_command};
    use ecsh::ecscript::Value;
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

    #[test]
    fn read_complete_command_accepts_ecscript_without_semicolon() {
        let mut input = ShellInput::scripted([InputLine::Line("let x = 1".into())]);
        let result = read_complete_command(&mut input, &state()).unwrap();

        match result {
            ReadCommand::Line(buffer) => assert_eq!(buffer, "let x = 1"),
            _ => panic!("expected buffered ecscript input"),
        }

        assert_eq!(input.recorded_prompts().len(), 1);
    }

    #[test]
    fn load_startup_rc_populates_interactive_script_env() {
        let home = std::env::temp_dir().join(format!("ecsh-{}-rc-home", std::process::id()));
        std::fs::create_dir_all(&home).unwrap();
        let rc_path = home.join(".ecshrc");
        std::fs::write(&rc_path, "let greeting = \"rc-ok\"\n").unwrap();

        let old_home = std::env::var_os("HOME");
        unsafe { std::env::set_var("HOME", &home) };

        let mut shell = state();
        load_startup_rc(&mut shell);

        let greeting = shell.script_env.get("greeting", 0).unwrap();
        assert_eq!(greeting, Value::String("rc-ok".to_string()));

        match old_home {
            Some(value) => unsafe { std::env::set_var("HOME", value) },
            None => unsafe { std::env::remove_var("HOME") },
        }

        let _ = std::fs::remove_file(rc_path);
        let _ = std::fs::remove_dir(home);
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
