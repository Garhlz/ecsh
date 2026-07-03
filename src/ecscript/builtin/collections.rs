use std::{cell::RefCell, collections::HashSet, path::PathBuf, rc::Rc};

use crate::ecscript::{
    error::{RuntimeError, RuntimeErrorKind},
    func::call_function_with_ctx,
    value::{display_value, BuiltinContext, Value},
};

use super::support::{
    check_signature, checked_array_index, expect_array, expect_function, param, ParamType,
    Signature,
};

const LEN_TYPES: &[ParamType] = &[ParamType::Array, ParamType::Object, ParamType::String];
const SIG_RANGE: Signature = Signature::exact(
    "range",
    &[param("start", ParamType::Int), param("end", ParamType::Int)],
);
const SIG_LEN: Signature = Signature::exact("len", &[param("value", ParamType::OneOf(LEN_TYPES))]);
const SIG_CLONE: Signature = Signature::exact("clone", &[param("value", ParamType::Any)]);
const SIG_PUSH: Signature = Signature::at_least(
    "push",
    &[param("array", ParamType::Array)],
    Some(ParamType::Any),
    2,
);
const SIG_POP: Signature = Signature::exact("pop", &[param("array", ParamType::Array)]);
const SIG_INSERT: Signature = Signature::exact(
    "insert",
    &[
        param("array", ParamType::Array),
        param("index", ParamType::Int),
        param("value", ParamType::Any),
    ],
);
const SIG_REMOVE: Signature = Signature::exact(
    "remove",
    &[
        param("array", ParamType::Array),
        param("index", ParamType::Int),
    ],
);
const SIG_SLICE: Signature = Signature::exact(
    "slice",
    &[
        param("array", ParamType::Array),
        param("start", ParamType::Int),
        param("end", ParamType::Int),
    ],
);
const SIG_KEYS: Signature = Signature::exact("keys", &[param("object", ParamType::Object)]);
const SIG_VALUES: Signature = Signature::exact("values", &[param("object", ParamType::Object)]);
const SIG_MAP: Signature = Signature::exact(
    "map",
    &[
        param("array", ParamType::Array),
        param("function", ParamType::Function),
    ],
);
const SIG_FILTER: Signature = Signature::exact(
    "filter",
    &[
        param("array", ParamType::Array),
        param("function", ParamType::Function),
    ],
);
const SIG_REDUCE: Signature = Signature::exact(
    "reduce",
    &[
        param("array", ParamType::Array),
        param("initial", ParamType::Any),
        param("function", ParamType::Function),
    ],
);
const SIG_EACH: Signature = Signature::exact(
    "each",
    &[
        param("array", ParamType::Array),
        param("function", ParamType::Function),
    ],
);
const SIG_ANY: Signature = Signature::exact(
    "any",
    &[
        param("array", ParamType::Array),
        param("function", ParamType::Function),
    ],
);
const SIG_ALL: Signature = Signature::exact(
    "all",
    &[
        param("array", ParamType::Array),
        param("function", ParamType::Function),
    ],
);
const SIG_FIND: Signature = Signature::exact(
    "find",
    &[
        param("array", ParamType::Array),
        param("function", ParamType::Function),
    ],
);
const SIG_JOIN: Signature = Signature::exact(
    "join",
    &[
        param("array", ParamType::Array),
        param("separator", ParamType::String),
    ],
);
const SIG_JOIN_PATH: Signature = Signature::exact(
    "join_path",
    &[
        param("left", ParamType::String),
        param("right", ParamType::String),
    ],
);

