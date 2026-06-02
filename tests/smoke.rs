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

fn example_path(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("ecscript")
        .join(name)
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
fn smoke_runs_example_ecscript_files_via_ecsh() {
    let cases = [
        ("loop_and_accumulate.ecs", "sum"),
        ("closures_and_state.ecs", "snapshots"),
        ("objects_and_collections.ecs", "stat-keys"),
        ("env_and_json.ecs", "patched-range"),
    ];

    for (name, marker) in cases {
        let output = run_ecsh_file_output(&example_path(name));
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        assert!(output.status.success(), "{name} failed, stderr: {stderr}");
        assert!(
            stdout.lines().any(|line| line == marker),
            "{name} missing marker `{marker}`\nstdout:\n{stdout}"
        );
    }
}

#[test]
fn smoke_initializes_shell_environment_variables() {
    let output = run_ecsh_with_env(
        "println(env(\"SHELL\"))\nprintln(env(\"PWD\"))\nprintln(env(\"SHLVL\"))\nexit\n",
        &[("SHLVL", "7")],
    );
    let mut lines = output.lines();

    assert_eq!(
        lines.next(),
        Some(env!("CARGO_BIN_EXE_ecsh")),
        "output was:\n{output}"
    );
    assert_eq!(
        lines.next(),
        std::env::current_dir()
            .ok()
            .map(|cwd| cwd.to_string_lossy().into_owned())
            .as_deref(),
        "output was:\n{output}"
    );
    assert_eq!(lines.next(), Some("8"), "output was:\n{output}");
}

#[test]
fn smoke_cd_updates_pwd_and_oldpwd_env() {
    let cwd = std::env::current_dir()
        .expect("cwd")
        .to_string_lossy()
        .into_owned();
    let output = run_ecsh(
        "println(env(\"PWD\"))\ncd /tmp\nprintln(env(\"OLDPWD\"))\nprintln(env(\"PWD\"))\nexit\n",
    );
    let lines = output.lines().collect::<Vec<_>>();

    assert!(lines.contains(&cwd.as_str()), "output was:\n{output}");
    assert!(lines.contains(&"/tmp"), "output was:\n{output}");
}

#[test]
fn smoke_runs_module_imports_via_ecsh_file_mode() {
    let dir = temp_path("stage10-module");
    std::fs::create_dir_all(&dir).expect("failed to create module dir");
    let main_path = dir.join("main.ecs");
    let foo_path = dir.join("foo.ecs");
    std::fs::write(&foo_path, "let hidden = 1\npub let visible = hidden + 1\n")
        .expect("failed to write foo module");
    std::fs::write(&main_path, "use ./foo.ecs as foo\nprintln(foo.visible)\n")
        .expect("failed to write main script");

    let output = run_ecsh_file_output(&main_path);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(output.status.success(), "stderr: {stderr}");
    assert!(
        stdout.lines().any(|line| line == "2"),
        "stdout was:\n{stdout}"
    );

    let _ = std::fs::remove_file(main_path);
    let _ = std::fs::remove_file(foo_path);
    let _ = std::fs::remove_dir(dir);
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
fn smoke_source_can_override_prompt() {
    let path = temp_path("stage10-prompt.ecs");
    std::fs::write(&path, "prompt((ctx) => { return \"P> \"; })\n")
        .expect("failed to write prompt script");

    let output = run_ecsh_output(&format!("source {}\necho hi\nexit\n", path.display()));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("P> "), "stdout was:\n{stdout}");

    let _ = std::fs::remove_file(path);
}

#[test]
fn smoke_source_runs_after_cd_hook() {
    let path = temp_path("stage10-after-cd.ecs");
    std::fs::write(
        &path,
        "hook(\"after_cd\", (ctx) => { print(\"cd:\"); println(ctx.cwd); })\n",
    )
    .expect("failed to write after_cd hook script");

    let output = run_ecsh(&format!("source {}\ncd /tmp\nexit\n", path.display()));
    assert!(
        output.lines().any(|line| line == "cd:/tmp"),
        "output was:\n{output}"
    );

    let _ = std::fs::remove_file(path);
}

#[test]
fn smoke_source_runs_preexec_and_postexec_hooks() {
    let path = temp_path("stage10-pre-post.ecs");
    std::fs::write(
        &path,
        "hook(\"preexec\", (ctx) => { print(\"pre:\"); println(ctx.command); })\n\
         hook(\"postexec\", (ctx) => { print(\"post:\"); println(ctx.status); })\n",
    )
    .expect("failed to write pre/post hook script");

    let output = run_ecsh(&format!("source {}\necho hi\nexit\n", path.display()));
    assert!(
        output.lines().any(|line| line == "pre:echo hi"),
        "output was:\n{output}"
    );
    assert!(
        output.lines().any(|line| line == "post:0"),
        "output was:\n{output}"
    );

    let _ = std::fs::remove_file(path);
}

#[test]
fn smoke_module_init_can_register_prompt() {
    let module_path = std::env::temp_dir().join(format!(
        "ecsh_stage10_module_prompt_{}.ecs",
        std::process::id()
    ));
    let init_path = std::env::temp_dir().join(format!(
        "ecsh_stage10_module_prompt_init_{}.ecs",
        std::process::id()
    ));
    std::fs::write(
        &module_path,
        "pub func init() {\n    prompt((ctx) => { return \"M> \"; });\n}\n",
    )
    .expect("failed to write prompt module");
    std::fs::write(
        &init_path,
        format!(
            "use {} as prompt_mod\nprompt_mod.init()\n",
            module_path.display()
        ),
    )
    .expect("failed to write prompt init script");

    let output = run_ecsh_output(&format!("source {}\nexit\n", init_path.display()));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("M> "), "stdout was:\n{stdout}");

    let _ = std::fs::remove_file(module_path);
    let _ = std::fs::remove_file(init_path);
}

