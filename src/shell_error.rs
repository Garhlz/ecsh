//! shell 侧错误格式化。
//!
//! 这里先集中放 shell parse 错误的源码定位格式化逻辑，
//! 让 `main.rs` 不再直接承担这些字符串拼装细节。

use crate::ecscript::error::ParseError;

/// 将 shell parse 错误格式化为带源码行和 caret 的输出。
pub fn format_shell_parse_error(src: &str, err: &ParseError) -> String {
    let offset = err.offset.min(src.len());
    let line_start = src[..offset].rfind('\n').map(|idx| idx + 1).unwrap_or(0);
    let line_end = src[offset..]
        .find('\n')
        .map(|idx| offset + idx)
        .unwrap_or(src.len());
    let line = src[..line_start].bytes().filter(|b| *b == b'\n').count() + 1;
    let column = src[line_start..offset].chars().count() + 1;
    let text = &src[line_start..line_end];
    let gutter_width = line.to_string().len().max(2);
    let caret_padding = " ".repeat(column.saturating_sub(1));

    format!(
        "shell parse error at {}:{}: {}\n{:>width$} | {}\n{:>width$} | {}^",
        line,
        column,
        err.message,
        line,
        text,
        "",
        caret_padding,
        width = gutter_width,
    )
}

#[cfg(test)]
mod tests {
    use super::format_shell_parse_error;
    use crate::ecscript::error::ParseError;

    #[test]
    fn formats_single_line_shell_parse_error() {
        let src = r#"echo "unterminated"#;
        let err = ParseError::incomplete(src.len(), "unterminated double quote");
        let rendered = format_shell_parse_error(src, &err);

        assert!(rendered.contains("shell parse error at 1:19"));
        assert!(rendered.contains(r#"echo "unterminated"#));
        assert!(rendered.contains("^"));
    }

    #[test]
    fn formats_multiline_shell_parse_error() {
        let src = "echo ok\ncat <";
        let err = ParseError::new(src.len(), "missing filename after <");
        let rendered = format_shell_parse_error(src, &err);

        assert!(rendered.contains("shell parse error at 2:6"));
        assert!(rendered.contains("cat <"));
    }
}
