use std::collections::BTreeSet;

use crate::ecscript::{
    error::{RuntimeError, RuntimeErrorKind},
    value::{BuiltinContext, Value},
};
use crate::specs::{self, CallableSpec, SpecKind};

use super::support::{expect_arity, expect_shell_state};

/// Format a single CallableSpec into a help string.
fn format_spec(spec: &CallableSpec) -> String {
    let mut buf = String::new();
    buf.push_str(&format!("== {} ({})\n", spec.name, kind_label(spec.kind)));
    buf.push_str(&format!("  Signature: {}\n", spec.signature));
    buf.push_str(&format!("  Summary:   {}\n", spec.summary));
    buf.push_str(&format!("  Details:   {}\n", spec.details));
    if !spec.examples.is_empty() {
        buf.push_str("  Examples:\n");
        for ex in spec.examples {
            buf.push_str(&format!("    ecsh> {}\n", ex));
        }
    }
    buf
}

fn kind_label(kind: SpecKind) -> &'static str {
    match kind {
        SpecKind::Builtin => "ecscript builtin",
        SpecKind::ShellExtension => "shell extension",
        SpecKind::ShellBuiltin => "shell builtin",
    }
}

/// `help()` — return an overview of all callables grouped by kind/category.
fn help_overview() -> String {
    let mut buf = String::from("ecsh Help Overview\n");
    buf.push_str("==================\n\n");

    // Group by kind
    for kind in &[
        SpecKind::Builtin,
        SpecKind::ShellExtension,
        SpecKind::ShellBuiltin,
    ] {
        let kind_name = match kind {
            SpecKind::Builtin => "Ecscript Builtins",
            SpecKind::ShellExtension => "Shell Extensions",
            SpecKind::ShellBuiltin => "Shell Builtins",
        };
        buf.push_str(&format!("== {}\n", kind_name));

        let mut entries: Vec<&CallableSpec> = specs::all_specs()
            .iter()
            .filter(|s| s.kind == *kind)
            .collect();
        entries.sort_by_key(|s| s.name);

        // Group by category within kind
        let mut categories: BTreeSet<&str> = BTreeSet::new();
        for e in &entries {
            categories.insert(e.category);
        }
        for cat in &categories {
            buf.push_str(&format!("  [{}]\n", cat));
            for e in &entries {
                if e.category == *cat {
                    buf.push_str(&format!("    {} — {}\n", e.name, e.summary));
                }
            }
        }
        buf.push('\n');
    }

    buf.push_str("Use help(\"name\") for detailed information on a specific callable.\n");
    buf
}

/// `help(name)` — return detailed help for matching spec entries.
fn help_for_name(name: &str, span: usize) -> Result<String, RuntimeError> {
    let matches = specs::find_specs_by_name(name);

    if matches.is_empty() {
        return Err(RuntimeError::new(
            span,
            RuntimeErrorKind::UndefinedVariable,
            format!("help: no callable named '{}'", name),
        ));
    }

    if matches.len() == 1 {
        Ok(format_spec(matches[0]))
    } else {
        let mut buf = format!("help: '{}' appears in multiple kinds:\n\n", name);
        for spec in &matches {
            buf.push_str(&format_spec(spec));
            buf.push('\n');
        }
        Ok(buf)
    }
}

pub(super) fn help_builtin(
    args: &[Value],
    span: usize,
    _ctx: &BuiltinContext<'_>,
) -> Result<Value, RuntimeError> {
    // arity 0 or 1
    if args.len() > 1 {
        return Err(RuntimeError::new(
            span,
            RuntimeErrorKind::ArityMismatch,
            format!("help expects 0 or 1 arguments, got {}", args.len()),
        ));
    }

    if args.is_empty() {
        Ok(Value::String(help_overview()))
    } else {
        let Value::String(name) = &args[0] else {
            return Err(RuntimeError::new(
                span,
                RuntimeErrorKind::TypeMismatch,
                format!("help expects String argument, got {}", args[0].type_name()),
            ));
        };
        let text = help_for_name(name, span)?;
        Ok(Value::String(text))
    }
}

pub(super) fn builtins_builtin(
    args: &[Value],
    span: usize,
    _ctx: &BuiltinContext<'_>,
) -> Result<Value, RuntimeError> {
    expect_arity(args, 0, span, "builtins")?;

    let mut names: Vec<String> = specs::all_specs()
        .iter()
        .filter(|s| s.kind == SpecKind::Builtin)
        .map(|s| s.name.to_string())
        .collect();

    // Sort ascending and deduplicate
    names.sort();
    names.dedup();

    Ok(Value::Array(std::rc::Rc::new(std::cell::RefCell::new(
        names.into_iter().map(Value::String).collect(),
    ))))
}

pub(super) fn extensions_builtin(
    args: &[Value],
    span: usize,
    _ctx: &BuiltinContext<'_>,
) -> Result<Value, RuntimeError> {
    expect_arity(args, 0, span, "extensions")?;

    let mut names: Vec<String> = specs::all_specs()
        .iter()
        .filter(|s| s.kind == SpecKind::ShellExtension)
        .map(|s| s.name.to_string())
        .collect();

    names.sort();
    names.dedup();

    Ok(Value::Array(std::rc::Rc::new(std::cell::RefCell::new(
        names.into_iter().map(Value::String).collect(),
    ))))
}

pub(super) fn commands_builtin(
    args: &[Value],
    span: usize,
    ctx: &BuiltinContext<'_>,
) -> Result<Value, RuntimeError> {
    expect_arity(args, 0, span, "commands")?;
    let state = expect_shell_state(ctx.shell_state, span, "commands")?;

    let mut entries: Vec<(String, String)> = Vec::new();

    // 1. ecscript builtins
    for spec in specs::all_specs() {
        if spec.kind == SpecKind::Builtin {
            entries.push((spec.name.to_string(), "builtin".to_string()));
        }
    }

    // 2. shell builtins from the same specs table that help() queries,
    //    so commands() and help() always agree on what is documented.
    for spec in specs::shell_builtin_specs() {
        entries.push((spec.name.to_string(), "shell_builtin".to_string()));
    }
    // Also include shell builtins that exist at runtime but lack a spec
    // entry yet — mark them so consumers know help() may not document them.
    for name in crate::builtin::BUILTIN_NAMES {
        if !entries.iter().any(|(n, k)| n == *name && k == "shell_builtin") {
            entries.push(((*name).to_string(), "shell_builtin".to_string()));
        }
    }

    // 3. aliases
    for name in state.aliases.keys() {
        entries.push((name.clone(), "alias".to_string()));
    }

    // 4. registered script commands
    for name in state.extensions.borrow().script_commands.keys() {
        entries.push((name.clone(), "registered_command".to_string()));
    }

    // Sort deterministically by (name, kind)
    entries.sort();

    let arr: Vec<Value> = entries
        .into_iter()
        .map(|(name, kind)| {
            use std::collections::HashMap;
            let obj = HashMap::from([
                ("name".to_string(), Value::String(name)),
                ("kind".to_string(), Value::String(kind)),
            ]);
            Value::Object(std::rc::Rc::new(std::cell::RefCell::new(obj)))
        })
        .collect();

    Ok(Value::Array(std::rc::Rc::new(std::cell::RefCell::new(arr))))
}
