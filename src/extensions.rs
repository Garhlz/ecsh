use crate::diagnostics::print_error;
use crate::ecscript::{Environment, RuntimeError, RuntimeErrorKind, Value, call_function_with_ctx};
use crate::types::{CommandStatus, JobStatus, ShellState};
use std::cell::RefCell;
use std::collections::HashMap;
use std::io;
use std::os::fd::AsRawFd;
use std::rc::Rc;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum HookName {
    BeforePrompt,
    AfterCd,
    Preexec,
    Postexec,
}

impl HookName {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "before_prompt" => Some(Self::BeforePrompt),
            "after_cd" => Some(Self::AfterCd),
            "preexec" => Some(Self::Preexec),
            "postexec" => Some(Self::Postexec),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct CompletionItem {
    pub value: String,
    pub display: Option<String>,
    pub desc: Option<String>,
    pub kind: Option<String>,
}

#[derive(Default)]
pub struct ExtensionRegistry {
    pub hooks: HashMap<HookName, Vec<Value>>,
    pub prompt_handler: Option<Value>,
    pub completions: HashMap<String, Value>,
    pub script_commands: HashMap<String, Value>,
    pub key_bindings: HashMap<String, Value>,
    pub last_duration_ms: Option<u128>,
}

impl ExtensionRegistry {
    pub fn new() -> Self {
        Self::default()
    }
}

pub type SharedExtensions = Rc<RefCell<ExtensionRegistry>>;

pub fn new_extensions() -> SharedExtensions {
    Rc::new(RefCell::new(ExtensionRegistry::new()))
}

pub fn invoke_callback(
    func: &Value,
    arg: Value,
    env: &Environment<'_>,
    state: &ShellState,
    label: &str,
) -> Result<Value, RuntimeError> {
    let Value::Function(func) = func else {
        return Err(RuntimeError::new(
            0,
            RuntimeErrorKind::NotCallable,
            format!(
                "{} handler must be function, got {}",
                label,
                func.type_name()
            ),
        ));
    };

    match call_function_with_ctx(func.clone(), vec![arg], env, Some(state), None, label, 0)? {
        Some(value) => Ok(value),
        None => Ok(Value::Nil),
    }
}

pub fn run_hooks(name: HookName, ctx: Value, state: &ShellState) {
    let handlers = state
        .extensions
        .borrow()
        .hooks
        .get(&name)
        .cloned()
        .unwrap_or_default();
    for handler in handlers {
        if let Err(err) = invoke_callback(&handler, ctx.clone(), &state.script_env, state, "hook") {
            print_error(err.format_with_source(""));
        }
    }
}

pub fn resolve_prompt(state: &ShellState) -> Result<Option<String>, RuntimeError> {
    let handler = state.extensions.borrow().prompt_handler.clone();
    let Some(handler) = handler else {
        return Ok(None);
    };

    let value = invoke_callback(
        &handler,
        prompt_context(state),
        &state.script_env,
        state,
        "prompt",
    )?;
    let Value::String(prompt) = value else {
        return Err(RuntimeError::new(
            0,
            RuntimeErrorKind::TypeMismatch,
            format!(
                "prompt handler must return String, got {}",
                value.type_name()
            ),
        ));
    };
    Ok(Some(prompt))
}

pub fn resolve_completion(
    state: &ShellState,
    command: &str,
    line: &str,
    word: &str,
    argv: Vec<String>,
    arg_index: usize,
) -> Result<Option<Vec<CompletionItem>>, RuntimeError> {
    let handler = state.extensions.borrow().completions.get(command).cloned();
    let Some(handler) = handler else {
        return Ok(None);
    };

    let value = invoke_callback(
        &handler,
        completion_context(line, word, argv, arg_index),
        &state.script_env,
        state,
        "complete",
    )?;
    Ok(Some(completion_items_from_value(value)?))
}

pub fn has_registered_command(state: &ShellState, name: &str) -> bool {
    state.extensions.borrow().script_commands.contains_key(name)
}

pub fn run_registered_command(
    state: &ShellState,
    name: &str,
    args: Vec<String>,
) -> Result<Option<CommandStatus>, RuntimeError> {
    let handler = state.extensions.borrow().script_commands.get(name).cloned();
    let Some(handler) = handler else {
        return Ok(None);
    };
    let value = invoke_callback(
        &handler,
        registered_command_context(name, args),
        &state.script_env,
        state,
        "registered command",
    )?;
    let status = match value {
        Value::Nil => CommandStatus::success(),
        Value::Int(code) if code >= 0 => CommandStatus::new(i32::try_from(code).map_err(|_| {
            RuntimeError::new(
                0,
                RuntimeErrorKind::TypeMismatch,
                "registered command status exceeds i32 range",
            )
        })?),
        other => {
            return Err(RuntimeError::new(
                0,
                RuntimeErrorKind::TypeMismatch,
                format!(
                    "registered command handler must return Nil or non-negative Int, got {}",
                    other.type_name()
                ),
            ));
        }
    };
    Ok(Some(status))
}

pub fn before_prompt_context(state: &ShellState) -> Value {
    object([
        ("cwd", Value::String(current_cwd())),
        ("status", Value::Int(state.last_status.code as i64)),
    ])
}

pub fn after_cd_context(old_cwd: String, cwd: String) -> Value {
    object([
        ("old_cwd", Value::String(old_cwd)),
        ("cwd", Value::String(cwd)),
    ])
}

pub fn preexec_context(command: &str) -> Value {
    object([
        ("command", Value::String(command.to_string())),
        ("cwd", Value::String(current_cwd())),
    ])
}

pub fn postexec_context(command: &str, status: CommandStatus, duration_ms: u128) -> Value {
    object([
        ("command", Value::String(command.to_string())),
        ("cwd", Value::String(current_cwd())),
        ("status", Value::Int(status.code as i64)),
        ("duration_ms", Value::Int(duration_ms as i64)),
    ])
}

pub fn prompt_context(state: &ShellState) -> Value {
    let duration = state.extensions.borrow().last_duration_ms.unwrap_or(0);
    let jobs = state
        .jobs
        .iter()
        .filter(|job| !matches!(job.status, JobStatus::Done(_)))
        .count() as i64;
    object([
        ("cwd", Value::String(current_cwd())),
        ("shell", Value::String(current_shell())),
        ("status", Value::Int(state.last_status.code as i64)),
        ("jobs", Value::Int(jobs)),
        ("shlvl", Value::Int(current_shlvl())),
        ("terminal_width", Value::Int(current_terminal_width())),
        ("duration_ms", Value::Int(duration as i64)),
    ])
}

fn completion_context(line: &str, word: &str, argv: Vec<String>, arg_index: usize) -> Value {
    object([
        ("line", Value::String(line.to_string())),
        ("word", Value::String(word.to_string())),
        (
            "argv",
            Value::Array(Rc::new(RefCell::new(
                argv.into_iter().map(Value::String).collect(),
            ))),
        ),
        ("arg_index", Value::Int(arg_index as i64)),
        ("cwd", Value::String(current_cwd())),
    ])
}

fn registered_command_context(name: &str, args: Vec<String>) -> Value {
    object([
        ("name", Value::String(name.to_string())),
        (
            "args",
            Value::Array(Rc::new(RefCell::new(
                args.into_iter().map(Value::String).collect(),
            ))),
        ),
        ("cwd", Value::String(current_cwd())),
    ])
}

fn completion_items_from_value(value: Value) -> Result<Vec<CompletionItem>, RuntimeError> {
    let Value::Array(items) = value else {
        return Err(RuntimeError::new(
            0,
            RuntimeErrorKind::TypeMismatch,
            format!(
                "complete handler must return Array<Object>, got {}",
                value.type_name()
            ),
        ));
    };

    items
        .borrow()
        .iter()
        .cloned()
        .map(completion_item_from_value)
        .collect()
}

fn completion_item_from_value(value: Value) -> Result<CompletionItem, RuntimeError> {
    let Value::Object(item) = value else {
        return Err(RuntimeError::new(
            0,
            RuntimeErrorKind::TypeMismatch,
            format!("completion item must be Object, got {}", value.type_name()),
        ));
    };
    let item = item.borrow();

    let value = match item.get("value") {
        Some(Value::String(value)) => value.clone(),
        Some(other) => {
            return Err(RuntimeError::new(
                0,
                RuntimeErrorKind::TypeMismatch,
                format!(
                    "completion item field 'value' must be String, got {}",
                    other.type_name()
                ),
            ));
        }
        None => {
            return Err(RuntimeError::new(
                0,
                RuntimeErrorKind::TypeMismatch,
                "completion item missing String field 'value'",
            ));
        }
    };

    Ok(CompletionItem {
        value,
        display: optional_string_field(&item, "display")?,
        desc: optional_string_field(&item, "desc")?,
        kind: optional_string_field(&item, "kind")?,
    })
}

fn optional_string_field(
    item: &HashMap<String, Value>,
    field: &str,
) -> Result<Option<String>, RuntimeError> {
    match item.get(field) {
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(other) => Err(RuntimeError::new(
            0,
            RuntimeErrorKind::TypeMismatch,
            format!(
                "completion item field '{}' must be String, got {}",
                field,
                other.type_name()
            ),
        )),
        None => Ok(None),
    }
}

fn object<const N: usize>(entries: [(&str, Value); N]) -> Value {
    let map = entries
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect::<HashMap<_, _>>();
    Value::Object(Rc::new(RefCell::new(map)))
}

fn current_cwd() -> String {
    std::env::current_dir()
        .map(|cwd| cwd.to_string_lossy().into_owned())
        .unwrap_or_else(|_| ".".to_string())
}

fn current_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "ecsh".to_string())
}

