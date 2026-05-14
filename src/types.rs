pub type ShellResult<T> = Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, PartialEq, Eq)]
pub struct Command {
    pub program: String,
    pub args: Vec<String>,
    pub redirection: Redirection,
}

// 用于在诊断信息中还原命令的可读形式。
impl std::fmt::Display for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.program)?;

        for arg in &self.args {
            write!(f, " {}", arg)?;
        }

        if let Some(stdin) = &self.redirection.stdin {
            write!(f, " < {}", stdin)?;
        }

        if let Some(stdout) = &self.redirection.stdout {
            match stdout {
                OutputRedirection::Truncate(path) => {
                    write!(f, " > {}", path)?;
                }
                OutputRedirection::Append(path) => {
                    write!(f, " >> {}", path)?;
                }
            }
        }

        Ok(())
    }
}

pub struct ShellState {
    pub last_status: CommandStatus,
}

/*
管道按标准 shell 语义使用 `|`。

cmd0 | cmd1 | cmd2

cmd0:
  stdin  <- shell stdin
  stdout -> pipe0 write end

cmd1:
  stdin  <- pipe0 read end
  stdout -> pipe1 write end

cmd n-1:
  stdin  <- pipe n-2 read end
  stdout -> shell stdout
*/
#[derive(Debug, PartialEq, Eq)]
pub struct Pipeline {
    pub commands: Vec<Command>,
}

// 一行输入解析后的语法结构。Command/Pipeline 是执行单元；
// AndThen/OrElse 用于表达 `&&` / `||` 的控制流。
#[derive(Debug, PartialEq, Eq)]
pub enum ParsedLine {
    Command(Command),
    Pipeline(Pipeline),
    AndThen(Box<ParsedLine>, Box<ParsedLine>), // &&的控制流
    OrElse(Box<ParsedLine>, Box<ParsedLine>),  // ||的控制流
    Sequence(Box<ParsedLine>, Box<ParsedLine>), // ;的控制流
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandStatus {
    pub code: i32,
}

impl CommandStatus {
    pub fn success() -> Self {
        Self { code: 0 }
    }

    pub fn failure() -> Self {
        Self { code: 1 }
    }

    pub fn new(code: i32) -> Self {
        Self { code }
    }
}

// 命令状态和 shell 控制流分开表达，避免用 bool 同时表示多种含义。
#[derive(Debug, PartialEq, Eq)]
pub enum CommandFlow {
    Continue(CommandStatus),
    Exit(CommandStatus),
}

#[derive(Default, Debug, PartialEq, Eq)]
pub struct Redirection {
    pub stdin: Option<String>,
    pub stdout: Option<OutputRedirection>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum OutputRedirection {
    Truncate(String),
    Append(String),
}

#[derive(Debug, PartialEq, Eq)]
pub enum Token {
    Word(String),
    Pipe,
    AndIf,
    OrIf,
    RedirectionIn,
    RedirectionTruncate,
    RedirectionAppend,
    Semicolon,
}

#[derive(Debug, PartialEq, Eq)]
pub enum LexerStatus {
    Normal,
    SingleQuoted,
    DoubleQuoted,
}
