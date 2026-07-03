//! 类型定义：shell 的所有核心数据结构。
//!
//! 按语义分层：
//!   - 输入层：Token, LexerStatus
//!   - 解析层：ParsedLine, ParsedJob, Command, Pipeline
//!   - 执行层：ShellState, CommandStatus, CommandFlow, Redirection
//!   - 作业层：Job, JobProcess, JobStatus, ProcessState

use crate::ecscript::env::Environment;
use crate::extensions::SharedExtensions;
use nix::unistd::Pid;
use std::collections::HashMap;
use std::os::fd::RawFd;
use std::rc::Rc;
/// shell 内部统一的 Result 类型。
pub type ShellResult<T> = Result<T, Box<dyn std::error::Error>>;

// ── 命令结构 ────────────────────────────────────────────────────────

/// 一条完整的命令：程序名 + 参数列表 + 重定向。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    pub program: ShellWord,
    pub args: Vec<ShellWord>,
    pub redirection: Redirection,
}

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
                OutputRedirection::Truncate(path) => write!(f, " > {}", path)?,
                OutputRedirection::Append(path) => write!(f, " >> {}", path)?,
            }
        }
        Ok(())
    }
}

/// 管道：一组用 `|` 连接的命令。
///
/// 数据流（以 3 条命令为例）：
///   cmd0: stdin ← shell stdin,   stdout → pipe0 write
///   cmd1: stdin ← pipe0 read,   stdout → pipe1 write
///   cmd2: stdin ← pipe1 read,   stdout → shell stdout
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pipeline {
    pub commands: Vec<Command>,
}

/// 解析后的语法结构 AST。
///
/// 三种控制流操作符：
///   - AndThen：`&&`，左侧成功（code=0）才执行右侧
///   - OrElse：`||`，左侧失败（code≠0）才执行右侧
///   - Sequence：`;`，左侧执行完无论成败都执行右侧（除非左侧请求 exit）
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParsedLine {
    Command(Command),
    Pipeline(Pipeline),
    AndThen(Rc<ParsedLine>, Rc<ParsedLine>),
    OrElse(Rc<ParsedLine>, Rc<ParsedLine>),
    Sequence(Rc<ParsedLine>, Rc<ParsedLine>),
}

/// 解析后的完整作业：语法结构 + 前后台标志 + 命令原文。
///
/// line 只表达语法控制流；`background` 是执行层语义（`&`），分开存放。
/// `command_line` 保存用户输入原文，供 `jobs` 命令展示。
#[derive(Debug, PartialEq, Eq)]
pub struct ParsedJob {
    pub line: ParsedLine,
    pub background: bool,
    pub command_line: String,
}

// ── 运行时状态 ──────────────────────────────────────────────────────

/// Shell 全局运行时状态。
#[derive(Clone)]
pub struct ShellState {
    /// 上一条命令的退出码（`$?` 的值）。
    pub last_status: CommandStatus,

    /// 只有真实 tty 交互模式才启用 job control。
    pub interactive: bool,

    /// shell 自己的进程组 PGID。
    pub shell_pgid: Option<Pid>,

    /// 控制终端 fd。
    pub shell_terminal_fd: Option<RawFd>,

    /// 后台和已停止的 job 表。
    pub jobs: Vec<Job>,

    /// 下一个分配的 job id。
    pub next_job_id: usize,

    /// 当前占用终端前台的进程组。
    pub current_fg_pgid: Option<Pid>,

    pub script_env: Rc<Environment<'static>>, // ← 新增：ecscript 全局根环境
    pub aliases: HashMap<String, String>,
    pub traps: HashMap<String, String>,
    pub command_history: Vec<String>,
    pub extensions: SharedExtensions,
    pub module_loader: Option<Rc<crate::ecscript::ModuleLoader>>,
}

/// 命令退出码（即 `$?` 的值）。
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

/// 控制流：命令执行结果 + 是否要求 shell 退出。
///
/// 把"退出码"和"exit 命令"分开表达，
/// 避免用特殊状态码（如 -1）同时表达两种含义。
#[derive(Debug, PartialEq, Eq)]
pub enum CommandFlow {
    Continue(CommandStatus),
    Exit(CommandStatus),
}

// ── 重定向 ──────────────────────────────────────────────────────────

