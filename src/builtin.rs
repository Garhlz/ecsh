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
}

// 所有内置命令名称只在这里维护一份。
// 将命令名称映射为对应的 BuiltinKind。
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
        _ => None,
    }
}

// 当前仅允许纯输出型内置命令出现在管道中。
pub fn is_builtin_allowed_in_pipeline(kind: BuiltinKind) -> bool {
    matches!(
        kind,
        BuiltinKind::Help | BuiltinKind::Pwd | BuiltinKind::Env | BuiltinKind::Status
    )
}

pub fn run_builtin(command: &Command, state: &mut ShellState) -> Option<CommandFlow> {
    let kind = builtin_kind(command)?;

    match kind {
        BuiltinKind::Help => {
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
    }
}

fn print_help_title() {
    let use_color = isatty(io::stdout()).unwrap_or(false);

    // 和 prompt 一样，只有输出到真实终端时才写入 ANSI 颜色。
    // 这里故意做成“每个词一个颜色”的小彩蛋；重定向到文件时仍然是纯文本。
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

fn color_prefix<'a>(use_color: bool, color: &'a str) -> &'a str {
    if use_color { color } else { "" }
}

fn run_cd(command: &Command) -> CommandStatus {
    if command.args.len() > 1 {
        print_error("cd: too many arguments");
        return CommandStatus::failure();
    }

    let dir = if command.args.is_empty() {
        // 未提供参数时，切换到环境变量 `HOME` 指向的目录。
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

fn run_env() -> CommandStatus {
    for (key, value) in std::env::vars() {
        println!("{}={}", key, value);
    }

    CommandStatus::success()
}

// 当前只支持 `export KEY=value` 这一种格式。
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

fn is_valid_env_key(key: &str) -> bool {
    let mut chars = key.chars();

    // 空字符串不满足 shell 风格的环境变量命名规则。
    let Some(first) = chars.next() else {
        return false;
    };

    if !first.is_ascii_alphabetic() && first != '_' {
        return false;
    }

    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn run_unset(command: &Command) -> CommandStatus {
    if command.args.len() != 1 {
        print_error("unset: usage: unset KEY");
        return CommandStatus::failure();
    }

    // 先校验变量名，避免 `remove_var` 在非法变量名上 panic。
    let key = &command.args[0];
    if !is_valid_env_key(key) {
        print_error(format!("unset: invalid variable name: {}", key));
        return CommandStatus::failure();
    }

    unsafe { std::env::remove_var(key) };
    CommandStatus::success()
}

// `clear` 负责清空当前屏幕，scrollback 能否清除取决于终端实现。
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

fn run_status(state: &mut ShellState) -> CommandStatus {
    println!("{}", state.last_status.code);
    CommandStatus::success()
}
