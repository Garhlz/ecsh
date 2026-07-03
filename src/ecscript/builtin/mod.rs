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
    all_builtin, any_builtin, clone_builtin, each_builtin, filter_builtin, find_builtin,
    insert_builtin, join_builtin, join_path_builtin, keys_builtin, len_builtin, map_builtin,
    pop_builtin, push_builtin, range_builtin, reduce_builtin, remove_builtin, slice_builtin,
    values_builtin,
};
use command::{
    capture_command_builtin, command_builder_builtin, command_result_object, lines_from_command,
    run_builtin_command, text_from_command,
};
use introspection::{builtins_builtin, commands_builtin, extensions_builtin, help_builtin};
use io::{format_print_args, write_stdout};
use json::{from_json_value, to_json_value};
use support::{
    ParamType, Signature, check_signature, expect_array, expect_shell_state,
    object_string_map_from_value, param,
};

const SIG_ENV: Signature = Signature::exact("env", &[param("name", ParamType::String)]);
const SIG_SET_ENV: Signature = Signature::exact(
    "set_env",
    &[
        param("name", ParamType::String),
        param("value", ParamType::String),
    ],
);
const SIG_UNSET_ENV: Signature = Signature::exact("unset_env", &[param("name", ParamType::String)]);
const SIG_CWD: Signature = Signature::exact("cwd", &[]);
const SIG_STDIN: Signature = Signature::exact("stdin", &[]);
const SIG_READ_LINES: Signature = Signature::exact("read_lines", &[]);
const SIG_TO_JSON: Signature = Signature::exact("to_json", &[param("value", ParamType::Any)]);
const SIG_FROM_JSON: Signature = Signature::exact("from_json", &[param("text", ParamType::String)]);
const SIG_PRINT: Signature = Signature::at_least("print", &[], Some(ParamType::Any), 0);
const SIG_PRINTLN: Signature = Signature::at_least("println", &[], Some(ParamType::Any), 0);
const SIG_WITH_ENV: Signature = Signature::exact(
    "with_env",
    &[
        param("command", ParamType::Command),
        param("env_map", ParamType::Object),
    ],
);
const SIG_WITH_CWD: Signature = Signature::exact(
    "with_cwd",
    &[
        param("command", ParamType::Command),
        param("path", ParamType::String),
    ],
);
const SIG_WRITE_LINES: Signature =
    Signature::exact("write_lines", &[param("array", ParamType::Array)]);
