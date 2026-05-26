use crate::ecscript::{
    error::{RuntimeError, RuntimeErrorKind},
    io_state,
    value::{Builtin, Value, display_value},
};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    io::{self, Write},
    rc::Rc,
};

pub fn lookup_builtin(name: &str) -> Option<Builtin> {
    match name {
        "env" => Some(Builtin::Env),
        "range" => Some(Builtin::Range),
        "len" => Some(Builtin::Len),
        "to_json" => Some(Builtin::ToJson),
        "keys" => Some(Builtin::Keys),
        "values" => Some(Builtin::Values),
        "push" => Some(Builtin::Push),
        "pop" => Some(Builtin::Pop),
        "insert" => Some(Builtin::Insert),
        "remove" => Some(Builtin::Remove),
        "print" => Some(Builtin::Print),
        "println" => Some(Builtin::Println),
        _ => None,
    }
}

pub fn run_builtin(builtin: Builtin, args: Vec<Value>, span: usize) -> Result<Value, RuntimeError> {
    match builtin {
        Builtin::Env => {
            expect_arity(&args, 1, span, "env")?;
            let Value::String(name) = &args[0] else {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!("env expects String, got {}", args[0].type_name()),
                ));
            };

            Ok(match std::env::var(name) {
                Ok(value) => Value::String(value),
                Err(_) => Value::Nil,
            })
        }
        Builtin::Range => {
            expect_arity(&args, 2, span, "range")?;
            let Value::Int(start) = args[0] else {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!("range expects Int start, got {}", args[0].type_name()),
                ));
            };
            let Value::Int(end) = args[1] else {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!("range expects Int end, got {}", args[1].type_name()),
                ));
            };

            let values = if start <= end {
                (start..=end).map(Value::Int).collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            Ok(Value::Array(Rc::new(RefCell::new(values))))
        }
        Builtin::Len => {
            expect_arity(&args, 1, span, "len")?;

            match &args[0] {
                Value::Array(arr) => Ok(Value::Int(arr.borrow().len() as i64)),
                Value::Object(obj) => Ok(Value::Int(obj.borrow().len() as i64)),
                // unicode 标量个数
                Value::String(s) => Ok(Value::Int(s.chars().count() as i64)),

                other => Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!(
                        "len expects Array, Object or String, got {}",
                        other.type_name()
                    ),
                )),
            }
        }
        Builtin::Push => {
            if args.len() < 2 {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::ArityMismatch,
                    format!("push expects at least 2 arguments, got {}", args.len()),
                ));
            }

            let Value::Array(arr) = &args[0] else {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!("push expects Array, got {}", args[0].type_name()),
                ));
            };

            let mut arr_b = arr.borrow_mut();
            for arg in &args[1..] {
                arr_b.push(arg.clone());
            }
            drop(arr_b);

            Ok(Value::Nil)
        }
        Builtin::Pop => {
            expect_arity(&args, 1, span, "pop")?;
            let Value::Array(arr) = &args[0] else {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!("pop expects Array, got {}", args[0].type_name()),
                ));
            };

            let mut arr_b = arr.borrow_mut();
            Ok(arr_b.pop().unwrap_or(Value::Nil))
        }
        Builtin::Insert => {
            expect_arity(&args, 3, span, "insert")?;

            let Value::Array(arr) = &args[0] else {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!("insert expects Array, got {}", args[0].type_name()),
                ));
            };

            let Value::Int(index) = &args[1] else {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!("insert expects Int index, got {}", args[1].type_name()),
                ));
            };

            let insert_at = checked_array_index(*index, arr.borrow().len(), true, span, "insert")?;
            arr.borrow_mut().insert(insert_at, args[2].clone());

            Ok(Value::Nil)
        }
        Builtin::Remove => {
            expect_arity(&args, 2, span, "remove")?;

            let Value::Array(arr) = &args[0] else {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!("remove expects Array, got {}", args[0].type_name()),
                ));
            };

            let Value::Int(index) = &args[1] else {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!("remove expects Int index, got {}", args[1].type_name()),
                ));
            };

            let mut arr_b = arr.borrow_mut();
            let remove_at = checked_array_index(*index, arr_b.len(), false, span, "remove")?;
            let val = arr_b.remove(remove_at);

            drop(arr_b);

            Ok(val)
        }
        Builtin::Keys => {
            expect_arity(&args, 1, span, "keys")?;

            let Value::Object(obj) = &args[0] else {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!("keys expects Object, got {}", args[0].type_name()),
                ));
            };

            let obj_b = obj.borrow();
            let mut keys = obj_b.keys().cloned().collect::<Vec<String>>();
            keys.sort();
            let keys = keys.into_iter().map(Value::String).collect::<Vec<Value>>();
            Ok(Value::Array(Rc::new(RefCell::new(keys))))
        }
        Builtin::Values => {
            expect_arity(&args, 1, span, "values")?;

            let Value::Object(obj) = &args[0] else {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!("values expects Object, got {}", args[0].type_name()),
                ));
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
        Builtin::ToJson => {
            expect_arity(&args, 1, span, "to_json")?;
            let json = to_json_value(&args[0], span)?;
            Ok(Value::String(json.to_string()))
        }
        Builtin::Print => {
            let text = format_print_args(&args);
            write_stdout(&text, false, span)?;
            Ok(Value::Nil)
        }
        Builtin::Println => {
            let text = format_print_args(&args);
            write_stdout(&text, true, span)?;
            Ok(Value::Nil)
        }
    }
}

