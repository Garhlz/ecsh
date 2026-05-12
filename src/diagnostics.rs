use std::fmt;
use std::io::{self, Write};

// 交互式 shell 需要尽快显示错误；统一在打印后刷新 stderr。
pub fn print_error(message: impl fmt::Display) {
    eprintln!("{}", message);
    let _ = io::stderr().flush();
}
