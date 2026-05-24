use ecsh::parser::parse_line;
use ecsh::types::{
    Command, CommandStatus, OutputRedirection, ParsedJob, ParsedLine, Pipeline, Redirection,
    ShellState, ShellWord,
};

fn state() -> ShellState {
    ShellState {
        last_status: CommandStatus::new(3),
        interactive: false,
        shell_pgid: None,
        shell_terminal_fd: None,
        jobs: Vec::new(),
        next_job_id: 1,
        current_fg_pgid: None,
        script_env: ecsh::ecscript::env::Environment::new(),
    }
}

fn command(program: &str, args: &[&str]) -> Command {
    Command {
        program: ShellWord::lit(program),
        args: args.iter().map(|&arg| ShellWord::lit(arg)).collect(),
        redirection: Redirection::default(),
    }
}

fn parsed_job(line: ParsedLine, command_line: &str) -> ParsedJob {
    ParsedJob {
        line,
        background: false,
        command_line: command_line.to_string(),
    }
}

fn background_job(line: ParsedLine, command_line: &str) -> ParsedJob {
    ParsedJob {
        line,
        background: true,
        command_line: command_line.to_string(),
    }
}

#[test]
fn parses_simple_command_with_quotes() {
    assert_eq!(
        parse_line(r#"echo "hello world""#, &state()).unwrap(),
        parsed_job(
            ParsedLine::Command(command("echo", &["hello world"])),
            r#"echo "hello world""#,
        )
    );
}

#[test]
fn parses_command_with_redirection_without_spaces() {
    assert_eq!(
        parse_line("echo hi>out.txt", &state()).unwrap(),
        parsed_job(
            ParsedLine::Command(Command {
                program: ShellWord::lit("echo"),
                args: vec![ShellWord::lit("hi")],
                redirection: Redirection {
                    stdin: None,
                    stdout: Some(OutputRedirection::Truncate(ShellWord::lit("out.txt"))),
                },
            }),
            "echo hi>out.txt"
        )
    );
}

#[test]
fn parses_pipeline() {
    assert_eq!(
        parse_line("cat<in.txt | grep hi", &state()).unwrap(),
        parsed_job(
            ParsedLine::Pipeline(Pipeline {
                commands: vec![
                    Command {
                        program: ShellWord::lit("cat"),
                        args: vec![],
                        redirection: Redirection {
                            stdin: Some(ShellWord::lit("in.txt")),
                            stdout: None,
                        },
                    },
                    command("grep", &["hi"]),
                ],
            }),
            "cat<in.txt | grep hi"
        )
    );
}

#[test]
fn parses_logical_operators() {
    assert_eq!(
        parse_line("true && echo ok", &state()).unwrap(),
        parsed_job(
            ParsedLine::AndThen(
                Box::new(ParsedLine::Command(command("true", &[]))),
                Box::new(ParsedLine::Command(command("echo", &["ok"]))),
            ),
            "true && echo ok"
        )
    );

    assert_eq!(
        parse_line("false || echo fallback", &state()).unwrap(),
        parsed_job(
            ParsedLine::OrElse(
                Box::new(ParsedLine::Command(command("false", &[]))),
                Box::new(ParsedLine::Command(command("echo", &["fallback"]))),
            ),
            "false || echo fallback"
        )
    );
}

#[test]
fn parses_left_associative_logical_chain() {
    assert_eq!(
        parse_line("false || true && echo done", &state()).unwrap(),
        parsed_job(
            ParsedLine::AndThen(
                Box::new(ParsedLine::OrElse(
                    Box::new(ParsedLine::Command(command("false", &[]))),
                    Box::new(ParsedLine::Command(command("true", &[]))),
                )),
                Box::new(ParsedLine::Command(command("echo", &["done"]))),
            ),
            "false || true && echo done"
        )
    );
}

#[test]
fn parses_sequence_as_lowest_precedence_left_associative_operator() {
    assert_eq!(
        parse_line("echo a; echo b; echo c", &state()).unwrap(),
        parsed_job(
            ParsedLine::Sequence(
                Box::new(ParsedLine::Sequence(
                    Box::new(ParsedLine::Command(command("echo", &["a"]))),
                    Box::new(ParsedLine::Command(command("echo", &["b"]))),
                )),
                Box::new(ParsedLine::Command(command("echo", &["c"]))),
            ),
            "echo a; echo b; echo c"
        )
    );

    assert_eq!(
        parse_line("false && echo no; echo yes", &state()).unwrap(),
        parsed_job(
            ParsedLine::Sequence(
                Box::new(ParsedLine::AndThen(
                    Box::new(ParsedLine::Command(command("false", &[]))),
                    Box::new(ParsedLine::Command(command("echo", &["no"]))),
                )),
                Box::new(ParsedLine::Command(command("echo", &["yes"]))),
            ),
            "false && echo no; echo yes"
        )
    );
}

#[test]
fn parses_backslash_escaped_words() {
    assert_eq!(
        parse_line(r#"echo hello\ world"#, &state()).unwrap(),
        parsed_job(
            ParsedLine::Command(command("echo", &["hello world"])),
            r#"echo hello\ world"#,
        )
    );

    assert_eq!(
        parse_line(r#"echo \| cat"#, &state()).unwrap(),
        parsed_job(
            ParsedLine::Command(command("echo", &["|", "cat"])),
            r#"echo \| cat"#
        )
    );

    assert_eq!(
        parse_line(r#"echo "\$HOME""#, &state()).unwrap(),
        parsed_job(
            ParsedLine::Command(command("echo", &["$HOME"])),
            r#"echo "\$HOME""#
        )
    );
}

#[test]
fn parses_quoted_pipeline_operator_as_word() {
    assert_eq!(
        parse_line(r#"echo "a|b""#, &state()).unwrap(),
        parsed_job(
            ParsedLine::Command(command("echo", &["a|b"])),
            r#"echo "a|b""#
        )
    );
}

#[test]
fn parses_background_command_and_pipeline() {
    assert_eq!(
        parse_line("sleep 1 &", &state()).unwrap(),
        background_job(ParsedLine::Command(command("sleep", &["1"])), "sleep 1 &")
    );

    assert_eq!(
        parse_line("echo hi | cat &", &state()).unwrap(),
        background_job(
            ParsedLine::Pipeline(Pipeline {
                commands: vec![command("echo", &["hi"]), command("cat", &[])],
            }),
            "echo hi | cat &",
        )
    );
}

#[test]
fn reports_parser_errors() {
    assert_eq!(
        parse_line("echo hi |", &state()).unwrap_err(),
        "empty command in pipeline"
    );
    assert_eq!(
        parse_line("true &&", &state()).unwrap_err(),
        "missing command after &&"
    );
    assert_eq!(
        parse_line("echo >", &state()).unwrap_err(),
        "missing filename after >"
    );
    assert_eq!(
        parse_line("echo hi > a > b", &state()).unwrap_err(),
        "duplicate stdout redirection"
    );
    assert_eq!(
        parse_line("; echo hi", &state()).unwrap_err(),
        "missing command before ;"
    );
    assert_eq!(
        parse_line("echo hi;", &state()).unwrap_err(),
        "missing command after ;"
    );
    assert_eq!(
        parse_line("sleep 1 & echo later", &state()).unwrap_err(),
        "background '&' is only supported at the end of a command"
    );
    assert_eq!(
        parse_line("true && echo ok &", &state()).unwrap_err(),
        "background execution is only supported for a single command or pipeline"
    );
    assert_eq!(
        parse_line("&", &state()).unwrap_err(),
        "missing command before &"
    );
}
