//! ecsh 入口：主循环、续行读取和顶层执行分派。

use ecsh::diagnostics::print_error;
use ecsh::ecscript::{
    Environment, ModuleLoader, ParseError, RuntimeError, RuntimeErrorKind, Stmt, Value,
    display_value, eval_top_level_script_with_ctx, parse_top_level_script,
    repl_output_needs_newline, reset_repl_output_state,
    run_script_file_with_ctx as run_ecscript_file_with_ctx,
    run_script_file_with_stdin as run_ecscript_file_with_stdin,
};
use ecsh::executor::{init_shell_job_control, reap_background_jobs, run_command, run_pipeline};
use ecsh::extensions::{
    HookName, before_prompt_context, new_extensions, postexec_context, preexec_context,
    resolve_prompt, run_hooks,
};
use ecsh::input::{InputLine, ShellInput};
use ecsh::parser::parse_line;
use ecsh::prompt::build_prompt;
use ecsh::shell_error::format_shell_parse_error;
use ecsh::types::{CommandFlow, CommandStatus, ParsedJob, ParsedLine, ShellState};
use std::collections::HashMap;
use std::{
    env,
    io::{self, IsTerminal, Read},
    path::Path,
    rc::Rc,
    time::Instant,
};

/// 入口：有文件参数走脚本执行，否则进入交互 REPL。
fn main() -> Result<(), Box<dyn std::error::Error>> {
    match script_file_arg()? {
        Some(path) => run_script_file(&path),
        None => main_loop(),
    }
}

/// 解析命令行参数，返回可选的脚本文件路径。
///
/// 只接受至多一个参数作为脚本路径，多余参数直接报错。
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

/// 以文件模式执行 `.ecs` 脚本。
///
/// 若 stdin 不是终端，会提前将其内容快照下来供脚本内的 `stdin()` 等函数消费。
fn run_script_file(path: impl AsRef<Path>) -> Result<(), Box<dyn std::error::Error>> {
    let env = Rc::new(Environment::new());
    let stdin_text = read_optional_stdin_text()?;

    match run_ecscript_file_with_stdin(path, &env, stdin_text.as_deref()) {
        Ok(()) => Ok(()),
        Err(err) => {
            print_error(err.format_for_user());
            Ok(())
        }
    }
}

/// 文件模式下，如果当前进程 stdin 不是终端，就一次性读成文本快照。
///
/// 这样 `.ecs` 脚本里的 `stdin()` / `read_lines()` 可以消费这份输入，
/// 同时不会影响交互 REPL 的输入循环。
fn read_optional_stdin_text() -> Result<Option<String>, Box<dyn std::error::Error>> {
    if io::stdin().is_terminal() {
        return Ok(None);
    }

    let mut text = String::new();
    io::stdin().read_to_string(&mut text)?;
    Ok(Some(text))
}

/// 初始化输入与全局状态，然后进入交互主循环。
///
/// 主循环流程：
/// 1. 回收已完成的后台作业
/// 2. 读取一条完整命令（含续行）
/// 3. 分派到 shell / ecscript 两套路径执行
/// 4. 根据返回值更新 `last_status`，或处理 `exit` 退出
fn main_loop() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = ShellInput::new()?;
    initialize_shell_environment();
    let mut state = new_shell_state(input.is_interactive());
    init_shell_job_control(&mut state)?;
    load_startup_rc(&mut state);
    input.print_welcome();

    loop {
        reap_background_jobs(&mut state)?;

        input.sync_shell_state(&state);
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
                state.extensions.borrow_mut().last_duration_ms = Some(0);
                run_hooks(
                    HookName::Postexec,
                    postexec_context(line, state.last_status, 0),
                    &state,
                );
                continue;
            }
            Err(TopLevelError::EcscriptParse(err)) => {
                print_error(err.format_with_source(line));
                state.last_status = CommandStatus::failure();
                state.extensions.borrow_mut().last_duration_ms = Some(0);
                run_hooks(
                    HookName::Postexec,
                    postexec_context(line, state.last_status, 0),
                    &state,
                );
                continue;
            }
        };

        run_hooks(HookName::Preexec, preexec_context(line), &state);
        let start = Instant::now();
        match run_top_level_input(input, &mut state) {
            Ok(CommandFlow::Continue(current_status)) => {
                state.last_status = current_status;
                let duration = start.elapsed().as_millis();
                state.extensions.borrow_mut().last_duration_ms = Some(duration);
                run_hooks(
                    HookName::Postexec,
                    postexec_context(line, current_status, duration),
                    &state,
                );
            }
            Ok(CommandFlow::Exit(current_status)) => {
                let duration = start.elapsed().as_millis();
                state.last_status = current_status;
                state.extensions.borrow_mut().last_duration_ms = Some(duration);
                run_hooks(
                    HookName::Postexec,
                    postexec_context(line, current_status, duration),
                    &state,
                );
                break;
            }
            Err(err) => {
                if repl_output_needs_newline() {
                    println!();
                }
                print_error(err.format_with_source(line));
                state.last_status = CommandStatus::failure();
                let duration = start.elapsed().as_millis();
                state.extensions.borrow_mut().last_duration_ms = Some(duration);
                run_hooks(
                    HookName::Postexec,
                    postexec_context(line, state.last_status, duration),
                    &state,
                );
                continue;
            }
        }
    }

    let _ = run_named_trap("EXIT", &mut state)?;
    input.save_history();
    Ok(())
}