pub(super) fn range_builtin(args: &[Value], span: usize) -> Result<Value, RuntimeError> {
    check_signature(&SIG_RANGE, args, span)?;
    let Value::Int(start) = args[0] else {
        unreachable!()
    };
    let Value::Int(end) = args[1] else {
        unreachable!()
    };

    let values = if start <= end {
        (start..=end).map(Value::Int).collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    Ok(Value::Array(Rc::new(RefCell::new(values))))
}

pub(super) fn len_builtin(args: &[Value], span: usize) -> Result<Value, RuntimeError> {
    check_signature(&SIG_LEN, args, span)?;
    match &args[0] {
        Value::Array(arr) => Ok(Value::Int(arr.borrow().len() as i64)),
        Value::Object(obj) => Ok(Value::Int(obj.borrow().len() as i64)),
        Value::String(s) => Ok(Value::Int(s.chars().count() as i64)),
        _ => unreachable!(),
    }
}

pub(super) fn clone_builtin(args: &[Value], span: usize) -> Result<Value, RuntimeError> {
    check_signature(&SIG_CLONE, args, span)?;
    let mut visiting = HashSet::new();
    clone_value(&args[0], span, &mut visiting)
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum VisitKey {
    Array(*const RefCell<Vec<Value>>),
    Object(*const RefCell<std::collections::HashMap<String, Value>>),
}

fn clone_value(
    value: &Value,
    span: usize,
    visiting: &mut HashSet<VisitKey>,
) -> Result<Value, RuntimeError> {
    match value {
        Value::Array(arr) => {
            let key = VisitKey::Array(Rc::as_ptr(arr));
            if !visiting.insert(key) {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::CircularReference,
                    "clone cannot copy circular Array reference",
                ));
            }
            let cloned_items = arr
                .borrow()
                .iter()
                .map(|item| clone_value(item, span, visiting))
                .collect::<Result<Vec<_>, _>>()?;
            visiting.remove(&key);
            Ok(Value::Array(Rc::new(RefCell::new(cloned_items))))
        }
        Value::Object(obj) => {
            let key = VisitKey::Object(Rc::as_ptr(obj));
            if !visiting.insert(key) {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::CircularReference,
                    "clone cannot copy circular Object reference",
                ));
            }
            let cloned_fields = obj
                .borrow()
                .iter()
                .map(|(key, value)| Ok((key.clone(), clone_value(value, span, visiting)?)))
                .collect::<Result<std::collections::HashMap<_, _>, RuntimeError>>()?;
            visiting.remove(&key);
            Ok(Value::Object(Rc::new(RefCell::new(cloned_fields))))
        }
        Value::Function(_) | Value::Builtin(_) | Value::Command(_) => Err(RuntimeError::new(
            span,
            RuntimeErrorKind::TypeMismatch,
            format!("clone cannot copy {} values", value.type_name()),
        )),
        other => Ok(other.clone()),
    }
}

pub(super) fn push_builtin(args: &[Value], span: usize) -> Result<Value, RuntimeError> {
    check_signature(&SIG_PUSH, args, span)?;
    let arr = expect_array(&args[0], span, "push")?;
    let mut arr_b = arr.borrow_mut();
    for arg in &args[1..] {
        arr_b.push(arg.clone());
    }
    Ok(Value::Nil)
}

pub(super) fn pop_builtin(args: &[Value], span: usize) -> Result<Value, RuntimeError> {
    check_signature(&SIG_POP, args, span)?;
    let arr = expect_array(&args[0], span, "pop")?;
    Ok(arr.borrow_mut().pop().unwrap_or(Value::Nil))
}

pub(super) fn insert_builtin(args: &[Value], span: usize) -> Result<Value, RuntimeError> {
    check_signature(&SIG_INSERT, args, span)?;
    let arr = expect_array(&args[0], span, "insert")?;
    let Value::Int(index) = &args[1] else {
        unreachable!()
    };
    let insert_at = checked_array_index(*index, arr.borrow().len(), true, span, "insert")?;
    arr.borrow_mut().insert(insert_at, args[2].clone());
    Ok(Value::Nil)
}

pub(super) fn remove_builtin(args: &[Value], span: usize) -> Result<Value, RuntimeError> {
    check_signature(&SIG_REMOVE, args, span)?;
    let arr = expect_array(&args[0], span, "remove")?;
    let Value::Int(index) = &args[1] else {
        unreachable!()
    };
    let mut arr_b = arr.borrow_mut();
    let remove_at = checked_array_index(*index, arr_b.len(), false, span, "remove")?;
    Ok(arr_b.remove(remove_at))
}

