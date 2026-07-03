use super::{
    format_print_args, run_builtin,
    support::{ParamType, Signature, check_signature, param},
};
use crate::ecscript::{
    env::Environment,
    error::RuntimeErrorKind,
    value::{Builtin, BuiltinContext, Function, Value},
};
use crate::extensions::{HookName, new_extensions};
use crate::types::{CommandStatus, ShellState};
use std::{cell::RefCell, collections::HashMap, rc::Rc};

fn ctx() -> BuiltinContext<'static> {
    BuiltinContext {
        shell_state: None,
        env: Box::leak(Box::new(Environment::new())),
        stdin_text: None,
    }
}

fn shell_ctx<'a>(state: &'a ShellState, env: &'a Environment<'a>) -> BuiltinContext<'a> {
    BuiltinContext {
        shell_state: Some(state),
        env,
        stdin_text: None,
    }
}

fn no_op_func() -> Value {
    Value::Function(Rc::new(Function {
        name: None,
        params: vec!["ctx".into()],
        stmts: vec![],
        captures: HashMap::new(),
    }))
}

fn simple_command_value(program: &str) -> Value {
    Value::Command(crate::ecscript::value::CommandInvocation {
        command: crate::ecscript::value::CommandValue::Simple(crate::types::Command {
            program: crate::types::ShellWord::lit(program),
            args: vec![],
            redirection: crate::types::Redirection::default(),
        }),
        cwd_override: None,
        env_override: None,
        stdin_override: None,
    })
}

