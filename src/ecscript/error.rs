//! ecscript 错误类型。
//!   - ParseError   ：词法/语法阶段 — 源码有问题，拒绝执行
//!   - RuntimeError ：执行阶段 — 语法没问题但语义出错

/// 词法/语法错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub offset: usize,
    pub message: String,
}

impl ParseError {
    pub fn new(offset: usize, message: impl Into<String>) -> Self {
        Self {
            offset,
            message: message.into(),
        }
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
    BreakOutsideLoop,
    ContinueOutsideLoop,
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
