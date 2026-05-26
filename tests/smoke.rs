use std::io::Write;
use std::process::{Command, Output, Stdio};

fn run_ecsh(input: &str) -> String {
    let output = run_ecsh_output_with_env(input, &[]);
    visible_output(&String::from_utf8_lossy(&output.stdout))
}

fn run_ecsh_with_env(input: &str, envs: &[(&str, &str)]) -> String {
    let output = run_ecsh_output_with_env(input, envs);
    visible_output(&String::from_utf8_lossy(&output.stdout))
}

fn run_ecsh_output(input: &str) -> Output {
    run_ecsh_output_with_env(input, &[])
}

fn run_ecsh_output_with_env(input: &str, envs: &[(&str, &str)]) -> Output {
    let exe = env!("CARGO_BIN_EXE_ecsh");
    let mut command = Command::new(exe);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    for (key, value) in envs {
        command.env(key, value);
    }

    let mut child = command.spawn().expect("failed to spawn ecsh");

    child
        .stdin
        .as_mut()
        .expect("failed to open ecsh stdin")
        .write_all(input.as_bytes())
        .expect("failed to write ecsh input");

    let output = child.wait_with_output().expect("failed to wait for ecsh");
    assert!(
        output.status.success(),
        "ecsh exited with {:?}, stderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn run_ecsh_file_output(path: &std::path::Path) -> Output {
    let exe = env!("CARGO_BIN_EXE_ecsh");
    Command::new(exe)
        .arg(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("failed to run ecsh script file")
}

fn temp_path(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("ecsh-{}-{}", std::process::id(), name))
}

fn visible_output(raw: &str) -> String {
    raw.lines()
        .filter_map(|line| {
            if line.starts_with("[ecsh] ") {
                None
            } else {
                let mut rest = line;
                loop {
                    if let Some(stripped) = rest.strip_prefix("$ ") {
                        rest = stripped;
                        continue;
                    }
                    if let Some(stripped) = rest.strip_prefix("... ") {
                        rest = stripped;
                        continue;
                    }
                    break;
                }
                Some(rest)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn smoke_executes_conditionals_and_quotes() {
    let output = run_ecsh(
        r#"echo "hello world"
true && echo ok
false && echo no
false || echo fallback
exit
"#,
    );

    assert!(output.contains("hello world"));
    assert!(output.contains("ok"));
    assert!(output.contains("fallback"));
    assert!(!output.contains("\nno\n"));
}

#[test]
fn smoke_handles_redirection_and_status() {
    let output = run_ecsh(
        r#"echo hi>/tmp/ecsh_smoke_out
cat</tmp/ecsh_smoke_out
false
status
exit
"#,
    );

    assert!(output.contains("hi"));
    assert!(output.lines().any(|line| line == "1"));

    let _ = std::fs::remove_file("/tmp/ecsh_smoke_out");
}

#[test]
fn smoke_executes_sequence_operator() {
    let output = run_ecsh(
        r#"echo a; echo b
false; echo still
exit; echo no
"#,
    );

    assert!(output.lines().any(|line| line == "a"));
    assert!(output.lines().any(|line| line == "b"));
    assert!(output.contains("still"));
    assert!(!output.lines().any(|line| line == "no"));
}

#[test]
fn smoke_launches_background_jobs_and_lists_them() {
    let output = run_ecsh(
        r#"sleep 1 &
jobs
exit
"#,
    );

    assert!(output.contains("[1]"));
    assert!(output.contains("Running"));
    assert!(output.contains("sleep 1 &"));
}

#[test]
fn smoke_rejects_background_builtin_commands() {
    let output = run_ecsh(
        r#"jobs &
status
exit
"#,
    );

    assert!(output.lines().any(|line| line == "1"));
}

#[test]
fn smoke_expands_runtime_shell_words() {
    let output = run_ecsh_with_env(
        r#"${...["echo", "from-head"]}
echo prefix-$HOME
echo ${env("HOME")}
echo ${1 + 2}
echo ${...["a", "b", "c"]}
echo $(printf cmdsub)
false
${"status"}
exit
"#,
        &[("HOME", "/tmp/ecsh-stage6-home")],
    );

    assert!(output.contains("from-head"));
    assert!(output.contains("prefix-/tmp/ecsh-stage6-home"));
    assert!(output.lines().any(|line| line == "/tmp/ecsh-stage6-home"));
    assert!(output.lines().any(|line| line == "3"));
    assert!(output.lines().any(|line| line == "a b c"));
    assert!(output.contains("cmdsub"));
    assert!(output.lines().any(|line| line == "1"));
}

#[test]
fn smoke_expands_builtin_args_and_redirection_targets() {
    let out_path = format!(
        "{}/ecsh-stage6-redirection-out",
        std::env::temp_dir().display()
    );
    let output = run_ecsh_with_env(
        r#"cd ${"/tmp"}
pwd
echo hi > $OUT_PATH
cat < ${env("OUT_PATH")}
exit
"#,
        &[("OUT_PATH", &out_path)],
    );

    assert!(output.lines().any(|line| line == "/tmp"));
    assert!(output.lines().any(|line| line == "hi"));

    let _ = std::fs::remove_file(out_path);
}

#[test]
fn smoke_continues_multiline_double_quoted_input() {
    let output = run_ecsh(
        "echo \"hello\nworld\"
exit
",
    );

    assert!(output.contains("hello\nworld"));
}

#[test]
fn smoke_continues_multiline_expr_expansion() {
    let output = run_ecsh(
        "echo ${1 +\n2}
exit
",
    );

    assert!(output.lines().any(|line| line == "3"));
}

#[test]
fn smoke_rejects_removed_dollar_bracket_syntax() {
    let output = run_ecsh_output(
        "echo $[1 + 2]
exit
",
    );
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(stderr.contains("$[expr] has been removed; use ${expr}"));
}

#[test]
fn smoke_reports_ecscript_parse_error_inside_script_block() {
    let output = run_ecsh_output(
        "for i in 0..3 {\necho $i\n}
exit
",
    );
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(stderr.contains("ecscript parse error"));
    assert!(stderr.contains("unexpected character '$'"));
    assert!(!stderr.contains("execvp failed"));
}

#[test]
fn smoke_reports_incomplete_input_at_eof() {
    let output = run_ecsh_output("echo \"unterminated");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(stderr.contains("shell parse error at 1:19"));
    assert!(stderr.contains("unterminated double quote"));
    assert!(stderr.contains("echo \"unterminated"));
}

#[test]
fn smoke_continues_multiline_command_substitution() {
    let output = run_ecsh(
        "echo $(printf hel\nprintf lo)
exit
",
    );

    assert!(output.contains("hello"));
}

#[test]
fn smoke_supports_alias_and_unalias() {
    let output = run_ecsh(
        "alias ll='echo alias-ok'\nll\nunalias ll\nstatus
exit
",
    );

    assert!(output.contains("alias-ok"));
    assert!(output.lines().any(|line| line == "0"));
}

#[test]
fn smoke_runs_exit_trap() {
    let output = run_ecsh(
        "trap 'echo bye-from-trap' EXIT
exit
",
    );

    assert!(output.contains("bye-from-trap"));
}

#[test]
fn smoke_print_without_newline_still_separates_next_prompt() {
    let output = run_ecsh_output(
        "print(1)
exit
",
    );
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(stdout.contains("1\n[ecsh]"), "stdout was:\n{stdout}");
}

#[test]
fn smoke_runs_ecscript_file_via_ecsh() {
    let path = temp_path("stage7-script.ecs");
    std::fs::write(&path, "let xs = range(1, 3)\nprintln(xs)\n").expect("failed to write script");

    let output = run_ecsh_file_output(&path);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(output.status.success(), "stderr: {stderr}");
    assert!(stdout.contains("[1, 2, 3]"), "stdout was:\n{stdout}");

    let _ = std::fs::remove_file(path);
}

#[test]
fn smoke_formats_ecscript_file_errors_via_ecsh() {
    let path = temp_path("stage7-script-error.ecs");
    std::fs::write(&path, "let x = add(1, 2;\n").expect("failed to write script");

    let output = run_ecsh_file_output(&path);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(output.status.success(), "stderr: {stderr}");
    assert!(stderr.contains("ecscript parse error at 1:17"));
    assert!(stderr.contains("let x = add(1, 2;"));

    let _ = std::fs::remove_file(path);
}

#[test]
fn smoke_source_runs_ecscript_file_in_current_session() {
    let path = temp_path("stage7-source.ecs");
    std::fs::write(&path, "let x = 40\nx += 2\n").expect("failed to write source script");

    let output = run_ecsh(&format!(
        "source {}\nprintln(x)
exit
",
        path.display()
    ));
    assert!(
        output.lines().any(|line| line == "42"),
        "output was:\n{output}"
    );

    let _ = std::fs::remove_file(path);
}

#[test]
fn smoke_dot_runs_ecscript_file_in_current_session() {
    let path = temp_path("stage7-dot.ecs");
    std::fs::write(&path, "let msg = \"dot-ok\"\n").expect("failed to write dot script");

    let output = run_ecsh(&format!(
        ". {}\nprintln(msg)
exit
",
        path.display()
    ));
    assert!(
        output.lines().any(|line| line == "dot-ok"),
        "output was:\n{output}"
    );

    let _ = std::fs::remove_file(path);
}

#[test]
fn smoke_supports_type_which_and_history() {
    let output = run_ecsh(
        "alias ll='echo alias-ok'\n\
         type ll help sh\n\
         which ll help sh\n\
         history\n\
         exit\n",
    );

    assert!(output.contains("ll is aliased to `echo alias-ok`"));
    assert!(output.contains("help is a shell builtin"));
    assert!(output.contains("alias ll='echo alias-ok'"));
    assert!(output.contains("help: shell builtin"));
    assert!(output.contains("type ll help sh"));
}

#[test]
#[test]
fn smoke_ecscript_range_builtin() {
    let output = run_ecsh_output(
        "let nums = range(0, 3); println(nums[0], nums[1], nums[2], nums[3]); exit;",
    );
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("0 1 2 3"));
}

#[test]
fn smoke_ecscript_env_builtin() {
    let output = run_ecsh_output("let x = env(\"PATH\"); println(len(x) > 0); exit;");
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("true"));
}

#[test]
fn smoke_ecscript_nil_for_missing_env() {
    let result = run_ecsh("let x = env(\"ECSH_NOEXIST_VAR\"); println(x == nil); exit\n");
    assert!(result.contains("true"));
}
