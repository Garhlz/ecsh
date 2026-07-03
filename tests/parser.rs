use ecsh::ecscript::value::CommandValue;
use ecsh::extensions::new_extensions;
use ecsh::parser::{parse_command_literal, parse_line};
use ecsh::types::{
    Command, CommandStatus, OutputRedirection, ParsedJob, ParsedLine, Pipeline, Redirection,
    ShellState, ShellWord, WordFragment,
};
use std::collections::HashMap;
use std::rc::Rc;

fn state() -> ShellState {
    ShellState {
        last_status: CommandStatus::new(3),
        interactive: false,
        shell_pgid: None,
        shell_terminal_fd: None,
        jobs: Vec::new(),
        next_job_id: 1,
        current_fg_pgid: None,
        script_env: Rc::new(ecsh::ecscript::env::Environment::new()),
        aliases: HashMap::new(),
        traps: HashMap::new(),
        command_history: Vec::new(),
        extensions: new_extensions(),
        module_loader: None,
    }
}

fn command(program: &str, args: &[&str]) -> Command {
    Command {
        program: ShellWord::lit(program),
        args: args.iter().map(|&arg| ShellWord::lit(arg)).collect(),
        redirection: Redirection::default(),
    }
}

fn quoted_word(text: &str) -> ShellWord {
    ShellWord {
        fragments: vec![WordFragment::QuotedLit(text.into())],
    }
}

fn mixed_word(fragments: Vec<WordFragment>) -> ShellWord {
    ShellWord { fragments }
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
            ParsedLine::Command(Command {
                program: ShellWord::lit("echo"),
                args: vec![quoted_word("hello world")],
                redirection: Redirection::default(),
            }),
            r#"echo "hello world""#,
        )
    );
}

#[test]
fn parses_command_literal_as_single_command() {
    let parsed = parse_command_literal(r#"echo "${x}" > out.txt"#).unwrap();
    let CommandValue::Simple(command) = parsed else {
        panic!("expected single command literal");
    };
    assert_eq!(command.program.as_lit_str(), Some("echo"));
    assert_eq!(command.args.len(), 1);
    assert!(command.redirection.stdout.is_some());
}

#[test]
fn parses_pipeline_in_command_literal() {
    let parsed = parse_command_literal("echo hi | cat").unwrap();
    let CommandValue::Pipeline(pipeline) = parsed else {
        panic!("expected pipeline command literal");
    };
    assert_eq!(pipeline.commands.len(), 2);
    assert_eq!(pipeline.commands[0].program.as_lit_str(), Some("echo"));
    assert_eq!(pipeline.commands[1].program.as_lit_str(), Some("cat"));
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
                Rc::new(ParsedLine::Command(command("true", &[]))),
                Rc::new(ParsedLine::Command(command("echo", &["ok"]))),
            ),
            "true && echo ok"
        )
    );

    assert_eq!(
        parse_line("false || echo fallback", &state()).unwrap(),
        parsed_job(
            ParsedLine::OrElse(
                Rc::new(ParsedLine::Command(command("false", &[]))),
                Rc::new(ParsedLine::Command(command("echo", &["fallback"]))),
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
                Rc::new(ParsedLine::OrElse(
                    Rc::new(ParsedLine::Command(command("false", &[]))),
                    Rc::new(ParsedLine::Command(command("true", &[]))),
                )),
                Rc::new(ParsedLine::Command(command("echo", &["done"]))),
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
                Rc::new(ParsedLine::Sequence(
                    Rc::new(ParsedLine::Command(command("echo", &["a"]))),
                    Rc::new(ParsedLine::Command(command("echo", &["b"]))),
                )),
                Rc::new(ParsedLine::Command(command("echo", &["c"]))),
            ),
            "echo a; echo b; echo c"
        )
    );

    assert_eq!(
        parse_line("false && echo no; echo yes", &state()).unwrap(),
        parsed_job(
            ParsedLine::Sequence(
                Rc::new(ParsedLine::AndThen(
                    Rc::new(ParsedLine::Command(command("false", &[]))),
                    Rc::new(ParsedLine::Command(command("echo", &["no"]))),
                )),
                Rc::new(ParsedLine::Command(command("echo", &["yes"]))),
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
            ParsedLine::Command(Command {
                program: ShellWord::lit("echo"),
                args: vec![mixed_word(vec![
                    WordFragment::Lit("hello".into()),
                    WordFragment::QuotedLit(" ".into()),
                    WordFragment::Lit("world".into()),
                ])],
                redirection: Redirection::default(),
            }),
            r#"echo hello\ world"#,
        )
    );

    assert_eq!(
        parse_line(r#"echo \| cat"#, &state()).unwrap(),
        parsed_job(
            ParsedLine::Command(Command {
                program: ShellWord::lit("echo"),
                args: vec![quoted_word("|"), ShellWord::lit("cat")],
                redirection: Redirection::default(),
            }),
            r#"echo \| cat"#
        )
    );

    assert_eq!(
        parse_line(r#"echo "\$HOME""#, &state()).unwrap(),
        parsed_job(
            ParsedLine::Command(Command {
                program: ShellWord::lit("echo"),
                args: vec![quoted_word("$HOME")],
                redirection: Redirection::default(),
            }),
            r#"echo "\$HOME""#
        )
    );
}

#[test]
fn parses_quoted_pipeline_operator_as_word() {
    assert_eq!(
        parse_line(r#"echo "a|b""#, &state()).unwrap(),
        parsed_job(
            ParsedLine::Command(Command {
                program: ShellWord::lit("echo"),
                args: vec![quoted_word("a|b")],
                redirection: Redirection::default(),
            }),
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
        parse_line("echo hi |", &state()).unwrap_err().message,
        "empty command in pipeline"
    );
    assert_eq!(
        parse_line("true &&", &state()).unwrap_err().message,
        "missing command after &&"
    );
    assert_eq!(
        parse_line("echo >", &state()).unwrap_err().message,
        "missing filename after >"
    );
    assert_eq!(
        parse_line("echo hi > a > b", &state()).unwrap_err().message,
        "duplicate stdout redirection"
    );
    assert_eq!(
        parse_line("; echo hi", &state()).unwrap_err().message,
        "missing command before ;"
    );
    assert_eq!(
        parse_line("echo hi;", &state()).unwrap_err().message,
        "missing command after ;"
    );
    assert_eq!(
        parse_line("sleep 1 & echo later", &state())
            .unwrap_err()
            .message,
        "background '&' is only supported at the end of a command"
    );
    assert_eq!(
        parse_line("true && echo ok &", &state())
            .unwrap_err()
            .message,
        "background execution is only supported for a single command or pipeline"
    );
    assert_eq!(
        parse_line("&", &state()).unwrap_err().message,
        "missing command before &"
    );
}

#[test]
fn expands_top_level_alias_before_parsing() {
    let mut state = state();
    state.aliases.insert("ll".into(), "ls -l".into());

    assert_eq!(
        parse_line("ll /tmp", &state).unwrap(),
        parsed_job(
            ParsedLine::Command(command("ls", &["-l", "/tmp"])),
            "ll /tmp",
        )
    );
}
