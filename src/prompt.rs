//! Shell 提示符生成：用户@主机:目录 [状态码]
//!
//! prompt 分两行：
//!   第 1 行：[ecsh] user@host:~/path [exit_code]   — 环境信息
//!   第 2 行：$                                       — 输入提示符
//!
//! 只有在 stdout 是真实终端时才输出 ANSI 颜色，
//! 重定向到文件或管道时输出纯文本。

use crate::types::{ShellResult, ShellState};
use nix::unistd::{gethostname, isatty};
use std::io;

const ANSI_RESET: &str = "\x1b[0m";
const ANSI_BOLD_MAGENTA: &str = "\x1b[1;35m";
const ANSI_BOLD_GREEN: &str = "\x1b[1;32m";
const ANSI_BOLD_CYAN: &str = "\x1b[1;36m";
const ANSI_BOLD_BLUE: &str = "\x1b[1;34m";
const ANSI_BOLD_RED: &str = "\x1b[1;31m";
const ANSI_BOLD_YELLOW: &str = "\x1b[1;33m";

/// 构建 prompt 字符串。
///
/// 格式：`[ecsh] user@host:~/path [exit_code]\n$ `
/// 上一条命令成功（code=0）时不显示状态码，避免 prompt 长期被冗余信息占据。
pub fn build_prompt(state: &ShellState) -> ShellResult<String> {
    let cwd = format_cwd()?;
    let user = format_user();
    let host = format_host();
    let use_color = isatty(io::stdout())?;
    let mut prompt = String::new();

    // shell 标识：醒目区分 shell 类型。
    prompt.push_str(color_prefix(use_color, ANSI_BOLD_MAGENTA));
    prompt.push_str("[ecsh]");
    prompt.push_str(color_prefix(use_color, ANSI_RESET));
    prompt.push(' ');

    // 第一行：user@host:目录
    prompt.push_str(color_prefix(use_color, ANSI_BOLD_GREEN));
    prompt.push_str(&user);
    prompt.push('@');
    prompt.push_str(color_prefix(use_color, ANSI_BOLD_CYAN));
    prompt.push_str(&host);
    prompt.push_str(color_prefix(use_color, ANSI_RESET));
    prompt.push(':');

    prompt.push_str(color_prefix(use_color, ANSI_BOLD_BLUE));
    prompt.push_str(&cwd);
    prompt.push_str(color_prefix(use_color, ANSI_RESET));

    // 非零退出码用红色标注。
    if state.last_status.code != 0 {
        prompt.push(' ');
        prompt.push_str(color_prefix(use_color, ANSI_BOLD_RED));
        prompt.push_str(&format!("[{}]", state.last_status.code));
        prompt.push_str(color_prefix(use_color, ANSI_RESET));
    }

    // 第二行：输入提示符。
    prompt.push('\n');
    prompt.push_str(color_prefix(use_color, ANSI_BOLD_YELLOW));
    prompt.push_str("$ ");
    prompt.push_str(color_prefix(use_color, ANSI_RESET));
    Ok(prompt)
}

/// 格式化当前目录，将 HOME 路径替换为 `~`。
///
/// 例如：`/home/elaine/work` → `~/work`
/// 当前目录正好是 HOME 时直接输出 `~`。
fn format_cwd() -> ShellResult<String> {
    let pwd = std::env::current_dir()?;
    let pwd_str = pwd.to_str().ok_or_else(|| "pwd error")?;
    let home = std::env::var("HOME")?;

    let mut cwd = String::new();

    if pwd_str == home.as_str() {
        cwd.push_str("~");
    } else if pwd_str.starts_with(&home) {
        cwd.push('~');
        let suffix = pwd_str.strip_prefix(&home).ok_or_else(|| "pwd error")?;
        cwd.push_str(suffix);
    } else {
        cwd.push_str(pwd_str);
    }
    Ok(cwd)
}

/// 获取用户名：取 $USER 环境变量，不存在时 fallback 为 "unknown"。
fn format_user() -> String {
    std::env::var("USER").unwrap_or_else(|_| "unknown".to_string())
}

/// 获取主机名：先尝试 $HOSTNAME 环境变量，再尝试 gethostname() 系统调用。
///
/// gethostname() 是 POSIX 系统调用，返回内核中配置的主机名。
/// SSH 场景下 HOSTNAME 可能未设置，因此 gethostname 是可靠兜底。
fn format_host() -> String {
    if let Ok(host) = std::env::var("HOSTNAME") {
        if !host.is_empty() {
            return host;
        }
    }

    gethostname()
        .ok()
        .and_then(|host| host.into_string().ok())
        .filter(|host| !host.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

/// 根据 use_color 开关决定输出颜色代码还是空字符串。
fn color_prefix<'a>(use_color: bool, color: &'a str) -> &'a str {
    if use_color { color } else { "" }
}
