//! 输入处理：交互式终端使用 rustyline（行编辑 + 历史），非终端使用 std::io::stdin。
//!
//! 两种模式：
//!   - Interactive：真实 tty，使用 rustyline 提供方向键、行编辑、命令历史
//!   - Plain：管道/脚本驱动，使用标准 stdin 逐行读取
//!
//! 历史文件存储在 ~/.ecsh_history。

use crate::builtin::print_help;
use crate::completion::{EcshEditor, new_editor};
use nix::unistd::isatty;
use rustyline::error::ReadlineError;
use std::collections::VecDeque;
use std::io::{self, Write};
use std::path::PathBuf;

/// 交互模式：rustyline 接管终端，支持行编辑和历史。
/// 非交互模式：纯管道读取，不加任何终端处理。
pub enum ShellInput {
    Interactive {
        editor: EcshEditor,
        history_path: Option<PathBuf>,
    },
    Plain,
    Scripted {
        lines: VecDeque<InputLine>,
        prompts: Vec<String>,
        history: Vec<String>,
    },
}

/// 读入一行的结果：
///   - Line(line)：正常读到一行输入
///   - Interrupted：Ctrl-C 取消了当前行（不是退出 shell）
///   - Eof：Ctrl-D 或管道结束
pub enum InputLine {
    Line(String),
    Interrupted,
    Eof,
}

impl ShellInput {
    /// 创建 ShellInput 实例。
    ///
    /// 如果 stdin 或 stdout 不是 tty（被管道重定向等），
    /// 走 Plain 模式，避免 rustyline 破坏脚本化输入。
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        if !isatty(io::stdin())? || !isatty(io::stdout())? {
            return Ok(ShellInput::Plain);
        }

        let mut editor = new_editor()?;
        let history_path = history_path();

        if let Some(path) = &history_path {
            let _ = editor.load_history(path);
        }

        Ok(ShellInput::Interactive {
            editor,
            history_path,
        })
    }

    /// 读出用户的一行输入。
    ///
    /// 交互模式：调用 rustyline 的 readline，支持编辑和补全。
    ///   - Ctrl-C → InputLine::Interrupted
    ///   - Ctrl-D → InputLine::Eof
    /// 非交互模式：用标准 stdin 读取，读到空行即为 Eof。
    pub fn read_line(&mut self, prompt: &str) -> Result<InputLine, Box<dyn std::error::Error>> {
        match self {
            ShellInput::Interactive { editor, .. } => match editor.readline(prompt) {
                Ok(line) => Ok(InputLine::Line(line)),
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
            ShellInput::Scripted { lines, prompts, .. } => {
                prompts.push(prompt.to_string());
                Ok(lines.pop_front().unwrap_or(InputLine::Eof))
            }
        }
    }

    /// 只在交互模式下写入历史；续行输入会在主循环拼成完整命令后统一写入。
    pub fn add_history_entry(&mut self, entry: &str) {
        if entry.trim().is_empty() {
            return;
        }

        let ShellInput::Interactive { editor, .. } = self else {
            if let ShellInput::Scripted { history, .. } = self {
                history.push(entry.to_string());
            }
            return;
        };
        let _ = editor.add_history_entry(entry);
    }

    /// shell 退出前保存历史文件。
    ///
    /// 保存失败不影响 shell 的正常退出，所以忽略错误。
    pub fn save_history(&mut self) {
        let ShellInput::Interactive {
            editor,
            history_path: Some(path),
        } = self
        else {
            return;
        };
        let _ = editor.save_history(path);
    }

    /// 交互模式下打印欢迎信息和帮助。
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

    /// 判断当前是否是交互模式（tty）。
    pub fn is_interactive(&self) -> bool {
        matches!(self, ShellInput::Interactive { .. })
    }

    pub fn scripted(lines: impl IntoIterator<Item = InputLine>) -> Self {
        Self::Scripted {
            lines: lines.into_iter().collect(),
            prompts: Vec::new(),
            history: Vec::new(),
        }
    }

    pub fn recorded_prompts(&self) -> &[String] {
        let ShellInput::Scripted { prompts, .. } = self else {
            panic!("recorded_prompts is only available for scripted input");
        };
        prompts
    }

    pub fn recorded_history(&self) -> &[String] {
        let ShellInput::Scripted { history, .. } = self else {
            panic!("recorded_history is only available for scripted input");
        };
        history
    }
}

/// 历史文件路径：~/.ecsh_history
fn history_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".ecsh_history"))
}
