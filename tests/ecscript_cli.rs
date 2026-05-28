use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

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

fn run_ecscript_file_with_stdin(path: &Path, input: &str) -> Output {
    let exe = env!("CARGO_BIN_EXE_ecscript");
    let mut child = Command::new(exe)
        .arg(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn ecscript file with stdin");

    child
        .stdin
        .as_mut()
        .expect("failed to open ecscript file stdin")
        .write_all(input.as_bytes())
        .expect("failed to write ecscript file stdin");

    child
        .wait_with_output()
        .expect("failed to wait for ecscript file with stdin")
}

fn temp_script_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("ecsh-{name}-{nanos}.ecs"))
}

fn temp_script_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("ecsh-{name}-{nanos}"));
    std::fs::create_dir_all(&path).expect("failed to create temp script dir");
    path
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
        (
            "loop_and_accumulate.ecs",
            &[
                "sum",
                "15",
                "squares",
                "[1,4,9,16,25]",
                "countdown",
                "[15,12,9]",
            ][..],
        ),
        (
            "closures_and_state.ecs",
            &["snapshots", "[11,13,18]", "worker-results", "[0,1,4]"][..],
        ),
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
        (
            "env_and_json.ecs",
            &["HOME", "payload", "patched-range", "[3,99,5,6]"][..],
        ),
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
        ("boundary_shell_in_block.ecs", "unexpected character '$'"),
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

#[test]
fn ecscript_file_mode_supports_text_value_bridges() {
    let path = temp_script_path("stdin-bridge");
    std::fs::write(
        &path,
        r#"println(stdin());
println(read_lines());
write_lines(read_lines());
"#,
    )
    .expect("failed to write temp script");

    let output = run_ecscript_file_with_stdin(&path, "a\nb\n");
    let _ = std::fs::remove_file(&path);

    assert!(
        output.status.success(),
        "stdin bridge script failed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "a\nb\n\n[\"a\", \"b\"]\na\nb\n"
    );
}

#[test]
fn ecscript_file_mode_reuses_cached_module_objects() {
    let dir = temp_script_dir("module-cache-cli");
    let main_path = dir.join("main.ecs");
    let foo_path = dir.join("foo.ecs");

    std::fs::write(&foo_path, "pub let xs = []\n").expect("failed to write foo module");
    std::fs::write(
        &main_path,
        "use ./foo.ecs as a\nuse ./foo.ecs as b\npush(a.xs, 1)\nprintln(len(b.xs))\n",
    )
    .expect("failed to write main module script");

    let output = run_ecscript_file(&main_path);
    let _ = std::fs::remove_file(&main_path);
    let _ = std::fs::remove_file(&foo_path);
    let _ = std::fs::remove_dir(&dir);

    assert!(
        output.status.success(),
        "ecscript module cache script failed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "1\n");
}

#[test]
fn ecscript_file_mode_reports_circular_module_imports() {
    let dir = temp_script_dir("module-cycle-cli");
    let main_path = dir.join("main.ecs");
    let a_path = dir.join("a.ecs");
    let b_path = dir.join("b.ecs");

    std::fs::write(&main_path, "use ./a.ecs as a\n").expect("failed to write main");
    std::fs::write(&a_path, "use ./b.ecs as b\npub let a = 1\n").expect("failed to write a");
    std::fs::write(&b_path, "use ./a.ecs as a\npub let b = 1\n").expect("failed to write b");

    let output = run_ecscript_file(&main_path);
    let _ = std::fs::remove_file(&main_path);
    let _ = std::fs::remove_file(&a_path);
    let _ = std::fs::remove_file(&b_path);
    let _ = std::fs::remove_dir(&dir);

    assert!(
        !output.status.success(),
        "ecscript circular module import unexpectedly succeeded"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("circular module import detected"));
}