fn current_shlvl() -> i64 {
    std::env::var("SHLVL")
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(1)
}

fn current_terminal_width() -> i64 {
    let stdout = io::stdout();
    let fd = stdout.as_raw_fd();
    let mut size = libc::winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };

    // Prompt callbacks run on the main shell thread, so a direct ioctl read is safe here.
    let cols = unsafe {
        if libc::ioctl(fd, libc::TIOCGWINSZ, &mut size) == 0 && size.ws_col > 0 {
            Some(size.ws_col as i64)
        } else {
            None
        }
    };

    cols.or_else(|| {
        std::env::var("COLUMNS")
            .ok()
            .and_then(|value| value.parse::<i64>().ok())
            .filter(|value| *value > 0)
    })
    .unwrap_or(80)
}

/// Parses a key string like "ctrl-x", "tab", "enter" into a `(KeyCode, Modifiers)` pair.
/// Returns an error if the key string is not recognised.
pub fn parse_key_string(
    key: &str,
    span: usize,
) -> Result<(rustyline::KeyCode, rustyline::Modifiers), RuntimeError> {
    use rustyline::{KeyCode, Modifiers};
    let lower = key.to_lowercase();
    // Named special keys (no modifiers)
    match lower.as_str() {
        "tab" => return Ok((KeyCode::Tab, Modifiers::NONE)),
        "enter" | "return" => return Ok((KeyCode::Enter, Modifiers::NONE)),
        "esc" | "escape" => return Ok((KeyCode::Esc, Modifiers::NONE)),
        "backspace" => return Ok((KeyCode::Backspace, Modifiers::NONE)),
        "delete" | "del" => return Ok((KeyCode::Delete, Modifiers::NONE)),
        "up" => return Ok((KeyCode::Up, Modifiers::NONE)),
        "down" => return Ok((KeyCode::Down, Modifiers::NONE)),
        "left" => return Ok((KeyCode::Left, Modifiers::NONE)),
        "right" => return Ok((KeyCode::Right, Modifiers::NONE)),
        "home" => return Ok((KeyCode::Home, Modifiers::NONE)),
        "end" => return Ok((KeyCode::End, Modifiers::NONE)),
        _ => {}
    }

    // Modified keys: ctrl-<char>, alt-<char>
    if let Some(rest) = lower.strip_prefix("ctrl-") {
        let ch = parse_single_char_key(rest, key, span)?;
        return Ok((KeyCode::Char(ch.to_ascii_uppercase()), Modifiers::CTRL));
    }
    if let Some(rest) = lower.strip_prefix("alt-") {
        let ch = parse_single_char_key(rest, key, span)?;
        return Ok((KeyCode::Char(ch.to_ascii_lowercase()), Modifiers::ALT));
    }

    // Plain single-char key (only printable ASCII, digits, some punctuation)
    if key.len() == 1 {
        let ch = key.chars().next().unwrap();
        if ch.is_ascii_graphic() || ch == ' ' {
            return Ok((KeyCode::Char(ch), Modifiers::NONE));
        }
    }

    Err(RuntimeError::new(
        span,
        RuntimeErrorKind::TypeMismatch,
        format!("unsupported key string '{}'", key),
    ))
}

