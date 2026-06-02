use crate::ecscript::{ast::Stmt, env::Environment};
use crate::types::ShellState;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Array(Rc<RefCell<Vec<Value>>>),
    Object(Rc<RefCell<HashMap<String, Value>>>),
    Function(Rc<Function>),
    Builtin(Builtin),
    Command(CommandInvocation),
}

impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Nil => "Nil",
            Value::Bool(_) => "Bool",
            Value::Int(_) => "Int",
            Value::Float(_) => "Float",
            Value::String(_) => "String",
            Value::Array(_) => "Array",
            Value::Object(_) => "Object",
            Value::Builtin(_) => "Builtin",
            Value::Function(_) => "Function",
            Value::Command(_) => "Command",
        }
    }
}

/// 验证数组索引，返回对应的 `usize`。
///
/// `allow_end` 为 true 时允许 `index == len`（用于 insert）；
/// 否则要求 `index < len`（用于读写和 remove）。
pub fn validate_array_index(
    index: i64,
    len: usize,
    allow_end: bool,
    span: usize,
) -> Result<usize, crate::ecscript::error::RuntimeError> {
    use crate::ecscript::error::{RuntimeError, RuntimeErrorKind};
    if index < 0 {
        return Err(RuntimeError::new(
            span,
            RuntimeErrorKind::IndexOutOfBounds,
            format!("array index {} out of bounds for length {}", index, len),
        ));
    }
    let idx = index as usize;
    let max = if allow_end {
        len
    } else {
        len.saturating_sub(1)
    };
    if idx > max && (allow_end || len > 0) || (!allow_end && len == 0) {
        return Err(RuntimeError::new(
            span,
            RuntimeErrorKind::IndexOutOfBounds,
            format!("array index {} out of bounds for length {}", index, len),
        ));
    }
    Ok(idx)
}

#[derive(Debug, Clone, PartialEq)]
pub enum Builtin {
    CommandBuilder,
    Env,
    SetEnv,
    UnsetEnv,
    Cwd,
    Stdin,
    ReadLines,
    Range,
    Len,
    Keys,
    Values,
    ToJson,
    FromJson,
    Print,
    Println,
    Push,
    Pop,
    Insert,
    Remove,
    Slice,
    Run,
    Capture,
    Text,
    Lines,
    WithEnv,
    WithCwd,
    Map,
    Filter,
    Reduce,
    Each,
    Any,
    All,
    Find,
    Join,
    JoinPath,
    WriteLines,
    Hook,
    Prompt,
    Complete,
    RegisterCommand,
    SetCwd,
    Bind,
    Trim,
    Help,
    Builtins,
    Extensions,
    Commands,
}

