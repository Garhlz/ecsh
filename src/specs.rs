//! Unified callable/spec metadata for ecscript builtins, shell extensions, and shell builtins.
//!
//! This module provides a single queryable spec model that describes every callable
//! known to the shell: language-level builtin functions, extension registration
//! functions, and native shell builtin commands.
//!
//! The static tables are the authoritative source of metadata (signature, summary,
//! examples, etc.) for each entry. Lookup and execution behaviour is unchanged;
//! this layer only adds queryable documentation-shaped data.

use serde::Serialize;

/// Kinds of callables the shell knows about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SpecKind {
    /// An ecscript language-level builtin function (e.g. `map`, `to_json`).
    Builtin,
    /// A shell-extension registration function (e.g. `hook`, `prompt`, `bind`).
    ShellExtension,
    /// A native shell builtin command (e.g. `cd`, `history`, `source`).
    ShellBuiltin,
}

/// Metadata describing one callable (builtin / extension / shell-builtin).
#[derive(Debug, Clone, Serialize)]
pub struct CallableSpec {
    pub name: &'static str,
    pub kind: SpecKind,
    pub category: &'static str,
    pub signature: &'static str,
    pub summary: &'static str,
    pub details: &'static str,
    pub examples: &'static [&'static str],
}

// ── Static spec tables ───────────────────────────────────────────────

macro_rules! spec {
    ($name:expr, $kind:expr, $category:expr, $sig:expr, $summary:expr, $details:expr $(, $ex:expr)*) => {
        CallableSpec {
            name: $name,
            kind: $kind,
            category: $category,
            signature: $sig,
            summary: $summary,
            details: $details,
            examples: &[$($ex),*],
        }
    };
}

/// All registered callable specs in one static slice.
pub fn all_specs() -> &'static [CallableSpec] {
    &ALL_SPECS
}

/// Find every spec whose name matches `name` (may return multiple across kinds).
pub fn find_specs_by_name(name: &str) -> Vec<&'static CallableSpec> {
    ALL_SPECS.iter().filter(|s| s.name == name).collect()
}

/// Find the exact spec for a specific `(kind, name)` pair.
pub fn find_spec(kind: SpecKind, name: &str) -> Option<&'static CallableSpec> {
    ALL_SPECS.iter().find(|s| s.kind == kind && s.name == name)
}

/// All ecscript language-level builtin specs.
pub fn builtin_specs() -> Vec<&'static CallableSpec> {
    ALL_SPECS
        .iter()
        .filter(|s| s.kind == SpecKind::Builtin)
        .collect()
}

/// All shell-extension specs.
pub fn shell_extension_specs() -> Vec<&'static CallableSpec> {
    ALL_SPECS
        .iter()
        .filter(|s| s.kind == SpecKind::ShellExtension)
        .collect()
}

/// All shell-builtin specs.
pub fn shell_builtin_specs() -> Vec<&'static CallableSpec> {
    ALL_SPECS
        .iter()
        .filter(|s| s.kind == SpecKind::ShellBuiltin)
        .collect()
}

/// Every registered callable name (may contain duplicates across kinds).
pub fn all_names() -> Vec<&'static str> {
    ALL_SPECS.iter().map(|s| s.name).collect()
}

// ── Master table ─────────────────────────────────────────────────────

