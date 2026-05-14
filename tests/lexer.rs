use ecsh::lexer::tokenize;
use ecsh::types::{CommandStatus, ShellState, Token};

fn state() -> ShellState {
    ShellState {
        last_status: CommandStatus::new(7),
    }
}

#[test]
fn tokenizes_plain_words() {
    assert_eq!(
        tokenize("echo hello", &state()).unwrap(),
        vec![
            Token::Word("echo".to_string()),
            Token::Word("hello".to_string()),
        ]
    );
}

#[test]
fn tokenizes_single_and_double_quotes() {
    assert_eq!(
        tokenize(r#"echo "hello world""#, &state()).unwrap(),
        vec![
            Token::Word("echo".to_string()),
            Token::Word("hello world".to_string()),
        ]
    );

    assert_eq!(
        tokenize("echo '$HOME'", &state()).unwrap(),
        vec![
            Token::Word("echo".to_string()),
            Token::Word("$HOME".to_string()),
        ]
    );
}

#[test]
fn joins_quoted_fragments_into_one_word() {
    assert_eq!(
        tokenize(r#"echo a"b"c 'd e'"#, &state()).unwrap(),
        vec![
            Token::Word("echo".to_string()),
            Token::Word("abc".to_string()),
            Token::Word("d e".to_string()),
        ]
    );

    assert_eq!(
        tokenize(r#"echo "" ''"#, &state()).unwrap(),
        vec![Token::Word("echo".to_string())]
    );
}

#[test]
fn keeps_operators_literal_inside_quotes() {
    assert_eq!(
        tokenize(r#"echo "a|b && c > d""#, &state()).unwrap(),
        vec![
            Token::Word("echo".to_string()),
            Token::Word("a|b && c > d".to_string()),
        ]
    );
}

#[test]
fn expands_variables_in_normal_and_double_quoted_words() {
    let home = std::env::var("HOME").unwrap_or_default();

    assert_eq!(
        tokenize("echo $?", &state()).unwrap(),
        vec![
            Token::Word("echo".to_string()),
            Token::Word("7".to_string()),
        ]
    );

    assert_eq!(
        tokenize(r#"echo "$HOME""#, &state()).unwrap(),
        vec![Token::Word("echo".to_string()), Token::Word(home.clone())]
    );

    assert_eq!(
        tokenize("echo prefix-${HOME}/x", &state()).unwrap(),
        vec![
            Token::Word("echo".to_string()),
            Token::Word(format!("prefix-{}/x", home)),
        ]
    );
}

#[test]
fn tokenizes_operators_without_spaces() {
    assert_eq!(
        tokenize("cat<in|grep hi>>out", &state()).unwrap(),
        vec![
            Token::Word("cat".to_string()),
            Token::RedirectionIn,
            Token::Word("in".to_string()),
            Token::Pipe,
            Token::Word("grep".to_string()),
            Token::Word("hi".to_string()),
            Token::RedirectionAppend,
            Token::Word("out".to_string()),
        ]
    );
}

#[test]
fn tokenizes_logical_operators() {
    assert_eq!(
        tokenize("true&&echo ok||echo fallback", &state()).unwrap(),
        vec![
            Token::Word("true".to_string()),
            Token::AndIf,
            Token::Word("echo".to_string()),
            Token::Word("ok".to_string()),
            Token::OrIf,
            Token::Word("echo".to_string()),
            Token::Word("fallback".to_string()),
        ]
    );
}

#[test]
fn tokenizes_sequence_operator() {
    assert_eq!(
        tokenize("echo a;echo b", &state()).unwrap(),
        vec![
            Token::Word("echo".to_string()),
            Token::Word("a".to_string()),
            Token::Semicolon,
            Token::Word("echo".to_string()),
            Token::Word("b".to_string()),
        ]
    );

    assert_eq!(
        tokenize(r#"echo "a;b" 'c;d'"#, &state()).unwrap(),
        vec![
            Token::Word("echo".to_string()),
            Token::Word("a;b".to_string()),
            Token::Word("c;d".to_string()),
        ]
    );
}

#[test]
fn reports_lexer_errors() {
    assert_eq!(
        tokenize("echo \"unterminated", &state()).unwrap_err(),
        "unterminated double quote"
    );
    assert_eq!(
        tokenize("echo ${}", &state()).unwrap_err(),
        "empty variable name in braces"
    );
    assert_eq!(
        tokenize("echo ${1}", &state()).unwrap_err(),
        "invalid variable name in braces"
    );
    assert_eq!(
        tokenize("echo ${HOME", &state()).unwrap_err(),
        "unterminated ${...} expansion"
    );
    assert_eq!(
        tokenize("sleep 1 &", &state()).unwrap_err(),
        "single '&' is not supported yet"
    );
}
