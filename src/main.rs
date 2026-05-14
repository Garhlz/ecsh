use ecsh::diagnostics::print_error;
use ecsh::executor::{run_command, run_pipeline};
use ecsh::input::{InputLine, ShellInput};
use ecsh::parser::parse_line;
use ecsh::prompt::build_prompt;
use ecsh::types::{CommandFlow, CommandStatus, ParsedLine, ShellState};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    main_loop()
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
                state.last_status = current_status; // 修改shellStatus，进行下一轮循环
            }
            CommandFlow::Exit(_current_status) => {
                break;
            }
        }
    }

    input.save_history();
    Ok(())
}

fn run_parsed_line(
    parsed: &ParsedLine,
    state: &mut ShellState,
) -> Result<CommandFlow, Box<dyn std::error::Error>> {
    match parsed {
        ParsedLine::Command(command) => run_command(command, state),
        // 此处run_command 返回的已经是 Result<CommandFlow, _>
        ParsedLine::Pipeline(pipeline) => Ok(CommandFlow::Continue(run_pipeline(pipeline, state)?)),
        // `&&`：左侧成功才执行右侧；如果左侧请求 exit，则直接向上传递退出请求。
        ParsedLine::AndThen(left, right) => match run_parsed_line(left, state)? {
            CommandFlow::Exit(status) => Ok(CommandFlow::Exit(status)),
            CommandFlow::Continue(status) => {
                if status.code == 0 {
                    state.last_status = status;
                    run_parsed_line(right, state)
                } else {
                    // 左侧失败，直接返回左侧状态码
                    Ok(CommandFlow::Continue(status))
                }
            }
        },
        // `||`：左侧失败才执行右侧；如果左侧请求 exit，同样不再继续执行右侧。
        ParsedLine::OrElse(left, right) => match run_parsed_line(left, state)? {
            CommandFlow::Exit(status) => Ok(CommandFlow::Exit(status)),
            CommandFlow::Continue(status) => {
                if status.code != 0 {
                    state.last_status = status;
                    run_parsed_line(right, state)
                } else {
                    Ok(CommandFlow::Continue(status))
                }
            }
        },
        // `;`: 左侧退出才不执行右边
        ParsedLine::Sequence(left, right) => match run_parsed_line(left, state)? {
            CommandFlow::Exit(status) => Ok(CommandFlow::Exit(status)),
            CommandFlow::Continue(status) => {
                state.last_status = status;
                run_parsed_line(right, state)
            }
        },
    }
}
