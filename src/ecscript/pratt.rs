use crate::ecscript::error::ParseError;

use crate::ecscript::ast::{Expr, ExprKind, Literal};
use crate::ecscript::lexer::{Delimiter, Token, TokenKind};

pub struct TokenStream<'a> {
    tokens: &'a [Token],
    pos: usize,
}

impl<'a> TokenStream<'a> {
    fn new(tokens: &'a [Token]) -> Self {
        TokenStream { tokens, pos: 0 }
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn consume(&mut self) {
        self.pos += 1;
    }

    fn check(&self, kind: &TokenKind) -> bool {
        self.peek().kind == *kind
    }

    fn current_offset(&self) -> usize {
        self.peek().end
    }
}

pub fn parse_expr(tokens: &[Token]) -> Result<Expr, ParseError> {
    let mut state = TokenStream::new(tokens);
    let expr = pratt_parser(&mut state, 0)?;
    if state.check(&TokenKind::EOF) {
        // 在这里判断是否已经收尾
        Ok(expr)
    } else {
        Err(ParseError::new(
            state.current_offset(),
            "unexpected token after expression".to_string(),
        ))
    }
}

fn pratt_parser(state: &mut TokenStream<'_>, min_bp: u8) -> Result<Expr, ParseError> {
    let mut left: Expr;

    let prefix_span = state.current_offset();
    match state.peek().kind.clone() {
        TokenKind::Int(i) => {
            left = Expr {
                kind: ExprKind::Literal(Literal::Int(i)),
                span: prefix_span,
            };
            state.consume();
        }
        TokenKind::Float(f) => {
            left = Expr {
                kind: ExprKind::Literal(Literal::Float(f)),
                span: prefix_span,
            };
            state.consume();
        }
        TokenKind::String(s) => {
            left = Expr {
                kind: ExprKind::Literal(Literal::String(s)),
                span: prefix_span,
            };
            state.consume();
        }
        TokenKind::Nil => {
            left = Expr {
                kind: ExprKind::Literal(Literal::Nil),
                span: prefix_span,
            };
            state.consume();
        }
        TokenKind::True => {
            left = Expr {
                kind: ExprKind::Literal(Literal::Bool(true)),
                span: prefix_span,
            };
            state.consume();
        }
        TokenKind::False => {
            left = Expr {
                kind: ExprKind::Literal(Literal::Bool(false)),
                span: prefix_span,
            };
            state.consume();
        }

        TokenKind::Identifier(s) => {
            left = Expr {
                kind: ExprKind::Variable(s),
                span: prefix_span,
            };
            state.consume();
        }

        // prefix
        TokenKind::Operator(oper) => {
            if let Some((cur_bp, prefix_oper)) = oper.prefix_info() {
                state.consume();
                let right = pratt_parser(state, cur_bp)?;
                left = Expr {
                    kind: ExprKind::Prefix(prefix_oper, Box::new(right)),
                    span: prefix_span,
                };
            } else {
                return Err(ParseError::new(
                    state.current_offset(),
                    "unexpected operator at start of expression".to_string(),
                ));
            }
        }
        // (
        TokenKind::Delimiter(Delimiter::LParen) => {
            state.consume();
            left = pratt_parser(state, 0)?;
            if state.check(&TokenKind::Delimiter(Delimiter::RParen)) {
                state.consume();
            } else {
                return Err(ParseError::new(
                    state.current_offset(),
                    "expected ')'".to_string(),
                ));
            }
        }
        _ => {
            return Err(ParseError::new(
                state.current_offset(),
                "expected expression".to_string(),
            ));
        }
    }

    // 中缀递归处理
    // 例如优先级相同的左结合操作符，就会建立一颗深度在左侧的语法树
    loop {
        let op_span = state.current_offset();
        let kind = state.peek().kind.clone();
        match kind {
            TokenKind::Operator(oper) => {
                if let Some((left_bp, right_bp, infix_oper)) = oper.infix_info() {
                    if left_bp <= min_bp {
                        break;
                    }
                    state.consume();
                    let right = pratt_parser(state, right_bp)?;
                    left = Expr {
                        kind: ExprKind::Infix(Box::new(left), infix_oper, Box::new(right)),
                        span: op_span,
                    };
                } else {
                    break;
                }
            }
            _ => break,
        }
    }
    Ok(left)
}

#[cfg(test)]
mod tests {
    use super::parse_expr;
    use crate::ecscript::ast::{Expr, ExprKind, InfixOper, Literal, PrefixOper};
    use crate::ecscript::lexer::tokenize;

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
        assert_parse("3.14", lit_float(3.14));
    }

    #[test]
    fn parses_variable_reference() {
        assert_parse("foo", var("foo"));
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

    // ── 错误 ────────────────────────────────────────────

    #[test]
    fn reports_missing_right_paren() {
        assert_parse_error("(1 + 2", 6, "expected ')'");
    }

    #[test]
    fn reports_trailing_garbage() {
        assert_parse_error("1 + 2 3", 7, "unexpected token after expression");
    }

    #[test]
    fn reports_empty_input() {
        assert_parse_error("", 0, "expected expression");
    }

    #[test]
    fn reports_bare_operator_at_start() {
        assert_parse_error("* 5", 1, "unexpected operator at start of expression");
    }
}