const SIG_HOOK: Signature = Signature::exact(
    "hook",
    &[
        param("name", ParamType::String),
        param("function", ParamType::Function),
    ],
);
const SIG_PROMPT: Signature = Signature::exact("prompt", &[param("function", ParamType::Function)]);
const SIG_COMPLETE: Signature = Signature::exact(
    "complete",
    &[
        param("name", ParamType::String),
        param("function", ParamType::Function),
    ],
);
const SIG_REGISTER_COMMAND: Signature = Signature::exact(
    "register_command",
    &[
        param("name", ParamType::String),
        param("function", ParamType::Function),
    ],
);
const SIG_SET_CWD: Signature = Signature::exact("set_cwd", &[param("path", ParamType::String)]);
const SIG_TRIM: Signature = Signature::exact("trim", &[param("text", ParamType::String)]);
const SIG_BIND: Signature = Signature::exact(
    "bind",
    &[
        param("key", ParamType::String),
        param("function", ParamType::Function),
    ],
);

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
        "clone" => Some(Builtin::Clone),
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
/// 简单参数先通过 [`check_signature`] 验证，复杂对象协议再由 builtin
/// 自己做字段级检查。
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
            check_signature(&SIG_ENV, &args, span)?;
            let Value::String(name) = &args[0] else {
                unreachable!()
            };

            Ok(match std::env::var(name) {
                Ok(value) => Value::String(value),
                Err(_) => Value::Nil,
            })
        }
        Builtin::SetEnv => {
            check_signature(&SIG_SET_ENV, &args, span)?;
            let Value::String(name) = &args[0] else {
                unreachable!()
            };
            let Value::String(value) = &args[1] else {
                unreachable!()
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
            check_signature(&SIG_UNSET_ENV, &args, span)?;
            let Value::String(name) = &args[0] else {
                unreachable!()
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
            check_signature(&SIG_CWD, &args, span)?;
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
            check_signature(&SIG_STDIN, &args, span)?;
            Ok(Value::String(
                ctx.stdin_text.unwrap_or_default().to_string(),
            ))
        }
        Builtin::ReadLines => {
            // `read_lines()` 是 `stdin()` 的按行视图，方便直接接数组高阶函数。
            check_signature(&SIG_READ_LINES, &args, span)?;
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
        Builtin::Clone => clone_builtin(&args, span),
        Builtin::Push => push_builtin(&args, span),
        Builtin::Pop => pop_builtin(&args, span),
        Builtin::Insert => insert_builtin(&args, span),
        Builtin::Remove => remove_builtin(&args, span),
        Builtin::Slice => slice_builtin(&args, span),
        Builtin::Keys => keys_builtin(&args, span),
        Builtin::Values => values_builtin(&args, span),
        Builtin::ToJson => {
            check_signature(&SIG_TO_JSON, &args, span)?;
            let json = to_json_value(&args[0], span)?;
            Ok(Value::String(json.to_string()))
        }
        Builtin::FromJson => {
            check_signature(&SIG_FROM_JSON, &args, span)?;
            let Value::String(text) = &args[0] else {
                unreachable!()
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
            check_signature(&SIG_PRINT, &args, span)?;
            let text = format_print_args(&args);
            write_stdout(&text, false, span)?;
            Ok(Value::Nil)
        }
        Builtin::Println => {
            check_signature(&SIG_PRINTLN, &args, span)?;
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
            check_signature(&SIG_WITH_ENV, &args, span)?;
            let Value::Command(command) = &args[0] else {
                unreachable!()
            };

            let mut derived = command.clone();
            let mut env_override = derived.env_override.take().unwrap_or_default();
            for (key, value) in object_string_map_from_value(&args[1], span, "with_env")? {
                env_override.insert(key, value);
            }
            derived.env_override = Some(env_override);
            Ok(Value::Command(derived))
        }
        Builtin::WithCwd => {
            // `with_cwd(cmd, path)` 也是不可变派生：返回一个带 cwd override 的新命令值。
            check_signature(&SIG_WITH_CWD, &args, span)?;
            let Value::Command(command) = &args[0] else {
                unreachable!()
            };
            let Value::String(path) = &args[1] else {
                unreachable!()
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
            check_signature(&SIG_WRITE_LINES, &args, span)?;
            let arr = expect_array(&args[0], span, "write_lines")?;
            let items = arr.borrow().clone();
            for item in items {
                write_stdout(&display_value(&item), true, span)?;
            }
            Ok(Value::Nil)
        }
        Builtin::Hook => {
            check_signature(&SIG_HOOK, &args, span)?;
            let state = expect_shell_state(ctx.shell_state, span, "hook")?;
            let Value::String(name) = &args[0] else {
                unreachable!()
            };
            let hook_name = HookName::parse(name).ok_or_else(|| {
                RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!("unknown hook '{}'", name),
                )
            })?;
            let Value::Function(func) = &args[1] else {
                unreachable!()
            };
            state
                .extensions
                .borrow_mut()
                .hooks
                .entry(hook_name)
                .or_default()
                .push(Value::Function(func.clone()));
            Ok(Value::Nil)
        }
        Builtin::Prompt => {
            check_signature(&SIG_PROMPT, &args, span)?;
            let state = expect_shell_state(ctx.shell_state, span, "prompt")?;
            let Value::Function(func) = &args[0] else {
                unreachable!()
            };
            state.extensions.borrow_mut().prompt_handler = Some(Value::Function(func.clone()));
            Ok(Value::Nil)
        }
        Builtin::Complete => {
            check_signature(&SIG_COMPLETE, &args, span)?;
            let state = expect_shell_state(ctx.shell_state, span, "complete")?;
            let Value::String(name) = &args[0] else {
                unreachable!()
            };
            let Value::Function(func) = &args[1] else {
                unreachable!()
            };
            state
                .extensions
                .borrow_mut()
                .completions
                .insert(name.clone(), Value::Function(func.clone()));
            Ok(Value::Nil)
        }
        Builtin::RegisterCommand => {
            check_signature(&SIG_REGISTER_COMMAND, &args, span)?;
            let state = expect_shell_state(ctx.shell_state, span, "register_command")?;
            let Value::String(name) = &args[0] else {
                unreachable!()
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
            let Value::Function(func) = &args[1] else {
                unreachable!()
            };
            state
                .extensions
                .borrow_mut()
                .script_commands
                .insert(name.clone(), Value::Function(func.clone()));
            Ok(Value::Nil)
        }
        Builtin::SetCwd => {
            check_signature(&SIG_SET_CWD, &args, span)?;
            let state = expect_shell_state(ctx.shell_state, span, "set_cwd")?;
            let Value::String(path) = &args[0] else {
                unreachable!()
            };
            crate::builtin::set_current_dir_with_hooks(path, state).map_err(|err| {
                RuntimeError::new(span, RuntimeErrorKind::IoError, format!("set_cwd: {err}"))
            })?;
            Ok(Value::Nil)
        }
        Builtin::Trim => {
            check_signature(&SIG_TRIM, &args, span)?;
            let Value::String(text) = &args[0] else {
                unreachable!()
            };
            Ok(Value::String(text.trim().to_string()))
        }
        Builtin::Bind => {
            check_signature(&SIG_BIND, &args, span)?;
            let state = expect_shell_state(ctx.shell_state, span, "bind")?;
            let Value::String(key) = &args[0] else {
                unreachable!()
            };
            let Value::Function(func) = &args[1] else {
                unreachable!()
            };
            crate::extensions::parse_key_string(key, span)?;
            state
                .extensions
                .borrow_mut()
                .key_bindings
                .insert(key.clone(), Value::Function(func.clone()));
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
