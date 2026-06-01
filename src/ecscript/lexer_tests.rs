use super::{Delimiter, Operator, TokenKind, tokenize};

fn kinds(src: &str) -> Vec<TokenKind> {
    tokenize(src)
        .unwrap()
        .into_iter()
        .map(|token| token.kind)
        .collect()
}

fn assert_kinds(src: &str, expected: Vec<TokenKind>) {
    assert_eq!(kinds(src), expected);
}

fn assert_lex_error(src: &str, offset: usize, message: &str) {
    let err = tokenize(src).unwrap_err();

    assert_eq!(err.offset, offset);
    assert_eq!(err.message, message);
}

fn ident(name: &str) -> TokenKind {
    TokenKind::Identifier(name.to_string())
}

fn string(text: &str) -> TokenKind {
    TokenKind::String(text.to_string())
}

fn int(value: i64) -> TokenKind {
    TokenKind::Int(value)
}

fn float(value: f64) -> TokenKind {
    TokenKind::Float(value)
}

fn op(operator: Operator) -> TokenKind {
    TokenKind::Operator(operator)
}

fn delimiter(delimiter: Delimiter) -> TokenKind {
    TokenKind::Delimiter(delimiter)
}

#[test]
fn lexes_keywords_and_identifiers() {
    assert_kinds(
        "nil true false foo123 _bar9",
        vec![
            TokenKind::Nil,
            TokenKind::True,
            TokenKind::False,
            ident("foo123"),
            ident("_bar9"),
            TokenKind::EOF,
        ],
    );
}

#[test]
fn string_unescapes_common_sequences() {
    assert_kinds(
        "\"a\\n\\t\\\\\\\"b\"",
        vec![string("a\n\t\\\"b"), TokenKind::EOF],
    );
}

#[test]
fn raw_string_keeps_backslashes_literal() {
    assert_kinds(
        "r\"c:\\tmp\\ecs\\test.txt\"",
        vec![string("c:\\tmp\\ecs\\test.txt"), TokenKind::EOF],
    );
}

#[test]
fn raw_string_does_not_unescape_sequences() {
    assert_kinds(r#"r"\n\t\\""#, vec![string(r#"\n\t\\"#), TokenKind::EOF]);
}

#[test]
fn raw_string_prefix_does_not_break_identifiers() {
    assert_kinds(
        "raw r foo",
        vec![ident("raw"), ident("r"), ident("foo"), TokenKind::EOF],
    );
}

#[test]
fn lexes_command_literal_as_single_token() {
    assert_kinds(
        r#"cmd{ echo "${x}" > out.txt }"#,
        vec![
            TokenKind::CommandLiteral(r#" echo "${x}" > out.txt "#.to_string()),
            TokenKind::EOF,
        ],
    );
}

#[test]
fn eq_eq_lexes_as_equality_operator() {
    assert_kinds(
        "== != <= >= && || |> ! = += -= *= /= %=",
        vec![
            op(Operator::EqEq),
            op(Operator::NotEq),
            op(Operator::LtEq),
            op(Operator::GtEq),
            op(Operator::AndAnd),
            op(Operator::OrOr),
            op(Operator::PipeForward),
            op(Operator::Bang),
            delimiter(Delimiter::Eq),
            delimiter(Delimiter::PlusEq),
            delimiter(Delimiter::MinusEq),
            delimiter(Delimiter::StarEq),
            delimiter(Delimiter::SlashEq),
            delimiter(Delimiter::PercentEq),
            TokenKind::EOF,
        ],
    );
}

#[test]
fn distinguishes_ranges_from_floats() {
    assert_kinds(
        "1..2 1..=2 .123 1.23",
        vec![
            int(1),
            delimiter(Delimiter::DotDot),
            int(2),
            int(1),
            delimiter(Delimiter::DotDotEq),
            int(2),
            float(0.123),
            float(1.23),
            TokenKind::EOF,
        ],
    );
}

#[test]
fn reports_invalid_integer_suffix() {
    assert_lex_error(
        "123ab",
        3,
        "invalid numeric literal; expected separator after number, found 'a'",
    );
}

#[test]
fn reports_invalid_float_suffix() {
    assert_lex_error(
        "1.23ab",
        4,
        "invalid numeric literal; expected separator after number, found 'a'",
    );
}

#[test]
fn reports_invalid_leading_dot_float_suffix() {
    assert_lex_error(
        ".123abc",
        4,
        "invalid numeric literal; expected separator after number, found 'a'",
    );
}

#[test]
fn reports_unterminated_string() {
    assert_lex_error("\"abc", 4, "unterminated string literal");
}

#[test]
fn reports_unterminated_raw_string() {
    assert_lex_error("r\"abc", 5, "unterminated raw string literal");
}

#[test]
fn reports_unknown_escape() {
    assert_lex_error("\"\\x\"", 3, "unknown escape '\\x'");
}

#[test]
fn reports_unknown_character() {
    assert_lex_error("@", 0, "unexpected character '@'");
}

#[test]
fn reports_single_ampersand() {
    assert_lex_error("&", 1, "unexpected '&'; did you mean '&&'?");
}

#[test]
fn reports_single_pipe() {
    assert_lex_error("|", 1, "unexpected '|'; did you mean '||'?");
}
