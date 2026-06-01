use super::parse_expr;
use crate::ecscript::ast::{Expr, ExprKind, InfixOper, Literal, PrefixOper, Stmt, StmtKind};
use crate::ecscript::lexer::tokenize;
use crate::ecscript::value::CommandValue;

/// 解析源码并断言 AST 结构一致（忽略 span）。
fn assert_parse(src: &str, expected: Expr) {
    let actual = parse_src(src);
    assert_eq!(actual.kind, expected.kind, "mismatch for source: {}", src);
}

fn assert_parse_error(src: &str, offset: usize, message: &str) {
    let tokens = tokenize(src).unwrap();
    let err = parse_expr(&tokens).unwrap_err();

    assert_eq!(err.offset, offset);
    assert_eq!(err.message, message);
}

fn parse_src(src: &str) -> Expr {
    let tokens = tokenize(src).unwrap();
    parse_expr(&tokens).unwrap()
}

fn lit_nil() -> Expr {
    Expr {
        kind: ExprKind::Literal(Literal::Nil),
        span: 0,
    }
}

fn lit_bool(value: bool) -> Expr {
    Expr {
        kind: ExprKind::Literal(Literal::Bool(value)),
        span: 0,
    }
}

fn lit_int(value: i64) -> Expr {
    Expr {
        kind: ExprKind::Literal(Literal::Int(value)),
        span: 0,
    }
}

fn lit_float(value: f64) -> Expr {
    Expr {
        kind: ExprKind::Literal(Literal::Float(value)),
        span: 0,
    }
}

fn var(name: &str) -> Expr {
    Expr {
        kind: ExprKind::Variable(name.to_string()),
        span: 0,
    }
}

fn prefix(operator: PrefixOper, expr: Expr) -> Expr {
    Expr {
        kind: ExprKind::Prefix(operator, Box::new(expr)),
        span: 0,
    }
}

fn infix(left: Expr, operator: InfixOper, right: Expr) -> Expr {
    Expr {
        kind: ExprKind::Infix(Box::new(left), operator, Box::new(right)),
        span: 0,
    }
}

fn array(elements: Vec<Expr>) -> Expr {
    Expr {
        kind: ExprKind::Array(elements),
        span: 0,
    }
}

fn object(entries: Vec<(&str, Expr)>) -> Expr {
    Expr {
        kind: ExprKind::Object(
            entries
                .into_iter()
                .map(|(key, value)| (key.to_string(), value))
                .collect(),
        ),
        span: 0,
    }
}

fn call(callee: Expr, args: Vec<Expr>) -> Expr {
    Expr {
        kind: ExprKind::Call(Box::new(callee), args),
        span: 0,
    }
}

fn lambda(params: Vec<&str>, body: Vec<Stmt>) -> Expr {
    Expr {
        kind: ExprKind::FuncLiteral {
            params: params.into_iter().map(str::to_string).collect(),
            body,
        },
        span: 0,
    }
}

fn return_stmt(value: Option<Expr>) -> Stmt {
    Stmt {
        kind: StmtKind::Return { value },
        span: 0,
    }
}

#[test]
fn parses_operator_precedence() {
    assert_parse(
        "1 + 2 * 3",
        infix(
            lit_int(1),
            InfixOper::Add,
            infix(lit_int(2), InfixOper::Mul, lit_int(3)),
        ),
    );
}

#[test]
fn parses_prefix_and_grouping() {
    assert_parse(
        "!(1 < 2)",
        prefix(
            PrefixOper::Not,
            infix(lit_int(1), InfixOper::Lt, lit_int(2)),
        ),
    );
}

// ── 字面量 ──────────────────────────────────────────

#[test]
fn parses_all_literal_types() {
    assert_parse("nil", lit_nil());
    assert_parse("true", lit_bool(true));
    assert_parse("false", lit_bool(false));
    assert_parse("42", lit_int(42));
    assert_parse("2.5", lit_float(2.5));
}

#[test]
fn parses_variable_reference() {
    assert_parse("foo", var("foo"));
}