fn parse_single_char_key(s: &str, original: &str, span: usize) -> Result<char, RuntimeError> {
    if s.len() != 1 {
        return Err(RuntimeError::new(
            span,
            RuntimeErrorKind::TypeMismatch,
            format!("unsupported key string '{}'", original),
        ));
    }
    let ch = s.chars().next().unwrap();
    if !ch.is_ascii_alphabetic() {
        return Err(RuntimeError::new(
            span,
            RuntimeErrorKind::TypeMismatch,
            format!("unsupported key string '{}'", original),
        ));
    }
    Ok(ch)
}

/// Convert a parsed key to a `rustyline::Event` for binding registration.
pub fn key_to_event(
    keycode: rustyline::KeyCode,
    modifiers: rustyline::Modifiers,
) -> rustyline::Event {
    use rustyline::{Event, KeyEvent};
    Event::KeySeq(vec![KeyEvent(keycode, modifiers)])
}

/// Context object passed to bind callbacks.
fn bind_context(key: &str, line: &str, cursor: usize) -> Value {
    let cwd = current_cwd();
    object([
        ("key", Value::String(key.to_string())),
        ("line", Value::String(line.to_string())),
        ("cursor", Value::Int(cursor as i64)),
        ("cwd", Value::String(cwd)),
    ])
}

