use std::io::Write;
use std::process::{Command, Stdio};

fn run_ecsh(input: &str) -> String {
    let exe = env!("CARGO_BIN_EXE_ecsh");
    let mut child = Command::new(exe)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn ecsh");

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
