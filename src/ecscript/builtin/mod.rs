use std::{cell::RefCell, rc::Rc};

use crate::ecscript::{
    display_value,
    error::{RuntimeError, RuntimeErrorKind},
    value::{Builtin, BuiltinContext, Value},
};
use crate::extensions::HookName;

mod collections;
mod command;
mod introspection;
mod io;
mod json;
mod support;

use collections::{
    all_builtin, any_builtin, each_builtin, filter_builtin, find_builtin, insert_builtin,
    join_builtin, join_path_builtin, keys_builtin, len_builtin, map_builtin, pop_builtin,
    push_builtin, range_builtin, reduce_builtin, remove_builtin, slice_builtin, values_builtin,
};
use command::{
    capture_command_builtin, command_builder_builtin, command_result_object, lines_from_command,
    run_builtin_command, text_from_command,
};
use introspection::{builtins_builtin, commands_builtin, extensions_builtin, help_builtin};
use io::{format_print_args, write_stdout};
use json::{from_json_value, to_json_value};
use support::{expect_arity, expect_array, expect_function, expect_shell_state};

/// 根据名字查找内置函数枚举。
///
/// 这里只做纯字符串匹配，所有实际语义都在 [`run_builtin`] 里分发。
pub fn lookup_builtin(name: &str) -> Option<Builtin> {
    match name {
        "command" => Some(Builtin::CommandBuilder),
        "env" => Some(Builtin::Env),
        "set_env" => Some(Builtin::SetEnv),
        "unset_env" => Some(Builtin::UnsetEnv),
        "cwd" => Some(Builtin::Cwd),
        "stdin" => Some(Builtin::Stdin),
        "read_lines" => Some(Builtin::ReadLines),
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
        "join_path" => Some(Builtin::JoinPath),
        "write_lines" => Some(Builtin::WriteLines),
        "hook" => Some(Builtin::Hook),
        "prompt" => Some(Builtin::Prompt),
        "complete" => Some(Builtin::Complete),
        "register_command" => Some(Builtin::RegisterCommand),
        "set_cwd" => Some(Builtin::SetCwd),
        "trim" => Some(Builtin::Trim),
        "bind" => Some(Builtin::Bind),
        "help" => Some(Builtin::Help),
        "builtins" => Some(Builtin::Builtins),
        "extensions" => Some(Builtin::Extensions),
        "commands" => Some(Builtin::Commands),
        _ => None,
    }
}