pub(super) fn slice_builtin(args: &[Value], span: usize) -> Result<Value, RuntimeError> {
    check_signature(&SIG_SLICE, args, span)?;
    let arr = expect_array(&args[0], span, "slice")?;
    let Value::Int(start) = &args[1] else {
        unreachable!()
    };
    let Value::Int(end) = &args[2] else {
        unreachable!()
    };
    let values = arr.borrow();
    let start = checked_array_index(*start, values.len(), true, span, "slice")?;
    let end = checked_array_index(*end, values.len(), true, span, "slice")?;
    if start > end {
        return Err(RuntimeError::new(
            span,
            RuntimeErrorKind::IndexOutOfBounds,
            format!("slice start {} exceeds end {}", start, end),
        ));
    }
    Ok(Value::Array(Rc::new(RefCell::new(
        values[start..end].to_vec(),
    ))))
}

pub(super) fn keys_builtin(args: &[Value], span: usize) -> Result<Value, RuntimeError> {
    check_signature(&SIG_KEYS, args, span)?;
    let Value::Object(obj) = &args[0] else {
        unreachable!()
    };
    let obj_b = obj.borrow();
    let mut keys = obj_b.keys().cloned().collect::<Vec<String>>();
    keys.sort();
    Ok(Value::Array(Rc::new(RefCell::new(
        keys.into_iter().map(Value::String).collect(),
    ))))
}

pub(super) fn values_builtin(args: &[Value], span: usize) -> Result<Value, RuntimeError> {
    check_signature(&SIG_VALUES, args, span)?;
    let Value::Object(obj) = &args[0] else {
        unreachable!()
    };
    let obj_b = obj.borrow();
    let mut entries = obj_b.iter().collect::<Vec<_>>();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    let values = entries
        .into_iter()
        .map(|(_, value)| value.clone())
        .collect::<Vec<Value>>();
    Ok(Value::Array(Rc::new(RefCell::new(values))))
}

