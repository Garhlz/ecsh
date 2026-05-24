use std::io::Write;
use std::process::{Command, Stdio};

fn run_ecsh(input: &str) -> String {
    run_ecsh_with_env(input, &[])
}

fn run_ecsh_with_env(input: &str, envs: &[(&str, &str)]) -> String {
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

    String::from_utf8_lossy(&output.stdout).into_owned()
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
    assert!(output.contains("\n1\n"));

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

    assert!(output.contains("\na\n"));
    assert!(output.contains("\nb\n"));
    assert!(output.contains("still"));
    assert!(!output.contains("\nno\n"));
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

    assert!(output.contains("\n1\n"));
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
    assert!(output.contains("\n/tmp/ecsh-stage6-home\n"));
    assert!(output.contains("\n3\n"));
    assert!(output.contains("\na b c\n"));
    assert!(output.contains("cmdsub"));
    assert!(output.contains("\n1\n"));
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

    assert!(output.contains("\n/tmp\n"));
    assert!(output.contains("\nhi\n"));

    let _ = std::fs::remove_file(out_path);
}