static ALL_SPECS: &[CallableSpec] = &[
    // ── ecscript language-level builtins ─────────────────────────────
    // Collection operations
    spec!(
        "range",
        SpecKind::Builtin,
        "Collection",
        "range(start, end)",
        "Create an inclusive integer range from start to end.",
        "Returns Array<Int>. If start is greater than end, returns an empty array.",
        "range(1, 4)"
    ),
    spec!(
        "len",
        SpecKind::Builtin,
        "Collection",
        "len(value)",
        "Return the length of an Array, Object, or String.",
        "For Strings, length is counted in Unicode scalar values.",
        "len([1, 2, 3])",
        "len({a: 1})",
        "len(\"ecsh\")"
    ),
    spec!(
        "keys",
        SpecKind::Builtin,
        "Collection",
        "keys(object)",
        "Return an array of the object's keys.",
        "Keys are returned as Strings sorted in ascending order.",
        "keys({a: 1, b: 2})"
    ),
    spec!(
        "values",
        SpecKind::Builtin,
        "Collection",
        "values(object)",
        "Return an array of the object's values.",
        "Values are returned in the same order as the sorted keys().",
        "values({a: 1, b: 2})"
    ),
    spec!(
        "push",
        SpecKind::Builtin,
        "Collection",
        "push(array, value...)",
        "Append one or more values to the end of an array.",
        "Mutates the array in place and returns Nil.",
        "push(arr, 42)",
        "push(arr, 1, 2, 3)"
    ),
    spec!(
        "pop",
        SpecKind::Builtin,
        "Collection",
        "pop(array)",
        "Remove and return the last element of an array.",
        "Returns Nil if the array is empty.",
        "pop(arr)"
    ),
    spec!(
        "insert",
        SpecKind::Builtin,
        "Collection",
        "insert(array, index, value)",
        "Insert a value at the given index, shifting later elements right.",
        "Mutates the array in place and returns Nil. Out-of-bounds indices raise a runtime error.",
        "insert(arr, 0, \"hi\")"
    ),
    spec!(
        "remove",
        SpecKind::Builtin,
        "Collection",
        "remove(array, index)",
        "Remove the element at index and return it.",
        "Shifts later elements left. Out-of-bounds indices raise a runtime error.",
        "remove(arr, 2)"
    ),
    spec!(
        "slice",
        SpecKind::Builtin,
        "Collection",
        "slice(array, start, end)",
        "Return a new array with elements [start, end).",
        "start is inclusive, end is exclusive.",
        "slice(arr, 1, 3)"
    ),
    spec!(
        "map",
        SpecKind::Builtin,
        "Collection",
        "map(array, func)",
        "Apply a function to every element of an array, returning a new array of results.",
        "func receives one argument: the element value.",
        "map([1, 2, 3], (x) => x * 2)"
    ),
    spec!(
        "filter",
        SpecKind::Builtin,
        "Collection",
        "filter(array, func)",
        "Return a new array containing only elements for which func returns truthy.",
        "func receives one argument: the element value, and must return Bool.",
        "filter([1, 2, 3], (x) => x > 1)"
    ),
    spec!(
        "reduce",
        SpecKind::Builtin,
        "Collection",
        "reduce(array, initial, func)",
        "Reduce an array to a single value by repeatedly calling func(acc, element).",
        "initial is the starting accumulator value.",
        "reduce([1, 2, 3], 0, (acc, x) => acc + x)"
    ),
    spec!(
        "each",
        SpecKind::Builtin,
        "Collection",
        "each(array, func)",
        "Call func(element) for every element. Returns Nil.",
        "Use for side effects; the return value of func is ignored.",
        "each([1, 2, 3], (x) => print(x))"
    ),
    spec!(
        "any",
        SpecKind::Builtin,
        "Collection",
        "any(array, func)",
        "Return true if func returns truthy for at least one element.",
        "Short-circuits on first match.",
        "any([1, 2, 3], (x) => x > 2)"
    ),
    spec!(
        "all",
        SpecKind::Builtin,
        "Collection",
        "all(array, func)",
        "Return true if func returns truthy for every element.",
        "Short-circuits on first failure.",
        "all([1, 2, 3], (x) => x > 0)"
    ),
    spec!(
        "find",
        SpecKind::Builtin,
        "Collection",
        "find(array, func)",
        "Return the first element for which func returns truthy, or Nil.",
        "func receives one argument: the element value.",
        "find([1, 2, 3], (x) => x > 1)"
    ),
    spec!(
        "join",
        SpecKind::Builtin,
        "Collection",
        "join(array, separator)",
        "Join array elements into a single string with separator between them.",
        "Each element is converted to its string representation.",
        "join([\"a\", \"b\"], \",\")"
    ),
    spec!(
        "join_path",
        SpecKind::Builtin,
        "Collection",
        "join_path(left, right)",
        "Join two path segments with the OS path separator.",
        "Useful for constructing filesystem paths portably.",
        "join_path(\"/home/user\", \"file.txt\")"
    ),
    // I/O
    spec!(
        "print",
        SpecKind::Builtin,
        "I/O",
        "print(values...)",
        "Print values to stdout without a trailing newline.",
        "Multiple values are printed separated by spaces.",
        "print(\"Hello\", \"World\")"
    ),
    spec!(
        "println",
        SpecKind::Builtin,
        "I/O",
        "println(values...)",
        "Print values to stdout followed by a newline.",
        "Equivalent to print(...) + newline.",
        "println(\"Hello\")"
    ),
    spec!(
        "stdin",
        SpecKind::Builtin,
        "I/O",
        "stdin()",
        "Return the stdin text snapshot provided by the current host context.",
        "In file or piped execution this is the full input text. In interactive REPL it is usually empty.",
        "stdin()"
    ),
    spec!(
        "read_lines",
        SpecKind::Builtin,
        "I/O",
        "read_lines()",
        "Return the current stdin snapshot split into lines.",
        "Each line has its trailing newline stripped.",
        "read_lines()"
    ),
    spec!(
        "write_lines",
        SpecKind::Builtin,
        "I/O",
        "write_lines(lines)",
        "Write an array of strings to stdout, one per line.",
        "Each element is followed by a newline.",
        "write_lines([\"a\", \"b\"])"
    ),
    // Command execution
    spec!(
        "command",
        SpecKind::Builtin,
        "Command",
        "command(program, args...)",
        "Build a command value from a program name and arguments.",
        "Returns a Command value that can be passed to run/capture.",
        "command(\"ls\", \"-l\")"
    ),
    spec!(
        "run",
        SpecKind::Builtin,
        "Command",
        "run(cmd)",
        "Execute a command, forwarding its stdout/stderr directly.",
        "Returns the exit status object.",
        "run(command(\"ls\"))"
    ),
    spec!(
        "capture",
        SpecKind::Builtin,
        "Command",
        "capture(cmd)",
        "Execute a command and capture its result as an object.",
        "Returns { code, signal, stdout, stderr, duration_ms, ok }.",
        "capture(command(\"echo\", \"hi\"))"
    ),
    spec!(
        "text",
        SpecKind::Builtin,
        "Command",
        "text(cmd)",
        "Execute a command and return its output as a single String.",
        "Raises a runtime error if the command exits non-zero or is terminated by signal.",
        "text(command(\"cat\", \"file.txt\"))"
    ),
    spec!(
        "lines",
        SpecKind::Builtin,
        "Command",
        "lines(cmd)",
        "Execute a command and return its output lines as an Array.",
        "Raises a runtime error if the command exits non-zero or is terminated by signal.",
        "lines(command(\"ls\"))"
    ),
    spec!(
        "with_env",
        SpecKind::Builtin,
        "Command",
        "with_env(cmd, env_map)",
        "Return a new command with additional environment variables.",
        "env_map is an Object of String→String pairs.",
        "with_env(command(\"printenv\"), {FOO: \"bar\"})"
    ),
    spec!(
        "with_cwd",
        SpecKind::Builtin,
        "Command",
        "with_cwd(cmd, dir)",
        "Return a new command that runs in the given working directory.",
        "Does not change the shell's current directory.",
        "with_cwd(command(\"ls\"), \"/tmp\")"
    ),
    // Environment
    spec!(
        "env",
        SpecKind::Builtin,
        "Environment",
        "env(name)",
        "Get the value of an environment variable, or Nil if unset.",
        "Reads from the process environment.",
        "env(\"HOME\")"
    ),
    spec!(
        "set_env",
        SpecKind::Builtin,
        "Environment",
        "set_env(name, value)",
        "Set an environment variable in the current process.",
        "Both name and value must be Strings.",
        "set_env(\"FOO\", \"bar\")"
    ),
    spec!(
        "unset_env",
        SpecKind::Builtin,
        "Environment",
        "unset_env(name)",
        "Remove an environment variable from the current process.",
        "No error if the variable was not set.",
        "unset_env(\"FOO\")"
    ),
    spec!(
        "cwd",
        SpecKind::Builtin,
        "Environment",
        "cwd()",
        "Return the current working directory as a String.",
        "Equivalent to getcwd(3).",
        "cwd()"
    ),
    // JSON
    spec!(
        "to_json",
        SpecKind::Builtin,
        "JSON",
        "to_json(value)",
        "Serialize an ecscript value to a JSON string.",
        "Supports Nil, Bool, Int, Float, String, Array, and Object.",
        "to_json({a: 1})"
    ),
    spec!(
        "from_json",
        SpecKind::Builtin,
        "JSON",
        "from_json(string)",
        "Parse a JSON string into an ecscript value.",
        "Returns the parsed value or raises an error on invalid JSON.",
        "from_json(\"[1,2,3]\")"
    ),
    // String
    spec!(
        "trim",
        SpecKind::Builtin,
        "String",
        "trim(string)",
        "Return the string with leading and trailing whitespace removed.",
        "Whitespace is defined by Unicode rules.",
        "trim(\"  hello  \")"
    ),
    // Introspection
    spec!(
        "help",
        SpecKind::Builtin,
        "Introspection",
        "help([name])",
        "With no arguments, return an overview of all callables grouped by kind and category. With a name argument, return detailed help for the matching callable.",
        "If the name appears in multiple kinds, all entries are shown. If no match is found, an error is returned.",
        "help()",
        "help(\"map\")"
    ),
    spec!(
        "builtins",
        SpecKind::Builtin,
        "Introspection",
        "builtins()",
        "Return an Array of all ecscript builtin function names, sorted alphabetically.",
        "Names are drawn from the metadata specs. The list is deduplicated.",
        "builtins()"
    ),
    spec!(
        "extensions",
        SpecKind::Builtin,
        "Introspection",
        "extensions()",
        "Return an Array of all shell extension function names, sorted alphabetically.",
        "Names are drawn from the metadata specs. The list is deduplicated.",
        "extensions()"
    ),
    spec!(
        "commands",
        SpecKind::Builtin,
        "Introspection",
        "commands()",
        "Return an Array of Objects describing all visible command-like names.",
        "Each object has `name` (String) and `kind` (String) fields. Requires interactive shell context. Sources include ecscript builtins, shell builtins, aliases, and registered script commands.",
        "commands()"
    ),
    // ── Shell extension registration ─────────────────────────────────
    spec!(
        "hook",
        SpecKind::ShellExtension,
        "Extension",
        "hook(name, func)",
        "Register a function to be called when a shell hook event fires.",
        "Valid hook names: before_prompt, after_cd, preexec, postexec.",
        "hook(\"after_cd\", (ctx) => { println(ctx.cwd); })"
    ),
    spec!(
        "prompt",
        SpecKind::ShellExtension,
        "Extension",
        "prompt(func)",
        "Register a function that returns the shell prompt string.",
        "func receives a context object with shell metadata and must return a String.",
        "prompt((ctx) => ctx.cwd + \" $ \")"
    ),
    spec!(
        "complete",
        SpecKind::ShellExtension,
        "Extension",
        "complete(command_name, func)",
        "Register a tab-completion handler for a command.",
        "func receives a context object and returns Array<Object> completion candidates.",
        "complete(\"git\", (ctx) => [{ value: \"status\" }])"
    ),
    spec!(
        "bind",
        SpecKind::ShellExtension,
        "Extension",
        "bind(key_sequence, func)",
        "Bind a key sequence to a function that is called when the key is pressed.",
        "func receives a context object and may return an action object such as { action: \"set_line\", text: \"...\" }.",
        "bind(\"ctrl-r\", (ctx) => { return nil; })"
    ),
    spec!(
        "register_command",
        SpecKind::ShellExtension,
        "Extension",
        "register_command(name, func)",
        "Register an ecscript function as a shell command.",
        "func receives a context object and may return Nil or a non-negative Int exit code.",
        "register_command(\"greet\", (ctx) => { println(\"hi\"); })"
    ),
    spec!(
        "set_cwd",
        SpecKind::ShellExtension,
        "Extension",
        "set_cwd(path)",
        "Change the shell's current working directory, triggering after_cd hooks.",
        "Prefer this over raw chdir so hooks run.",
        "set_cwd(\"/tmp\")"
    ),
    // ── Shell builtin commands ───────────────────────────────────────
    spec!(
        "cd",
        SpecKind::ShellBuiltin,
        "Navigation",
        "cd [dir]",
        "Change the current working directory.",
        "If dir is omitted, changes to $HOME. Updates PWD and OLDPWD.",
        "cd /tmp",
        "cd"
    ),
    spec!(
        "pwd",
        SpecKind::ShellBuiltin,
        "Navigation",
        "pwd",
        "Print the current working directory.",
        "Outputs the absolute path of the current directory.",
        "pwd"
    ),
    spec!(
        "history",
        SpecKind::ShellBuiltin,
        "History",
        "history",
        "Display the command history with line numbers.",
        "Each entry is prefixed with its index for use with !-expansion.",
        "history"
    ),
    spec!(
        "type",
        SpecKind::ShellBuiltin,
        "Introspection",
        "type name",
        "Show how a command name would be interpreted by the shell.",
        "Reports whether the name is an alias, builtin, script command, or external binary.",
        "type ls",
        "type cd"
    ),
    spec!(
        "which",
        SpecKind::ShellBuiltin,
        "Introspection",
        "which name",
        "Print the resolved shell interpretation or external command path for a name.",
        "Reports aliases, shell builtins, ecscript shell commands, or PATH hits.",
        "which ls",
        "which cd"
    ),
    spec!(
        "source",
        SpecKind::ShellBuiltin,
        "Scripting",
        "source file",
        "Read and execute ecscript commands from a file in the current shell context.",
        "Also available as '.'. Changes to environment and aliases persist.",
        "source ~/.ecshrc"
    ),
    spec!(
        "reload_rc",
        SpecKind::ShellBuiltin,
        "Config",
        "reload_rc",
        "Re-execute the startup rc file (~/.ecshrc) in a fresh runtime.",
        "Script environment, aliases, traps, extensions, and modules are replaced on success; existing state is kept on failure.",
        "reload_rc"
    ),
];

