use ecsh::lexer::tokenize;
use ecsh::types::{CommandStatus, ShellState, ShellWord, Token, WordFragment};
use std::collections::HashMap;

fn state() -> ShellState {
    ShellState {
        last_status: CommandStatus::new(7),
        interactive: false,
        shell_pgid: None,
        shell_terminal_fd: None,
        jobs: Vec::new(),
        next_job_id: 1,
        current_fg_pgid: None,
        script_env: ecsh::ecscript::env::Environment::new(),
        aliases: HashMap::new(),
        traps: HashMap::new(),
        command_history: Vec::new(),
    }
}

#[test]
fn tokenizes_plain_words() {
    assert_eq!(
        tokenize("echo hello", &state()).unwrap(),
        vec![
            Token::Word(ShellWord::lit("echo")),
            Token::Word(ShellWord::lit("hello")),
        ]
    );
}

#[test]
fn tokenizes_single_and_double_quotes() {
    assert_eq!(
        tokenize(r#"echo "hello world""#, &state()).unwrap(),
        vec![
            Token::Word(ShellWord::lit("echo")),
            Token::Word(ShellWord::lit("hello world")),
        ]
    );

    assert_eq!(
        tokenize("echo '$HOME'", &state()).unwrap(),
        vec![
            Token::Word(ShellWord::lit("echo")),
            Token::Word(ShellWord::lit("$HOME")),
        ]
    );
}

#[test]
fn joins_quoted_fragments_into_one_word() {
    assert_eq!(
        tokenize(r#"echo a"b"c 'd e'"#, &state()).unwrap(),
        vec![
            Token::Word(ShellWord::lit("echo")),
            Token::Word(ShellWord::lit("abc")),
            Token::Word(ShellWord::lit("d e")),
        ]
    );

    assert_eq!(
        tokenize(r#"echo "" ''"#, &state()).unwrap(),
        vec![Token::Word(ShellWord::lit("echo"))]
    );
}

#[test]
fn keeps_operators_literal_inside_quotes() {
    assert_eq!(
        tokenize(r#"echo "a|b && c > d""#, &state()).unwrap(),
        vec![
            Token::Word(ShellWord::lit("echo")),
            Token::Word(ShellWord::lit("a|b && c > d")),
        ]
    );
}

#[test]
fn collects_dollar_fragments_without_expanding() {
    assert_eq!(
        tokenize("echo $?", &state()).unwrap(),
        vec![
            Token::Word(ShellWord {
                fragments: vec![WordFragment::Lit("echo".into())]
            }),
            Token::Word(ShellWord {
                fragments: vec![WordFragment::Var("?".into())]
            }),
        ]
    );

    assert_eq!(
        tokenize(r#"echo "$HOME""#, &state()).unwrap(),
        vec![
            Token::Word(ShellWord {
                fragments: vec![WordFragment::Lit("echo".into())]
            }),
            Token::Word(ShellWord {
                fragments: vec![WordFragment::Var("HOME".into())]
            }),
        ]
    );

    assert_eq!(
        tokenize("echo prefix-${HOME}/x", &state()).unwrap(),
        vec![
            Token::Word(ShellWord {
                fragments: vec![WordFragment::Lit("echo".into())]
            }),
            Token::Word(ShellWord {
                fragments: vec![
                    WordFragment::Lit("prefix-".into()),
                    WordFragment::Expr {
                        src: "HOME".into(),
                        spread: false,
                    },
                    WordFragment::Lit("/x".into()),
                ]
            }),
        ]
    );
}

#[test]
fn tokenizes_command_and_expr_fragments_with_preserved_source() {
    assert_eq!(
        tokenize(r#"echo $(printf "a\"b") $(printf a\))"#, &state()).unwrap(),
        vec![
            Token::Word(ShellWord::lit("echo")),
            Token::Word(ShellWord {
                fragments: vec![WordFragment::Cmd(r#"printf "a\"b""#.into())]
            }),
            Token::Word(ShellWord {
                fragments: vec![WordFragment::Cmd(r#"printf a\)"#.into())]
            }),
        ]
    );

    assert_eq!(
        tokenize(r#"echo ${"a\"b"} ${...[1, 2]}"#, &state()).unwrap(),
        vec![
            Token::Word(ShellWord::lit("echo")),
            Token::Word(ShellWord {
                fragments: vec![WordFragment::Expr {
                    src: r#""a\"b""#.into(),
                    spread: false,
                }]
            }),
            Token::Word(ShellWord {
                fragments: vec![WordFragment::Expr {
                    src: "[1, 2]".into(),
                    spread: true,
                }]
            }),
        ]
    );
}

#[test]
fn tokenizes_operators_without_spaces() {
    assert_eq!(
        tokenize("cat<in|grep hi>>out", &state()).unwrap(),
        vec![
            Token::Word(ShellWord::lit("cat")),
            Token::RedirectionIn,
            Token::Word(ShellWord::lit("in")),
            Token::Pipe,
            Token::Word(ShellWord::lit("grep")),
            Token::Word(ShellWord::lit("hi")),
            Token::RedirectionAppend,
            Token::Word(ShellWord::lit("out")),
        ]
    );
}

#[test]
fn tokenizes_logical_operators() {
    assert_eq!(
        tokenize("true&&echo ok||echo fallback", &state()).unwrap(),
        vec![
            Token::Word(ShellWord::lit("true")),
            Token::AndIf,
            Token::Word(ShellWord::lit("echo")),
            Token::Word(ShellWord::lit("ok")),
            Token::OrIf,
            Token::Word(ShellWord::lit("echo")),
            Token::Word(ShellWord::lit("fallback")),
        ]
    );
}

#[test]
fn tokenizes_background_operator() {
    assert_eq!(
        tokenize("sleep 1 &", &state()).unwrap(),
        vec![
            Token::Word(ShellWord::lit("sleep")),
            Token::Word(ShellWord::lit("1")),
            Token::Ampersand,
        ]
    );
}

#[test]
fn tokenizes_sequence_operator() {
    assert_eq!(
        tokenize("echo a;echo b", &state()).unwrap(),
        vec![
            Token::Word(ShellWord::lit("echo")),
            Token::Word(ShellWord::lit("a")),
            Token::Semicolon,
            Token::Word(ShellWord::lit("echo")),
            Token::Word(ShellWord::lit("b")),
        ]
    );

    assert_eq!(
        tokenize(r#"echo "a;b" 'c;d'"#, &state()).unwrap(),
        vec![
            Token::Word(ShellWord::lit("echo")),
            Token::Word(ShellWord::lit("a;b")),
            Token::Word(ShellWord::lit("c;d")),
        ]
    );
}

#[test]
fn tokenizes_backslash_escapes() {
    assert_eq!(
        tokenize(r#"echo hello\ world \| \$HOME \; \" \'"#, &state()).unwrap(),
        vec![
            Token::Word(ShellWord::lit("echo")),
            Token::Word(ShellWord::lit("hello world")),
            Token::Word(ShellWord::lit("|")),
            Token::Word(ShellWord::lit("$HOME")),
            Token::Word(ShellWord::lit(";")),
            Token::Word(ShellWord::lit("\"")),
            Token::Word(ShellWord::lit("'")),
        ]
    );

    assert_eq!(
        tokenize(r#"echo 'a\ b'"#, &state()).unwrap(),
        vec![
            Token::Word(ShellWord::lit("echo")),
            Token::Word(ShellWord::lit(r#"a\ b"#)),
        ]
    );

    assert_eq!(
        tokenize(r#"echo "price: \$10" "path: C:\\tmp" "a\q""#, &state()).unwrap(),
        vec![
            Token::Word(ShellWord::lit("echo")),
            Token::Word(ShellWord::lit("price: $10")),
            Token::Word(ShellWord::lit(r#"path: C:\tmp"#)),
            Token::Word(ShellWord::lit(r#"a\q"#)),
        ]
    );
}

#[test]
fn reports_lexer_errors() {
    assert_eq!(
        tokenize("echo \"unterminated", &state())
            .unwrap_err()
            .message,
        "unterminated double quote"
    );
    assert_eq!(
        tokenize("echo ${}", &state()).unwrap_err().message,
        "empty expression in braces"
    );
    assert_eq!(
        tokenize("echo $[1]", &state()).unwrap_err().message,
        "$[expr] has been removed; use ${expr}"
    );
    assert_eq!(
        tokenize("echo $[...[1, 2]]", &state()).unwrap_err().message,
        "$[...expr] has been removed; use ${...expr}"
    );
    assert_eq!(
        tokenize("echo ${HOME", &state()).unwrap_err().message,
        "unterminated ${...} expansion"
    );
    assert_eq!(
        tokenize(r#"echo hello\"#, &state()).unwrap_err().message,
        "trailing backslash"
    );
    assert_eq!(
        tokenize(r#"echo "hello\"#, &state()).unwrap_err().message,
        "trailing backslash in double quotes"
    );
}
