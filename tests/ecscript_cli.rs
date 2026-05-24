use std::io::Write;
use std::process::{Command, Output, Stdio};

fn run_ecscript_stdin(input: &str) -> Output {
    let exe = env!("CARGO_BIN_EXE_ecscript");
    let mut child = Command::new(exe)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn ecscript");

    child
        .stdin
        .as_mut()
        .expect("failed to open ecscript stdin")
        .write_all(input.as_bytes())
        .expect("failed to write ecscript input");

    child
        .wait_with_output()
        .expect("failed to wait for ecscript")
}

#[test]
fn ecscript_runs_script_from_stdin() {
    let output = run_ecscript_stdin(
        r#"let name = "ecs";
println("hello", name);
"#,
    );

    assert!(
        output.status.success(),
        "ecscript exited with {:?}, stderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "hello ecs\n");
}

#[test]
fn ecscript_supports_eval_flag() {
    let exe = env!("CARGO_BIN_EXE_ecscript");
    let output = Command::new(exe)
        .args(["-e", "println(len([1, 2, 3]));"])
        .output()
        .expect("failed to run ecscript -e");

    assert!(
        output.status.success(),
        "ecscript exited with {:?}, stderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "3\n");
}

#[test]
fn ecscript_formats_parse_errors_with_source() {
    let output = run_ecscript_stdin("let x = add(1, 2;\n");

    assert!(
        !output.status.success(),
        "ecscript should fail on parse error"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ecscript parse error at 1:17: expected ',' or ')'"));
    assert!(stderr.contains("1 | let x = add(1, 2;"));
}