// ── Unit tests ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    // ── Coverage: required names are present ─────────────────────────

    #[test]
    fn all_ecscript_builtin_names_present() {
        let names: HashSet<&str> = builtin_specs().iter().map(|s| s.name).collect();
        let expected = [
            "command",
            "env",
            "set_env",
            "unset_env",
            "cwd",
            "stdin",
            "read_lines",
            "range",
            "len",
            "to_json",
            "from_json",
            "keys",
            "values",
            "push",
            "pop",
            "insert",
            "remove",
            "slice",
            "print",
            "println",
            "run",
            "capture",
            "text",
            "lines",
            "with_env",
            "with_cwd",
            "map",
            "filter",
            "reduce",
            "each",
            "any",
            "all",
            "find",
            "join",
            "join_path",
            "write_lines",
            "help",
            "builtins",
            "extensions",
            "commands",
            "trim",
        ];
        for name in expected {
            assert!(
                names.contains(name),
                "missing ecscript builtin spec: {name}"
            );
        }
        assert_eq!(
            names.len(),
            expected.len(),
            "unexpected extra ecscript builtin specs"
        );
    }

    #[test]
    fn all_shell_extension_names_present() {
        let names: HashSet<&str> = shell_extension_specs().iter().map(|s| s.name).collect();
        let expected = [
            "hook",
            "prompt",
            "complete",
            "bind",
            "register_command",
            "set_cwd",
        ];
        for name in expected {
            assert!(names.contains(name), "missing shell extension spec: {name}");
        }
        assert_eq!(
            names.len(),
            expected.len(),
            "unexpected extra shell extension specs"
        );
    }

    #[test]
    fn all_shell_builtin_names_present() {
        let names: HashSet<&str> = shell_builtin_specs().iter().map(|s| s.name).collect();
        let expected = [
            "cd",
            "pwd",
            "history",
            "type",
            "which",
            "source",
            "reload_rc",
        ];
        for name in expected {
            assert!(names.contains(name), "missing shell builtin spec: {name}");
        }
        assert_eq!(
            names.len(),
            expected.len(),
            "unexpected extra shell builtin specs"
        );
    }

    // ── Kind partitioning ────────────────────────────────────────────

    #[test]
    fn kind_partitioning_is_mutually_exclusive() {
        let builtins: HashSet<&str> = builtin_specs().iter().map(|s| s.name).collect();
        let extensions: HashSet<&str> = shell_extension_specs().iter().map(|s| s.name).collect();
        let shell_builtins: HashSet<&str> = shell_builtin_specs().iter().map(|s| s.name).collect();

        // No name appears in more than one kind.
        for name in &builtins {
            assert!(
                !extensions.contains(name),
                "{name} in both Builtin and ShellExtension"
            );
            assert!(
                !shell_builtins.contains(name),
                "{name} in both Builtin and ShellBuiltin"
            );
        }
        for name in &extensions {
            assert!(
                !shell_builtins.contains(name),
                "{name} in both ShellExtension and ShellBuiltin"
            );
        }
    }

    // ── No duplicate (kind, name) pairs ──────────────────────────────

    #[test]
    fn no_duplicate_kind_name_pairs() {
        let mut seen = HashSet::new();
        for spec in all_specs() {
            let key = (spec.kind, spec.name);
            assert!(
                seen.insert(key),
                "duplicate spec: ({spec:?}, {name})",
                spec = spec.kind,
                name = spec.name
            );
        }
    }

    // ── find_specs_by_name works across kinds ────────────────────────

    #[test]
    fn find_specs_by_name_exact_match() {
        let specs = find_specs_by_name("map");
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].kind, SpecKind::Builtin);
        assert_eq!(specs[0].name, "map");
    }

    #[test]
    fn find_specs_by_name_no_match() {
        assert!(find_specs_by_name("nonexistent").is_empty());
    }

    // ── Representative field checks ──────────────────────────────────

    #[test]
    fn representative_ecscript_builtin_spec() {
        let specs = find_specs_by_name("map");
        let spec = specs.first().expect("map spec exists");
        assert_eq!(spec.kind, SpecKind::Builtin);
        assert_eq!(spec.category, "Collection");
        assert!(!spec.signature.is_empty());
        assert!(!spec.summary.is_empty());
        assert!(!spec.details.is_empty());
        assert!(!spec.examples.is_empty());
    }

    #[test]
    fn representative_shell_extension_spec() {
        let specs = find_specs_by_name("hook");
        let spec = specs.first().expect("hook spec exists");
        assert_eq!(spec.kind, SpecKind::ShellExtension);
        assert_eq!(spec.category, "Extension");
        assert!(!spec.signature.is_empty());
        assert!(!spec.summary.is_empty());
        assert!(!spec.details.is_empty());
        assert!(!spec.examples.is_empty());
    }

    #[test]
    fn representative_shell_builtin_spec() {
        let specs = find_specs_by_name("cd");
        let spec = specs.first().expect("cd spec exists");
        assert_eq!(spec.kind, SpecKind::ShellBuiltin);
        assert_eq!(spec.category, "Navigation");
        assert!(!spec.signature.is_empty());
        assert!(!spec.summary.is_empty());
        assert!(!spec.details.is_empty());
    }

    // ── all_names / all_specs consistency ────────────────────────────

    #[test]
    fn all_names_matches_all_specs_count() {
        assert_eq!(all_names().len(), all_specs().len());
    }

    #[test]
    fn total_spec_count() {
        // 41 ecscript builtins + 6 shell extensions + 7 shell builtins
        assert_eq!(all_specs().len(), 54);
    }

    #[test]
    fn examples_use_ecscript_double_quoted_strings() {
        for spec in all_specs() {
            for example in spec.examples {
                assert!(
                    !example.contains('\''),
                    "example for {} contains single quote syntax: {}",
                    spec.name,
                    example
                );
            }
        }
    }
}