/// 分发执行内置函数。
///
/// 所有内置函数共享一套类型检查和错误报告模式：
/// 先通过 [`expect_arity`] / [`expect_array`] / [`expect_function`]
/// 验证参数，再执行具体逻辑。
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
            command_builder_builtin(&args, span)
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
        Builtin::SetEnv => {
            expect_arity(&args, 2, span, "set_env")?;
            let Value::String(name) = &args[0] else {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!("set_env expects String name, got {}", args[0].type_name()),
                ));
            };
            let Value::String(value) = &args[1] else {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!("set_env expects String value, got {}", args[1].type_name()),
                ));
            };
            if !crate::builtin::is_valid_env_key(name) {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!("set_env invalid variable name: {name}"),
                ));
            }

            // ecsh executes ecscript on its main thread, matching the shell export builtin.
            unsafe { std::env::set_var(name, value) };
            Ok(Value::Nil)
        }
        Builtin::UnsetEnv => {
            expect_arity(&args, 1, span, "unset_env")?;
            let Value::String(name) = &args[0] else {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!("unset_env expects String name, got {}", args[0].type_name()),
                ));
            };
            if !crate::builtin::is_valid_env_key(name) {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!("unset_env invalid variable name: {name}"),
                ));
            }

            unsafe { std::env::remove_var(name) };
            Ok(Value::Nil)
        }
        Builtin::Cwd => {
            expect_arity(&args, 0, span, "cwd")?;
            let cwd = std::env::current_dir().map_err(|err| {
                RuntimeError::new(
                    span,
                    RuntimeErrorKind::IoError,
                    format!("cwd failed: {}", err),
                )
            })?;
            Ok(Value::String(cwd.to_string_lossy().into_owned()))
        }
        Builtin::Stdin => {
            // `stdin()` 返回执行入口预先提供的 stdin 文本快照。
            // 它不主动从管道阻塞读取，适合脚本参数或重定向输入场景。
            expect_arity(&args, 0, span, "stdin")?;
            Ok(Value::String(
                ctx.stdin_text.unwrap_or_default().to_string(),
            ))
        }
        Builtin::ReadLines => {
            // `read_lines()` 是 `stdin()` 的按行视图，方便直接接数组高阶函数。
            expect_arity(&args, 0, span, "read_lines")?;
            let lines = ctx
                .stdin_text
                .unwrap_or_default()
                .lines()
                .map(|line| Value::String(line.to_string()))
                .collect::<Vec<_>>();
            Ok(Value::Array(Rc::new(RefCell::new(lines))))
        }
        Builtin::Range => {
            // `range(start, end)` 返回闭区间 `[start, end]` 的 Int 数组。
            // `start > end` 时返回空数组。
            range_builtin(&args, span)
        }
        Builtin::Len => len_builtin(&args, span),
        Builtin::Push => push_builtin(&args, span),
        Builtin::Pop => pop_builtin(&args, span),
        Builtin::Insert => insert_builtin(&args, span),
        Builtin::Remove => remove_builtin(&args, span),
        Builtin::Slice => slice_builtin(&args, span),
        Builtin::Keys => keys_builtin(&args, span),
        Builtin::Values => values_builtin(&args, span),
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
        Builtin::Run => run_builtin_command(&args, span, ctx),
        Builtin::Capture => {
            // `capture(cmd)` 面向“检查结果”场景：返回包含
            // `code` / `signal` / `stdout` / `stderr` / `ok` / `duration_ms` 的结果对象。
            // 非零退出码不自动报错，由调用者自行检查 `.ok` 或 `.code`。
            let result = capture_command_builtin("capture", &args, span, ctx)?;
            Ok(command_result_object(result))
        }
        Builtin::Text => text_from_command(&args, span, ctx),
        Builtin::Lines => lines_from_command(&args, span, ctx),
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
        Builtin::Map => map_builtin(&args, span, ctx),
        Builtin::Filter => filter_builtin(&args, span, ctx),
        Builtin::Reduce => reduce_builtin(&args, span, ctx),
        Builtin::Each => each_builtin(&args, span, ctx),
        Builtin::Any => any_builtin(&args, span, ctx),
        Builtin::All => all_builtin(&args, span, ctx),
        Builtin::Find => find_builtin(&args, span, ctx),
        Builtin::Join => join_builtin(&args, span),
        Builtin::JoinPath => join_path_builtin(&args, span),
        Builtin::WriteLines => {
            // `write_lines(arr)` 把数组每项按 display 风格输出到 stdout，每项占一行。
            // 这是 value 流转为文本输出的反向桥接入口。
            expect_arity(&args, 1, span, "write_lines")?;
            let arr = expect_array(&args[0], span, "write_lines")?;
            let items = arr.borrow().clone();
            for item in items {
                write_stdout(&display_value(&item), true, span)?;
            }
            Ok(Value::Nil)
        }
        Builtin::Hook => {
            expect_arity(&args, 2, span, "hook")?;
            let state = expect_shell_state(ctx.shell_state, span, "hook")?;
            let Value::String(name) = &args[0] else {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!("hook expects String name, got {}", args[0].type_name()),
                ));
            };
            let hook_name = HookName::parse(name).ok_or_else(|| {
                RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!("unknown hook '{}'", name),
                )
            })?;
            let func = expect_function(&args[1], span, "hook")?;
            state
                .extensions
                .borrow_mut()
                .hooks
                .entry(hook_name)
                .or_default()
                .push(Value::Function(func));
            Ok(Value::Nil)
        }
        Builtin::Prompt => {
            expect_arity(&args, 1, span, "prompt")?;
            let state = expect_shell_state(ctx.shell_state, span, "prompt")?;
            let func = expect_function(&args[0], span, "prompt")?;
            state.extensions.borrow_mut().prompt_handler = Some(Value::Function(func));
            Ok(Value::Nil)
        }
        Builtin::Complete => {
            expect_arity(&args, 2, span, "complete")?;
            let state = expect_shell_state(ctx.shell_state, span, "complete")?;
            let Value::String(name) = &args[0] else {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!("complete expects String name, got {}", args[0].type_name()),
                ));
            };
            let func = expect_function(&args[1], span, "complete")?;
            state
                .extensions
                .borrow_mut()
                .completions
                .insert(name.clone(), Value::Function(func));
            Ok(Value::Nil)
        }
        Builtin::RegisterCommand => {
            expect_arity(&args, 2, span, "register_command")?;
            let state = expect_shell_state(ctx.shell_state, span, "register_command")?;
            let Value::String(name) = &args[0] else {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!(
                        "register_command expects String name, got {}",
                        args[0].type_name()
                    ),
                ));
            };
            if name.is_empty()
                || name.contains('/')
                || name.chars().any(|ch| ch.is_ascii_whitespace())
            {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!("register_command invalid command name: {name}"),
                ));
            }
            if crate::builtin::BUILTIN_NAMES.contains(&name.as_str()) {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!("register_command cannot override shell builtin: {name}"),
                ));
            }
            let func = expect_function(&args[1], span, "register_command")?;
            state
                .extensions
                .borrow_mut()
                .script_commands
                .insert(name.clone(), Value::Function(func));
            Ok(Value::Nil)
        }
        Builtin::SetCwd => {
            expect_arity(&args, 1, span, "set_cwd")?;
            let state = expect_shell_state(ctx.shell_state, span, "set_cwd")?;
            let Value::String(path) = &args[0] else {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!("set_cwd expects String path, got {}", args[0].type_name()),
                ));
            };
            crate::builtin::set_current_dir_with_hooks(path, state).map_err(|err| {
                RuntimeError::new(span, RuntimeErrorKind::IoError, format!("set_cwd: {err}"))
            })?;
            Ok(Value::Nil)
        }
        Builtin::Trim => {
            expect_arity(&args, 1, span, "trim")?;
            let Value::String(text) = &args[0] else {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!("trim expects String, got {}", args[0].type_name()),
                ));
            };
            Ok(Value::String(text.trim().to_string()))
        }
        Builtin::Bind => {
            expect_arity(&args, 2, span, "bind")?;
            let state = expect_shell_state(ctx.shell_state, span, "bind")?;
            let Value::String(key) = &args[0] else {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!("bind expects String key, got {}", args[0].type_name()),
                ));
            };
            let func = expect_function(&args[1], span, "bind")?;
            crate::extensions::parse_key_string(key, span)?;
            state
                .extensions
                .borrow_mut()
                .key_bindings
                .insert(key.clone(), Value::Function(func));
            Ok(Value::Nil)
        }
        Builtin::Help => help_builtin(&args, span, &ctx),
        Builtin::Builtins => builtins_builtin(&args, span, &ctx),
        Builtin::Extensions => extensions_builtin(&args, span, &ctx),
        Builtin::Commands => commands_builtin(&args, span, &ctx),
    }
}

#[cfg(test)]
mod tests;
