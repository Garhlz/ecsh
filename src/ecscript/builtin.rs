use crate::ecscript::{
    error::{RuntimeError, RuntimeErrorKind},
    func::call_function,
    io_state,
    value::{Builtin, BuiltinContext, Value, display_value},
};
use crate::executor::{capture_command_invocation, run_command_invocation};

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    io::{self, Write},
    rc::Rc,
};

pub fn lookup_builtin(name: &str) -> Option<Builtin> {
    match name {
        "command" => Some(Builtin::CommandBuilder),
        "env" => Some(Builtin::Env),
        "range" => Some(Builtin::Range),
        "len" => Some(Builtin::Len),
        "to_json" => Some(Builtin::ToJson),
        "from_json" => Some(Builtin::FromJson),
        "keys" => Some(Builtin::Keys),
        "values" => Some(Builtin::Values),
        "push" => Some(Builtin::Push),
        "pop" => Some(Builtin::Pop),
        "insert" => Some(Builtin::Insert),
        "remove" => Some(Builtin::Remove),
        "slice" => Some(Builtin::Slice),
        "print" => Some(Builtin::Print),
        "println" => Some(Builtin::Println),
        "run" => Some(Builtin::Run),
        "capture" => Some(Builtin::Capture),
        "text" => Some(Builtin::Text),
        "lines" => Some(Builtin::Lines),
        "with_env" => Some(Builtin::WithEnv),
        "with_cwd" => Some(Builtin::WithCwd),
        "map" => Some(Builtin::Map),
        "filter" => Some(Builtin::Filter),
        "reduce" => Some(Builtin::Reduce),
        "each" => Some(Builtin::Each),
        "any" => Some(Builtin::Any),
        "all" => Some(Builtin::All),
        "find" => Some(Builtin::Find),
        "join" => Some(Builtin::Join),
        _ => None,
    }
}

