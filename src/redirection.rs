//! 重定向处理：`<` 输入重定向、`>` / `>>` 输出重定向。
//!
//! 分两条路径：
//!   1. shell 进程内（builtin 用）→ apply → run → flush → restore 五步走
//!   2. 子进程（外部命令用）→ 直接 dup2，不恢复（进程 exec 或 exit 后 fd 自动消失）
//!
//! 涉及的 POSIX 调用：
//!   - open(path, flags, mode) → 打开文件，返回 fd
//!   - dup(fd)       → 复制 fd，返回指向同一文件的新 fd（用于保存原始 stdin/stdout）
//!   - dup2_stdin(fd) → 让 stdin(0) 指向 fd
//!   - dup2_stdout(fd)→ 让 stdout(1) 指向 fd

use crate::diagnostics::print_error;
use crate::types::{Command, OutputRedirection, ShellResult, ShellWord};
use nix::fcntl::{OFlag, open};
use nix::sys::stat::Mode;
use nix::unistd::{dup, dup2_stdin, dup2_stdout};
use std::io::{self, Write};
use std::os::fd::OwnedFd;
use std::process;

/// 保存被重定向覆盖前的原始 stdin/stdout fd。
///
/// 每个字段是原 fd 的副本（通过 dup 创建），之后通过 dup2 恢复。
pub struct SavedRedirection {
    stdin: Option<OwnedFd>,
    stdout: Option<OwnedFd>,
}

/// 在 shell 进程内应用命令的重定向，返回保存的原始 fd。
///
/// 步骤：
///   1. 如果命令有 `<`：dup 保存原始 stdin → open 打开目标文件 → dup2_stdin 替换
///   2. 如果命令有 `>` / `>>`：同样步䠤处理 stdout
///   3. 如果任何步骤失败：恢复已修改的 fd（回滚），再返回错误
///
/// 用闭包包"可能失败的步骤"，是为了失败时统一回滚，
/// 而不是在中间 `?` 返回后遗留已修改的 fd。
pub fn apply_redirection_in_shell(command: &Command) -> ShellResult<SavedRedirection> {
    let mut saved = SavedRedirection {
        stdin: None,
        stdout: None,
    };

    let result = (|| -> ShellResult<()> {
        if let Some(path) = &command.redirection.stdin {
            // dup(io::stdin()) 复制当前 stdin fd，返回与它指向同一文件的新 fd。
            // 之后通过这个保存的副本来恢复。
            let original_stdin = dup(io::stdin())?;
            let fd = open_input_redirection(path)?;

            // dup2_stdin(&fd)：让 fd 0（stdin）指向 fd 所描述的文件。
            // 从此 println!/read_line 都作用于重定向文件。
            dup2_stdin(&fd)?;
            drop(fd);
            saved.stdin = Some(original_stdin);
        }

        if let Some(output_redirection) = &command.redirection.stdout {
            let original_stdout = dup(io::stdout())?;
            let fd = open_output_redirection(output_redirection)?;

            dup2_stdout(&fd)?;
            drop(fd);
            saved.stdout = Some(original_stdout);
        }

        Ok(())
    })();

    if let Err(err) = result {
        let _ = restore_redirection(saved);
        return Err(err);
    }

    Ok(saved)
}

/// 恢复之前被重定向修改的 stdin/stdout。
///
/// 把保存的原始 fd 通过 dup2 重新指回 stdin(0) 和 stdout(1)。
/// OwnedFd 在离开作用域时自动 close，不会影响已恢复的标准 fd。
pub fn restore_redirection(saved: SavedRedirection) -> ShellResult<()> {
    if let Some(stdin) = saved.stdin {
        dup2_stdin(&stdin)?;
    }

    if let Some(stdout) = saved.stdout {
        dup2_stdout(&stdout)?;
    }

    Ok(())
}

/// 刷新 stdout 和 stderr 的缓冲区。
///
/// 在恢复重定向之前必须调用，确保缓冲区内容写入到重定向目标文件，
/// 而不是在恢复后才被刷到终端。
pub fn flush_standard_streams() -> ShellResult<()> {
    io::stdout().flush()?;
    io::stderr().flush()?;
    Ok(())
}

/// 在子进程中应用命令的重定向。此函数不返回——成功则 exec 替换进程镜像，失败则 exit(127)。
///
/// 为什么子进程不需要 save/restore？
///   - 成功路径：execvp 后用新程序的镜像替换了当前进程，不需要恢复
///   - 失败路径：子进程直接 exit(127)，fd 随进程终止被内核回收
pub fn handle_redirection_or_exit(command: &Command) {
    if let Some(path) = &command.redirection.stdin {
        let fd = match open_input_redirection(path) {
            Ok(fd) => fd,
            Err(err) => {
                print_error(err);
                process::exit(127);
            }
        };

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

        if let Err(err) = dup2_stdout(&fd) {
            print_error(format!("dup2 stdout failed: {}", err));
            process::exit(127);
        }
        drop(fd);
    }
}

/// 打开输入重定向文件（`<`）。
///
/// open(path, O_RDONLY)：只读模式打开。文件必须已存在。
fn open_input_redirection(path: &ShellWord) -> ShellResult<OwnedFd> {
    let path = redirection_path(path, "<")?;
    open(path, OFlag::O_RDONLY, Mode::empty())
        .map_err(|err| format!("{}: cannot open for input: {}", path, err).into())
}

/// 打开输出重定向文件（`>` 或 `>>`）。
///
///   - `>` : O_CREAT | O_WRONLY | O_TRUNC → 不存在则创建，存在则清空
///   - `>>`: O_CREAT | O_WRONLY | O_APPEND → 不存在则创建，存在则追加
///   - mode: 0o644 → rw-r--r--（所有者可读写，组和其他可读）
fn open_output_redirection(output_redirection: &OutputRedirection) -> ShellResult<OwnedFd> {
    match output_redirection {
        OutputRedirection::Truncate(path) => open(
            redirection_path(path, ">")?,
            OFlag::O_CREAT | OFlag::O_WRONLY | OFlag::O_TRUNC,
            Mode::from_bits_truncate(0o644),
        )
        .map_err(|err| format!("{}: cannot open for output: {}", path, err).into()),
        OutputRedirection::Append(path) => open(
            redirection_path(path, ">>")?,
            OFlag::O_CREAT | OFlag::O_WRONLY | OFlag::O_APPEND,
            Mode::from_bits_truncate(0o644),
        )
        .map_err(|err| format!("{}: cannot open for output: {}", path, err).into()),
    }
}

fn redirection_path<'a>(path: &'a ShellWord, operator: &str) -> ShellResult<&'a str> {
    path.as_lit_str().ok_or_else(|| {
        format!(
            "internal error: {} redirection target should already be expanded to a literal path",
            operator
        )
        .into()
    })
}
