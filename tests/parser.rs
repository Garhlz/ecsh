use ecsh::parser::parse_line;
use ecsh::types::{
    Command, CommandStatus, OutputRedirection, ParsedLine, Pipeline, Redirection, ShellState,
};

fn state() -> ShellState {
    ShellState {
        last_status: CommandStatus::new(3),
    }
}

fn command(program: &str, args: &[&str]) -> Command {
    Command {
        program: program.to_string(),
        args: args.iter().map(|arg| arg.to_string()).collect(),
        redirection: Redirection::default(),
    }
}

#[test]
fn parses_simple_command_with_quotes() {
    assert_eq!(
        parse_line(r#"echo "hello world""#, &state()).unwrap(),
        ParsedLine::Command(command("echo", &["hello world"]))
    );
}

#[test]
fn parses_command_with_redirection_without_spaces() {
    assert_eq!(
        parse_line("echo hi>out.txt", &state()).unwrap(),
        ParsedLine::Command(Command {
            program: "echo".to_string(),
            args: vec!["hi".to_string()],
            redirection: Redirection {
                stdin: None,
                stdout: Some(OutputRedirection::Truncate("out.txt".to_string())),
            },
        })
    );
}

#[test]
fn parses_pipeline() {
    assert_eq!(
        parse_line("cat<in.txt | grep hi", &state()).unwrap(),
        ParsedLine::Pipeline(Pipeline {
            commands: vec![
                Command {
                    program: "cat".to_string(),
                    args: vec![],
                    redirection: Redirection {
                        stdin: Some("in.txt".to_string()),
                        stdout: None,
                    },
                },
                command("grep", &["hi"]),
            ],
        })
    );
}

#[test]
fn parses_logical_operators() {
    assert_eq!(
        parse_line("true && echo ok", &state()).unwrap(),
        ParsedLine::AndThen(
            Box::new(ParsedLine::Command(command("true", &[]))),
            Box::new(ParsedLine::Command(command("echo", &["ok"]))),
        )
    );

    assert_eq!(
        parse_line("false || echo fallback", &state()).unwrap(),
        ParsedLine::OrElse(
            Box::new(ParsedLine::Command(command("false", &[]))),
            Box::new(ParsedLine::Command(command("echo", &["fallback"]))),
        )
    );
}

#[test]
fn parses_left_associative_logical_chain() {
    assert_eq!(
        parse_line("false || true && echo done", &state()).unwrap(),
        ParsedLine::AndThen(
            Box::new(ParsedLine::OrElse(
                Box::new(ParsedLine::Command(command("false", &[]))),
                Box::new(ParsedLine::Command(command("true", &[]))),
            )),
            Box::new(ParsedLine::Command(command("echo", &["done"]))),
        )
    );
}

#[test]
fn parses_quoted_pipeline_operator_as_word() {
    assert_eq!(
        parse_line(r#"echo "a|b""#, &state()).unwrap(),
        ParsedLine::Command(command("echo", &["a|b"]))
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
}