pub fn run_builtin(
    builtin: Builtin,
    args: Vec<Value>,
    span: usize,
    ctx: BuiltinContext<'_>,
) -> Result<Value, RuntimeError> {
    match builtin {
        Builtin::CommandBuilder => {
            // `command(program, arg1, ...)` 是纯语言侧的 argv-first builder。
            // 它不解析 shell 语法，也不立即执行；只是把参数序列变成字面量命令值。
            if args.is_empty() {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::ArityMismatch,
                    "command expects at least 1 argument, got 0",
                ));
            }

            let program = shell_word_from_value("command", &args[0], span)?;
            let argv = args[1..]
                .iter()
                .map(|arg| shell_word_from_value("command", arg, span))
                .collect::<Result<Vec<_>, _>>()?;

            Ok(Value::Command(crate::ecscript::value::CommandInvocation {
                command: crate::ecscript::value::CommandValue::Simple(crate::types::Command {
                    program,
                    args: argv,
                    redirection: crate::types::Redirection::default(),
                }),
                cwd_override: None,
                env_override: None,
                stdin_override: None,
            }))
        }
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

            let arr = expect_array(&args[0], span, "push")?;

            let mut arr_b = arr.borrow_mut();
            for arg in &args[1..] {
                arr_b.push(arg.clone());
            }
            drop(arr_b);

            Ok(Value::Nil)
        }
        Builtin::Pop => {
            expect_arity(&args, 1, span, "pop")?;
            let arr = expect_array(&args[0], span, "pop")?;

            let mut arr_b = arr.borrow_mut();
            Ok(arr_b.pop().unwrap_or(Value::Nil))
        }
        Builtin::Insert => {
            expect_arity(&args, 3, span, "insert")?;

            let arr = expect_array(&args[0], span, "insert")?;

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

            let arr = expect_array(&args[0], span, "remove")?;

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
        Builtin::Slice => {
            expect_arity(&args, 3, span, "slice")?;

            let arr = expect_array(&args[0], span, "slice")?;
            let Value::Int(start) = &args[1] else {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!("slice expects Int start, got {}", args[1].type_name()),
                ));
            };
            let Value::Int(end) = &args[2] else {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!("slice expects Int end, got {}", args[2].type_name()),
                ));
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
        Builtin::FromJson => {
            expect_arity(&args, 1, span, "from_json")?;
            let Value::String(text) = &args[0] else {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!("from_json expects String, got {}", args[0].type_name()),
                ));
            };
            let parsed: serde_json::Value = serde_json::from_str(text).map_err(|err| {
                RuntimeError::new(
                    span,
                    RuntimeErrorKind::ParseInExpr,
                    format!("invalid JSON: {}", err),
                )
            })?;
            from_json_value(&parsed, span)
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
        Builtin::Run => {
            // `run(cmd)` 面向“直接执行”场景：继承当前终端，成功返回 `nil`，
            // 非零退出码和信号终止都提升成语言层错误。
            expect_arity(&args, 1, span, "run")?;
            let Value::Command(command) = &args[0] else {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!("run expects Command, got {}", args[0].type_name()),
                ));
            };
            let Some(state) = ctx.shell_state.as_deref() else {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::IoError,
                    "run is not available in this context",
                ));
            };
            let result = run_command_invocation(command, state).map_err(|err| {
                RuntimeError::new(span, RuntimeErrorKind::IoError, err.to_string())
            })?;
            if result.code != 0 || result.signal.is_some() {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::IoError,
                    format!(
                        "command failed with code {}{}",
                        result.code,
                        result
                            .signal
                            .map(|signal| format!(" (signal {})", signal))
                            .unwrap_or_default()
                    ),
                ));
            }
            Ok(Value::Nil)
        }
        Builtin::Capture => {
            // `capture(cmd)` 面向“检查结果”场景：保留 stdout/stderr/code/signal，
            // 非零退出码不自动报错，由调用者自己检查结果对象。
            let result = capture_command_builtin("capture", &args, span, ctx)?;
            Ok(command_result_object(result))
        }
        Builtin::Text => {
            // `text(cmd)` 是 `capture(cmd)` 的便捷包装：要求命令成功，
            // 然后直接返回 stdout 文本。
            let result = capture_command_builtin("text", &args, span, ctx)?;
            ensure_command_success(&result, span)?;
            Ok(Value::String(result.stdout))
        }
        Builtin::Lines => {
            // `lines(cmd)` 同样要求命令成功，但把 stdout 按行拆成字符串数组，
            // 方便直接接到后续 value flow / 高阶函数处理。
            let result = capture_command_builtin("lines", &args, span, ctx)?;
            ensure_command_success(&result, span)?;
            Ok(Value::Array(Rc::new(RefCell::new(
                result
                    .stdout
                    .lines()
                    .map(|line| Value::String(line.to_string()))
                    .collect(),
            ))))
        }
        Builtin::WithEnv => {
            // `with_env(cmd, obj)` 返回派生后的命令值；
            // 只更新命令值自己的环境覆盖，不修改当前 shell 进程环境。
            expect_arity(&args, 2, span, "with_env")?;
            let Value::Command(command) = &args[0] else {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!(
                        "with_env expects Command as first argument, got {}",
                        args[0].type_name()
                    ),
                ));
            };
            let Value::Object(env_obj) = &args[1] else {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!(
                        "with_env expects Object as second argument, got {}",
                        args[1].type_name()
                    ),
                ));
            };

            let mut derived = command.clone();
            let mut env_override = derived.env_override.take().unwrap_or_default();
            for (key, value) in env_obj.borrow().iter() {
                let Value::String(text) = value else {
                    return Err(RuntimeError::new(
                        span,
                        RuntimeErrorKind::TypeMismatch,
                        format!(
                            "with_env expects Object<String>; key '{}' has {}",
                            key,
                            value.type_name()
                        ),
                    ));
                };
                env_override.insert(key.clone(), text.clone());
            }
            derived.env_override = Some(env_override);
            Ok(Value::Command(derived))
        }
        Builtin::WithCwd => {
            // `with_cwd(cmd, path)` 也是不可变派生：返回一个带 cwd override 的新命令值。
            expect_arity(&args, 2, span, "with_cwd")?;
            let Value::Command(command) = &args[0] else {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!(
                        "with_cwd expects Command as first argument, got {}",
                        args[0].type_name()
                    ),
                ));
            };
            let Value::String(path) = &args[1] else {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!(
                        "with_cwd expects String as second argument, got {}",
                        args[1].type_name()
                    ),
                ));
            };

            let mut derived = command.clone();
            derived.cwd_override = Some(path.clone());
            Ok(Value::Command(derived))
        }
        Builtin::Map => {
            expect_arity(&args, 2, span, "map")?;

            let arr = expect_array(&args[0], span, "map")?;

            let func = expect_function(&args[1], span, "map")?;

            // 先复制一份当前数组内容，再逐项调用回调。
            // 这样 `map` 的遍历边界很清楚，也避免把 `RefCell` 借用跨过整个回调执行过程。
            let items = arr.borrow().clone();

            let mut result = Vec::with_capacity(items.len());

            for value in items {
                let mapped =
                    call_function(func.clone(), &vec![value], ctx.env, span)?.unwrap_or(Value::Nil);
                result.push(mapped);
            }

            Ok(Value::Array(Rc::new(RefCell::new(result))))
        }
        Builtin::Filter => {
            expect_arity(&args, 2, span, "filter")?;

            let arr = expect_array(&args[0], span, "filter")?;

            let func = expect_function(&args[1], span, "filter")?;

            let items = arr.borrow().clone();

            let mut result = Vec::new();

            for value in items {
                let bool_value = call_function(func.clone(), &vec![value.clone()], ctx.env, span)?
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
        Builtin::Reduce => {
            expect_arity(&args, 3, span, "reduce")?;
            let arr = expect_array(&args[0], span, "reduce")?;

            let initial = &args[1];

            let func = expect_function(&args[2], span, "reduce")?;

            let items = arr.borrow().clone();

            let mut acc = initial.clone();

            for item in items {
                acc = call_function(func.clone(), &vec![acc, item], ctx.env, span)?
                    .unwrap_or(Value::Nil);
            }

            Ok(acc)
        }
        Builtin::Each => {
            expect_arity(&args, 2, span, "each")?;

            let arr = expect_array(&args[0], span, "each")?;

            let func = expect_function(&args[1], span, "each")?;

            let items = arr.borrow().clone();

            for item in items {
                let _ = call_function(func.clone(), &vec![item], ctx.env, span)?;
            }
            Ok(Value::Nil)
        }
        Builtin::Any => {
            expect_arity(&args, 2, span, "any")?;

            let arr = expect_array(&args[0], span, "any")?;

            let func = expect_function(&args[1], span, "any")?;

            let items = arr.borrow().clone();

            for item in items {
                let b =
                    call_function(func.clone(), &vec![item], ctx.env, span)?.unwrap_or(Value::Nil);
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

            return Ok(Value::Bool(false));
        }
        Builtin::All => {
            expect_arity(&args, 2, span, "all")?;

            let arr = expect_array(&args[0], span, "all")?;

            let func = expect_function(&args[1], span, "all")?;

            let items = arr.borrow().clone();

            for item in items {
                let b =
                    call_function(func.clone(), &vec![item], ctx.env, span)?.unwrap_or(Value::Nil);
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

            return Ok(Value::Bool(true));
        }
        Builtin::Find => {
            expect_arity(&args, 2, span, "find")?;

            let arr = expect_array(&args[0], span, "find")?;

            let func = expect_function(&args[1], span, "find")?;

            let items = arr.borrow().clone();

            for item in items {
                let matched = call_function(func.clone(), &vec![item.clone()], ctx.env, span)?
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
        Builtin::Join => {
            expect_arity(&args, 2, span, "join")?;

            let arr = expect_array(&args[0], span, "join")?;

            let Value::String(sep) = &args[1] else {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!("join expects String separator, got {}", args[1].type_name()),
                ));
            };

            let items = arr.borrow();

            let text = items
                .iter()
                .map(display_value)
                .collect::<Vec<_>>()
                .join(sep);
            Ok(Value::String(text))
        }
    }
}

fn capture_command_builtin(
    builtin_name: &str,
    args: &[Value],
    span: usize,
    ctx: BuiltinContext<'_>,
) -> Result<crate::ecscript::value::CommandResult, RuntimeError> {
    expect_arity(args, 1, span, builtin_name)?;
    let Value::Command(command) = &args[0] else {
        return Err(RuntimeError::new(
            span,
            RuntimeErrorKind::TypeMismatch,
            format!(
                "{} expects Command, got {}",
                builtin_name,
                args[0].type_name()
            ),
        ));
    };
    let Some(state) = ctx.shell_state else {
        return Err(RuntimeError::new(
            span,
            RuntimeErrorKind::IoError,
            format!("{builtin_name} is not available in this context"),
        ));
    };
    capture_command_invocation(command, state)
        .map_err(|err| RuntimeError::new(span, RuntimeErrorKind::IoError, err.to_string()))
}

fn ensure_command_success(
    result: &crate::ecscript::value::CommandResult,
    span: usize,
) -> Result<(), RuntimeError> {
    if result.code != 0 || result.signal.is_some() {
        return Err(RuntimeError::new(
            span,
            RuntimeErrorKind::IoError,
            format!(
                "command failed with code {}{}",
                result.code,
                result
                    .signal
                    .map(|signal| format!(" (signal {})", signal))
                    .unwrap_or_default()
            ),
        ));
    }
    Ok(())
}

fn command_result_object(result: crate::ecscript::value::CommandResult) -> Value {
    let mut fields = HashMap::new();
    fields.insert("code".to_string(), Value::Int(result.code as i64));
    fields.insert(
        "signal".to_string(),
        result
            .signal
            .map(|signal| Value::Int(signal as i64))
            .unwrap_or(Value::Nil),
    );
    fields.insert("stdout".to_string(), Value::String(result.stdout));
    fields.insert("stderr".to_string(), Value::String(result.stderr));
    fields.insert(
        "duration_ms".to_string(),
        Value::Int(i64::try_from(result.duration_ms).unwrap_or(i64::MAX)),
    );
    fields.insert(
        "ok".to_string(),
        Value::Bool(result.code == 0 && result.signal.is_none()),
    );
    Value::Object(Rc::new(RefCell::new(fields)))
}

fn shell_word_from_value(
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

fn expect_array(
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

fn expect_function(
    arg: &Value,
    span: usize,
    builtin_name: &str,
) -> Result<Rc<crate::ecscript::value::Function>, RuntimeError> {
    let Value::Function(func) = arg else {
        return Err(RuntimeError::new(
            span,
            RuntimeErrorKind::TypeMismatch,
            format!("{} expects function, got {}", builtin_name, arg.type_name()),
        ));
    };
    Ok(func.clone())
}

fn to_json_value(value: &Value, span: usize) -> Result<serde_json::Value, RuntimeError> {
    let mut visiting = HashSet::new();
    to_json_value_inner(value, span, &mut visiting)
}

fn from_json_value(value: &serde_json::Value, span: usize) -> Result<Value, RuntimeError> {
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
        env::Environment,
        error::RuntimeErrorKind,
        value::{Builtin, BuiltinContext, Value},
    };
    use std::{cell::RefCell, collections::HashMap, rc::Rc};

    fn ctx() -> BuiltinContext<'static> {
        BuiltinContext {
            shell_state: None,
            env: Box::leak(Box::new(Environment::new())),
        }
    }

    #[test]
    fn keys_are_sorted() {
        let obj = Rc::new(RefCell::new(HashMap::from([
            ("b".to_string(), Value::Int(2)),
            ("a".to_string(), Value::Int(1)),
        ])));

        let result = run_builtin(Builtin::Keys, vec![Value::Object(obj)], 0, ctx()).unwrap();
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
        let result =
            run_builtin(Builtin::Env, vec![Value::String("PATH".into())], 0, ctx()).unwrap();
        assert!(matches!(result, Value::String(_)));
    }

    #[test]
    fn env_returns_nil_for_missing_variable() {
        let result = run_builtin(
            Builtin::Env,
            vec![Value::String("ECSH_TEST_MISSING_ENV_VAR".into())],
            0,
            ctx(),
        )
        .unwrap();
        assert_eq!(result, Value::Nil);
    }

    #[test]
    fn env_requires_string_argument() {
        let err = run_builtin(Builtin::Env, vec![Value::Int(1)], 0, ctx()).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
        assert_eq!(err.message, "env expects String, got Int");
    }

    #[test]
    fn range_returns_closed_interval_array() {
        let result =
            run_builtin(Builtin::Range, vec![Value::Int(1), Value::Int(4)], 0, ctx()).unwrap();
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
        let result =
            run_builtin(Builtin::Range, vec![Value::Int(4), Value::Int(1)], 0, ctx()).unwrap();
        let Value::Array(values) = result else {
            panic!("expected array");
        };
        assert!(values.borrow().is_empty());
    }

    #[test]
    fn range_requires_int_arguments() {
        let err = run_builtin(
            Builtin::Range,
            vec![Value::Bool(true), Value::Int(1)],
            0,
            ctx(),
        )
        .unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
        assert_eq!(err.message, "range expects Int start, got Bool");

        let err = run_builtin(
            Builtin::Range,
            vec![Value::Int(1), Value::Bool(true)],
            0,
            ctx(),
        )
        .unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
        assert_eq!(err.message, "range expects Int end, got Bool");
    }

    #[test]
    fn values_follow_sorted_keys() {
        let obj = Rc::new(RefCell::new(HashMap::from([
            ("b".to_string(), Value::Int(2)),
            ("a".to_string(), Value::Int(1)),
        ])));

        let result = run_builtin(Builtin::Values, vec![Value::Object(obj)], 0, ctx()).unwrap();
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
            ctx(),
        )
        .unwrap_err();

        assert_eq!(err.kind, RuntimeErrorKind::IndexOutOfBounds);
    }

    #[test]
    fn remove_checks_out_of_bounds_index() {
        let arr = Rc::new(RefCell::new(vec![Value::Int(1)]));
        let err = run_builtin(
            Builtin::Remove,
            vec![Value::Array(arr), Value::Int(1)],
            0,
            ctx(),
        )
        .unwrap_err();

        assert_eq!(err.kind, RuntimeErrorKind::IndexOutOfBounds);
    }

    #[test]
    fn insert_reports_index_type() {
        let arr = Rc::new(RefCell::new(vec![Value::Int(1)]));
        let err = run_builtin(
            Builtin::Insert,
            vec![Value::Array(arr), Value::String("x".into()), Value::Int(2)],
            0,
            ctx(),
        )
        .unwrap_err();

        assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
        assert_eq!(err.message, "insert expects Int index, got String");
    }

    #[test]
    fn to_json_detects_array_cycle() {
        let arr = Rc::new(RefCell::new(Vec::new()));
        arr.borrow_mut().push(Value::Array(arr.clone()));

        let err = run_builtin(Builtin::ToJson, vec![Value::Array(arr)], 0, ctx()).unwrap_err();
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

        let result = run_builtin(Builtin::ToJson, vec![Value::Object(obj)], 0, ctx()).unwrap();
        assert_eq!(result, Value::String("{\"a\":1,\"b\":2}".into()));
    }

    #[test]
    fn from_json_parses_object_and_array_values() {
        let result = run_builtin(
            Builtin::FromJson,
            vec![Value::String("{\"a\":1,\"b\":[true,null]}".into())],
            0,
            ctx(),
        )
        .unwrap();

        let Value::Object(obj) = result else {
            panic!("expected object");
        };
        let obj = obj.borrow();
        assert_eq!(obj.get("a"), Some(&Value::Int(1)));

        let Value::Array(arr) = obj.get("b").cloned().expect("missing b") else {
            panic!("expected array");
        };
        assert_eq!(*arr.borrow(), vec![Value::Bool(true), Value::Nil]);
    }

    #[test]
    fn from_json_requires_string_argument() {
        let err = run_builtin(Builtin::FromJson, vec![Value::Int(1)], 0, ctx()).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
        assert_eq!(err.message, "from_json expects String, got Int");
    }

    #[test]
    fn from_json_reports_invalid_json() {
        let err = run_builtin(
            Builtin::FromJson,
            vec![Value::String("{bad json}".into())],
            0,
            ctx(),
        )
        .unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::ParseInExpr);
        assert!(err.message.starts_with("invalid JSON:"));
    }

    #[test]
    fn slice_returns_half_open_subarray() {
        let arr = Rc::new(RefCell::new(vec![
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
            Value::Int(4),
        ]));
        let result = run_builtin(
            Builtin::Slice,
            vec![Value::Array(arr), Value::Int(1), Value::Int(3)],
            0,
            ctx(),
        )
        .unwrap();
        let Value::Array(arr) = result else {
            panic!("expected array");
        };
        assert_eq!(*arr.borrow(), vec![Value::Int(2), Value::Int(3)]);
    }

    #[test]
    fn slice_rejects_start_after_end() {
        let arr = Rc::new(RefCell::new(vec![Value::Int(1), Value::Int(2)]));
        let err = run_builtin(
            Builtin::Slice,
            vec![Value::Array(arr), Value::Int(2), Value::Int(1)],
            0,
            ctx(),
        )
        .unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::IndexOutOfBounds);
        assert_eq!(err.message, "slice start 2 exceeds end 1");
    }

    #[test]
    fn map_treats_missing_return_as_nil() {
        let items = Rc::new(RefCell::new(vec![Value::Int(1), Value::Int(2)]));
        let func = Rc::new(crate::ecscript::value::Function {
            name: None,
            params: vec!["x".into()],
            stmts: vec![],
            captures: HashMap::new(),
        });

        let result = run_builtin(
            Builtin::Map,
            vec![Value::Array(items), Value::Function(func)],
            0,
            ctx(),
        )
        .unwrap();

        let Value::Array(mapped) = result else {
            panic!("expected array");
        };
        assert_eq!(*mapped.borrow(), vec![Value::Nil, Value::Nil]);
    }

    #[test]
    fn command_builtin_builds_simple_command() {
        let result = run_builtin(
            Builtin::CommandBuilder,
            vec![
                Value::String("/bin/echo".into()),
                Value::String("hello".into()),
                Value::Int(42),
                Value::Bool(true),
            ],
            0,
            ctx(),
        )
        .unwrap();

        let Value::Command(invocation) = result else {
            panic!("expected command");
        };
        let crate::ecscript::value::CommandValue::Simple(command) = invocation.command else {
            panic!("expected simple command");
        };
        assert_eq!(command.program.as_lit_str(), Some("/bin/echo"));
        assert_eq!(command.args[0].as_lit_str(), Some("hello"));
        assert_eq!(command.args[1].as_lit_str(), Some("42"));
        assert_eq!(command.args[2].as_lit_str(), Some("true"));
    }

    #[test]
    fn command_builtin_rejects_object_argument() {
        let object = Value::Object(Rc::new(RefCell::new(HashMap::new())));
        let err = run_builtin(
            Builtin::CommandBuilder,
            vec![Value::String("/bin/echo".into()), object],
            0,
            ctx(),
        )
        .unwrap_err();

        assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
        assert_eq!(
            err.message,
            "command only accepts String, Int, Float, Bool or Nil argv parts, got Object"
        );
    }

    #[test]
    fn with_env_derives_command_with_merged_override() {
        let command = Value::Command(crate::ecscript::value::CommandInvocation {
            command: crate::ecscript::value::CommandValue::Simple(crate::types::Command {
                program: crate::types::ShellWord::lit("printf"),
                args: vec![crate::types::ShellWord::lit("ok")],
                redirection: crate::types::Redirection::default(),
            }),
            cwd_override: None,
            env_override: Some(std::collections::BTreeMap::from([(
                "BASE".to_string(),
                "1".to_string(),
            )])),
            stdin_override: None,
        });
        let env_obj = Value::Object(Rc::new(RefCell::new(HashMap::from([(
            "EXTRA".to_string(),
            Value::String("2".into()),
        )]))));

        let result = run_builtin(Builtin::WithEnv, vec![command, env_obj], 0, ctx()).unwrap();
        let Value::Command(derived) = result else {
            panic!("expected command");
        };
        let overrides = derived.env_override.expect("missing env override");
        assert_eq!(overrides.get("BASE"), Some(&"1".to_string()));
        assert_eq!(overrides.get("EXTRA"), Some(&"2".to_string()));
    }

    #[test]
    fn with_env_rejects_non_string_values() {
        let command = Value::Command(crate::ecscript::value::CommandInvocation {
            command: crate::ecscript::value::CommandValue::Simple(crate::types::Command {
                program: crate::types::ShellWord::lit("printf"),
                args: vec![crate::types::ShellWord::lit("ok")],
                redirection: crate::types::Redirection::default(),
            }),
            cwd_override: None,
            env_override: None,
            stdin_override: None,
        });
        let env_obj = Value::Object(Rc::new(RefCell::new(HashMap::from([(
            "EXTRA".to_string(),
            Value::Int(2),
        )]))));

        let err = run_builtin(Builtin::WithEnv, vec![command, env_obj], 0, ctx()).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
        assert_eq!(
            err.message,
            "with_env expects Object<String>; key 'EXTRA' has Int"
        );
    }

    #[test]
    fn with_cwd_derives_command_with_override() {
        let command = Value::Command(crate::ecscript::value::CommandInvocation {
            command: crate::ecscript::value::CommandValue::Simple(crate::types::Command {
                program: crate::types::ShellWord::lit("pwd"),
                args: vec![],
                redirection: crate::types::Redirection::default(),
            }),
            cwd_override: None,
            env_override: None,
            stdin_override: None,
        });

        let result = run_builtin(
            Builtin::WithCwd,
            vec![command, Value::String("/tmp".into())],
            0,
            ctx(),
        )
        .unwrap();
        let Value::Command(derived) = result else {
            panic!("expected command");
        };
        assert_eq!(derived.cwd_override.as_deref(), Some("/tmp"));
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
        let result = run_builtin(Builtin::Env, vec![Value::String(var)], 0, ctx()).unwrap();
        assert_eq!(result, Value::Nil);
    }

    #[test]
    fn env_returns_env_value() {
        unsafe { std::env::set_var("ECSH_TEST_RUNTIME_VAR2", "runtime") };
        let result = run_builtin(
            Builtin::Env,
            vec![Value::String("ECSH_TEST_RUNTIME_VAR2".into())],
            0,
            ctx(),
        )
        .unwrap();
        assert_eq!(result, Value::String("runtime".into()));
        unsafe { std::env::remove_var("ECSH_TEST_RUNTIME_VAR2") };
    }

    #[test]
    fn env_rejects_wrong_type() {
        let err = run_builtin(Builtin::Env, vec![Value::Int(1)], 0, ctx()).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
    }

    #[test]
    fn range_produces_inclusive_range() {
        let result =
            run_builtin(Builtin::Range, vec![Value::Int(0), Value::Int(3)], 0, ctx()).unwrap();
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
        let result =
            run_builtin(Builtin::Range, vec![Value::Int(5), Value::Int(5)], 0, ctx()).unwrap();
        let Value::Array(arr) = result else {
            panic!("expected array")
        };
        assert_eq!(*arr.borrow(), vec![Value::Int(5)]);
    }

    #[test]
    fn range_reversed_returns_empty() {
        let result =
            run_builtin(Builtin::Range, vec![Value::Int(5), Value::Int(0)], 0, ctx()).unwrap();
        let Value::Array(arr) = result else {
            panic!("expected array")
        };
        assert!(arr.borrow().is_empty());
    }
}
