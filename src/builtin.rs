use crate::diagnostics::print_error;
use crate::types::Command;
use std::io::{self, Write};

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
}

pub enum BuiltinResult {
    Continue,
    Exit,
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
        _ => None,
    }
}

// 当前仅允许纯输出型内置命令出现在管道中。
pub fn is_builtin_allowed_in_pipeline(kind: BuiltinKind) -> bool {
    matches!(
        kind,
        BuiltinKind::Help | BuiltinKind::Pwd | BuiltinKind::Env
    )
}

pub fn run_builtin(command: &Command) -> Option<BuiltinResult> {
    let kind = builtin_kind(command)?;

    match kind {
        BuiltinKind::Help => {
            println!("ecsh builtins:");
            println!("  help - show this help message");
            println!("  cd - change current working directory");
            println!("  pwd - print working directory");
            println!("  exit - exit the shell");
            println!("  env - print environment variables");
            println!("  export KEY=value - set environment variable");
            println!("  unset KEY - remove environment variable");
            println!("  clear - clear the terminal screen");
            Some(BuiltinResult::Continue)
        }
        BuiltinKind::Exit => Some(BuiltinResult::Exit),
        BuiltinKind::Cd => {
            run_cd(command);
            Some(BuiltinResult::Continue)
        }
        BuiltinKind::Pwd => {
            run_pwd();
            Some(BuiltinResult::Continue)
        }
        BuiltinKind::Env => {
            run_env();
            Some(BuiltinResult::Continue)
        }
        BuiltinKind::Export => {
            run_export(command);
            Some(BuiltinResult::Continue)
        }
        BuiltinKind::Unset => {
            run_unset(command);
            Some(BuiltinResult::Continue)
        }
        BuiltinKind::Clear => {
            run_clear();
            Some(BuiltinResult::Continue)
        }
    }
}

fn run_cd(command: &Command) {
    if command.args.len() > 1 {
        print_error("cd: too many arguments");
        return;
    }

    let dir = if command.args.is_empty() {
        // 未提供参数时，切换到环境变量 `HOME` 指向的目录。
        match std::env::var("HOME") {
            Ok(home) => home,
            Err(_) => {
                print_error("cd: HOME not set");
                return;
            }
        }
    } else {
        command.args[0].clone()
    };

    if let Err(err) = std::env::set_current_dir(&dir) {
        print_error(format!("cd: {}: {}", dir, err));
    }
}

fn run_pwd() {
    match std::env::current_dir() {
        Ok(path) => println!("{}", path.display()),
        Err(err) => {
            print_error(format!("pwd: {}", err));
        }
    };
}

fn run_env() {
    for (key, value) in std::env::vars() {
        println!("{}={}", key, value);
    }
}

// 当前只支持 `export KEY=value` 这一种格式。
fn run_export(command: &Command) {
    if command.args.len() != 1 {
        print_error("export: usage: export KEY=value");
        return;
    }

    let Some((key, value)) = command.args[0].split_once('=') else {
        print_error("export: usage: export KEY=value");
        return;
    };

    if key.is_empty() {
        print_error("export: usage: export KEY=value");
        return;
    }

    if !is_valid_env_key(key) {
        print_error(format!("export: invalid variable name: {}", key));
        return;
    }

    unsafe { std::env::set_var(key, value) };
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

fn run_unset(command: &Command) {
    if command.args.len() != 1 {
        print_error("unset: usage: unset KEY");
        return;
    }

    // 先校验变量名，避免 `remove_var` 在非法变量名上 panic。
    let key = &command.args[0];
    if !is_valid_env_key(key) {
        print_error(format!("unset: invalid variable name: {}", key));
        return;
    }

    unsafe { std::env::remove_var(key) };
}

// `clear` 负责清空当前屏幕，scrollback 能否清除取决于终端实现。
fn run_clear() {
    print!("\x1b[2J\x1b[3J\x1b[H");
    let _ = io::stdout().flush();
}
