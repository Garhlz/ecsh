use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

use crate::ecscript::{
    error::{RuntimeError, RuntimeErrorKind},
    value::{Function, Value},
};
use crate::types::ShellState;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub(super) enum ParamType {
    Any,
    Nil,
    Bool,
    Int,
    Float,
    String,
    Array,
    Object,
    Function,
    Command,
    OneOf(&'static [ParamType]),
}

impl ParamType {
    fn matches(self, value: &Value) -> bool {
        match self {
            ParamType::Any => true,
            ParamType::Nil => matches!(value, Value::Nil),
            ParamType::Bool => matches!(value, Value::Bool(_)),
            ParamType::Int => matches!(value, Value::Int(_)),
            ParamType::Float => matches!(value, Value::Float(_)),
            ParamType::String => matches!(value, Value::String(_)),
            ParamType::Array => matches!(value, Value::Array(_)),
            ParamType::Object => matches!(value, Value::Object(_)),
            ParamType::Function => matches!(value, Value::Function(_)),
            ParamType::Command => matches!(value, Value::Command(_)),
            ParamType::OneOf(types) => types.iter().any(|ty| ty.matches(value)),
        }
    }

    fn display(self) -> String {
        match self {
            ParamType::Any => "Any".to_string(),
            ParamType::Nil => "Nil".to_string(),
            ParamType::Bool => "Bool".to_string(),
            ParamType::Int => "Int".to_string(),
            ParamType::Float => "Float".to_string(),
            ParamType::String => "String".to_string(),
            ParamType::Array => "Array".to_string(),
            ParamType::Object => "Object".to_string(),
            ParamType::Function => "Function".to_string(),
            ParamType::Command => "Command".to_string(),
            ParamType::OneOf(types) => display_type_list(types),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ParamSpec {
    name: &'static str,
    ty: ParamType,
}

pub(super) const fn param(name: &'static str, ty: ParamType) -> ParamSpec {
    ParamSpec { name, ty }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub(super) enum Arity {
    Exact(usize),
    AtLeast(usize),
    Range { min: usize, max: usize },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Signature {
    name: &'static str,
    params: &'static [ParamSpec],
    variadic: Option<ParamType>,
    arity: Arity,
}

impl Signature {
    pub(super) const fn exact(name: &'static str, params: &'static [ParamSpec]) -> Self {
        Self {
            name,
            params,
            variadic: None,
            arity: Arity::Exact(params.len()),
        }
    }

    pub(super) const fn at_least(
        name: &'static str,
        params: &'static [ParamSpec],
        variadic: Option<ParamType>,
        min: usize,
    ) -> Self {
        Self {
            name,
            params,
            variadic,
            arity: Arity::AtLeast(min),
        }
    }

    #[allow(dead_code)]
    pub(super) const fn range(
        name: &'static str,
        params: &'static [ParamSpec],
        min: usize,
        max: usize,
    ) -> Self {
        Self {
            name,
            params,
            variadic: None,
            arity: Arity::Range { min, max },
        }
    }
}

pub(super) fn check_signature(
    sig: &Signature,
    args: &[Value],
    span: usize,
) -> Result<(), RuntimeError> {
    check_arity(sig, args.len(), span)?;

    for (arg, param) in args.iter().zip(sig.params.iter()) {
        if !param.ty.matches(arg) {
            return Err(type_error(sig.name, param.name, param.ty, arg, span));
        }
    }

    if let Some(variadic) = sig.variadic {
        for arg in &args[sig.params.len()..] {
            if !variadic.matches(arg) {
                return Err(type_error(sig.name, "value", variadic, arg, span));
            }
        }
    }

    Ok(())
}

fn check_arity(sig: &Signature, actual: usize, span: usize) -> Result<(), RuntimeError> {
    let ok = match sig.arity {
        Arity::Exact(expected) => actual == expected,
        Arity::AtLeast(min) => actual >= min,
        Arity::Range { min, max } => actual >= min && actual <= max,
    };
    if ok {
        return Ok(());
    }

    Err(RuntimeError::new(
        span,
        RuntimeErrorKind::ArityMismatch,
        arity_message(sig.name, sig.arity, actual),
    ))
}

fn arity_message(name: &str, arity: Arity, actual: usize) -> String {
    match arity {
        Arity::Exact(expected) => {
            let noun = if expected == 1 {
                "argument"
            } else {
                "arguments"
            };
            format!("{name} expects {expected} {noun}, got {actual}")
        }
        Arity::AtLeast(min) => {
            format!("{name} expects at least {min} arguments, got {actual}")
        }
        Arity::Range { min, max } => {
            format!("{name} expects {min} to {max} arguments, got {actual}")
        }
    }
}

fn type_error(
    builtin_name: &str,
    param_name: &str,
    expected: ParamType,
    actual: &Value,
    span: usize,
) -> RuntimeError {
    RuntimeError::new(
        span,
        RuntimeErrorKind::TypeMismatch,
        format!(
            "{builtin_name} argument '{param_name}' expects {}, got {}",
            expected.display(),
            actual.type_name()
        ),
    )
}

fn display_type_list(types: &[ParamType]) -> String {
    match types {
        [] => String::new(),
        [one] => one.display(),
        [first, second] => format!("{} or {}", first.display(), second.display()),
        _ => {
            let mut rendered = String::new();
            for (idx, ty) in types.iter().enumerate() {
                if idx > 0 {
                    if idx == types.len() - 1 {
                        rendered.push_str(", or ");
                    } else {
                        rendered.push_str(", ");
                    }
                }
                rendered.push_str(&ty.display());
            }
            rendered
        }
    }
}

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

pub(super) fn expect_shell_state<'a>(
    shell_state: Option<&'a ShellState>,
    span: usize,
    builtin_name: &str,
) -> Result<&'a ShellState, RuntimeError> {
    shell_state.ok_or_else(|| {
        RuntimeError::new(
            span,
            RuntimeErrorKind::IoError,
            format!("{builtin_name} requires interactive ecsh shell context"),
        )
    })
}

pub(super) fn object_string_map_from_value(
    value: &Value,
    span: usize,
    builtin_name: &str,
) -> Result<BTreeMap<String, String>, RuntimeError> {
    let Value::Object(obj) = value else {
        unreachable!()
    };

    let mut out = BTreeMap::new();
    for (key, value) in obj.borrow().iter() {
        let Value::String(text) = value else {
            return Err(RuntimeError::new(
                span,
                RuntimeErrorKind::TypeMismatch,
                format!(
                    "{builtin_name} expects Object<String>; key '{}' has {}",
                    key,
                    value.type_name()
                ),
            ));
        };
        out.insert(key.clone(), text.clone());
    }
    Ok(out)
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
