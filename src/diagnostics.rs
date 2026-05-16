//! 错误诊断输出：统一在 stderr 打印错误消息并立即 flush。
//!
//! 交互式 shell 需要尽快显示错误信息，
//! 所以每次 print_error 后都 flush stderr，避免缓冲导致的延迟。

use std::fmt;
use std::io::{self, Write};

/// 向 stderr 输出错误信息并立即 flush。
pub fn print_error(message: impl fmt::Display) {
    eprintln!("{}", message);
    let _ = io::stderr().flush();
}