/// 单条命令的重定向描述。
#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub struct Redirection {
    /// `< file`：stdin 从文件读取。
    pub stdin: Option<ShellWord>,
    /// `> file` 或 `>> file`：stdout 写入文件。
    pub stdout: Option<OutputRedirection>,
}

/// 输出重定向的两种模式。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutputRedirection {
    /// `>`：覆盖写入。
    Truncate(ShellWord),
    /// `>>`：追加写入。
    Append(ShellWord),
}

// ── 作业控制 ────────────────────────────────────────────────────────

/// Shell 管理的一个作业，对应一个进程组。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    /// Shell 分配的 job 编号（`jobs` 命令里的 [N]）。
    pub id: usize,

    /// 作业的进程组 ID。
    pub pgid: Pid,

    /// 用户输入的命令行原文。
    pub command_line: String,

    /// 作业的聚合状态。
    pub status: JobStatus,

    /// 管道最后一条命令的 PID，作业退出码取它的结果。
    pub last_pid: Pid,

    /// 作业中的所有进程。
    pub processes: Vec<JobProcess>,
}

/// 作业中的单个进程成员。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JobProcess {
    pub pid: Pid,

    /// shell 观察到的进程状态。
    pub state: ProcessState,

    /// 该进程最后的退出或终止状态。
    pub last_status: Option<CommandStatus>,
}

/// 单个进程的运行状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessState {
    /// 正在运行中。
    Running,
    /// 被信号暂停（通常是 Ctrl-Z → SIGTSTP）。
    Stopped,
    /// 已退出或被信号终止。
    Completed,
}

/// 整个作业的聚合状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobStatus {
    Running,
    Stopped,
    Done(CommandStatus),
}

// ── 词法分析 ────────────────────────────────────────────────────────

/// 词法分析的输出 Token。
#[derive(Debug, PartialEq)]
pub enum Token {
    /// 普通词（程序名、参数、文件名等）。
    Word(ShellWord),
    /// `|`
    Pipe,
    /// `&&`
    AndIf,
    /// `||`
    OrIf,
    /// `&`
    Ampersand,
    /// `<`
    RedirectionIn,
    /// `>`
    RedirectionTruncate,
    /// `>>`
    RedirectionAppend,
    /// `;`
    Semicolon,
}
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct ShellWord {
    pub fragments: Vec<WordFragment>,
}

impl ShellWord {
    /// 构造一个纯字面量的 ShellWord。
    pub fn lit(s: impl Into<String>) -> Self {
        ShellWord {
            fragments: vec![WordFragment::Lit(s.into())],
        }
    }

    /// 如果 ShellWord 恰好只含一个 Lit 片段，返回其字面量文本。
    pub fn as_lit_str(&self) -> Option<&str> {
        match self.fragments.as_slice() {
            [WordFragment::Lit(s)] => Some(s.as_str()),
            [WordFragment::QuotedLit(s)] => Some(s.as_str()),
            _ => None,
        }
    }
}

impl std::fmt::Display for ShellWord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for frag in &self.fragments {
            match frag {
                WordFragment::Lit(s) => write!(f, "{}", s)?,
                WordFragment::QuotedLit(s) => write!(f, "{}", s)?,
                WordFragment::Var(name) => write!(f, "${}", name)?,
                WordFragment::Cmd(src) => write!(f, "$({})", src)?,
                WordFragment::Expr { src, spread } => {
                    if *spread {
                        write!(f, "${{{}}}", format!("...{}", src))?
                    } else {
                        write!(f, "${{{}}}", src)?
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum WordFragment {
    /// 未引用字面量，可参与 glob pattern 解释。
    Lit(String),
    /// 来自引号或反斜杠转义的字面量，只作为普通文本。
    QuotedLit(String),
    Var(String),
    Cmd(String),
    Expr {
        src: String,
        spread: bool,
    },
}

/// 词法分析器的状态机状态。
#[derive(Debug, PartialEq, Eq)]
pub enum LexerStatus {
    /// 正常模式：空格和操作符有特殊含义。
    Normal,
    /// 单引号内：所有字符按字面量处理，不展开变量。
    SingleQuoted,
    /// 双引号内：保留字面量但展开 $ 变量。
    DoubleQuoted,
}