#[test]
fn smoke_starship_prompt_adapter_sets_ecsh_context() {
    let bin_dir = temp_path("stage10-starship-bin");
    std::fs::create_dir_all(&bin_dir).expect("failed to create fake starship dir");
    let starship_path = bin_dir.join("starship");
    std::fs::write(
        &starship_path,
        "#!/bin/sh\nprintf 'starship:%s:%s:%s:%s' \"$STARSHIP_SHELL\" \"$SHELL\" \"$PWD\" \"$*\"\n",
    )
    .expect("failed to write fake starship");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut perms = std::fs::metadata(&starship_path)
            .expect("metadata")
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&starship_path, perms).expect("chmod");
    }

    let init_path = temp_path("stage10-starship-init.ecs");
    std::fs::write(
        &init_path,
        format!(
            "use {} as starship\nstarship.init()\n",
            example_path("starship_prompt.ecs").display()
        ),
    )
    .expect("failed to write starship init");

    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = run_ecsh_output_with_env(
        &format!("source {}\nexit\n", init_path.display()),
        &[("PATH", &path), ("COLUMNS", "123"), ("SHLVL", "9")],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(stdout.contains("starship:ecsh:"), "stdout was:\n{stdout}");
    assert!(
        stdout.contains(env!("CARGO_BIN_EXE_ecsh")),
        "stdout was:\n{stdout}"
    );
    assert!(
        stdout.contains("--terminal-width 123"),
        "stdout was:\n{stdout}"
    );
    assert!(stdout.contains("--jobs 0"), "stdout was:\n{stdout}");
    assert!(stdout.contains("--shlvl 10"), "stdout was:\n{stdout}");

    let _ = std::fs::remove_file(starship_path);
    let _ = std::fs::remove_dir(bin_dir);
    let _ = std::fs::remove_file(init_path);
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
fn smoke_source_can_set_and_unset_shell_environment() {
    let path = temp_path("stage10-set-env.ecs");
    std::fs::write(
        &path,
        "set_env(\"ECSH_SOURCE_ENV\", \"configured\")\n\
         println(env(\"ECSH_SOURCE_ENV\"))\n\
         unset_env(\"ECSH_SOURCE_ENV\")\n\
         println(env(\"ECSH_SOURCE_ENV\") == nil)\n",
    )
    .expect("failed to write environment source script");

    let output = run_ecsh(&format!("source {}\nexit\n", path.display()));

    assert!(
        output.lines().any(|line| line == "configured"),
        "output was:\n{output}"
    );
    assert!(
        output.lines().any(|line| line == "true"),
        "output was:\n{output}"
    );

    let _ = std::fs::remove_file(path);
}

#[test]
fn smoke_source_registers_ecscript_shell_command() {
    let path = temp_path("stage10-register-command.ecs");
    std::fs::write(
        &path,
        "register_command(\"greet\", (ctx) => {\n\
             print(ctx.name)\n\
             print(\":\")\n\
             println(join(ctx.args, \",\"))\n\
             return 7\n\
         })\n",
    )
    .expect("failed to write registered command source script");

    let output = run_ecsh(&format!(
        "source {}\ngreet one two\nstatus\ntype greet\nwhich greet\nexit\n",
        path.display()
    ));

    assert!(
        output.lines().any(|line| line == "greet:one,two"),
        "output was:\n{output}"
    );
    assert!(
        output.lines().any(|line| line == "7"),
        "output was:\n{output}"
    );
    assert!(output.contains("greet is an ecscript shell command"));
    assert!(output.contains("greet: ecscript shell command"));

    let _ = std::fs::remove_file(path);
}

#[test]
fn smoke_registered_command_set_cwd_runs_after_cd_hook() {
    let path = temp_path("stage10-register-command-set-cwd.ecs");
    std::fs::write(
        &path,
        "hook(\"after_cd\", (ctx) => {\n\
             print(\"cd:\")\n\
             println(ctx.cwd)\n\
         })\n\
         register_command(\"jump_tmp\", (ctx) => {\n\
             set_cwd(\"/tmp\")\n\
         })\n",
    )
    .expect("failed to write set_cwd source script");

    let output = run_ecsh(&format!("source {}\njump_tmp\npwd\nexit\n", path.display()));

    assert!(
        output.lines().any(|line| line == "cd:/tmp"),
        "output was:\n{output}"
    );
    assert!(
        output.lines().any(|line| line == "/tmp"),
        "output was:\n{output}"
    );

    let _ = std::fs::remove_file(path);
}

#[test]
fn smoke_zoxide_adapter_registers_commands() {
    let bin_dir = temp_path("stage10-zoxide-bin");
    std::fs::create_dir_all(&bin_dir).expect("failed to create fake zoxide dir");
    let zoxide_path = bin_dir.join("zoxide");
    std::fs::write(
        &zoxide_path,
        "#!/bin/sh\nif [ \"$1\" = query ]; then printf /tmp; fi\n",
    )
    .expect("failed to write fake zoxide");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut perms = std::fs::metadata(&zoxide_path)
            .expect("metadata")
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&zoxide_path, perms).expect("chmod");
    }

    let init_path = temp_path("stage10-zoxide-init.ecs");
    std::fs::write(
        &init_path,
        format!(
            "use {} as zoxide\nzoxide.init()\n",
            example_path("zoxide.ecs").display()
        ),
    )
    .expect("failed to write zoxide init script");

    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = run_ecsh_output_with_env(
        &format!(
            "source {}\ntype z zi\nz project\npwd\nexit\n",
            init_path.display()
        ),
        &[("PATH", &path)],
    );
    let stdout = visible_output(&String::from_utf8_lossy(&output.stdout));
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(stdout.contains("z is an ecscript shell command"));
    assert!(stdout.contains("zi is an ecscript shell command"));
    assert!(
        stdout.lines().any(|line| line == "/tmp"),
        "stdout was:\n{stdout}\nstderr was:\n{stderr}"
    );

    let _ = std::fs::remove_file(zoxide_path);
    let _ = std::fs::remove_dir(bin_dir);
    let _ = std::fs::remove_file(init_path);
}