fn interactive_state() -> ShellState {
    ShellState {
        last_status: CommandStatus::success(),
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
fn signature_exact_arity_reports_too_few_and_too_many_arguments() {
    const SIG: Signature = Signature::exact(
        "range",
        &[param("start", ParamType::Int), param("end", ParamType::Int)],
    );

    let err = check_signature(&SIG, &[Value::Int(1)], 7).unwrap_err();
    assert_eq!(err.kind, RuntimeErrorKind::ArityMismatch);
    assert_eq!(err.offset, 7);
    assert_eq!(err.message, "range expects 2 arguments, got 1");

    let err = check_signature(&SIG, &[Value::Int(1), Value::Int(2), Value::Int(3)], 0).unwrap_err();
    assert_eq!(err.kind, RuntimeErrorKind::ArityMismatch);
    assert_eq!(err.message, "range expects 2 arguments, got 3");
}

#[test]
fn signature_at_least_arity_reports_missing_arguments() {
    const SIG: Signature = Signature::at_least(
        "push",
        &[param("array", ParamType::Array)],
        Some(ParamType::Any),
        2,
    );

    let err = check_signature(&SIG, &[Value::Array(Rc::new(RefCell::new(vec![])))], 0).unwrap_err();
    assert_eq!(err.kind, RuntimeErrorKind::ArityMismatch);
    assert_eq!(err.message, "push expects at least 2 arguments, got 1");
}

#[test]
fn signature_checks_variadic_argument_type() {
    const SIG: Signature = Signature::at_least(
        "numbers",
        &[param("head", ParamType::Int)],
        Some(ParamType::Int),
        1,
    );

    let err = check_signature(&SIG, &[Value::Int(1), Value::String("x".into())], 0).unwrap_err();
    assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
    assert_eq!(
        err.message,
        "numbers argument 'value' expects Int, got String"
    );
}

#[test]
fn signature_one_of_display_uses_stable_joining() {
    const LEN_TYPES: &[ParamType] = &[ParamType::Array, ParamType::Object, ParamType::String];
    const SIG: Signature = Signature::exact("len", &[param("value", ParamType::OneOf(LEN_TYPES))]);

    let err = check_signature(&SIG, &[no_op_func()], 0).unwrap_err();
    assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
    assert_eq!(
        err.message,
        "len argument 'value' expects Array, Object, or String, got Function"
    );
}

#[test]
fn signature_type_errors_include_parameter_name() {
    const SIG: Signature = Signature::exact("env", &[param("name", ParamType::String)]);

    let err = check_signature(&SIG, &[Value::Bool(true)], 0).unwrap_err();
    assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
    assert_eq!(err.message, "env argument 'name' expects String, got Bool");
}

#[test]
fn migrated_builtin_names_have_spec_entries() {
    let migrated = [
        "command",
        "env",
        "set_env",
        "unset_env",
        "cwd",
        "stdin",
        "read_lines",
        "range",
        "len",
        "clone",
        "keys",
        "values",
        "slice",
        "to_json",
        "from_json",
        "trim",
        "print",
        "println",
        "push",
        "pop",
        "insert",
        "remove",
        "map",
        "filter",
        "reduce",
        "each",
        "any",
        "all",
        "find",
        "join",
        "join_path",
        "run",
        "capture",
        "text",
        "lines",
        "with_cwd",
        "write_lines",
        "set_cwd",
    ];
    let spec_names = crate::specs::all_specs()
        .into_iter()
        .map(|spec| spec.name)
        .collect::<std::collections::HashSet<_>>();

    for name in migrated {
        assert!(spec_names.contains(name), "missing builtin spec for {name}");
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
    assert_eq!(err.message, "env argument 'name' expects String, got Int");
}

#[test]
fn set_env_and_unset_env_modify_process_environment() {
    let name = "ECSH_TEST_SET_ENV_BUILTIN";
    unsafe { std::env::remove_var(name) };

    let result = run_builtin(
        Builtin::SetEnv,
        vec![
            Value::String(name.into()),
            Value::String("configured".into()),
        ],
        0,
        ctx(),
    )
    .unwrap();
    assert_eq!(result, Value::Nil);
    assert_eq!(std::env::var(name).ok().as_deref(), Some("configured"));

    let result = run_builtin(
        Builtin::UnsetEnv,
        vec![Value::String(name.into())],
        0,
        ctx(),
    )
    .unwrap();
    assert_eq!(result, Value::Nil);
    assert_eq!(std::env::var_os(name), None);
}

#[test]
fn set_env_and_unset_env_reject_invalid_names() {
    let err = run_builtin(
        Builtin::SetEnv,
        vec![
            Value::String("INVALID-NAME".into()),
            Value::String("value".into()),
        ],
        0,
        ctx(),
    )
    .unwrap_err();
    assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
    assert_eq!(err.message, "set_env invalid variable name: INVALID-NAME");

    let err = run_builtin(
        Builtin::UnsetEnv,
        vec![Value::String("INVALID-NAME".into())],
        0,
        ctx(),
    )
    .unwrap_err();
    assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
    assert_eq!(err.message, "unset_env invalid variable name: INVALID-NAME");
}

#[test]
fn hook_requires_shell_context() {
    let err = run_builtin(
        Builtin::Hook,
        vec![Value::String("before_prompt".into()), no_op_func()],
        0,
        ctx(),
    )
    .unwrap_err();
    assert_eq!(err.kind, RuntimeErrorKind::IoError);
    assert_eq!(err.message, "hook requires interactive ecsh shell context");
}

#[test]
fn hook_rejects_unknown_name() {
    let env = Environment::new();
    let state = interactive_state();

    let err = run_builtin(
        Builtin::Hook,
        vec![Value::String("nope".into()), no_op_func()],
        0,
        shell_ctx(&state, &env),
    )
    .unwrap_err();
    assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
    assert_eq!(err.message, "unknown hook 'nope'");
}

#[test]
fn hook_reports_name_type_from_signature() {
    let err = run_builtin(Builtin::Hook, vec![Value::Int(1), no_op_func()], 0, ctx()).unwrap_err();

    assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
    assert_eq!(err.message, "hook argument 'name' expects String, got Int");
}

#[test]
fn prompt_and_complete_register_handlers() {
    let env = Environment::new();
    let state = interactive_state();

    run_builtin(
        Builtin::Prompt,
        vec![no_op_func()],
        0,
        shell_ctx(&state, &env),
    )
    .unwrap();
    run_builtin(
        Builtin::Complete,
        vec![Value::String("git".into()), no_op_func()],
        0,
        shell_ctx(&state, &env),
    )
    .unwrap();
    run_builtin(
        Builtin::Hook,
        vec![Value::String("before_prompt".into()), no_op_func()],
        0,
        shell_ctx(&state, &env),
    )
    .unwrap();

    let registry = state.extensions.borrow();
    assert!(registry.prompt_handler.is_some());
    assert!(registry.completions.contains_key("git"));
    assert_eq!(
        registry.hooks.get(&HookName::BeforePrompt).unwrap().len(),
        1
    );
}

#[test]
fn prompt_reports_function_type_from_signature() {
    let err = run_builtin(Builtin::Prompt, vec![Value::String("x".into())], 0, ctx()).unwrap_err();

    assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
    assert_eq!(
        err.message,
        "prompt argument 'function' expects Function, got String"
    );
}

#[test]
fn complete_reports_function_type_from_signature() {
    let err = run_builtin(
        Builtin::Complete,
        vec![Value::String("git".into()), Value::Bool(true)],
        0,
        ctx(),
    )
    .unwrap_err();

    assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
    assert_eq!(
        err.message,
        "complete argument 'function' expects Function, got Bool"
    );
}

#[test]
fn register_command_registers_handler() {
    let env = Environment::new();
    let state = interactive_state();

    run_builtin(
        Builtin::RegisterCommand,
        vec![Value::String("z".into()), no_op_func()],
        0,
        shell_ctx(&state, &env),
    )
    .unwrap();

    assert!(state.extensions.borrow().script_commands.contains_key("z"));
}

#[test]
fn register_command_reports_name_type_from_signature() {
    let err = run_builtin(
        Builtin::RegisterCommand,
        vec![Value::Int(1), no_op_func()],
        0,
        ctx(),
    )
    .unwrap_err();

    assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
    assert_eq!(
        err.message,
        "register_command argument 'name' expects String, got Int"
    );
}

#[test]
fn register_command_rejects_shell_builtin_name() {
    let env = Environment::new();
    let state = interactive_state();

    let err = run_builtin(
        Builtin::RegisterCommand,
        vec![Value::String("cd".into()), no_op_func()],
        0,
        shell_ctx(&state, &env),
    )
    .unwrap_err();

    assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
    assert_eq!(
        err.message,
        "register_command cannot override shell builtin: cd"
    );
}

#[test]
fn bind_reports_function_type_from_signature() {
    let err = run_builtin(
        Builtin::Bind,
        vec![Value::String("ctrl-x".into()), Value::Nil],
        0,
        ctx(),
    )
    .unwrap_err();

    assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
    assert_eq!(
        err.message,
        "bind argument 'function' expects Function, got Nil"
    );
}

#[test]
fn bind_registers_handler_for_supported_key() {
    let env = Environment::new();
    let state = interactive_state();

    run_builtin(
        Builtin::Bind,
        vec![Value::String("ctrl-x".into()), no_op_func()],
        0,
        shell_ctx(&state, &env),
    )
    .unwrap();

    assert!(
        state
            .extensions
            .borrow()
            .key_bindings
            .contains_key("ctrl-x")
    );
}

#[test]
fn bind_rejects_unsupported_key_string() {
    let env = Environment::new();
    let state = interactive_state();

    let err = run_builtin(
        Builtin::Bind,
        vec![Value::String("ctrl-enter".into()), no_op_func()],
        0,
        shell_ctx(&state, &env),
    )
    .unwrap_err();

    assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
    assert_eq!(err.message, "unsupported key string 'ctrl-enter'");
}

#[test]
fn trim_removes_surrounding_whitespace() {
    let result = run_builtin(
        Builtin::Trim,
        vec![Value::String(" \n/tmp/ecsh\t ".into())],
        0,
        ctx(),
    )
    .unwrap();

    assert_eq!(result, Value::String("/tmp/ecsh".into()));
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
    assert_eq!(err.message, "range argument 'start' expects Int, got Bool");

    let err = run_builtin(
        Builtin::Range,
        vec![Value::Int(1), Value::Bool(true)],
        0,
        ctx(),
    )
    .unwrap_err();
    assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
    assert_eq!(err.message, "range argument 'end' expects Int, got Bool");
}

#[test]
fn len_reports_one_of_signature_error() {
    let err = run_builtin(Builtin::Len, vec![no_op_func()], 0, ctx()).unwrap_err();
    assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
    assert_eq!(
        err.message,
        "len argument 'value' expects Array, Object, or String, got Function"
    );
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
    assert_eq!(
        err.message,
        "insert argument 'index' expects Int, got String"
    );
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
    assert_eq!(
        err.message,
        "from_json argument 'text' expects String, got Int"
    );
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
fn slice_reports_start_type_from_signature() {
    let arr = Rc::new(RefCell::new(vec![]));
    let err = run_builtin(
        Builtin::Slice,
        vec![Value::Array(arr), Value::String("0".into()), Value::Int(1)],
        0,
        ctx(),
    )
    .unwrap_err();

    assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
    assert_eq!(
        err.message,
        "slice argument 'start' expects Int, got String"
    );
}

#[test]
fn push_reports_array_type_from_signature() {
    let err = run_builtin(Builtin::Push, vec![Value::Int(1), Value::Int(2)], 0, ctx()).unwrap_err();

    assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
    assert_eq!(err.message, "push argument 'array' expects Array, got Int");
}

#[test]
fn pop_reports_array_type_from_signature() {
    let err = run_builtin(Builtin::Pop, vec![Value::Int(1)], 0, ctx()).unwrap_err();

    assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
    assert_eq!(err.message, "pop argument 'array' expects Array, got Int");
}

#[test]
fn clone_deep_copies_arrays_and_objects() {
    let inner = Value::Array(Rc::new(RefCell::new(vec![Value::Int(1)])));
    let original = Rc::new(RefCell::new(HashMap::from([("inner".into(), inner)])));

    let result = run_builtin(
        Builtin::Clone,
        vec![Value::Object(original.clone())],
        0,
        ctx(),
    )
    .unwrap();

    let Value::Object(copy) = result else {
        panic!("expected object");
    };
    let Value::Array(copy_inner) = copy.borrow().get("inner").cloned().unwrap() else {
        panic!("expected nested array");
    };
    copy_inner.borrow_mut().push(Value::Int(2));

    let Value::Array(original_inner) = original.borrow().get("inner").cloned().unwrap() else {
        panic!("expected nested array");
    };
    assert_eq!(*original_inner.borrow(), vec![Value::Int(1)]);
    assert_eq!(*copy_inner.borrow(), vec![Value::Int(1), Value::Int(2)]);
}

#[test]
fn clone_rejects_circular_arrays() {
    let arr = Rc::new(RefCell::new(Vec::new()));
    arr.borrow_mut().push(Value::Array(arr.clone()));

    let err = run_builtin(Builtin::Clone, vec![Value::Array(arr)], 0, ctx()).unwrap_err();
    assert_eq!(err.kind, RuntimeErrorKind::CircularReference);
    assert_eq!(err.message, "clone cannot copy circular Array reference");
}

#[test]
fn clone_rejects_callable_and_command_values() {
    let err = run_builtin(Builtin::Clone, vec![no_op_func()], 0, ctx()).unwrap_err();
    assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
    assert_eq!(err.message, "clone cannot copy Function values");

    let err =
        run_builtin(Builtin::Clone, vec![Value::Builtin(Builtin::Len)], 0, ctx()).unwrap_err();
    assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
    assert_eq!(err.message, "clone cannot copy Builtin values");
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
fn map_reports_function_type_from_signature() {
    let items = Rc::new(RefCell::new(vec![Value::Int(1)]));
    let err = run_builtin(
        Builtin::Map,
        vec![Value::Array(items), Value::String("not a function".into())],
        0,
        ctx(),
    )
    .unwrap_err();

    assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
    assert_eq!(
        err.message,
        "map argument 'function' expects Function, got String"
    );
}

#[test]
fn join_reports_separator_type_from_signature() {
    let items = Rc::new(RefCell::new(vec![Value::Int(1)]));
    let err = run_builtin(
        Builtin::Join,
        vec![Value::Array(items), Value::Bool(true)],
        0,
        ctx(),
    )
    .unwrap_err();

    assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
    assert_eq!(
        err.message,
        "join argument 'separator' expects String, got Bool"
    );
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
fn run_reports_command_type_from_signature() {
    let err = run_builtin(Builtin::Run, vec![Value::String("echo".into())], 0, ctx()).unwrap_err();

    assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
    assert_eq!(
        err.message,
        "run argument 'command' expects Command, got String"
    );
}

#[test]
fn capture_reports_command_type_from_signature() {
    let err = run_builtin(Builtin::Capture, vec![Value::Int(1)], 0, ctx()).unwrap_err();

    assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
    assert_eq!(
        err.message,
        "capture argument 'command' expects Command, got Int"
    );
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
    let mut command = match simple_command_value("printf") {
        Value::Command(command) => command,
        _ => unreachable!(),
    };
    command.env_override = Some(std::collections::BTreeMap::from([(
        "BASE".to_string(),
        "1".to_string(),
    )]));
    let command = Value::Command(command);
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
    let command = simple_command_value("printf");
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
fn with_env_reports_command_type_from_signature() {
    let env_obj = Value::Object(Rc::new(RefCell::new(HashMap::new())));
    let err = run_builtin(Builtin::WithEnv, vec![Value::Int(1), env_obj], 0, ctx()).unwrap_err();

    assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
    assert_eq!(
        err.message,
        "with_env argument 'command' expects Command, got Int"
    );
}

#[test]
fn with_env_reports_env_map_type_from_signature() {
    let err = run_builtin(
        Builtin::WithEnv,
        vec![simple_command_value("printf"), Value::String("x".into())],
        0,
        ctx(),
    )
    .unwrap_err();

    assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
    assert_eq!(
        err.message,
        "with_env argument 'env_map' expects Object, got String"
    );
}

#[test]
fn with_cwd_derives_command_with_override() {
    let command = simple_command_value("pwd");

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
fn with_cwd_reports_path_type_from_signature() {
    let err = run_builtin(
        Builtin::WithCwd,
        vec![simple_command_value("pwd"), Value::Int(1)],
        0,
        ctx(),
    )
    .unwrap_err();

    assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
    assert_eq!(
        err.message,
        "with_cwd argument 'path' expects String, got Int"
    );
}

#[test]
fn write_lines_reports_array_type_from_signature() {
    let err = run_builtin(
        Builtin::WriteLines,
        vec![Value::String("x".into())],
        0,
        ctx(),
    )
    .unwrap_err();

    assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
    assert_eq!(
        err.message,
        "write_lines argument 'array' expects Array, got String"
    );
}

#[test]
fn set_cwd_reports_path_type_from_signature_before_shell_context() {
    let err = run_builtin(Builtin::SetCwd, vec![Value::Int(1)], 0, ctx()).unwrap_err();

    assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
    assert_eq!(
        err.message,
        "set_cwd argument 'path' expects String, got Int"
    );
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

// ── introspection builtins ──────────────────────────────────────────

#[test]
fn help_no_args_returns_overview() {
    let result = run_builtin(Builtin::Help, vec![], 0, ctx()).unwrap();
    let Value::String(text) = result else {
        panic!("expected string");
    };
    assert!(text.contains("ecsh Help Overview"));
    assert!(text.contains("Ecscript Builtins"));
    assert!(text.contains("Shell Extensions"));
    assert!(text.contains("Shell Builtins"));
    // Should mention at least some known names
    assert!(text.contains("map"));
    assert!(text.contains("hook"));
    assert!(text.contains("cd"));
    assert!(text.contains("Use help(\"name\")"));
}

#[test]
fn help_with_name_returns_formatted_help() {
    let result = run_builtin(Builtin::Help, vec![Value::String("map".into())], 0, ctx()).unwrap();
    let Value::String(text) = result else {
        panic!("expected string");
    };
    assert!(text.contains("map (ecscript builtin)"));
    assert!(text.contains("Signature:"));
    assert!(text.contains("Summary:"));
    assert!(text.contains("Details:"));
    assert!(text.contains("Examples:"));
    assert!(text.contains("map(array, func)"));
}

#[test]
fn help_unknown_name_returns_error() {
    let err = run_builtin(
        Builtin::Help,
        vec![Value::String("nonexistent_fn".into())],
        0,
        ctx(),
    )
    .unwrap_err();
    assert_eq!(err.kind, RuntimeErrorKind::UndefinedVariable);
    assert!(err.message.contains("nonexistent_fn"));
}

#[test]
fn help_too_many_args_returns_arity_error() {
    let err = run_builtin(
        Builtin::Help,
        vec![Value::String("a".into()), Value::String("b".into())],
        0,
        ctx(),
    )
    .unwrap_err();
    assert_eq!(err.kind, RuntimeErrorKind::ArityMismatch);
}

#[test]
fn help_wrong_arg_type_returns_type_error() {
    let err = run_builtin(Builtin::Help, vec![Value::Int(42)], 0, ctx()).unwrap_err();
    assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
}

#[test]
fn builtins_returns_sorted_array_of_names() {
    let result = run_builtin(Builtin::Builtins, vec![], 0, ctx()).unwrap();
    let Value::Array(arr) = result else {
        panic!("expected array");
    };
    let names: Vec<String> = arr
        .borrow()
        .iter()
        .map(|v| match v {
            Value::String(s) => s.clone(),
            _ => panic!("expected string"),
        })
        .collect();
    // Must be sorted
    assert!(
        names.windows(2).all(|w| w[0] <= w[1]),
        "names not sorted: {names:?}"
    );
    // Should include known builtins
    assert!(names.contains(&"map".to_string()));
    assert!(names.contains(&"range".to_string()));
    assert!(names.contains(&"help".to_string()));
    assert!(names.contains(&"builtins".to_string()));
    // Must not include shell extensions or shell builtins
    assert!(!names.contains(&"hook".to_string()));
    assert!(!names.contains(&"cd".to_string()));
    // Must be deduplicated
    let mut unique = names.clone();
    unique.dedup();
    assert_eq!(names.len(), unique.len());
}

#[test]
fn builtins_rejects_args() {
    let err =
        run_builtin(Builtin::Builtins, vec![Value::String("a".into())], 0, ctx()).unwrap_err();
    assert_eq!(err.kind, RuntimeErrorKind::ArityMismatch);
}

#[test]
fn extensions_returns_sorted_array_of_names() {
    let result = run_builtin(Builtin::Extensions, vec![], 0, ctx()).unwrap();
    let Value::Array(arr) = result else {
        panic!("expected array");
    };
    let names: Vec<String> = arr
        .borrow()
        .iter()
        .map(|v| match v {
            Value::String(s) => s.clone(),
            _ => panic!("expected string"),
        })
        .collect();
    assert!(
        names.windows(2).all(|w| w[0] <= w[1]),
        "names not sorted: {names:?}"
    );
    assert!(names.contains(&"hook".to_string()));
    assert!(names.contains(&"prompt".to_string()));
    assert!(names.contains(&"bind".to_string()));
    assert!(!names.contains(&"map".to_string()));
    // Deduplicated
    let mut unique = names.clone();
    unique.dedup();
    assert_eq!(names.len(), unique.len());
}

#[test]
fn extensions_rejects_args() {
    let err = run_builtin(
        Builtin::Extensions,
        vec![Value::String("a".into())],
        0,
        ctx(),
    )
    .unwrap_err();
    assert_eq!(err.kind, RuntimeErrorKind::ArityMismatch);
}

#[test]
fn commands_requires_shell_context() {
    let err = run_builtin(Builtin::Commands, vec![], 0, ctx()).unwrap_err();
    assert_eq!(err.kind, RuntimeErrorKind::IoError);
    assert!(err.message.contains("shell context"));
}

/// Extract `(name, kind)` tuple from a {name, kind} Value::Object.
fn extract_name_kind(item: &Value) -> (String, String) {
    let Value::Object(obj) = item else {
        panic!("expected Object, got {:?}", item);
    };
    let obj_ref = obj.borrow();
    let name = match obj_ref.get("name") {
        Some(Value::String(s)) => s.clone(),
        other => panic!("name must be String, got {:?}", other),
    };
    let kind = match obj_ref.get("kind") {
        Some(Value::String(s)) => s.clone(),
        other => panic!("kind must be String, got {:?}", other),
    };
    (name, kind)
}

#[test]
fn commands_returns_objects_with_name_and_kind() {
    let env = Environment::new();
    let mut state = interactive_state();
    state.aliases.insert("ll".into(), "ls -la".into());
    state
        .extensions
        .borrow_mut()
        .script_commands
        .insert("mycmd".into(), no_op_func());

    let result = run_builtin(Builtin::Commands, vec![], 0, shell_ctx(&state, &env)).unwrap();
    let Value::Array(arr) = result else {
        panic!("expected array");
    };
    let items = arr.borrow();
    assert!(!items.is_empty(), "commands should not be empty");

    for item in items.iter() {
        let (name, kind) = extract_name_kind(item);
        assert!(
            kind == "builtin"
                || kind == "shell_builtin"
                || kind == "alias"
                || kind == "registered_command",
            "unexpected kind '{kind}' for '{name}'"
        );
    }

    // Verify sorted deterministically by (name, kind)
    let pairs: Vec<(String, String)> = items.iter().map(|item| extract_name_kind(item)).collect();
    assert!(
        pairs.windows(2).all(|w| w[0] <= w[1]),
        "commands not sorted: {pairs:?}"
    );

    // Should include shell builtins
    let has_shell_builtin = items.iter().any(|item| {
        let (name, kind) = extract_name_kind(item);
        name == "cd" && kind == "shell_builtin"
    });
    assert!(has_shell_builtin, "should include shell builtin 'cd'");

    // Should include aliases
    let has_alias = items.iter().any(|item| {
        let (name, kind) = extract_name_kind(item);
        name == "ll" && kind == "alias"
    });
    assert!(has_alias, "should include alias 'll'");

    // Should include registered commands
    let has_registered = items.iter().any(|item| {
        let (name, kind) = extract_name_kind(item);
        name == "mycmd" && kind == "registered_command"
    });
    assert!(has_registered, "should include registered_command 'mycmd'");
}

#[test]
fn commands_rejects_args() {
    let err =
        run_builtin(Builtin::Commands, vec![Value::String("a".into())], 0, ctx()).unwrap_err();
    assert_eq!(err.kind, RuntimeErrorKind::ArityMismatch);
}