/// Convert a bind callback return value into a `rustyline::Cmd`.
/// Returns `None` if the result is `nil` (default behavior).
pub fn bind_result_to_cmd(result: &Value) -> Result<Option<rustyline::Cmd>, RuntimeError> {
    if matches!(result, Value::Nil) {
        return Ok(None);
    }

    let Value::Object(obj) = result else {
        return Err(RuntimeError::new(
            0,
            RuntimeErrorKind::TypeMismatch,
            format!(
                "bind handler must return nil or Object, got {}",
                result.type_name()
            ),
        ));
    };
    let obj = obj.borrow();

    let action = match obj.get("action") {
        Some(Value::String(s)) => s.clone(),
        Some(other) => {
            return Err(RuntimeError::new(
                0,
                RuntimeErrorKind::TypeMismatch,
                format!("action field must be String, got {}", other.type_name()),
            ));
        }
        None => {
            return Err(RuntimeError::new(
                0,
                RuntimeErrorKind::TypeMismatch,
                "bind result missing 'action' field",
            ));
        }
    };

    use rustyline::Cmd;
    let cmd = match action.as_str() {
        "accept" => Cmd::AcceptLine,
        "newline" => Cmd::Newline,
        "complete" => Cmd::Complete,
        "complete_hint" => Cmd::CompleteHint,
        "clear_screen" => Cmd::ClearScreen,
        "history_search_backward" => Cmd::HistorySearchBackward,
        "history_search_forward" => Cmd::HistorySearchForward,
        "previous_history" => Cmd::PreviousHistory,
        "next_history" => Cmd::NextHistory,
        "interrupt" => Cmd::Interrupt,
        "insert" => {
            let text = match obj.get("text") {
                Some(Value::String(s)) => s.clone(),
                Some(other) => {
                    return Err(RuntimeError::new(
                        0,
                        RuntimeErrorKind::TypeMismatch,
                        format!("insert text must be String, got {}", other.type_name()),
                    ));
                }
                None => {
                    return Err(RuntimeError::new(
                        0,
                        RuntimeErrorKind::TypeMismatch,
                        "insert action requires 'text' field",
                    ));
                }
            };
            Cmd::Insert(1, text)
        }
        other => {
            return Err(RuntimeError::new(
                0,
                RuntimeErrorKind::TypeMismatch,
                format!("unsupported bind action '{}'", other),
            ));
        }
    };

    Ok(Some(cmd))
}

