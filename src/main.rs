mod builtin;
mod diagnostics;
mod executor;
mod parser;
mod redirection;
mod types;

use crate::diagnostics::print_error;
use crate::executor::{run_command, run_pipeline};
use crate::parser::parse_line;
use crate::types::{CommandFlow, ParsedLine};
use std::io::{self, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    main_loop()
}

fn main_loop() -> Result<(), Box<dyn std::error::Error>> {
    // TODO 主循环当前统一返回 Box<dyn Error>，便于早期阶段快速迭代。
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

        match parse_line(line) {
            Ok(None) => continue,
            Ok(Some(ParsedLine::Command(command))) => match run_command(&command)? {
                CommandFlow::Continue(_status) => {}
                CommandFlow::Exit(_status) => break,
            },
            Ok(Some(ParsedLine::Pipeline(pipeline))) => {
                let _status = run_pipeline(&pipeline)?;
            }
            Err(err) => {
                print_error(format!("parse line: {}", err));
                continue;
            }
        }
    }

    Ok(())
}
