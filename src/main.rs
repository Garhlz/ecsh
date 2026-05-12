use nix::sys::wait::waitpid;
use nix::unistd::{ForkResult, execvp, fork};
use std::ffi::CString;
use std::io::{self, Write};
use std::process;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    main_loop()
}

struct Command {
    program: String,
    args: Vec<String>,
}

fn main_loop() -> Result<(), Box<dyn std::error::Error>> {
    // 函数可能返回任意实现了 std::error::Error 的错误。适合早期开发
    loop {
        print!("shell> ");
        io::stdout().flush()?; // flush也会返回result

        let mut line = String::new();

        io::stdin().read_line(&mut line)?; // 直接返回错误

        let line = line.trim(); // 移除前后缀的空白符号,返回&str

        if line.is_empty() {
            continue;
        }

        let command = match parse_args(line) {
            Some(command) => command,
            None => continue,
        };

        fn should_print_lifecycle(command: &Command) -> bool {
            command.program != "clear"
        }

        let print_lifecycle = should_print_lifecycle(&command);

        if print_lifecycle {
            println!("{} starting...", &command.program);
        }

        // TODO 这里目前还十分简陋
        let should_exit = match run_builtin(&command) {
            Some(BuiltinResult::Continue) => false,
            Some(BuiltinResult::Exit) => true,
            None => {
                if let Err(err) = run_external(&command) {
                    eprintln!("{}: {}", command.program, err);
                    let _ = io::stderr().flush();
                    // 捕获并打印错误
                } // 希望报告错误后继续运行，所以不直接退出
                false
            }
        };

        if print_lifecycle {
            println!("{} ending.", &command.program);
        }
        if should_exit {
            break;
        }
    }
    Ok(())
}

// 对 shell 来说，空命令不是错误。用户直接按回车，shell 应该继续下一轮
// 所以这里用Option
fn parse_args(line: &str) -> Option<Command> {
    let mut iter = line.split_whitespace().map(|word| word.to_string());
    let program = iter.next()?; // 如果是None直接返回，否则解包
    let args = iter.collect();
    Some(Command { program, args })
}

enum BuiltinResult {
    Continue,
    Exit,
}

fn run_builtin(command: &Command) -> Option<BuiltinResult> {
    match command.program.as_str() {
        "help" => {
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
        "exit" => Some(BuiltinResult::Exit),
        "cd" => {
            run_cd(command);
            Some(BuiltinResult::Continue)
        }
        "pwd" => {
            match std::env::current_dir() {
                Ok(path) => println!("{}", path.display()),
                Err(err) => {
                    eprintln!("pwd: {}", err);
                    let _ = io::stderr().flush();
                }
            };
            Some(BuiltinResult::Continue)
        }
        "env" => {
            run_env();
            Some(BuiltinResult::Continue)
        }
        "export" => {
            run_export(command);
            Some(BuiltinResult::Continue)
        }
        "unset" => {
            run_unset(command);
            Some(BuiltinResult::Continue)
        }
        "clear" => {
            run_clear();
            Some(BuiltinResult::Continue)
        }
        _ => None,
    }
}
type ShellResult<T> = Result<T, Box<dyn std::error::Error>>;

fn run_cd(command: &Command) {
    if command.args.len() > 1 {
        eprintln!("cd: too many arguments");
        let _ = io::stderr().flush();
        return;
    }

    let dir = if command.args.is_empty() {
        // 没有参数，切换到$HOME
        match std::env::var("HOME") {
            // 取出环境变量$HOME
            Ok(home) => home,
            Err(_) => {
                eprintln!("cd: HOME not set");
                let _ = io::stderr().flush();
                return;
            }
        }
    } else {
        command.args[0].clone()
    };

    if let Err(err) = std::env::set_current_dir(&dir) {
        eprintln!("cd: {}: {}", dir, err);
        let _ = io::stderr().flush();
    }
}

fn run_env() {
    for (key, value) in std::env::vars() {
        println!("{}={}", key, value);
    }
}

// 先只支持这种格式: export KEY=value
fn run_export(command: &Command) {
    if command.args.len() != 1 {
        eprintln!("export: usage: export KEY=value");
        let _ = io::stderr().flush();
        return;
    }

    // split_once只在第一个位置分开，和这里功能一样
    let Some((key, value)) = command.args[0].split_once('=') else {
        eprintln!("export: usage: export KEY=value");
        let _ = io::stderr().flush();
        return;
    };

    if key.is_empty() {
        eprintln!("export: usage: export KEY=value");
        let _ = io::stderr().flush();
        return;
    }

    if !is_valid_env_key(key) {
        eprintln!("export: invalid variable name: {}", key);
        let _ = io::stderr().flush();
        return;
    }

    unsafe { std::env::set_var(key, value) };
}

fn is_valid_env_key(key: &str) -> bool {
    let mut chars = key.chars();

    // Empty keys do not match the shell-style environment variable name rule.
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
        eprintln!("unset: usage: unset KEY");
        let _ = io::stderr().flush();
        return;
    }

    // 先校验变量名，避免 remove_var 在非法变量名上 panic。
    let key = &command.args[0];
    if !is_valid_env_key(key) {
        eprintln!("unset: invalid variable name: {}", key);
        let _ = io::stderr().flush();
        return;
    }

    unsafe { std::env::remove_var(key) };
}

// clear 清除当前屏幕，scrollback 是否清除取决于终端
fn run_clear() {
    print!("\x1b[2J\x1b[3J\x1b[H");
    let _ = io::stdout().flush();
}

fn run_external(command: &Command) -> ShellResult<()> {
    let mut argv = Vec::new();
    argv.push(command.program.clone());
    argv.extend(command.args.clone());

    let c_argv: Vec<CString> = argv
        .iter()
        .map(|arg| CString::new(arg.as_str()))
        .collect::<Result<Vec<_>, _>>()?;
    // 这里把Vec<Result<CString, Err>>转成了Result<Vec<CString>, Err>
    // 由于直接返回ShellResult, 所以不用在这里错误处理，直接用？即可

    match unsafe { fork()? } {
        // 这里也用？解包
        ForkResult::Parent { child } => {
            // 父进程，等待子进程退出
            waitpid(child, None)?;
        }
        ForkResult::Child => {
            // 子进程
            // execvp 成功不会回来。execvp 里的 p 表示会根据 PATH 环境变量搜索命令
            match execvp(&c_argv[0], &c_argv) {
                Ok(_) => unreachable!("execvp should not return on success"), // 理论上这个分支永远不会被执行
                Err(err) => {
                    // 失败之后，子进程必须退出
                    eprintln!("{}: execvp failed: {}", command.program, err);
                    let _ = io::stderr().flush();
                    process::exit(127);
                }
            }
        }
    };
    Ok(())
}
