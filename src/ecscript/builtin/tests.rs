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
        stdin_text: None,
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
    let result = run_builtin(Builtin::Env, vec![Value::String("PATH".into())], 0, ctx()).unwrap();
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
fn cwd_returns_current_directory_string() {
    let result = run_builtin(Builtin::Cwd, vec![], 0, ctx()).unwrap();
    let Value::String(cwd) = result else {
        panic!("expected string");
    };
    assert!(!cwd.is_empty());
    assert!(std::path::Path::new(&cwd).is_absolute());
}

#[test]
fn stdin_and_read_lines_use_builtin_context_input() {
    let env = Box::leak(Box::new(Environment::new()));
    let ctx = BuiltinContext {
        shell_state: None,
        env,
        stdin_text: Some("a\nb\n"),
    };

    let stdin = run_builtin(Builtin::Stdin, vec![], 0, ctx).unwrap();
    assert_eq!(stdin, Value::String("a\nb\n".into()));

    let ctx = BuiltinContext {
        shell_state: None,
        env,
        stdin_text: Some("a\nb\n"),
    };
    let lines = run_builtin(Builtin::ReadLines, vec![], 0, ctx).unwrap();
    let Value::Array(lines) = lines else {
        panic!("expected array");
    };
    assert_eq!(
        *lines.borrow(),
        vec![Value::String("a".into()), Value::String("b".into())]
    );
}

#[test]
fn join_path_uses_platform_path_joining() {
    let result = run_builtin(
        Builtin::JoinPath,
        vec![Value::String("/tmp".into()), Value::String("ecsh".into())],
        0,
        ctx(),
    )
    .unwrap();
    let Value::String(path) = result else {
        panic!("expected string");
    };
    assert_eq!(
        std::path::PathBuf::from(path),
        std::path::PathBuf::from("/tmp").join("ecsh")
    );
}

#[test]
fn range_returns_closed_interval_array() {
    let result = run_builtin(Builtin::Range, vec![Value::Int(1), Value::Int(4)], 0, ctx()).unwrap();
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
    let result = run_builtin(Builtin::Range, vec![Value::Int(4), Value::Int(1)], 0, ctx()).unwrap();
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
    let result = run_builtin(Builtin::Range, vec![Value::Int(0), Value::Int(3)], 0, ctx()).unwrap();
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
    let result = run_builtin(Builtin::Range, vec![Value::Int(5), Value::Int(5)], 0, ctx()).unwrap();
    let Value::Array(arr) = result else {
        panic!("expected array")
    };
    assert_eq!(*arr.borrow(), vec![Value::Int(5)]);
}

#[test]
fn range_reversed_returns_empty() {
    let result = run_builtin(Builtin::Range, vec![Value::Int(5), Value::Int(0)], 0, ctx()).unwrap();
    let Value::Array(arr) = result else {
        panic!("expected array")
    };
    assert!(arr.borrow().is_empty());
}