pub(super) fn map_builtin(
    args: &[Value],
    span: usize,
    ctx: BuiltinContext<'_>,
) -> Result<Value, RuntimeError> {
    check_signature(&SIG_MAP, args, span)?;
    let arr = expect_array(&args[0], span, "map")?;
    let func = expect_function(&args[1], span, "map")?;
    let items = arr.borrow().clone();
    let mut result = Vec::with_capacity(items.len());
    for value in items {
        let mapped = call_function_with_ctx(
            func.clone(),
            vec![value],
            ctx.env,
            ctx.shell_state,
            ctx.stdin_text,
            "map callback",
            span,
        )?
        .unwrap_or(Value::Nil);
        result.push(mapped);
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

pub(super) fn filter_builtin(
    args: &[Value],
    span: usize,
    ctx: BuiltinContext<'_>,
) -> Result<Value, RuntimeError> {
    check_signature(&SIG_FILTER, args, span)?;
    let arr = expect_array(&args[0], span, "filter")?;
    let func = expect_function(&args[1], span, "filter")?;
    let items = arr.borrow().clone();
    let mut result = Vec::new();
    for value in items {
        let bool_value = call_function_with_ctx(
            func.clone(),
            vec![value.clone()],
            ctx.env,
            ctx.shell_state,
            ctx.stdin_text,
            "filter callback",
            span,
        )?
        .unwrap_or(Value::Nil);
        let Value::Bool(b) = bool_value else {
            return Err(RuntimeError::new(
                span,
                RuntimeErrorKind::TypeMismatch,
                format!(
                    "filter function expect bool, got {}",
                    bool_value.type_name()
                ),
            ));
        };
        if b {
            result.push(value);
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

pub(super) fn reduce_builtin(
    args: &[Value],
    span: usize,
    ctx: BuiltinContext<'_>,
) -> Result<Value, RuntimeError> {
    check_signature(&SIG_REDUCE, args, span)?;
    let arr = expect_array(&args[0], span, "reduce")?;
    let initial = &args[1];
    let func = expect_function(&args[2], span, "reduce")?;
    let items = arr.borrow().clone();
    let mut acc = initial.clone();
    for item in items {
        acc = call_function_with_ctx(
            func.clone(),
            vec![acc, item],
            ctx.env,
            ctx.shell_state,
            ctx.stdin_text,
            "reduce callback",
            span,
        )?
        .unwrap_or(Value::Nil);
    }
    Ok(acc)
}

pub(super) fn each_builtin(
    args: &[Value],
    span: usize,
    ctx: BuiltinContext<'_>,
) -> Result<Value, RuntimeError> {
    check_signature(&SIG_EACH, args, span)?;
    let arr = expect_array(&args[0], span, "each")?;
    let func = expect_function(&args[1], span, "each")?;
    let items = arr.borrow().clone();
    for item in items {
        let _ = call_function_with_ctx(
            func.clone(),
            vec![item],
            ctx.env,
            ctx.shell_state,
            ctx.stdin_text,
            "each callback",
            span,
        )?;
    }
    Ok(Value::Nil)
}

pub(super) fn any_builtin(
    args: &[Value],
    span: usize,
    ctx: BuiltinContext<'_>,
) -> Result<Value, RuntimeError> {
    check_signature(&SIG_ANY, args, span)?;
    let arr = expect_array(&args[0], span, "any")?;
    let func = expect_function(&args[1], span, "any")?;
    let items = arr.borrow().clone();
    for item in items {
        let b = call_function_with_ctx(
            func.clone(),
            vec![item],
            ctx.env,
            ctx.shell_state,
            ctx.stdin_text,
            "any callback",
            span,
        )?
        .unwrap_or(Value::Nil);
        let Value::Bool(b) = b else {
            return Err(RuntimeError::new(
                span,
                RuntimeErrorKind::TypeMismatch,
                format!("any function expect bool, got {}", b.type_name()),
            ));
        };
        if b {
            return Ok(Value::Bool(true));
        }
    }
    Ok(Value::Bool(false))
}

pub(super) fn all_builtin(
    args: &[Value],
    span: usize,
    ctx: BuiltinContext<'_>,
) -> Result<Value, RuntimeError> {
    check_signature(&SIG_ALL, args, span)?;
    let arr = expect_array(&args[0], span, "all")?;
    let func = expect_function(&args[1], span, "all")?;
    let items = arr.borrow().clone();
    for item in items {
        let b = call_function_with_ctx(
            func.clone(),
            vec![item],
            ctx.env,
            ctx.shell_state,
            ctx.stdin_text,
            "all callback",
            span,
        )?
        .unwrap_or(Value::Nil);
        let Value::Bool(b) = b else {
            return Err(RuntimeError::new(
                span,
                RuntimeErrorKind::TypeMismatch,
                format!("all function expect bool, got {}", b.type_name()),
            ));
        };
        if !b {
            return Ok(Value::Bool(false));
        }
    }
    Ok(Value::Bool(true))
}

pub(super) fn find_builtin(
    args: &[Value],
    span: usize,
    ctx: BuiltinContext<'_>,
) -> Result<Value, RuntimeError> {
    check_signature(&SIG_FIND, args, span)?;
    let arr = expect_array(&args[0], span, "find")?;
    let func = expect_function(&args[1], span, "find")?;
    let items = arr.borrow().clone();
    for item in items {
        let matched = call_function_with_ctx(
            func.clone(),
            vec![item.clone()],
            ctx.env,
            ctx.shell_state,
            ctx.stdin_text,
            "find callback",
            span,
        )?
        .unwrap_or(Value::Nil);
        let Value::Bool(matched) = matched else {
            return Err(RuntimeError::new(
                span,
                RuntimeErrorKind::TypeMismatch,
                format!("find function expect bool, got {}", matched.type_name()),
            ));
        };
        if matched {
            return Ok(item);
        }
    }
    Ok(Value::Nil)
}

pub(super) fn join_builtin(args: &[Value], span: usize) -> Result<Value, RuntimeError> {
    check_signature(&SIG_JOIN, args, span)?;
    let arr = expect_array(&args[0], span, "join")?;
    let Value::String(sep) = &args[1] else {
        unreachable!()
    };
    let items = arr.borrow();
    let text = items
        .iter()
        .map(display_value)
        .collect::<Vec<_>>()
        .join(sep);
    Ok(Value::String(text))
}

pub(super) fn join_path_builtin(args: &[Value], span: usize) -> Result<Value, RuntimeError> {
    check_signature(&SIG_JOIN_PATH, args, span)?;
    let Value::String(left) = &args[0] else {
        unreachable!()
    };
    let Value::String(right) = &args[1] else {
        unreachable!()
    };
    let joined = PathBuf::from(left).join(right);
    Ok(Value::String(joined.to_string_lossy().into_owned()))
}