/// 加载 `~/.ecshrc` 启动脚本。
///
/// 仅在交互模式下生效；非交互模式跳过，避免干扰脚本化执行。
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
    match run_ecscript_file_with_ctx(&path, &state.script_env, state, None) {
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

/// 一次命令读取的结果：成功读到一行、被 Ctrl-C 中断、或收到 EOF。
enum ReadCommand {
    Line(String),
    Interrupted,
    Eof,
}

/// 顶层输入分派结果：shell 命令或 ecscript 语句块。
enum TopLevelInput {
    Shell(ParsedJob),
    Ecscript(Vec<Stmt>),
}

/// 顶层分派阶段的解析错误，区分 shell 解析失败与 ecscript 解析失败。
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
        script_env: Rc::new(Environment::new()),
        aliases: HashMap::new(),
        traps: HashMap::new(),
        command_history: Vec::new(),
        extensions: new_extensions(),
        module_loader: Some(Rc::new(ModuleLoader::new())),
    }
}

fn initialize_shell_environment() {
    if let Some(shell_path) = current_shell_path() {
        unsafe { std::env::set_var("SHELL", shell_path) };
    }

    if let Ok(cwd) = std::env::current_dir() {
        unsafe { std::env::set_var("PWD", cwd) };
    }

    unsafe { std::env::set_var("SHLVL", next_shlvl().to_string()) };
}

fn current_shell_path() -> Option<String> {
    std::env::current_exe()
        .ok()
        .map(|path| path.to_string_lossy().into_owned())
        .or_else(|| std::env::args().next())
}

fn next_shlvl() -> i64 {
    std::env::var("SHLVL")
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value >= 0)
        .map(|value| value + 1)
        .unwrap_or(1)
}

/// 读取一条完整命令。
///
/// 当 lexer/parser 报 incomplete 时，继续用 `... ` 提示符读续行；
/// 当输入在 EOF 结束时，保留当前缓冲并交给后续解析阶段统一报错。
fn read_complete_command(
    input: &mut ShellInput,
    state: &ShellState,
) -> Result<ReadCommand, Box<dyn std::error::Error>> {
    run_hooks(HookName::BeforePrompt, before_prompt_context(state), state);
    let prompt = match resolve_prompt(state) {
        Ok(Some(prompt)) => prompt,
        Ok(None) => build_prompt(state)?,
        Err(err) => {
            print_error(err.format_with_source(""));
            build_prompt(state)?
        }
    };
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

/// 扫描源码文本判断 `{}` 是否仍未闭合。
///
/// 跳过单引号、双引号字符串及转义字符中的括号，只统计代码层的花括号深度。
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
    use super::{
        ReadCommand, current_shell_path, initialize_shell_environment, load_startup_rc, next_shlvl,
        read_complete_command,
    };
    use ecsh::ecscript::Value;
    use ecsh::input::{InputLine, ShellInput};
    use ecsh::test_support::env_lock;
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
        let _guard = env_lock().lock().unwrap();
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

    #[test]
    fn next_shlvl_increments_parent_level() {
        let _guard = env_lock().lock().unwrap();
        let old_shlvl = std::env::var_os("SHLVL");
        unsafe { std::env::set_var("SHLVL", "4") };
        assert_eq!(next_shlvl(), 5);

        match old_shlvl {
            Some(value) => unsafe { std::env::set_var("SHLVL", value) },
            None => unsafe { std::env::remove_var("SHLVL") },
        }
    }

    #[test]
    fn initialize_shell_environment_sets_shell_pwd_and_shlvl() {
        let _guard = env_lock().lock().unwrap();
        let old_shell = std::env::var_os("SHELL");
        let old_pwd = std::env::var_os("PWD");
        let old_shlvl = std::env::var_os("SHLVL");
        unsafe { std::env::set_var("SHLVL", "1") };

        initialize_shell_environment();

        assert_eq!(std::env::var("SHELL").ok(), current_shell_path(),);
        assert_eq!(
            std::env::var("PWD").ok(),
            std::env::current_dir()
                .ok()
                .map(|cwd| cwd.to_string_lossy().into_owned()),
        );
        assert_eq!(std::env::var("SHLVL").ok().as_deref(), Some("2"));

        match old_shell {
            Some(value) => unsafe { std::env::set_var("SHELL", value) },
            None => unsafe { std::env::remove_var("SHELL") },
        }
        match old_pwd {
            Some(value) => unsafe { std::env::set_var("PWD", value) },
            None => unsafe { std::env::remove_var("PWD") },
        }
        match old_shlvl {
            Some(value) => unsafe { std::env::set_var("SHLVL", value) },
            None => unsafe { std::env::remove_var("SHLVL") },
        }
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

/// `&&` / `||` 的条件分派骨架：先执行左侧，根据谓词决定是否执行右侧。
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