fn expect_arity(
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

fn to_json_value(value: &Value, span: usize) -> Result<serde_json::Value, RuntimeError> {
    let mut visiting = HashSet::new();
    to_json_value_inner(value, span, &mut visiting)
}

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
                entries.sort_by(|a, b| a.0.cmp(b.0)); // 保证输出稳定
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

fn checked_array_index(
    index: i64,
    len: usize,
    allow_end: bool,
    span: usize,
    _builtin_name: &str,
) -> Result<usize, RuntimeError> {
    crate::ecscript::value::validate_array_index(index, len, allow_end, span)
}

fn format_print_args(args: &[Value]) -> String {
    args.iter().map(display_value).collect::<Vec<_>>().join(" ")
}

fn write_stdout(text: &str, newline: bool, span: usize) -> Result<(), RuntimeError> {
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

#[cfg(test)]
mod tests {
    use super::{format_print_args, run_builtin};
    use crate::ecscript::{
        error::RuntimeErrorKind,
        value::{Builtin, Value},
    };
    use std::{cell::RefCell, collections::HashMap, rc::Rc};

    #[test]
    fn keys_are_sorted() {
        let obj = Rc::new(RefCell::new(HashMap::from([
            ("b".to_string(), Value::Int(2)),
            ("a".to_string(), Value::Int(1)),
        ])));

        let result = run_builtin(Builtin::Keys, vec![Value::Object(obj)], 0).unwrap();
        let Value::Array(keys) = result else {
            panic!("expected array");
        };

        assert_eq!(
            *keys.borrow(),
            vec![Value::String("a".into()), Value::String("b".into())]
        );
    }

    #[test]
    fn env_reads_environment_variable() {
        let result = run_builtin(Builtin::Env, vec![Value::String("PATH".into())], 0).unwrap();
        assert!(matches!(result, Value::String(_)));
    }

    #[test]
    fn env_returns_nil_for_missing_variable() {
        let result = run_builtin(
            Builtin::Env,
            vec![Value::String("ECSH_TEST_MISSING_ENV_VAR".into())],
            0,
        )
        .unwrap();
        assert_eq!(result, Value::Nil);
    }

    #[test]
    fn env_requires_string_argument() {
        let err = run_builtin(Builtin::Env, vec![Value::Int(1)], 0).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
        assert_eq!(err.message, "env expects String, got Int");
    }

    #[test]
    fn range_returns_closed_interval_array() {
        let result = run_builtin(Builtin::Range, vec![Value::Int(1), Value::Int(4)], 0).unwrap();
        let Value::Array(values) = result else {
            panic!("expected array");
        };
        assert_eq!(
            *values.borrow(),
            vec![Value::Int(1), Value::Int(2), Value::Int(3), Value::Int(4)]
        );
    }

    #[test]
    fn range_returns_empty_when_start_exceeds_end() {
        let result = run_builtin(Builtin::Range, vec![Value::Int(4), Value::Int(1)], 0).unwrap();
        let Value::Array(values) = result else {
            panic!("expected array");
        };
        assert!(values.borrow().is_empty());
    }

    #[test]
    fn range_requires_int_arguments() {
        let err =
            run_builtin(Builtin::Range, vec![Value::Bool(true), Value::Int(1)], 0).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
        assert_eq!(err.message, "range expects Int start, got Bool");

        let err =
            run_builtin(Builtin::Range, vec![Value::Int(1), Value::Bool(true)], 0).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
        assert_eq!(err.message, "range expects Int end, got Bool");
    }

    #[test]
    fn values_follow_sorted_keys() {
        let obj = Rc::new(RefCell::new(HashMap::from([
            ("b".to_string(), Value::Int(2)),
            ("a".to_string(), Value::Int(1)),
        ])));

        let result = run_builtin(Builtin::Values, vec![Value::Object(obj)], 0).unwrap();
        let Value::Array(values) = result else {
            panic!("expected array");
        };

        assert_eq!(*values.borrow(), vec![Value::Int(1), Value::Int(2)]);
    }

    #[test]
    fn insert_checks_negative_index() {
        let arr = Rc::new(RefCell::new(vec![Value::Int(1)]));
        let err = run_builtin(
            Builtin::Insert,
            vec![Value::Array(arr), Value::Int(-1), Value::Int(2)],
            0,
        )
        .unwrap_err();

        assert_eq!(err.kind, RuntimeErrorKind::IndexOutOfBounds);
    }

    #[test]
    fn remove_checks_out_of_bounds_index() {
        let arr = Rc::new(RefCell::new(vec![Value::Int(1)]));
        let err =
            run_builtin(Builtin::Remove, vec![Value::Array(arr), Value::Int(1)], 0).unwrap_err();

        assert_eq!(err.kind, RuntimeErrorKind::IndexOutOfBounds);
    }

    #[test]
    fn insert_reports_index_type() {
        let arr = Rc::new(RefCell::new(vec![Value::Int(1)]));
        let err = run_builtin(
            Builtin::Insert,
            vec![Value::Array(arr), Value::String("x".into()), Value::Int(2)],
            0,
        )
        .unwrap_err();

        assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
        assert_eq!(err.message, "insert expects Int index, got String");
    }

    #[test]
    fn to_json_detects_array_cycle() {
        let arr = Rc::new(RefCell::new(Vec::new()));
        arr.borrow_mut().push(Value::Array(arr.clone()));

        let err = run_builtin(Builtin::ToJson, vec![Value::Array(arr)], 0).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::CircularReference);
        assert_eq!(
            err.message,
            "to_json cannot serialize cyclic Array/Object values"
        );
    }

    #[test]
    fn to_json_sorts_object_keys() {
        let obj = Rc::new(RefCell::new(HashMap::from([
            ("b".to_string(), Value::Int(2)),
            ("a".to_string(), Value::Int(1)),
        ])));

        let result = run_builtin(Builtin::ToJson, vec![Value::Object(obj)], 0).unwrap();
        assert_eq!(result, Value::String("{\"a\":1,\"b\":2}".into()));
    }

    #[test]
    fn format_print_args_uses_display_style() {
        let text = format_print_args(&[
            Value::String("hi".into()),
            Value::Int(42),
            Value::Array(Rc::new(RefCell::new(vec![Value::Bool(true)]))),
        ]);

        assert_eq!(text, "hi 42 [true]");
    }

    #[test]
    fn env_returns_nil_for_missing_var() {
        let var = format!("ECSH_TEST_NONEXIST_{}", std::process::id());
        let result = run_builtin(Builtin::Env, vec![Value::String(var)], 0).unwrap();
        assert_eq!(result, Value::Nil);
    }

    #[test]
    fn env_returns_env_value() {
        unsafe { std::env::set_var("ECSH_TEST_RUNTIME_VAR2", "runtime") };
        let result = run_builtin(
            Builtin::Env,
            vec![Value::String("ECSH_TEST_RUNTIME_VAR2".into())],
            0,
        )
        .unwrap();
        assert_eq!(result, Value::String("runtime".into()));
        unsafe { std::env::remove_var("ECSH_TEST_RUNTIME_VAR2") };
    }

    #[test]
    fn env_rejects_wrong_type() {
        let err = run_builtin(Builtin::Env, vec![Value::Int(1)], 0).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
    }

    #[test]
    fn range_produces_inclusive_range() {
        let result = run_builtin(Builtin::Range, vec![Value::Int(0), Value::Int(3)], 0).unwrap();
        let Value::Array(arr) = result else {
            panic!("expected array")
        };
        assert_eq!(
            *arr.borrow(),
            vec![Value::Int(0), Value::Int(1), Value::Int(2), Value::Int(3)]
        );
    }

    #[test]
    fn range_single_element_when_start_equals_end() {
        let result = run_builtin(Builtin::Range, vec![Value::Int(5), Value::Int(5)], 0).unwrap();
        let Value::Array(arr) = result else {
            panic!("expected array")
        };
        assert_eq!(*arr.borrow(), vec![Value::Int(5)]);
    }

    #[test]
    fn range_reversed_returns_empty() {
        let result = run_builtin(Builtin::Range, vec![Value::Int(5), Value::Int(0)], 0).unwrap();
        let Value::Array(arr) = result else {
            panic!("expected array")
        };
        assert!(arr.borrow().is_empty());
    }
}