#[test]
fn smoke_bind_examples_can_be_sourced() {
    let output = run_ecsh_output(&format!(
        "use {} as bind_accept_hint\n\
         use {} as bind_insert_template\n\
         use {} as bind_history_search\n\
         bind_accept_hint.init()\n\
         bind_insert_template.init()\n\
         bind_history_search.init()\n\
         exit\n",
        example_path("bind_accept_hint.ecs").display(),
        example_path("bind_insert_template.ecs").display(),
        example_path("bind_history_search.ecs").display(),
    ));

    assert!(
        output.status.success(),
        "ecsh exited with {:?}, stderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).trim().is_empty(),
        "stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn smoke_repl_use_supports_session_module_cache() {
    let dir = temp_path("stage10-repl-module-cache");
    std::fs::create_dir_all(&dir).expect("failed to create repl module temp dir");
    let module_path = dir.join("foo.ecs");
    std::fs::write(&module_path, "pub let xs = []\n").expect("failed to write repl module");

    let output = run_ecsh(&format!(
        "cd {}\nuse ./foo.ecs as a\nuse ./foo.ecs as b\npush(a.xs, 1)\nprintln(len(b.xs))\nexit\n",
        dir.display()
    ));

    assert!(
        output.lines().any(|line| line == "1"),
        "output was:\n{output}"
    );

    let _ = std::fs::remove_file(module_path);
    let _ = std::fs::remove_dir(dir);
}

#[test]
fn smoke_registered_command_rejects_unsupported_shell_composition() {
    let path = temp_path("stage10-register-command-boundary.ecs");
    std::fs::write(&path, "register_command(\"noop\", (ctx) => {})\n")
        .expect("failed to write registered command boundary script");

    let output = run_ecsh_output(&format!(
        "source {}\nnoop &\nnoop | cat\nnoop > /tmp/ecsh-noop-output\nexit\n",
        path.display()
    ));
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("noop: ecscript shell command cannot run in the background"),
        "stderr was:\n{stderr}"
    );
    assert!(
        stderr.contains("pipeline: ecscript shell command is not supported in pipelines: noop"),
        "stderr was:\n{stderr}"
    );
    assert!(
        stderr.contains("noop: redirection is not supported for ecscript shell commands"),
        "stderr was:\n{stderr}"
    );

    let _ = std::fs::remove_file(path);
}

