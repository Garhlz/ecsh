//! ecscript 错误类型。
//!   - ParseError   ：词法/语法阶段 — 源码有问题，拒绝执行
//!   - RuntimeError ：执行阶段 — 语法没问题但语义出错

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceLocation<'a> {
    line: usize,
    column: usize,
    text: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LineIndex<'a> {
    src: &'a str,
    line_starts: Vec<usize>,
}

impl<'a> LineIndex<'a> {
    fn new(src: &'a str) -> Self {
        let mut line_starts = vec![0];
        for (idx, byte) in src.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(idx + 1);
            }
        }
        Self { src, line_starts }
    }

    fn locate(&self, offset: usize) -> SourceLocation<'a> {
        let offset = offset.min(self.src.len());
        let line_idx = self
            .line_starts
            .partition_point(|line_start| *line_start <= offset)
            .saturating_sub(1);
        let line_start = self.line_starts[line_idx];
        let line_end = self.line_end(line_idx);

        SourceLocation {
            line: line_idx + 1,
            column: self.src[line_start..offset].chars().count() + 1,
            text: &self.src[line_start..line_end],
        }
    }

    fn line_end(&self, line_idx: usize) -> usize {
        let line_start = self.line_starts[line_idx];
        let mut line_end = self
            .line_starts
            .get(line_idx + 1)
            .copied()
            .unwrap_or(self.src.len());
        while line_end > line_start {
            match self.src.as_bytes()[line_end - 1] {
                b'\n' | b'\r' => line_end -= 1,
                _ => break,
            }
        }
        line_end
    }
}

fn display_offset(src: &str, offset: usize) -> usize {
    let offset = offset.min(src.len());
    if offset == src.len() || offset == 0 {
        return offset;
    }

    src[..offset]
        .char_indices()
        .last()
        .map(|(idx, _)| idx)
        .unwrap_or(0)
}

fn format_with_source(kind: &str, offset: usize, message: &str, src: &str) -> String {
    let index = LineIndex::new(src);
    let location = index.locate(display_offset(src, offset));
    let gutter_width = location.line.to_string().len().max(2);
    let caret_padding = " ".repeat(location.column.saturating_sub(1));

    format!(
        "ecscript {} error at {}:{}: {}\n{:>width$} | {}\n{:>width$} | {}^",
        kind,
        location.line,
        location.column,
        message,
        location.line,
        location.text,
        "",
        caret_padding,
        width = gutter_width,
    )
}

/// 词法/语法错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub offset: usize,
    pub message: String,
    /// true if the parser hit EOF while expecting more tokens — used by REPL to detect incomplete input
    pub incomplete: bool,
}

impl ParseError {
    pub fn new(offset: usize, message: impl Into<String>) -> Self {
        Self {
            offset,
            message: message.into(),
            incomplete: false,
        }
    }

    pub fn incomplete(offset: usize, message: impl Into<String>) -> Self {
        Self {
            offset,
            message: message.into(),
            incomplete: true,
        }
    }

    pub fn format_with_source(&self, src: &str) -> String {
        format_with_source("parse", self.offset, &self.message, src)
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ecscript parse error at byte {}: {}",
            self.offset, self.message
        )
    }
}

impl std::error::Error for ParseError {}

/// 运行时错误分类。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeErrorKind {
    /// 变量未定义
    UndefinedVariable,
    /// 类型不匹配，如 `1 + "hello"` 或 `!0`
    TypeMismatch,
    /// 除零
    DivisionByZero,
    // 类型在当前作用域已经定义
    DuplicateVariable,
    // 数组越界
    IndexOutOfBounds,
    // 字段不存在
    NonExistentField,
    NotCallable,
    ArityMismatch,
    CircularReference,
    IoError,
    BreakOutsideLoop,
    ContinueOutsideLoop,
    ReturnOutsideFunction,
}

/// 运行时错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeError {
    pub offset: usize,
    pub kind: RuntimeErrorKind,
    pub message: String,
}

impl RuntimeError {
    pub fn new(offset: usize, kind: RuntimeErrorKind, message: impl Into<String>) -> Self {
        Self {
            offset,
            kind,
            message: message.into(),
        }
    }

    pub fn format_with_source(&self, src: &str) -> String {
        format_with_source("runtime", self.offset, &self.message, src)
    }
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ecscript runtime error at byte {}: {}",
            self.offset, self.message
        )
    }
}

impl std::error::Error for RuntimeError {}

/// 表达式求值的结果类型。
pub type EvalResult<T> = Result<T, RuntimeError>;
/*
在入口层拿到源码字符串和错误对象后，
print_error(err.format_with_source(src));
*/
#[cfg(test)]
mod tests {
    use super::{ParseError, RuntimeError, RuntimeErrorKind};

    #[test]
    fn parse_error_formats_with_line_column_and_caret() {
        let src = "let a = 1;\nlet b = 2;\nlet x = add(1, 2;\n";
        let err = ParseError::new(src.rfind(';').unwrap() + 1, "expected ')'");

        assert_eq!(
            err.format_with_source(src),
            "ecscript parse error at 3:17: expected ')'\n 3 | let x = add(1, 2;\n   |                 ^"
        );
    }

    #[test]
    fn runtime_error_formats_with_line_column_and_caret() {
        let src = "let x = 1;\nlet y = missing;\n";
        let err = RuntimeError::new(
            src.find("missing").unwrap() + "missing".len(),
            RuntimeErrorKind::UndefinedVariable,
            "undefined variable 'missing'",
        );

        assert_eq!(
            err.format_with_source(src),
            "ecscript runtime error at 2:15: undefined variable 'missing'\n 2 | let y = missing;\n   |               ^"
        );
    }

    #[test]
    fn eof_error_points_to_end_of_line() {
        let src = "func add(a, b) {\n    return a + b;\n";
        let err = ParseError::new(src.len(), "expected '}'");

        assert_eq!(
            err.format_with_source(src),
            "ecscript parse error at 3:1: expected '}'\n 3 | \n   | ^"
        );
    }
}
