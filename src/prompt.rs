use crate::types::{ShellResult, ShellState};
use nix::unistd::{gethostname, isatty};
use std::io;

// 直接使用 ANSI 转义序列给 prompt 上色。当前先不引入额外库，
// 让颜色逻辑仍然保持成普通字符串拼接。
const ANSI_RESET: &str = "\x1b[0m";
const ANSI_BOLD_MAGENTA: &str = "\x1b[1;35m";
const ANSI_BOLD_GREEN: &str = "\x1b[1;32m";
const ANSI_BOLD_CYAN: &str = "\x1b[1;36m";
const ANSI_BOLD_BLUE: &str = "\x1b[1;34m";
const ANSI_BOLD_RED: &str = "\x1b[1;31m";
const ANSI_BOLD_YELLOW: &str = "\x1b[1;33m";

pub fn build_prompt(state: &ShellState) -> ShellResult<String> {
    let cwd = format_cwd()?;
    let user = format_user();
    let host = format_host();
    let use_color = isatty(io::stdout())?;
    let mut prompt = String::new();

    // 只有 stdout 连接到真实终端时才输出 ANSI 颜色，避免重定向到文件或管道时
    // 把控制序列也一并写进去。
    prompt.push_str(color_prefix(use_color, ANSI_BOLD_MAGENTA));
    prompt.push_str("[ecsh]");
    prompt.push_str(color_prefix(use_color, ANSI_RESET));
    prompt.push(' ');

    // 第一行按“shell 标识 + 用户主机 + 当前目录 + 状态码”的顺序组织，
    // 这样在 SSH 场景下可以先确认 shell 身份，再确认当前机器与路径。
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

    // 成功状态不额外显示，避免 prompt 长期被冗余状态码占据。
    if state.last_status.code != 0 {
        prompt.push(' ');
        prompt.push_str(color_prefix(use_color, ANSI_BOLD_RED));
        prompt.push_str(&format!("[{}]", state.last_status.code));
        prompt.push_str(color_prefix(use_color, ANSI_RESET));
    }

    // 换到第二行再显示真正的输入提示符，让第一行专注于环境信息展示。
    // 结尾恢复默认颜色，避免用户输入继续沿用前面的着色状态。
    prompt.push('\n');
    prompt.push_str(color_prefix(use_color, ANSI_BOLD_YELLOW));
    prompt.push_str("$ ");
    prompt.push_str(color_prefix(use_color, ANSI_RESET));
    Ok(prompt)
}

fn format_cwd() -> ShellResult<String> {
    let pwd = std::env::current_dir()?;
    let pwd_str = pwd.to_str().ok_or_else(|| "pwd error")?;
    let home = std::env::var("HOME")?;

    let mut cwd = String::new();

    if pwd_str == home.as_str() {
        cwd.push_str("~");
    } else if pwd_str.starts_with(&home) {
        cwd.push('~');
        // `pwd_str` 已经是从 Path 借出来的 `&str`，这里直接在切片上做前缀裁剪即可，
        // 不需要像之前那样先额外分配一个新的 String。
        let suffix = pwd_str.strip_prefix(&home).ok_or_else(|| "pwd error")?;
        cwd.push_str(suffix);
    } else {
        cwd.push_str(pwd_str);
    }
    Ok(cwd)
}

fn format_user() -> String {
    std::env::var("USER").unwrap_or_else(|_| "unknown".to_string())
}

fn format_host() -> String {
    if let Ok(host) = std::env::var("HOSTNAME") {
        if !host.is_empty() {
            return host;
        }
    }

    // SSH 场景下不能假设 HOSTNAME 环境变量一定存在，因此再用系统调用兜底。
    gethostname()
        .ok()
        .and_then(|host| host.into_string().ok())
        .filter(|host| !host.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

fn color_prefix<'a>(use_color: bool, color: &'a str) -> &'a str {
    if use_color { color } else { "" }
}
