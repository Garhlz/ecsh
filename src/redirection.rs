//! 输入/输出重定向。
//!
//! shell 进程内执行 builtin 时需要 save/apply/restore；
//! 子进程路径只需要 dup2 到目标 fd，然后继续 exec 或 exit。

use crate::diagnostics::print_error;
use crate::types::{Command, OutputRedirection, ShellResult, ShellWord};
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

/// 在 shell 进程内应用重定向，失败时自动回滚。
pub fn apply_redirection_in_shell(command: &Command) -> ShellResult<SavedRedirection> {
    let mut saved = SavedRedirection {
        stdin: None,
        stdout: None,
    };

    let result = (|| -> ShellResult<()> {
        if let Some(path) = &command.redirection.stdin {
            saved.stdin = Some(redirect_stdin_in_shell(path)?);
        }

        if let Some(output_redirection) = &command.redirection.stdout {
            saved.stdout = Some(redirect_stdout_in_shell(output_redirection)?);
        }

        Ok(())
    })();

    if let Err(err) = result {
        let _ = restore_redirection(saved);
        return Err(err);
    }

    Ok(saved)
}

/// 恢复之前保存的标准输入/输出。
pub fn restore_redirection(saved: SavedRedirection) -> ShellResult<()> {
    if let Some(stdin) = saved.stdin {
        dup2_stdin(&stdin)?;
    }

    if let Some(stdout) = saved.stdout {
        dup2_stdout(&stdout)?;
    }

    Ok(())
}

/// 在恢复 fd 前主动刷新标准输出和标准错误。
pub fn flush_standard_streams() -> ShellResult<()> {
    io::stdout().flush()?;
    io::stderr().flush()?;
    Ok(())
}

/// 在子进程中应用重定向；失败则直接退出 127。
pub fn handle_redirection_or_exit(command: &Command) {
    if let Some(path) = &command.redirection.stdin {
        redirect_stdin_or_exit(path);
    }

    if let Some(output_redirection) = &command.redirection.stdout {
        redirect_stdout_or_exit(output_redirection);
    }
}

/// 以只读方式打开 `< file`。
fn open_input_redirection(path: &ShellWord) -> ShellResult<OwnedFd> {
    let path = redirection_path(path, "<")?;
    open(path, OFlag::O_RDONLY, Mode::empty())
        .map_err(|err| format!("{}: cannot open for input: {}", path, err).into())
}

/// 按 `>` 或 `>>` 的语义打开输出文件。
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

/// 断言重定向目标已经被展开成单个字面路径。
fn redirection_path<'a>(path: &'a ShellWord, operator: &str) -> ShellResult<&'a str> {
    path.as_lit_str().ok_or_else(|| {
        format!(
            "internal error: {} redirection target should already be expanded to a literal path",
            operator
        )
        .into()
    })
}

/// 在 shell 进程内接管 stdin，并返回之后用于恢复的原始 fd。
fn redirect_stdin_in_shell(path: &ShellWord) -> ShellResult<OwnedFd> {
    let original_stdin = dup(io::stdin())?;
    let fd = open_input_redirection(path)?;
    dup2_stdin(&fd)?;
    drop(fd);
    Ok(original_stdin)
}

/// 在 shell 进程内接管 stdout，并返回之后用于恢复的原始 fd。
fn redirect_stdout_in_shell(output_redirection: &OutputRedirection) -> ShellResult<OwnedFd> {
    let original_stdout = dup(io::stdout())?;
    let fd = open_output_redirection(output_redirection)?;
    dup2_stdout(&fd)?;
    drop(fd);
    Ok(original_stdout)
}

/// 在子进程里接管 stdin；失败时直接打印错误并退出。
fn redirect_stdin_or_exit(path: &ShellWord) {
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
}

/// 在子进程里接管 stdout；失败时直接打印错误并退出。
fn redirect_stdout_or_exit(output_redirection: &OutputRedirection) {
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
}