impl Builtin {
    pub fn name(&self) -> &'static str {
        match self {
            Builtin::CommandBuilder => "command",
            Builtin::Env => "env",
            Builtin::SetEnv => "set_env",
            Builtin::UnsetEnv => "unset_env",
            Builtin::Cwd => "cwd",
            Builtin::Stdin => "stdin",
            Builtin::ReadLines => "read_lines",
            Builtin::Range => "range",
            Builtin::Len => "len",
            Builtin::Keys => "keys",
            Builtin::Values => "values",
            Builtin::ToJson => "to_json",
            Builtin::FromJson => "from_json",
            Builtin::Print => "print",
            Builtin::Println => "println",
            Builtin::Push => "push",
            Builtin::Pop => "pop",
            Builtin::Insert => "insert",
            Builtin::Remove => "remove",
            Builtin::Slice => "slice",
            Builtin::Run => "run",
            Builtin::Capture => "capture",
            Builtin::Text => "text",
            Builtin::Lines => "lines",
            Builtin::WithEnv => "with_env",
            Builtin::WithCwd => "with_cwd",
            Builtin::Map => "map",
            Builtin::Filter => "filter",
            Builtin::Reduce => "reduce",
            Builtin::Each => "each",
            Builtin::Any => "any",
            Builtin::All => "all",
            Builtin::Find => "find",
            Builtin::Join => "join",
            Builtin::JoinPath => "join_path",
            Builtin::WriteLines => "write_lines",
            Builtin::Hook => "hook",
            Builtin::Prompt => "prompt",
            Builtin::Complete => "complete",
            Builtin::RegisterCommand => "register_command",
            Builtin::SetCwd => "set_cwd",
            Builtin::Trim => "trim",
            Builtin::Bind => "bind",
            Builtin::Help => "help",
            Builtin::Builtins => "builtins",
            Builtin::Extensions => "extensions",
            Builtin::Commands => "commands",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    pub name: Option<String>, // 后续支持匿名函数
    pub params: Vec<String>,
    pub stmts: Vec<Stmt>,
    pub captures: HashMap<String, Slot>,
}

pub type Slot = Rc<RefCell<Value>>;

pub enum Binding {
    Direct(Value),
    Shared(Slot), // 变量被提升到堆上
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum VisitKey {
    Array(*const RefCell<Vec<Value>>),
    Object(*const RefCell<HashMap<String, Value>>),
}

/// 命令调用的环境/条件
#[derive(Clone, Debug, PartialEq)]
pub struct CommandInvocation {
    pub command: CommandValue,
    pub cwd_override: Option<String>,
    pub env_override: Option<BTreeMap<String, String>>,
    pub stdin_override: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CommandValue {
    Simple(crate::types::Command),
    Pipeline(crate::types::Pipeline),
}

pub fn repr_value(value: &Value) -> String {
    let mut visiting = HashSet::new();
    repr_value_inner(value, &mut visiting)
}

#[derive(Clone, Debug, PartialEq)]
pub struct CommandResult {
    pub code: i32,
    pub signal: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u128,
}

pub struct BuiltinContext<'a> {
    pub shell_state: Option<&'a ShellState>,
    pub env: &'a Environment<'a>,
    pub stdin_text: Option<&'a str>,
}

pub fn display_value(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        _ => repr_value(value),
    }
}

fn repr_value_inner(value: &Value, visiting: &mut HashSet<VisitKey>) -> String {
    match value {
        Value::Nil => "nil".to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Int(value) => value.to_string(),
        Value::Float(value) => value.to_string(),
        Value::String(value) => format!("{:?}", value),
        Value::Array(values) => {
            let visit_key = VisitKey::Array(Rc::as_ptr(values));
            if !visiting.insert(visit_key) {
                return "[...]".to_string();
            }

            let rendered = {
                let borrowed = values.borrow();
                borrowed
                    .iter()
                    .map(|item| repr_value_inner(item, visiting))
                    .collect::<Vec<_>>()
                    .join(", ")
            };

            visiting.remove(&visit_key);
            format!("[{}]", rendered)
        }
        Value::Object(values) => {
            let visit_key = VisitKey::Object(Rc::as_ptr(values));
            if !visiting.insert(visit_key) {
                return "{...}".to_string();
            }

            let rendered = {
                let borrowed = values.borrow();
                let mut entries: Vec<_> = borrowed.iter().collect();
                entries.sort_by(|a, b| a.0.cmp(b.0));
                entries
                    .into_iter()
                    .map(|(key, value)| format!("{}: {}", key, repr_value_inner(value, visiting)))
                    .collect::<Vec<_>>()
                    .join(", ")
            };

            visiting.remove(&visit_key);
            format!("{{{}}}", rendered)
        }
        Value::Function(function) => match &function.name {
            Some(name) => format!("<func {}>", name),
            None => "<lambda>".to_string(),
        },
        Value::Builtin(builtin) => format!("<builtin {}>", builtin.name()),
        Value::Command(invocation) => match &invocation.command {
            CommandValue::Simple(command) => format!("<cmd {}>", command),
            CommandValue::Pipeline(pipeline) => {
                let rendered = pipeline
                    .commands
                    .iter()
                    .map(|command| command.to_string())
                    .collect::<Vec<_>>()
                    .join(" | ");
                format!("<cmd {}>", rendered)
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{Builtin, Value, display_value, repr_value};
    use std::{cell::RefCell, collections::HashMap, rc::Rc};

    #[test]
    fn repr_quotes_strings() {
        assert_eq!(
            repr_value(&Value::String("hi\nthere".into())),
            "\"hi\\nthere\""
        );
    }

    #[test]
    fn display_leaves_top_level_string_unquoted() {
        assert_eq!(display_value(&Value::String("hello".into())), "hello");
    }

    #[test]
    fn repr_sorts_object_keys() {
        let obj = Rc::new(RefCell::new(HashMap::from([
            ("b".to_string(), Value::Int(2)),
            ("a".to_string(), Value::Int(1)),
        ])));

        assert_eq!(repr_value(&Value::Object(obj)), "{a: 1, b: 2}");
    }

    #[test]
    fn repr_formats_builtin_name() {
        assert_eq!(
            repr_value(&Value::Builtin(Builtin::Println)),
            "<builtin println>"
        );
    }
}
