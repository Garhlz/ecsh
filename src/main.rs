use ecsh::builtin::print_help;
use ecsh::diagnostics::print_error;
use ecsh::executor::{run_command, run_pipeline};
use ecsh::parser::parse_line;
use ecsh::prompt::build_prompt;
use ecsh::types::{CommandFlow, CommandStatus, ParsedLine, ShellState};
use nix::unistd::isatty;
use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;
use std::io::{self, Write};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    main_loop()
}

enum ShellInput {
    Interactive {
        editor: DefaultEditor,
        history_path: Option<PathBuf>,
    },
    Plain,
}

enum InputLine {
    Line(String),
    Interrupted,
    Eof,
}

fn main_loop() -> Result<(), Box<dyn std::error::Error>> {
    // TODO 主循环当前统一返回 Box<dyn Error>，便于早期阶段快速迭代。
    // 用统一的 ShellState 维护状态，便于后续扩展。
    let mut state = ShellState {
        last_status: CommandStatus::success(),
    };
    let mut input = ShellInput::new()?;
    input.print_welcome();

    loop {
        let prompt = build_prompt(&state)?;
        let line = match input.read_line(&prompt)? {
            InputLine::Line(line) => line,
            InputLine::Interrupted => {
                // rustyline 在交互式终端中会把 Ctrl-C 转成 Interrupted。
                // 这表示“取消当前输入行”，而不是退出整个 shell。
                state.last_status = CommandStatus::new(130);
                continue;
            }
            InputLine::Eof => break,
        };
        let line = line.trim();

        // 空输入直接跳过，进入下一轮提示符。
        if line.is_empty() {
            // 直接按回车不应继续沿用上一条失败命令的状态码，
            // 否则 prompt 会一直显示旧错误状态。
            state.last_status = CommandStatus::success();
            continue;
        }

        let parsed = match parse_line(line, &state) {
            Ok(parsed) => parsed,
            Err(err) => {
                print_error(format!("parse line: {}", err));
                state.last_status = CommandStatus::failure();
                continue;
            }
        };

        match run_parsed_line(&parsed, &mut state)? {
            CommandFlow::Continue(current_status) => {
                state.last_status = current_status;
            }
            CommandFlow::Exit(_current_status) => {
                break;
            }
        }
    }

    input.save_history();
    Ok(())
}

impl ShellInput {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        // rustyline 需要接管真实终端，才能正确处理方向键、行编辑和历史记录。
        // 当 stdin/stdout 不是 tty 时，说明 ecsh 正在被管道或测试驱动，此时保留
        // 普通 read_line 路径，避免破坏脚本化输入。
        if !isatty(io::stdin())? || !isatty(io::stdout())? {
            return Ok(ShellInput::Plain);
        }

        let mut editor = DefaultEditor::new()?;
        let history_path = history_path();

        if let Some(path) = &history_path {
            // 历史文件首次不存在是正常情况；加载失败不应阻止 shell 启动。
            let _ = editor.load_history(path);
        }

        Ok(ShellInput::Interactive {
            editor,
            history_path,
        })
    }

    fn read_line(&mut self, prompt: &str) -> Result<InputLine, Box<dyn std::error::Error>> {
        match self {
            ShellInput::Interactive { editor, .. } => match editor.readline(prompt) {
                Ok(line) => {
                    if !line.trim().is_empty() {
                        // add_history_entry 可能因为配置或重复项策略返回错误。
                        // 这不是命令执行错误，因此这里只把它当作非关键路径处理。
                        let _ = editor.add_history_entry(line.as_str());
                    }
                    Ok(InputLine::Line(line))
                }
                Err(ReadlineError::Interrupted) => Ok(InputLine::Interrupted),
                Err(ReadlineError::Eof) => Ok(InputLine::Eof),
                Err(err) => Err(Box::new(err)),
            },
            ShellInput::Plain => {
                print!("{}", prompt);
                io::stdout().flush()?;

                let mut line = String::new();
                let bytes = io::stdin().read_line(&mut line)?;
                if bytes == 0 {
                    Ok(InputLine::Eof)
                } else {
                    Ok(InputLine::Line(line))
                }
            }
        }
    }

    fn save_history(&mut self) {
        let ShellInput::Interactive {
            editor,
            history_path: Some(path),
        } = self
        else {
            return;
        };

        // 历史保存失败不影响 shell 的退出状态。常见原因包括 HOME 不存在、
        // 目录权限不允许写入等。
        let _ = editor.save_history(path);
    }

    fn print_welcome(&self) {
        if !matches!(self, ShellInput::Interactive { .. }) {
            return;
        }

        println!("Welcome to ecsh.");
        println!("Elaine & Cornelia's shell is ready.");
        println!();
        print_help();
        println!();
    }
}

fn history_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".ecsh_history"))
}

fn run_parsed_line(
    parsed: &ParsedLine,
    state: &mut ShellState,
) -> Result<CommandFlow, Box<dyn std::error::Error>> {
    match parsed {
        ParsedLine::Command(command) => run_command(command, state),
        ParsedLine::Pipeline(pipeline) => Ok(CommandFlow::Continue(run_pipeline(pipeline, state)?)),
        // `&&`：左侧成功才执行右侧；如果左侧请求 exit，则直接向上传递退出请求。
        ParsedLine::AndThen(left, right) => match run_parsed_line(left, state)? {
            CommandFlow::Exit(status) => Ok(CommandFlow::Exit(status)),
            CommandFlow::Continue(status) => {
                if status.code == 0 {
                    run_parsed_line(right, state)
                } else {
                    Ok(CommandFlow::Continue(status))
                }
            }
        },
        // `||`：左侧失败才执行右侧；如果左侧请求 exit，同样不再继续执行右侧。
        ParsedLine::OrElse(left, right) => match run_parsed_line(left, state)? {
            CommandFlow::Exit(status) => Ok(CommandFlow::Exit(status)),
            CommandFlow::Continue(status) => {
                if status.code != 0 {
                    run_parsed_line(right, state)
                } else {
                    Ok(CommandFlow::Continue(status))
                }
            }
        },
    }
}
