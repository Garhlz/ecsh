use nix::sys::wait::waitpid;
use nix::unistd::{ForkResult, dup2_stdin, dup2_stdout, execvp, fork, pipe};
use std::ffi::CString;
use std::io::{self, Write};
use std::os::fd::{AsRawFd, OwnedFd};
use std::process;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    main_loop()
}

struct Command {
    program: String,
    args: Vec<String>,
}
/*
管道，按标准 shell 语义使用 '|'

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
// TODO 第一版暂时不处理引号，所以 echo "a|b" 会被错误地切开
struct Pipeline {
    commands: Vec<Command>,
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

        // TODO 这里的指令判断可以优化，尤其是数据结构
        // 解析并执行管道
        if line.contains('|') {
            match parse_pipeline(line) {
                Ok(Some(pipeline)) => {
                    let should_break = run_pipeline(&pipeline)?;
                    if should_break {
                        break;
                    }
                    continue; // 如果没有exit，处理完之后继续循环
                }
                Ok(None) => {}
                Err(err) => {
                    eprintln!("pipeline: {}", err);
                    let _ = io::stderr().flush();
                    continue;
                }
            }
        }

        // 解析并执行普通指令
        let command = match parse_args(line) {
            Some(command) => command,
            None => continue,
        };

        // 这里暂时还不会返回错误，但是先加上好了
        let should_break = run_command(&command)?;

        if should_break {
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

fn parse_pipeline(line: &str) -> Result<Option<Pipeline>, String> {
    if !line.contains('|') {
        return Ok(None);
    }

    let commands = line
        .split('|')
        .map(str::trim)
        // 如果parse_args返回None，说明有某段指令是空
        .map(|part| parse_args(part).ok_or_else(|| "empty command in pipeline".to_string()))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Some(Pipeline { commands }))
}

fn run_command(command: &Command) -> Result<bool, Box<dyn std::error::Error>> {
    let print_lifecycle = command.program != "clear";

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

    Ok(should_exit)
}

// 关闭子进程的所有原始 pipe fd
// 避免它们被 execvp 后的新程序继承
fn close_pipeline_fds_in_child(pipes: &[(OwnedFd, OwnedFd)]) {
    for (read_fd, write_fd) in pipes {
        unsafe {
            nix::libc::close(read_fd.as_raw_fd());
            nix::libc::close(write_fd.as_raw_fd());
        }
    }
}

// 表示运行结束之后是否退出
fn run_pipeline(pipeline: &Pipeline) -> Result<bool, Box<dyn std::error::Error>> {
    // TODO 暂时不支持内置指令，所以也不会用exit退出
    if pipeline.commands.iter().any(|cmd| is_builtin(cmd)) {
        eprintln!("pipeline: built-in commands are not supported in pipelines yet");
        return Ok(false);
    }

    let n = pipeline.commands.len();
    if n == 0 {
        return Ok(false);
    }

    // 创建n-1个匿名管道
    let mut pipes = Vec::new();
    for _ in 0..n - 1 {
        let (read_fd, write_fd) = pipe()?;
        pipes.push((read_fd, write_fd));
    }

    let mut children_pids = Vec::new();

    for (i, command) in pipeline.commands.iter().enumerate() {
        match unsafe { fork()? } {
            ForkResult::Parent { child } => {
                children_pids.push(child);
            }
            ForkResult::Child => {
                // 不是第一个命令，把stdin重定向到pipes[i-1]的输入
                if i != 0 {
                    // 如果失败，子进程直接结束
                    if let Err(err) = dup2_stdin(&pipes[i - 1].0) {
                        eprintln!("pipeline: dup2 stdin failed: {}", err);
                        process::exit(127);
                    }
                }
                // 不是最后一个命令，把stdout重定向到pipes[i]的输出
                if i != n - 1 {
                    if let Err(err) = dup2_stdout(&pipes[i].1) {
                        eprintln!("pipeline: dup2 stdout failed: {}", err);
                        process::exit(127);
                    }
                }

                // 需要关闭所有pipe fd。这里如果 drop(pipes)，会把整个 pipes move 掉
                // Rust 写法上，用 raw fd close 更方便
                close_pipeline_fds_in_child(&pipes);

                exec_external_or_exit(command);
            }
        }
    }
    drop(pipes); // 会丢弃 pipes 这个 Vec<(OwnedFd, OwnedFd)>的所有权。OwnedFd的Drop实现会调用close(fd)

    for child in children_pids {
        waitpid(child, None)?;
    }

    Ok(false)
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

fn is_builtin(command: &Command) -> bool {
    matches!(
        command.program.as_str(),
        "help" | "exit" | "cd" | "pwd" | "env" | "export" | "unset" | "clear"
    )
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

fn build_c_argv(command: &Command) -> ShellResult<Vec<CString>> {
    let mut argv = Vec::new();
    argv.push(command.program.clone());
    argv.extend(command.args.clone());

    argv.iter()
        .map(|arg| CString::new(arg.as_str()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| err.into())
    // 这里把Vec<Result<CString, Err>>转成了Result<Vec<CString>, Err>
}

// 构建c_argv，然后执行execvp，如果成功了子进程不会返回
fn exec_external_or_exit(command: &Command) -> ! {
    let c_argv = match build_c_argv(command) {
        Ok(c_argv) => c_argv,
        Err(err) => {
            eprintln!("{}: {}", command.program, err);
            let _ = io::stderr().flush();
            process::exit(127);
        }
    };

    match execvp(&c_argv[0], &c_argv) {
        Ok(_) => unreachable!("execvp should not return on success"),
        Err(err) => {
            eprintln!("{}: execvp failed: {}", command.program, err);
            let _ = io::stderr().flush();
            process::exit(127);
        }
    }
}

fn run_external(command: &Command) -> ShellResult<()> {
    match unsafe { fork()? } {
        // 这里也用？解包
        ForkResult::Parent { child } => {
            // 父进程，等待子进程退出
            waitpid(child, None)?;
        }
        ForkResult::Child => {
            // 子进程
            // execvp 成功不会回来。execvp 里的 p 表示会根据 PATH 环境变量搜索命令
            exec_external_or_exit(command);
        }
    };
    Ok(())
}
