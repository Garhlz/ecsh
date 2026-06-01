use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};

use crate::ecscript::{
    error::{RuntimeError, RuntimeErrorKind},
    value::Value,
};

// JSON 序列化入口：初始化 visited 集合，委托 `to_json_value_inner` 做递归转换。
pub(super) fn to_json_value(value: &Value, span: usize) -> Result<serde_json::Value, RuntimeError> {
    let mut visiting = HashSet::new();
    to_json_value_inner(value, span, &mut visiting)
}

// JSON 反序列化：将 `serde_json::Value` 映射为语言值。
// number 优先尝试 i64，失败后用 f64。
pub(super) fn from_json_value(
    value: &serde_json::Value,
    span: usize,
) -> Result<Value, RuntimeError> {
    match value {
        serde_json::Value::Null => Ok(Value::Nil),
        serde_json::Value::Bool(b) => Ok(Value::Bool(*b)),
        serde_json::Value::Number(n) => {
            if let Some(int) = n.as_i64() {
                Ok(Value::Int(int))
            } else if let Some(float) = n.as_f64() {
                Ok(Value::Float(float))
            } else {
                Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!("unsupported JSON number {}", n),
                ))
            }
        }
        serde_json::Value::String(s) => Ok(Value::String(s.clone())),
        serde_json::Value::Array(items) => Ok(Value::Array(Rc::new(RefCell::new(
            items
                .iter()
                .map(|item| from_json_value(item, span))
                .collect::<Result<Vec<_>, _>>()?,
        )))),
        serde_json::Value::Object(entries) => {
            let mut map = HashMap::new();
            for (key, value) in entries {
                map.insert(key.clone(), from_json_value(value, span)?);
            }
            Ok(Value::Object(Rc::new(RefCell::new(map))))
        }
    }
}

// 用指针地址作为 visit key，实现 JSON 序列化时的循环引用检测。
// 同一份 Rc 共享同一个 `*const RefCell<...>` 地址，
// 因此能检测到同一 Array/Object 被间接引用时产生的环。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum JsonVisitKey {
    Array(*const RefCell<Vec<Value>>),
    Object(*const RefCell<HashMap<String, Value>>),
}

fn to_json_value_inner(
    value: &Value,
    span: usize,
    visiting: &mut HashSet<JsonVisitKey>,
) -> Result<serde_json::Value, RuntimeError> {
    match value {
        Value::Nil => Ok(serde_json::Value::Null),
        Value::Bool(b) => Ok(serde_json::Value::Bool(*b)),
        Value::Int(i) => Ok(serde_json::Value::Number((*i).into())),
        Value::Float(f) => {
            let n = serde_json::Number::from_f64(*f).ok_or_else(|| {
                RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    "cannot json-encode NaN or infinity",
                )
            })?;
            Ok(serde_json::Value::Number(n))
        }
        Value::String(s) => Ok(serde_json::Value::String(s.clone())),
        Value::Array(arr) => {
            let visit_key = JsonVisitKey::Array(Rc::as_ptr(arr));
            if !visiting.insert(visit_key) {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::CircularReference,
                    "to_json cannot serialize cyclic Array/Object values",
                ));
            }

            let result = {
                let values = arr.borrow();
                let mut out = Vec::with_capacity(values.len());
                for item in values.iter() {
                    out.push(to_json_value_inner(item, span, visiting)?);
                }
                Ok(serde_json::Value::Array(out))
            };

            visiting.remove(&visit_key);
            result
        }
        Value::Object(obj) => {
            let visit_key = JsonVisitKey::Object(Rc::as_ptr(obj));
            if !visiting.insert(visit_key) {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::CircularReference,
                    "to_json cannot serialize cyclic Array/Object values",
                ));
            }

            let result = {
                let values = obj.borrow();
                let mut entries: Vec<_> = values.iter().collect();
                entries.sort_by(|a, b| a.0.cmp(b.0));
                let mut map = serde_json::Map::new();
                for (k, v) in entries {
                    map.insert(k.clone(), to_json_value_inner(v, span, visiting)?);
                }
                Ok(serde_json::Value::Object(map))
            };

            visiting.remove(&visit_key);
            result
        }
        _ => Err(RuntimeError::new(
            span,
            RuntimeErrorKind::TypeMismatch,
            format!("to_json does not support {}", value.type_name()),
        )),
    }
}