#[test]
fn smoke_ecscript_cwd_and_join_path_builtins() {
    let output = run_ecsh(
        r#"println(cwd())
println(join_path("/tmp", "ecsh"))
exit
"#,
    );

    assert!(
        output
            .lines()
            .next()
            .is_some_and(|line| line.starts_with('/'))
    );
    assert!(output.lines().any(|line| line == "/tmp/ecsh"));
}

#[test]
fn smoke_ecscript_nil_for_missing_env() {
    let result = run_ecsh("let x = env(\"ECSH_NOEXIST_VAR\"); println(x == nil); exit\n");
    assert!(result.contains("true"));
}

#[test]
fn smoke_ecscript_text_and_lines_execute_command_literals() {
    let output = run_ecsh(
        r#"println(text(cmd{ printf "hello" }))
println(lines(cmd{ printf "a\nb\n" }))
exit
"#,
    );

    assert!(output.lines().any(|line| line == "hello"));
    assert!(output.lines().any(|line| line == "[\"a\", \"b\"]"));
}

#[test]
fn smoke_ecscript_capture_returns_command_result() {
    let output = run_ecsh(
        r#"let result = capture(cmd{ sh -c "printf out; printf err 1>&2; exit 3" })
println(result.code, result.stdout, result.stderr, result.ok)
println(result)
exit
"#,
    );

    assert!(output.lines().any(|line| line == "3 out err false"));
    assert!(output.contains("{code: 3, duration_ms:"));
    assert!(output.contains("ok: false"));
    assert!(output.contains("signal: nil"));
    assert!(output.contains("stderr: \"err\""));
    assert!(output.contains("stdout: \"out\""));
}

#[test]
fn smoke_ecscript_run_executes_command_literal() {
    let path = temp_path("cmd-run-out");
    let output = run_ecsh(&format!(
        "run(cmd{{ sh -c \"echo hi\" > {} }})\ncat {}\nexit\n",
        path.display(),
        path.display()
    ));

    assert!(output.lines().any(|line| line == "hi"));
    let _ = std::fs::remove_file(path);
}

#[test]
fn smoke_ecscript_pipeline_command_literal_executes() {
    let output = run_ecsh(
        r#"println(text(cmd{ printf "foo" | tr o O }))
exit
"#,
    );

    assert!(output.lines().any(|line| line == "fOO"));
}

#[test]
fn smoke_ecscript_command_builder_executes() {
    let output = run_ecsh(
        r#"println(text(command("/bin/echo", "builder-ok", 7, true)))
exit
"#,
    );

    assert!(output.lines().any(|line| line == "builder-ok 7 true"));
}

#[test]
fn smoke_ecscript_command_literal_supports_pure_output_builtin() {
    let output = run_ecsh(
        r#"println(text(cmd{ status }))
exit
"#,
    );

    assert!(output.lines().any(|line| line == "0"));
}

#[test]
fn smoke_ecscript_from_json_parses_command_output() {
    let output = run_ecsh(
        r#"let data = from_json(text(cmd{ printf "{\"name\":\"ecsh\",\"nums\":[1,2,3]}" }))
println(data.name, data.nums[1])
exit
"#,
    );

    assert!(output.lines().any(|line| line == "ecsh 2"));
}

#[test]
fn smoke_ecscript_with_env_derives_command_value() {
    let output = run_ecsh(
        r#"let proc = with_env(cmd{ sh -c 'printf %s "$ECSH_CMD_BRIDGE"' }, { ECSH_CMD_BRIDGE: "bridge-ok" })
println(text(proc))
exit
"#,
    );

    assert!(output.lines().any(|line| line == "bridge-ok"));
}

#[test]
fn smoke_ecscript_with_cwd_derives_command_value() {
    let output = run_ecsh(
        r#"let proc = with_cwd(cmd{ /bin/pwd }, "/tmp")
println(text(proc))
exit
"#,
    );

    assert!(output.lines().any(|line| line == "/tmp"));
}

#[test]
fn smoke_ecscript_pipe_forward_chains_array_functions() {
    let output = run_ecsh(
        r#"let xs = range(1, 5)
println(xs |> filter((x) => x > 2) |> map((x) => x * 10) |> join(","))
exit
"#,
    );

    assert!(output.lines().any(|line| line == "30,40,50"));
}