/// Invoke a bind callback and return the resulting `rustyline::Cmd` (or `None` for default).
pub fn invoke_bind_callback(
    key: &str,
    line: &str,
    cursor: usize,
    state: &ShellState,
) -> Result<Option<rustyline::Cmd>, RuntimeError> {
    let handler = {
        let extensions = state.extensions.borrow();
        extensions.key_bindings.get(key).cloned()
    };
    let Some(handler) = handler else {
        return Ok(None);
    };

    let result = invoke_callback(
        &handler,
        bind_context(key, line, cursor),
        &state.script_env,
        state,
        "bind",
    )?;

    bind_result_to_cmd(&result)
}
#[cfg(test)]
mod tests {
    use super::{bind_result_to_cmd, object, prompt_context};
    use crate::ecscript::{Environment, Value};
    use crate::extensions::new_extensions;
    use crate::test_support::env_lock;
    use crate::types::{CommandStatus, ShellState};
    use rustyline::Cmd;
    use std::collections::HashMap;
    use std::rc::Rc;

    fn state() -> ShellState {
        ShellState {
            last_status: CommandStatus::new(7),
            interactive: true,
            shell_pgid: None,
            shell_terminal_fd: None,
            jobs: Vec::new(),
            next_job_id: 1,
            current_fg_pgid: None,
            script_env: Rc::new(Environment::new()),
            aliases: HashMap::new(),
            traps: HashMap::new(),
            command_history: Vec::new(),
            extensions: new_extensions(),
            module_loader: None,
        }
    }

    #[test]
    fn prompt_context_exposes_shell_metadata() {
        let _guard = env_lock().lock().unwrap();
        let old_shell = std::env::var_os("SHELL");
        let old_shlvl = std::env::var_os("SHLVL");
        unsafe {
            std::env::set_var("SHELL", "/tmp/ecsh-test");
            std::env::set_var("SHLVL", "3");
        }

        let value = prompt_context(&state());
        let Value::Object(obj) = value else {
            panic!("expected prompt context object");
        };
        let obj = obj.borrow();

        assert_eq!(
            obj.get("shell"),
            Some(&Value::String("/tmp/ecsh-test".into()))
        );
        assert_eq!(obj.get("shlvl"), Some(&Value::Int(3)));
        assert_eq!(obj.get("jobs"), Some(&Value::Int(0)));
        assert!(matches!(obj.get("terminal_width"), Some(Value::Int(width)) if *width > 0));

        match old_shell {
            Some(value) => unsafe { std::env::set_var("SHELL", value) },
            None => unsafe { std::env::remove_var("SHELL") },
        }
        match old_shlvl {
            Some(value) => unsafe { std::env::set_var("SHLVL", value) },
            None => unsafe { std::env::remove_var("SHLVL") },
        }
    }

    #[test]
    fn bind_result_maps_extended_actions() {
        let complete_hint =
            bind_result_to_cmd(&object([("action", Value::String("complete_hint".into()))]))
                .unwrap();
        assert_eq!(complete_hint, Some(Cmd::CompleteHint));

        let history_backward = bind_result_to_cmd(&object([(
            "action",
            Value::String("history_search_backward".into()),
        )]))
        .unwrap();
        assert_eq!(history_backward, Some(Cmd::HistorySearchBackward));

        let history_forward = bind_result_to_cmd(&object([(
            "action",
            Value::String("history_search_forward".into()),
        )]))
        .unwrap();
        assert_eq!(history_forward, Some(Cmd::HistorySearchForward));

        let prev_history = bind_result_to_cmd(&object([(
            "action",
            Value::String("previous_history".into()),
        )]))
        .unwrap();
        assert_eq!(prev_history, Some(Cmd::PreviousHistory));

        let next_history =
            bind_result_to_cmd(&object([("action", Value::String("next_history".into()))]))
                .unwrap();
        assert_eq!(next_history, Some(Cmd::NextHistory));
    }
    #[test]
    fn bind_result_nil_returns_none() {
        let result = bind_result_to_cmd(&Value::Nil).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn bind_result_unsupported_action_errors() {
        let obj = object([("action", Value::String("bogus_action".into()))]);
        let err = bind_result_to_cmd(&obj).unwrap_err();
        assert!(err.message.contains("unsupported bind action"));
        assert!(err.message.contains("bogus_action"));
    }

    #[test]
    fn bind_result_missing_action_errors() {
        let obj = object([]);
        let err = bind_result_to_cmd(&obj).unwrap_err();
        assert!(err.message.contains("missing 'action' field"));
    }

    #[test]
    fn bind_result_non_object_errors() {
        let err = bind_result_to_cmd(&Value::Int(1)).unwrap_err();
        assert!(err.message.contains("must return nil or Object"));
    }
}
