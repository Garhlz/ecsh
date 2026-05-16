//! 内置命令定义与实现。
//!
//! builtin 命令在 shell 自身进程执行（不 fork），因此能修改 shell 的工作目录、环境变量等状态。
//! 注意：jobs / fg / bg 虽然在命令表中注册，但实际执行由 executor 层负责，
//! run_builtin 对它们返回 None，交由特殊路径处理。

use crate::diagnostics::print_error;
use crate::types::Command;
use crate::types::{CommandFlow, CommandStatus, ShellState};
use nix::unistd::isatty;
use std::io::{self, Write};

const ANSI_RESET: &str = "\x1b[0m";
const ANSI_HOT_PINK: &str = "\x1b[1;38;5;201m";
const ANSI_NEON_GREEN: &str = "\x1b[1;38;5;120m";
const ANSI_SUN_YELLOW: &str = "\x1b[1;38;5;226m";
const ANSI_ELECTRIC_CYAN: &str = "\x1b[1;38;5;51m";
const ANSI_WARM_ORANGE: &str = "\x1b[1;38;5;214m";

/// 内置命令的种类。每种命令对应一段独立的执行逻辑。
#[derive(Clone, Copy)]
pub enum BuiltinKind {
    Help,
    Exit,
    Cd,
    Pwd,
    Env,
    Export,
    Unset,
    Clear,
    Status,
    Jobs,
    Fg,
    Bg,
}

/// 将命令名映射到 BuiltinKind。
///
/// 所有内置命令名称只在这里维护一份，避免分散在各个 match 语句中。
pub fn builtin_kind(command: &Command) -> Option<BuiltinKind> {
    match command.program.as_str() {
        "help" => Some(BuiltinKind::Help),
        "exit" => Some(BuiltinKind::Exit),
        "cd" => Some(BuiltinKind::Cd),
        "pwd" => Some(BuiltinKind::Pwd),
        "env" => Some(BuiltinKind::Env),
        "export" => Some(BuiltinKind::Export),
        "unset" => Some(BuiltinKind::Unset),
        "clear" => Some(BuiltinKind::Clear),
        "status" => Some(BuiltinKind::Status),
        "jobs" => Some(BuiltinKind::Jobs),
        "fg" => Some(BuiltinKind::Fg),
        "bg" => Some(BuiltinKind::Bg),
        _ => None,
    }
}

/// 判断某个内置命令是否可以出现在管道中。
///
/// 只允许纯输出型（只读、不修改 shell 状态）的内置命令进入管道。
/// cd / export / unset 这类会修改 shell 状态，必须由 shell 自身执行，不能放在子进程里。
pub fn is_builtin_allowed_in_pipeline(kind: BuiltinKind) -> bool {
    matches!(
        kind,
        BuiltinKind::Help | BuiltinKind::Pwd | BuiltinKind::Env | BuiltinKind::Status
    )
}

/// 执行内置命令（不包括 jobs/fg/bg）。
///
/// 返回 `Option<CommandFlow>`：
///   - Some(flow) → 已执行，返回控制流和状态码
///   - None       → 是 jobs/fg/bg，需要 executor 层特殊处理
pub fn run_builtin(command: &Command, state: &mut ShellState) -> Option<CommandFlow> {
    let kind = builtin_kind(command)?;

    match kind {
        BuiltinKind::Help => {
            print_help();
            Some(CommandFlow::Continue(CommandStatus::success()))
        }
        BuiltinKind::Exit => Some(CommandFlow::Exit(CommandStatus::success())),
        BuiltinKind::Cd => Some(CommandFlow::Continue(run_cd(command))),
        BuiltinKind::Pwd => Some(CommandFlow::Continue(run_pwd())),
        BuiltinKind::Env => Some(CommandFlow::Continue(run_env())),
        BuiltinKind::Export => Some(CommandFlow::Continue(run_export(command))),
        BuiltinKind::Unset => Some(CommandFlow::Continue(run_unset(command))),
        BuiltinKind::Clear => Some(CommandFlow::Continue(run_clear())),
        BuiltinKind::Status => Some(CommandFlow::Continue(run_status(state))),
        // jobs / fg / bg 需要访问作业表和前台等待逻辑，
        // 由 executor 层统一处理，避免 builtin 模块反向依赖 executor。
        BuiltinKind::Jobs | BuiltinKind::Fg | BuiltinKind::Bg => None,
    }
}

/// `help` 命令：打印所有内置命令的简单说明。
pub fn print_help() {
    print_help_title();
    println!("ecsh builtins:");
    println!("  help - show this help message");
    println!("  cd - change current working directory");
    println!("  pwd - print working directory");
    println!("  exit - exit the shell");
    println!("  env - print environment variables");
    println!("  export KEY=value - set environment variable");
    println!("  unset KEY - remove environment variable");
    println!("  clear - clear the terminal screen");
    println!("  status - print last command status");
    println!("  jobs - list background and stopped jobs");
    println!("  fg %N - move job N to the foreground");
    println!("  bg %N - resume job N in the background");
}

