use crate::diagnostics::print_error;
use crate::types::{Command, OutputRedirection, ShellResult};
use nix::fcntl::{OFlag, open};
use nix::sys::stat::Mode;
use nix::unistd::{dup, dup2_stdin, dup2_stdout};
use std::io::{self, Write};
use std::os::fd::OwnedFd;
use std::process;

pub struct SavedRedirection {
    stdin: Option<OwnedFd>,
    stdout: Option<OwnedFd>,
}

// 对内置命令应用临时重定向，并保存原始标准输入输出用于恢复。
//
// 外部命令会在 fork 出来的子进程中处理重定向，子进程退出后 fd 修改自然消失。
// 但 builtin 必须在 shell 进程自身执行，例如 `cd`、`export` 才能影响后续命令。
// 因此 builtin 重定向不能直接改完 fd 就结束，必须先保存原 fd，再在执行后恢复。
pub fn apply_redirection_in_shell(command: &Command) -> ShellResult<SavedRedirection> {
    let mut saved = SavedRedirection {
        stdin: None,
        stdout: None,
    };

    // 用闭包把“可能失败的重定向步骤”包起来，这样失败后可以统一恢复 fd。
    // 如果在中间直接使用 `?` 从函数返回，已经完成的 dup2 就没有机会回滚。
    let result = (|| -> ShellResult<()> {
        if let Some(path) = &command.redirection.stdin {
            // 保存标准输入 fd 的副本，避免重定向永久影响 shell 进程。
            // `dup` 返回一个新的 OwnedFd，它和原 stdin 指向同一个底层打开文件描述。
            let original_stdin = dup(io::stdin())?;
            let fd = open_input_redirection(path)?;

            // dup2_stdin 会让标准输入 fd 指向 `fd` 对应的文件
            dup2_stdin(&fd)?;

            // dup2 完成后，标准输入已经指向目标文件；这里可以关闭临时 fd。
            // OwnedFd 在 drop 时会自动 close，避免手动 close 后的所有权混乱。
            drop(fd);
            saved.stdin = Some(original_stdin);
        }

        if let Some(output_redirection) = &command.redirection.stdout {
            let original_stdout = dup(io::stdout())?;
            let fd = open_output_redirection(output_redirection)?;

            // stdout 被替换后，println! 等写标准输出的代码会写入重定向文件。
            dup2_stdout(&fd)?;
            drop(fd);
            saved.stdout = Some(original_stdout);
        }

        Ok(())
    })();
    // 这里不能直接用 `?` 返回，因为出错时仍需恢复已经修改过的 fd。
    // 闭包让重定向步骤先产出一个 Result，随后再统一处理回滚逻辑。
    if let Err(err) = result {
        let _ = restore_redirection(saved);
        return Err(err);
    }

    Ok(saved)
}

pub fn restore_redirection(saved: SavedRedirection) -> ShellResult<()> {
    if let Some(stdin) = saved.stdin {
        // 把标准输入重新指回保存下来的 fd。恢复后，SavedRedirection 持有的
        // OwnedFd 会在离开作用域时关闭，不会影响已经恢复好的标准 fd。
        dup2_stdin(&stdin)?;
    }

    if let Some(stdout) = saved.stdout {
        // 恢复 stdout 很关键，否则下一轮 shell 提示符也可能被写进重定向文件。
        dup2_stdout(&stdout)?;
    }

    Ok(())
}

pub fn flush_standard_streams() -> ShellResult<()> {
    // stdout 可能已经被重定向到文件。恢复 fd 前先 flush，确保缓冲区内容写入
    // 当前目标，而不是在恢复后被刷到终端。
    io::stdout().flush()?;
    io::stderr().flush()?;
    Ok(())
}

// 子进程不返回错误，执行失败也直接退出，执行成功也不会返回。
//
// 这个函数只用于 fork 后的子进程路径。子进程不能把错误继续返回给 shell 主循环，
// 否则父子进程会继续执行同一套 Rust 控制流；所以失败时打印错误并 exit(127)。
pub fn handle_redirection_or_exit(command: &Command) {
    if let Some(path) = &command.redirection.stdin {
        let fd = match open_input_redirection(path) {
            Ok(fd) => fd,
            Err(err) => {
                print_error(err);
                process::exit(127);
            }
        };

        // 在子进程中不需要保存原 stdin，因为 execvp 成功后会替换进程镜像；
        // execvp 失败时子进程也会立即退出。
        if let Err(err) = dup2_stdin(&fd) {
            print_error(format!("{}: dup2 stdin failed: {}", path, err));
            process::exit(127);
        }
        drop(fd);
    };

    if let Some(output_redirection) = &command.redirection.stdout {
        let fd = match open_output_redirection(output_redirection) {
            Ok(fd) => fd,
            Err(err) => {
                print_error(err);
                process::exit(127);
            }
        };

        // 同理，子进程 stdout 重定向不需要恢复。
        if let Err(err) = dup2_stdout(&fd) {
            print_error(format!("dup2 stdout failed: {}", err));
            process::exit(127);
        }
        drop(fd);
    }
}

// 打开输入重定向文件，并补充 shell 风格的错误上下文。
fn open_input_redirection(path: &str) -> ShellResult<OwnedFd> {
    // 输入重定向要求目标文件已经存在，因此只使用 O_RDONLY。
    open(path, OFlag::O_RDONLY, Mode::empty())
        .map_err(|err| format!("{}: cannot open for input: {}", path, err).into())
}

fn open_output_redirection(output_redirection: &OutputRedirection) -> ShellResult<OwnedFd> {
    match output_redirection {
        // `>`：不存在则创建，存在则清空。
        OutputRedirection::Truncate(path) => open(
            path.as_str(),
            OFlag::O_CREAT | OFlag::O_WRONLY | OFlag::O_TRUNC,
            Mode::from_bits_truncate(0o644),
        )
        .map_err(|err| format!("{}: cannot open for output: {}", path, err).into()),
        // `>>`：不存在则创建，存在则从文件末尾追加。
        OutputRedirection::Append(path) => open(
            path.as_str(),
            OFlag::O_CREAT | OFlag::O_WRONLY | OFlag::O_APPEND,
            Mode::from_bits_truncate(0o644),
        )
        .map_err(|err| format!("{}: cannot open for output: {}", path, err).into()),
    }
}
