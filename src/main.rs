use nix::sys::wait::waitpid;
use nix::unistd::{ForkResult, execvp, fork};
use std::ffi::CString;
use std::io::{self, Write};
use std::process;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    main_loop()
}

fn main_loop() -> Result<(), Box<dyn std::error::Error>> {
    // 函数可能返回任意实现了 std::error::Error 的错误。适合实验项目和早期开发。
    loop {
        print!("shell> ");
        io::stdout().flush()?; // flush也会返回result

        let mut line = String::new();

        io::stdin().read_line(&mut line)?; // 直接返回错误

        let line = line.trim(); // 移除前后缀的空白符号,返回&str

        if line.is_empty() {
            continue;
        }

        let args = parse_args(line);

        if args.is_empty() {
            continue;
        }

        // let first = args.iter().next().ok_or(err)

        let command_name = &args[0];

        println!("{} starting...", command_name);

        let should_exit = match run_builtin(&args) {
            Some(BuiltinResult::Continue) => false,
            Some(BuiltinResult::Exit) => true,
            None => {
                run_external(&args);
                false
            }
        };

        println!("{} ending.", command_name);

        if should_exit {
            break;
        }
    }
    Ok(())
}

fn parse_args(line: &str) -> Vec<String> {
    line.split_whitespace()
        .map(|word| word.to_string())
        .collect()
}

enum BuiltinResult {
    Continue,
    Exit,
}

// 目前仅支持help和exit指令
fn run_builtin(args: &[String]) -> Option<BuiltinResult> {
    match args[0].as_str() {
        "help" => {
            println!("Simple shell builtins:");
            println!("  help - show this help message");
            println!("  exit - exit the shell");
            Some(BuiltinResult::Continue)
        }
        "exit" => Some(BuiltinResult::Exit),
        _ => None,
    }
}

fn run_external(args: &[String]) {
    let c_args: Vec<CString> = args
        .iter()
        .map(|arg| CString::new(arg.as_str()).unwrap())
        // unwrap()强行解包，如果Err / None就panic
        // TODO 优化错误处理，之后再统一进行。考虑返回值都用Result包装
        .collect();
    match unsafe { fork() } {
        Ok(ForkResult::Parent { child }) => {
            // 父进程
            // 等待子进程退出
            if let Err(err) = waitpid(child, None) {
                eprintln!("waitpid failed: {}", err)
            }
        }
        Ok(ForkResult::Child) => {
            // 子进程
            // execvp 成功不会回来。execvp 里的 p 表示会根据 PATH 环境变量搜索命令
            match execvp(&c_args[0], &c_args) {
                Ok(_) => unreachable!(),
                Err(err) => {
                    eprintln!("execvp failed: {}", err);
                    process::exit(127);
                }
            }
        }

        Err(err) => {
            // 失败
            eprintln!("fork failed: {}", err);
        }
    }
}
