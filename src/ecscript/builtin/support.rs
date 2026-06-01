use std::{cell::RefCell, rc::Rc};

use crate::ecscript::{
    error::{RuntimeError, RuntimeErrorKind},
    value::{Function, Value},
};

// 内建参数校验的统一入口：立即比较 `args.len()` 和期望值，
// 不一致就报告 `ArityMismatch`。
pub(super) fn expect_arity(
    args: &[Value],
    count: usize,
    span: usize,
    builtin_name: &str,
) -> Result<(), RuntimeError> {
    if args.len() != count {
        let noun = if count == 1 { "argument" } else { "arguments" };
        Err(RuntimeError::new(
            span,
            RuntimeErrorKind::ArityMismatch,
            format!(
                "{} expects {} {}, got {}",
                builtin_name,
                count,
                noun,
                args.len()
            ),
        ))
    } else {
        Ok(())
    }
}

// 把参数解出 `Rc<RefCell<Vec<Value>>>`，失败时报类型错误。
// 返回的是 `Rc` clone，调用方可自由控制 `RefCell` 借用时机。
pub(super) fn expect_array(
    arg: &Value,
    span: usize,
    builtin_name: &str,
) -> Result<Rc<RefCell<Vec<Value>>>, RuntimeError> {
    let Value::Array(arr) = arg else {
        return Err(RuntimeError::new(
            span,
            RuntimeErrorKind::TypeMismatch,
            format!("{} expects Array, got {}", builtin_name, arg.type_name()),
        ));
    };
    Ok(arr.clone())
}

// 把参数解出 `Rc<Function>`，失败时报类型错误。
pub(super) fn expect_function(
    arg: &Value,
    span: usize,
    builtin_name: &str,
) -> Result<Rc<Function>, RuntimeError> {
    let Value::Function(func) = arg else {
        return Err(RuntimeError::new(
            span,
            RuntimeErrorKind::TypeMismatch,
            format!("{} expects function, got {}", builtin_name, arg.type_name()),
        ));
    };
    Ok(func.clone())
}

pub(super) fn checked_array_index(
    index: i64,
    len: usize,
    allow_end: bool,
    span: usize,
    _builtin_name: &str,
) -> Result<usize, RuntimeError> {
    crate::ecscript::value::validate_array_index(index, len, allow_end, span)
}

// 把语言值转成命令字面量 `ShellWord`，用于 `command()` builder。
// 当前只接受标量类型（String/Int/Float/Bool/Nil），
// 不接受 Array / Object / Function 等复合类型。
pub(super) fn shell_word_from_value(
    builtin_name: &str,
    value: &Value,
    span: usize,
) -> Result<crate::types::ShellWord, RuntimeError> {
    match value {
        Value::String(text) => Ok(crate::types::ShellWord::lit(text.clone())),
        Value::Int(num) => Ok(crate::types::ShellWord::lit(num.to_string())),
        Value::Float(num) => Ok(crate::types::ShellWord::lit(num.to_string())),
        Value::Bool(flag) => Ok(crate::types::ShellWord::lit(flag.to_string())),
        Value::Nil => Ok(crate::types::ShellWord::lit("nil")),
        other => Err(RuntimeError::new(
            span,
            RuntimeErrorKind::TypeMismatch,
            format!(
                "{builtin_name} only accepts String, Int, Float, Bool or Nil argv parts, got {}",
                other.type_name()
            ),
        )),
    }
}
