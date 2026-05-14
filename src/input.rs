use crate::builtin::print_help;
use nix::unistd::isatty;
use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;
use std::io::{self, Write};
use std::path::PathBuf;

pub enum ShellInput {
    Interactive {
        editor: DefaultEditor,
        history_path: Option<PathBuf>,
    },
    Plain,
}

pub enum InputLine {
    Line(String),
    Interrupted,
    Eof,
}

impl ShellInput {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
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

    pub fn read_line(&mut self, prompt: &str) -> Result<InputLine, Box<dyn std::error::Error>> {
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

    pub fn save_history(&mut self) {
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

    pub fn print_welcome(&self) {
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
