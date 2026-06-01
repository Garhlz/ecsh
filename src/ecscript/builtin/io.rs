use crate::ecscript::{
    error::{RuntimeError, RuntimeErrorKind},
    io_state,
    value::{Value, display_value},
};
use std::io::{self, Write};

pub(super) fn format_print_args(args: &[Value]) -> String {
    args.iter().map(display_value).collect::<Vec<_>>().join(" ")
}

// 将文本写到 stdout，同时通过 `io_state::note_output` 记录。
// `newline` 参数控制末尾是否追加换行。
pub(super) fn write_stdout(text: &str, newline: bool, span: usize) -> Result<(), RuntimeError> {
    let mut stdout = io::stdout().lock();
    if newline {
        writeln!(stdout, "{}", text).map_err(|err| {
            RuntimeError::new(
                span,
                RuntimeErrorKind::IoError,
                format!("stdout write failed: {}", err),
            )
        })?;
    } else {
        write!(stdout, "{}", text).map_err(|err| {
            RuntimeError::new(
                span,
                RuntimeErrorKind::IoError,
                format!("stdout write failed: {}", err),
            )
        })?;
        stdout.flush().map_err(|err| {
            RuntimeError::new(
                span,
                RuntimeErrorKind::IoError,
                format!("stdout flush failed: {}", err),
            )
        })?;
    }
    io_state::note_output(text, newline);
    Ok(())
}
