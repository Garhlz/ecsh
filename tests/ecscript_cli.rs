use std::io::Write;
use std::path::{Path, PathBuf};
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

fn example_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("ecscript")
        .join(name)
}

fn run_ecscript_file(path: &Path) -> Output {
    let exe = env!("CARGO_BIN_EXE_ecscript");
    Command::new(exe)
        .arg(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("failed to run ecscript file")
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

#[test]
fn ecscript_runs_success_examples() {
    let cases = [
        ("loop_and_accumulate.ecs", &["sum", "15", "squares", "[1,4,9,16,25]", "countdown", "[15,12,9]"][..]),
        ("closures_and_state.ecs", &["snapshots", "[11,13,18]", "worker-results", "[0,1,4]"][..]),
        (
            "objects_and_collections.ecs",
            &[
                "name",
                "elaine",
                "user",
                "{\"name\":\"elaine\",\"stats\":{\"commits\":7,\"reviews\":2},\"tags\":[\"rust\",\"shell\",\"ecs\"]}",
                "stat-keys",
                "[\"commits\",\"reviews\"]",
                "stat-values",
                "[7,2]",
            ][..],
        ),
        ("env_and_json.ecs", &["HOME", "payload", "patched-range", "[3,99,5,6]"][..]),
        (
            "std_iter_draft.ecs",
            &[
                "3",
                "15",
                "120",
                "1",
                "5",
                "true",
                "[1, 2, 3]",
                "[3, 4, 5]",
                "[5, 4, 3, 2, 1]",
            ][..],
        ),
    ];

    for (name, expected_lines) in cases {
        let output = run_ecscript_file(&example_path(name));
        assert!(
            output.status.success(),
            "{name} failed, stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        for expected in expected_lines {
            assert!(
                stdout.lines().any(|line| line == *expected),
                "{name} missing line `{expected}`\nstdout:\n{stdout}"
            );
        }
    }
}

#[test]
fn ecscript_reports_boundary_example_failures() {
    let cases = [
        (
            "boundary_shell_in_block.ecs",
            "unexpected character '$'",
        ),
        (
            "boundary_range_value.ecs",
            "range expressions are only valid in for loops; use range(start, end)",
        ),
    ];

    for (name, expected) in cases {
        let output = run_ecscript_file(&example_path(name));
        assert!(!output.status.success(), "{name} unexpectedly succeeded");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(expected),
            "{name} missing error `{expected}`\nstderr:\n{stderr}"
        );
    }
}
