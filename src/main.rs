mod builtin;
mod diagnostics;
mod executor;
mod parser;
mod redirection;
mod types;

use crate::diagnostics::print_error;
use crate::executor::{run_command, run_pipeline};
use crate::parser::parse_line;
use crate::types::{CommandFlow, CommandStatus, ParsedLine, ShellState};
use std::io::{self, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    main_loop()
}

fn main_loop() -> Result<(), Box<dyn std::error::Error>> {
    // TODO 主循环当前统一返回 Box<dyn Error>，便于早期阶段快速迭代。
    // 用统一的ShellState维护状态，便于后续扩展
    let mut state = ShellState {
        last_status: CommandStatus::success(),
    };

    loop {
        print!("shell> ");
        io::stdout().flush()?;

        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
        let line = line.trim();

        // 空输入直接跳过，进入下一轮提示符。
        if line.is_empty() {
            continue;
        }

        // 一行输入当前只会被解析成普通命令或管道命令。
        match parse_line(line, &state) {
            Ok(None) => continue,
            Ok(Some(ParsedLine::Command(command))) => {
                match run_command(&command, &mut state)? {
                    CommandFlow::Continue(current_status) => {
                        state.last_status = current_status;
                    }
                    CommandFlow::Exit(_current_status) => break,
                };
            }
            Ok(Some(ParsedLine::Pipeline(pipeline))) => {
                let current_status = run_pipeline(&pipeline, &mut state)?;
                state.last_status = current_status;
            }
            Err(err) => {
                print_error(format!("parse line: {}", err));
                let current_status = CommandStatus { code: 1 };
                state.last_status = current_status;
                continue;
            }
        }
    }

    Ok(())
}