#[test]
fn parses_command_literal_expression() {
    let expr = parse_src(r#"cmd{ echo "${x}" > out.txt }"#);
    let ExprKind::CommandLiteral(CommandValue::Simple(command)) = expr.kind else {
        panic!("expected command literal");
    };
    assert_eq!(command.program.as_lit_str(), Some("echo"));
    assert_eq!(command.args.len(), 1);
    assert!(command.redirection.stdout.is_some());
}

#[test]
fn parses_empty_array_literal() {
    assert_parse("[]", array(vec![]));
}

#[test]
fn parses_object_literal_with_identifier_keys() {
    assert_parse(
        "{name: 1, age: 2}",
        object(vec![("name", lit_int(1)), ("age", lit_int(2))]),
    );
}

#[test]
fn parses_field_index_and_call_chain() {
    assert_parse(
        "foo.bar[0](x)",
        Expr {
            kind: ExprKind::Call(
                Box::new(Expr {
                    kind: ExprKind::Index(
                        Box::new(Expr {
                            kind: ExprKind::Field(Box::new(var("foo")), "bar".into()),
                            span: 0,
                        }),
                        Box::new(lit_int(0)),
                    ),
                    span: 0,
                }),
                vec![var("x")],
            ),
            span: 0,
        },
    );
}

// ── 前缀运算符 ──────────────────────────────────────

#[test]
fn parses_prefix_negation() {
    assert_parse("-5", prefix(PrefixOper::Neg, lit_int(5)));
}

#[test]
fn parses_double_prefix() {
    assert_parse(
        "!!true",
        prefix(PrefixOper::Not, prefix(PrefixOper::Not, lit_bool(true))),
    );
}

// ── 算术运算符 ──────────────────────────────────────

#[test]
fn parses_all_arithmetic_operators() {
    assert_parse("1 + 2", infix(lit_int(1), InfixOper::Add, lit_int(2)));
    assert_parse("5 - 3", infix(lit_int(5), InfixOper::Sub, lit_int(3)));
    assert_parse("4 * 7", infix(lit_int(4), InfixOper::Mul, lit_int(7)));
    assert_parse("8 / 2", infix(lit_int(8), InfixOper::Div, lit_int(2)));
    assert_parse("10 % 3", infix(lit_int(10), InfixOper::Mod, lit_int(3)));
}

// ── 比较运算符 ──────────────────────────────────────

#[test]
fn parses_all_comparison_operators() {
    assert_parse("1 == 2", infix(lit_int(1), InfixOper::Eq, lit_int(2)));
    assert_parse("1 != 2", infix(lit_int(1), InfixOper::Ne, lit_int(2)));
    assert_parse("1 < 2", infix(lit_int(1), InfixOper::Lt, lit_int(2)));
    assert_parse("1 > 2", infix(lit_int(1), InfixOper::Gt, lit_int(2)));
    assert_parse("1 <= 2", infix(lit_int(1), InfixOper::Le, lit_int(2)));
    assert_parse("1 >= 2", infix(lit_int(1), InfixOper::Ge, lit_int(2)));
}

// ── 优先级：逻辑 < 比较 < 算术 ─────────────────────

#[test]
fn comparison_binds_tighter_than_logical() {
    // 1 < 2 && 3 > 0  →  (1 < 2) && (3 > 0)
    assert_parse(
        "1 < 2 && 3 > 0",
        infix(
            infix(lit_int(1), InfixOper::Lt, lit_int(2)),
            InfixOper::And,
            infix(lit_int(3), InfixOper::Gt, lit_int(0)),
        ),
    );
}

#[test]
fn and_binds_tighter_than_or() {
    // true || false && true  →  true || (false && true)
    assert_parse(
        "true || false && true",
        infix(
            lit_bool(true),
            InfixOper::Or,
            infix(lit_bool(false), InfixOper::And, lit_bool(true)),
        ),
    );
}

#[test]
fn pipe_forward_desugars_to_call() {
    assert_parse("x |> f()", call(var("f"), vec![var("x")]));
    assert_parse(
        "x |> f(1, 2)",
        call(var("f"), vec![var("x"), lit_int(1), lit_int(2)]),
    );
}

#[test]
fn pipe_forward_has_lower_precedence_than_arithmetic_and_logical_ops() {
    assert_parse(
        "1 + 2 |> f()",
        call(
            var("f"),
            vec![infix(lit_int(1), InfixOper::Add, lit_int(2))],
        ),
    );
    assert_parse(
        "true || false |> f()",
        call(
            var("f"),
            vec![infix(lit_bool(true), InfixOper::Or, lit_bool(false))],
        ),
    );
}

#[test]
fn pipe_forward_requires_call_expression_on_right() {
    assert_parse_error(
        "x |> f",
        4,
        "|> expects a call expression on the right-hand side",
    );
    assert_parse_error(
        "x |> y + 1",
        4,
        "|> expects a call expression on the right-hand side",
    );
}

// ── 结合性 ──────────────────────────────────────────

#[test]
fn addition_is_left_associative() {
    assert_parse(
        "1 + 2 + 3",
        infix(
            infix(lit_int(1), InfixOper::Add, lit_int(2)),
            InfixOper::Add,
            lit_int(3),
        ),
    );
}

#[test]
fn subtraction_is_left_associative() {
    assert_parse(
        "10 - 3 - 2",
        infix(
            infix(lit_int(10), InfixOper::Sub, lit_int(3)),
            InfixOper::Sub,
            lit_int(2),
        ),
    );
}

// ── 混合优先级 ──────────────────────────────────────

#[test]
fn multiplication_binds_tighter_than_addition_with_mixed_ops() {
    assert_parse(
        "1 + 2 * 3 + 4",
        infix(
            infix(
                lit_int(1),
                InfixOper::Add,
                infix(lit_int(2), InfixOper::Mul, lit_int(3)),
            ),
            InfixOper::Add,
            lit_int(4),
        ),
    );
}

#[test]
fn prefix_binds_tighter_than_binary() {
    // -1 + 2  →  (-1) + 2
    assert_parse(
        "-1 + 2",
        infix(
            prefix(PrefixOper::Neg, lit_int(1)),
            InfixOper::Add,
            lit_int(2),
        ),
    );
}

// ── 括号 ────────────────────────────────────────────

#[test]
fn parses_nested_parens() {
    assert_parse("((42))", lit_int(42));
}

#[test]
fn parses_newlines_inside_call_arguments() {
    assert_parse(
        "foo(\n1,\n2\n)",
        Expr {
            kind: ExprKind::Call(Box::new(var("foo")), vec![lit_int(1), lit_int(2)]),
            span: 0,
        },
    );
}

#[test]
fn parens_override_precedence() {
    assert_parse(
        "(1 + 2) * 3",
        infix(
            infix(lit_int(1), InfixOper::Add, lit_int(2)),
            InfixOper::Mul,
            lit_int(3),
        ),
    );
}

// ── 复杂表达式 ──────────────────────────────────────

#[test]
fn parses_complex_expression() {
    // -a + (b * 3) < 10 && !x
    assert_parse(
        "-a + b * 3 < 10 && !x",
        infix(
            infix(
                infix(
                    prefix(PrefixOper::Neg, var("a")),
                    InfixOper::Add,
                    infix(var("b"), InfixOper::Mul, lit_int(3)),
                ),
                InfixOper::Lt,
                lit_int(10),
            ),
            InfixOper::And,
            prefix(PrefixOper::Not, var("x")),
        ),
    );
}

#[test]
fn parses_lambda_with_expression_body() {
    assert_parse(
        "(x) => x + 1",
        lambda(
            vec!["x"],
            vec![return_stmt(Some(infix(
                var("x"),
                InfixOper::Add,
                lit_int(1),
            )))],
        ),
    );
}

#[test]
fn parses_lambda_with_block_body() {
    assert_parse(
        "(a, b) => { return a + b; }",
        lambda(
            vec!["a", "b"],
            vec![return_stmt(Some(infix(var("a"), InfixOper::Add, var("b"))))],
        ),
    );
}

#[test]
fn parens_without_arrow_remain_grouping() {
    assert_parse("(x + 1)", infix(var("x"), InfixOper::Add, lit_int(1)));
}

// ── 错误 ────────────────────────────────────────────

#[test]
fn reports_missing_right_paren() {
    assert_parse_error("(1 + 2", 6, "expected ')', found end of input");
}

#[test]
fn reports_trailing_garbage() {
    assert_parse_error(
        "1 + 2 3",
        7,
        "unexpected token after expression, found integer literal",
    );
}

#[test]
fn reports_empty_input() {
    assert_parse_error("", 0, "expected expression, found end of input");
}

#[test]
fn reports_bare_operator_at_start() {
    assert_parse_error("* 5", 1, "unexpected operator '*' at start of expression");
}

#[test]
fn reports_missing_lambda_body() {
    assert_parse_error("(x) =>", 6, "expected expression, found end of input");
}
