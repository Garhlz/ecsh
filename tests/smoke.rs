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
        r#"$[...["echo", "from-head"]]
echo prefix-$HOME
echo ${HOME}
echo $[1 + 2]
echo $[...["a", "b", "c"]]
echo $(printf cmdsub)
false
$["status"]
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
        r#"cd $["/tmp"]
pwd
echo hi > $OUT_PATH
cat < ${OUT_PATH}
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
    let output = run_ecsh("echo \"hello\nworld\"\nexit\n");

    assert!(output.contains("hello\nworld"));
}

#[test]
fn smoke_continues_multiline_expr_expansion() {
    let output = run_ecsh("echo $[1 +\n2]\nexit\n");

    assert!(output.lines().any(|line| line == "3"));
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
    let output = run_ecsh("echo $(printf hel\nprintf lo)\nexit\n");

    assert!(output.contains("hello"));
}

#[test]
fn smoke_supports_alias_and_unalias() {
    let output = run_ecsh("alias ll='echo alias-ok'\nll\nunalias ll\nstatus\nexit\n");

    assert!(output.contains("alias-ok"));
    assert!(output.lines().any(|line| line == "0"));
}

#[test]
fn smoke_runs_exit_trap() {
    let output = run_ecsh("trap 'echo bye-from-trap' EXIT\nexit\n");

    assert!(output.contains("bye-from-trap"));
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
