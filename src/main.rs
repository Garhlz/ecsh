//! ecsh 入口：主循环和 AST 执行调度。
//!
//! 主循环的每一轮迭代：
//!   1. reap_background_jobs  — 非阻塞回收后台子进程
//!   2. build_prompt          — 生成带颜色的提示符
//!   3. read_line             — 等待用户输入或脚本行
//!   4. parse_line            — 词法 + 语法分析
//!   5. run_parsed_line       — 根据 AST 类型分派执行

use ecsh::diagnostics::print_error;
use ecsh::ecscript::env::Environment;
use ecsh::executor::{init_shell_job_control, reap_background_jobs, run_command, run_pipeline};
use ecsh::input::{InputLine, ShellInput};
use ecsh::parser::parse_line;
use ecsh::prompt::build_prompt;
use ecsh::types::{CommandFlow, CommandStatus, ParsedJob, ParsedLine, ShellState};

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
    };
    // 交互模式下初始化 job control（设进程组、抢终端、忽略信号）。
    init_shell_job_control(&mut state)?;
    input.print_welcome();

    loop {
        // 1. 非阻塞回收已结束的后台子进程。
        reap_background_jobs(&mut state)?;

        // 2. 显示 prompt。
        let prompt = build_prompt(&state)?;

        // 3. 读取一行输入。
        let line = match input.read_line(&prompt)? {
            InputLine::Line(line) => line,
            InputLine::Interrupted => {
                // Ctrl-C 在读取输入时 → 取消当前行，显示新 prompt。
                // 退出码 130 = 128 + SIGINT(2)。
                state.last_status = CommandStatus::new(130);
                continue;
            }
            InputLine::Eof => break, // Ctrl-D 或管道结束 → 退出 shell
        };
        let line = line.trim();

        // 4. 空输入，重置状态码后直接下一轮。
        if line.is_empty() {
            state.last_status = CommandStatus::success();
            continue;
        }

        // 5. 词法 + 语法分析。
        let parsed = match parse_line(line, &state) {
            Ok(parsed) => parsed,
            Err(err) => {
                print_error(format!("parse line: {}", err));
                state.last_status = CommandStatus::failure();
                continue;
            }
        };

        // 6. 根据 AST 类型分派执行。
        match run_parsed_line(&parsed, &mut state)? {
            CommandFlow::Continue(current_status) => {
                state.last_status = current_status;
            }
            CommandFlow::Exit(_current_status) => {
                break; // exit 命令
            }
        }
    }

    input.save_history();
    Ok(())
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
