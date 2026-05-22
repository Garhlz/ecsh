use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Array(Rc<RefCell<Vec<Value>>>),
    Object(Rc<RefCell<HashMap<String, Value>>>),
    // Func(Rc<Func>),
    Builtin(Builtin),
}

impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Nil => "Nil",
            Value::Bool(_) => "Bool",
            Value::Int(_) => "Int",
            Value::Float(_) => "Float",
            Value::String(_) => "String",
            Value::Array(_) => "Array",
            Value::Object(_) => "Object",
            Value::Builtin(_) => "Builtin",
        }
    }
}

/// 验证数组索引，返回对应的 `usize`。
///
/// `allow_end` 为 true 时允许 `index == len`（用于 insert）；
/// 否则要求 `index < len`（用于读写和 remove）。
pub fn validate_array_index(
    index: i64,
    len: usize,
    allow_end: bool,
    span: usize,
) -> Result<usize, crate::ecscript::error::RuntimeError> {
    use crate::ecscript::error::{RuntimeError, RuntimeErrorKind};
    if index < 0 {
        return Err(RuntimeError::new(
            span,
            RuntimeErrorKind::IndexOutOfBounds,
            format!("array index {} out of bounds for length {}", index, len),
        ));
    }
    let idx = index as usize;
    let max = if allow_end {
        len
    } else {
        len.saturating_sub(1)
    };
    if idx > max && (allow_end || len > 0) || (!allow_end && len == 0) {
        return Err(RuntimeError::new(
            span,
            RuntimeErrorKind::IndexOutOfBounds,
            format!("array index {} out of bounds for length {}", index, len),
        ));
    }
    Ok(idx)
}

#[derive(Debug, Clone, PartialEq)]
pub enum Builtin {
    Len,
    Keys,
    Values,
    ToJson,
    Push,
    Pop,
    Insert,
    Remove,
}
