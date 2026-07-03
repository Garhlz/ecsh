use std::{cell::RefCell, collections::HashMap, rc::Rc};

use crate::ecscript::{
    error::{RuntimeError, RuntimeErrorKind},
    value::{BuiltinContext, CommandResult, Value},
};
use crate::executor::{capture_command_invocation, run_command_invocation};

use super::support::{check_signature, param, shell_word_from_value, ParamType, Signature};

const SIG_COMMAND: Signature = Signature::at_least(
    "command",
    &[param("program", ParamType::String)],
    Some(ParamType::Any),
    1,
);
const SIG_RUN: Signature = Signature::exact("run", &[param("command", ParamType::Command)]);
const SIG_CAPTURE: Signature = Signature::exact("capture", &[param("command", ParamType::Command)]);
const SIG_TEXT: Signature = Signature::exact("text", &[param("command", ParamType::Command)]);
const SIG_LINES: Signature = Signature::exact("lines", &[param("command", ParamType::Command)]);

pub(super) fn command_builder_builtin(args: &[Value], span: usize) -> Result<Value, RuntimeError> {
    check_signature(&SIG_COMMAND, args, span)?;

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

pub(super) fn run_builtin_command(
    args: &[Value],
    span: usize,
    ctx: BuiltinContext<'_>,
) -> Result<Value, RuntimeError> {
    check_signature(&SIG_RUN, args, span)?;
    let Value::Command(command) = &args[0] else {
        unreachable!()
    };
    let Some(state) = ctx.shell_state.as_deref() else {
        return Err(RuntimeError::new(
            span,
            RuntimeErrorKind::IoError,
            "run is not available in this context",
        ));
    };
    let result = run_command_invocation(command, state)
        .map_err(|err| RuntimeError::new(span, RuntimeErrorKind::IoError, err.to_string()))?;
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

// `capture` / `text` / `lines` 共享的执行入口：
// 解析 Command 参数 → 执行命令，统一返回 `CommandResult`，
// 调用方自行决定如何消费结果（完整对象 / stdout 文本 / 按行数组）。
pub(super) fn capture_command_builtin(
    builtin_name: &str,
    args: &[Value],
    span: usize,
    ctx: BuiltinContext<'_>,
) -> Result<CommandResult, RuntimeError> {
    let sig = match builtin_name {
        "capture" => &SIG_CAPTURE,
        "text" => &SIG_TEXT,
        "lines" => &SIG_LINES,
        _ => unreachable!(),
    };
    check_signature(sig, args, span)?;
    let Value::Command(command) = &args[0] else {
        unreachable!()
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

// 检查 `CommandResult` 是否成功（退出码 == 0 且未收到信号）。
// 失败时立即按统一格式构造运行时错误。
pub(super) fn ensure_command_success(
    result: &CommandResult,
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

// 把 `CommandResult` 转为语言层对象，返回的 Object 包含：
// `code` (Int) / `signal` (Int or Nil) / `stdout` (String) / `stderr` (String)
// `duration_ms` (Int) / `ok` (Bool)
pub(super) fn command_result_object(result: CommandResult) -> Value {
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

pub(super) fn text_from_command(
    args: &[Value],
    span: usize,
    ctx: BuiltinContext<'_>,
) -> Result<Value, RuntimeError> {
    let result = capture_command_builtin("text", args, span, ctx)?;
    ensure_command_success(&result, span)?;
    Ok(Value::String(result.stdout))
}

pub(super) fn lines_from_command(
    args: &[Value],
    span: usize,
    ctx: BuiltinContext<'_>,
) -> Result<Value, RuntimeError> {
    let result = capture_command_builtin("lines", args, span, ctx)?;
    ensure_command_success(&result, span)?;
    Ok(Value::Array(Rc::new(RefCell::new(
        result
            .stdout
            .lines()
            .map(|line| Value::String(line.to_string()))
            .collect(),
    ))))
}