/// 打印 help 标题行。每个词用不同颜色，重定向到文件时自动去掉颜色。
fn print_help_title() {
    let use_color = isatty(io::stdout()).unwrap_or(false);

    println!(
        "{}ecsh{} - {}Elaine{} {}&{} {}Cornelia's{} {}shell{}",
        color_prefix(use_color, ANSI_HOT_PINK),
        color_prefix(use_color, ANSI_RESET),
        color_prefix(use_color, ANSI_NEON_GREEN),
        color_prefix(use_color, ANSI_RESET),
        color_prefix(use_color, ANSI_SUN_YELLOW),
        color_prefix(use_color, ANSI_RESET),
        color_prefix(use_color, ANSI_ELECTRIC_CYAN),
        color_prefix(use_color, ANSI_RESET),
        color_prefix(use_color, ANSI_WARM_ORANGE),
        color_prefix(use_color, ANSI_RESET),
    );
}

/// 根据 use_color 开关决定输出颜色代码还是空字符串。
fn color_prefix<'a>(use_color: bool, color: &'a str) -> &'a str {
    if use_color { color } else { "" }
}

/// `cd [dir]` 命令：更改当前工作目录。
///
/// 不传参数时默认切换到 HOME 环境变量指向的目录。
/// 使用 `std::env::set_current_dir` 操作系统调用更改进程的工作目录。
fn run_cd(command: &Command) -> CommandStatus {
    if command.args.len() > 1 {
        print_error("cd: too many arguments");
        return CommandStatus::failure();
    }

    let dir = if command.args.is_empty() {
        match std::env::var("HOME") {
            Ok(home) => home,
            Err(_) => {
                print_error("cd: HOME not set");
                return CommandStatus::failure();
            }
        }
    } else {
        command.args[0].clone()
    };

    if let Err(err) = std::env::set_current_dir(&dir) {
        print_error(format!("cd: {}: {}", dir, err));
        return CommandStatus::failure();
    }

    CommandStatus::success()
}

/// `pwd` 命令：打印当前工作目录。
///
/// 使用 `std::env::current_dir()` 获取当前进程的工作目录。
fn run_pwd() -> CommandStatus {
    match std::env::current_dir() {
        Ok(path) => {
            println!("{}", path.display());
            CommandStatus::success()
        }
        Err(err) => {
            print_error(format!("pwd: {}", err));
            CommandStatus::failure()
        }
    }
}

/// `env` 命令：打印所有环境变量（KEY=value 格式，每行一个）。
fn run_env() -> CommandStatus {
    for (key, value) in std::env::vars() {
        println!("{}={}", key, value);
    }

    CommandStatus::success()
}

/// `export KEY=value` 命令：设置环境变量。
///
/// 当前只支持 `export KEY=value` 格式（没有单独的 `export NAME` 列出模式）。
/// `std::env::set_var` 是 unsafe 的：Rust 标准库标记为 unsafe 因为多线程下可能产生数据竞争。
/// ecsh 是单线程程序，所以这里实际是安全的。
fn run_export(command: &Command) -> CommandStatus {
    if command.args.len() != 1 {
        print_error("export: usage: export KEY=value");
        return CommandStatus::failure();
    }

    let Some((key, value)) = command.args[0].split_once('=') else {
        print_error("export: usage: export KEY=value");
        return CommandStatus::failure();
    };

    if key.is_empty() {
        print_error("export: usage: export KEY=value");
        return CommandStatus::failure();
    }

    if !is_valid_env_key(key) {
        print_error(format!("export: invalid variable name: {}", key));
        return CommandStatus::failure();
    }

    unsafe { std::env::set_var(key, value) };
    CommandStatus::success()
}

/// 校验环境变量名是否合法：首字符为字母或下划线，后续为字母/数字/下划线。
fn is_valid_env_key(key: &str) -> bool {
    let mut chars = key.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphabetic() && first != '_' {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

/// `unset KEY` 命令：删除环境变量。
///
/// 先校验变量名合法性再调用 `std::env::remove_var`，避免非法变量名导致 panic。
fn run_unset(command: &Command) -> CommandStatus {
    if command.args.len() != 1 {
        print_error("unset: usage: unset KEY");
        return CommandStatus::failure();
    }

    let key = &command.args[0];
    if !is_valid_env_key(key) {
        print_error(format!("unset: invalid variable name: {}", key));
        return CommandStatus::failure();
    }

    unsafe { std::env::remove_var(key) };
    CommandStatus::success()
}

/// `clear` 命令：清空终端屏幕。
///
/// ANSI 转义序列含义：
///   - \\x1b[2J → 清空整个屏幕
///   - \\x1b[3J → 清空 scrollback 缓冲区（不是所有终端都支持）
///   - \\x1b[H  → 光标移到左上角
fn run_clear() -> CommandStatus {
    print!("\x1b[2J\x1b[3J\x1b[H");
    match io::stdout().flush() {
        Ok(()) => CommandStatus::success(),
        Err(err) => {
            print_error(format!("clear: {}", err));
            CommandStatus::failure()
        }
    }
}

/// `status` 命令：打印上一条命令的退出码，供调试使用。
fn run_status(state: &mut ShellState) -> CommandStatus {
    println!("{}", state.last_status.code);
    CommandStatus::success()
}
