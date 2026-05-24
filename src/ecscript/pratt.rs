use crate::ecscript::error::ParseError;

use crate::ecscript::ast::{Expr, ExprKind, Literal, RangeExpr, Stmt, StmtKind};
use crate::ecscript::lexer::{Delimiter, Token, TokenKind};
use crate::ecscript::parser::expect_block;

/// Choose `ParseError::incomplete` when we hit EOF expecting more tokens,
/// otherwise `ParseError::new`.
fn parse_error(state: &TokenStream<'_>, message: impl Into<String>) -> ParseError {
    let offset = state.current_offset();
    let msg = message.into();
    if matches!(state.peek().kind, TokenKind::EOF) {
        ParseError::incomplete(offset, msg)
    } else {
        ParseError::new(offset, msg)
    }
}
pub struct TokenStream<'a> {
    tokens: &'a [Token],
    pos: usize,
}

impl<'a> TokenStream<'a> {
    pub fn new(tokens: &'a [Token]) -> Self {
        TokenStream { tokens, pos: 0 }
    }

    pub fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    pub fn consume(&mut self) {
        self.pos += 1;
    }

    pub fn peek_n(&self, n: usize) -> Option<&Token> {
        self.tokens.get(self.pos + n)
    }

    pub fn check(&self, kind: &TokenKind) -> bool {
        self.peek().kind == *kind
    }

    pub fn check_next(&self, kind: &TokenKind) -> bool {
        self.peek_n(1).is_some_and(|token| token.kind == *kind)
    }

    pub fn current_offset(&self) -> usize {
        self.peek().end
    }
    pub fn save(&self) -> usize {
        self.pos
    }
    pub fn load(&mut self, new_pos: usize) {
        self.pos = new_pos;
    }
}

/// 把token流转换为单个表达式
pub fn parse_expr(tokens: &[Token]) -> Result<Expr, ParseError> {
    let mut state = TokenStream::new(tokens);
    let expr = pratt_parser(&mut state, 0)?;
    if state.check(&TokenKind::EOF) {
        // 在这里判断是否已经收尾
        Ok(expr)
    } else {
        Err(ParseError::new(
            state.current_offset(),
            format!(
                "unexpected token after expression, found {}",
                state.peek().kind.describe()
            ),
        ))
    }
}

pub fn parse_expr_in(state: &mut TokenStream<'_>) -> Result<Expr, ParseError> {
    // 只解析表达式本身，不检查分号——分号由调用方（语句解析器）负责
    pratt_parser(state, 0)
}

fn pratt_parser(state: &mut TokenStream<'_>, min_bp: u8) -> Result<Expr, ParseError> {
    let mut left: Expr;

    let prefix_span = state.current_offset();
    // 前缀位置
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

        // prefix operator
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
                    format!(
                        "unexpected operator '{}' at start of expression",
                        oper.lexeme()
                    ),
                ));
            }
        }

        // `(` 用于改变运算优先级（这里是前缀位置，不会是函数调用）
        // 匿名函数 (x, y) => x + y; 也需要使用此位置
        TokenKind::Delimiter(Delimiter::LParen) => {
            let span = state.current_offset();
            state.consume();

            // save & load
            let pos = state.save();

            let params = {
                let mut try_parse_lambda = || -> Option<Vec<String>> {
                    let mut params: Vec<String> = Vec::new();
                    loop {
                        if state.check(&TokenKind::Delimiter(Delimiter::RParen)) {
                            state.consume();
                            break;
                        }

                        let TokenKind::Identifier(name) = state.peek().kind.clone() else {
                            return None;
                        };
                        state.consume();
                        params.push(name);

                        if state.check(&TokenKind::Delimiter(Delimiter::RParen)) {
                            state.consume();
                            break;
                        } else if state.check(&TokenKind::Delimiter(Delimiter::Comma)) {
                            state.consume();
                        } else {
                            return None;
                        }
                    }
                    // 必须有=>箭头
                    if !state.check(&TokenKind::Delimiter(Delimiter::FatArrow)) {
                        return None;
                    };
                    state.consume();
                    Some(params)
                };
                try_parse_lambda()
            };

            if let Some(params) = params {
                return parse_lambda(state, params, span);
            } else {
                state.load(pos);

                left = pratt_parser(state, 0)?;
                if state.check(&TokenKind::Delimiter(Delimiter::RParen)) {
                    state.consume();
                } else {
                    return Err(parse_error(
                        state,
                        format!("expected ')', found {}", state.peek().kind.describe()),
                    ));
                }
            }
        }

        // `[` 用于数组字面量  Array(Vec<Expr>) [1,2,3]
        TokenKind::Delimiter(Delimiter::LBracket) => {
            state.consume();
            let mut arr = Vec::new();
            loop {
                if state.check(&TokenKind::Delimiter(Delimiter::RBracket)) {
                    state.consume();
                    left = Expr {
                        kind: ExprKind::Array(arr),
                        span: state.current_offset(),
                    };
                    break;
                }
                // 可能是空数组
                let element = pratt_parser(state, 0)?;
                arr.push(element);

                if state.check(&TokenKind::Delimiter(Delimiter::Comma)) {
                    state.consume();
                } else if state.check(&TokenKind::Delimiter(Delimiter::RBracket)) {
                    state.consume();
                    left = Expr {
                        kind: ExprKind::Array(arr),
                        span: state.current_offset(),
                    };
                    break;
                } else {
                    return Err(parse_error(state, "expected ',' or ']'"));
                }
            }
        }

        // `{`用于对象的字面量表达式 Object(Vec<(String, Expr)>) {"a": 1, "b": 2}
        TokenKind::Delimiter(Delimiter::LBrace) => {
            state.consume();
            let mut obj: Vec<(String, Expr)> = Vec::new();

            loop {
                if state.check(&TokenKind::Delimiter(Delimiter::RBrace)) {
                    state.consume();
                    left = Expr {
                        kind: ExprKind::Object(obj),
                        span: state.current_offset(),
                    };
                    break;
                } // 支持为空

                let key: String = if let TokenKind::Identifier(s) = state.peek().kind.clone() {
                    state.consume();
                    // `{name: 1}` 里的 `name` 在语法层就降成字符串 key，避免后面被当变量求值。
                    s
                } else if let TokenKind::String(s) = state.peek().kind.clone() {
                    state.consume();
                    s
                } else {
                    return Err(ParseError::new(
                        state.current_offset(),
                        format!(
                            "expected object field name, found {}",
                            state.peek().kind.describe()
                        ),
                    ));
                };

                // 中间的冒号
                if matches!(
                    state.peek().kind.clone(),
                    TokenKind::Delimiter(Delimiter::Colon)
                ) {
                    state.consume();
                } else {
                    return Err(ParseError::new(
                        state.current_offset(),
                        format!("expected ':', found {}", state.peek().kind.describe()),
                    ));
                }

                let value = pratt_parser(state, 0)?;

                obj.push((key, value));

                if state.check(&TokenKind::Delimiter(Delimiter::Comma)) {
                    state.consume();
                } else if state.check(&TokenKind::Delimiter(Delimiter::RBrace)) {
                    state.consume();
                    left = Expr {
                        kind: ExprKind::Object(obj),
                        span: state.current_offset(),
                    };
                    break;
                } else {
                    return Err(parse_error(state, "expected ',' or '}'"));
                }
            }
        }

        _ => {
            return Err(parse_error(
                state,
                format!(
                    "expected expression, found {}",
                    state.peek().kind.describe()
                ),
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
                        // 这里是针对左结合运算符设置的
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
            // 以下是后缀位置
            TokenKind::Delimiter(Delimiter::Dot) => {
                // `.` 对象字段访问，是左结合，优先级很高
                let bp = 150;
                if bp <= min_bp {
                    break;
                }
                state.consume();
                if let TokenKind::Identifier(name) = state.peek().kind.clone() {
                    state.consume();
                    left = Expr {
                        kind: ExprKind::Field(Box::new(left), name),
                        span: state.current_offset(),
                    }
                } else {
                    return Err(ParseError::new(
                        state.current_offset(),
                        format!(
                            "expected field name after '.', found {}",
                            state.peek().kind.describe()
                        ),
                    ));
                }
            }
            TokenKind::Delimiter(Delimiter::LBracket) => {
                // 数组索引 var[expr]
                let bp = 150;
                if bp <= min_bp {
                    break;
                }
                state.consume();
                let expr = pratt_parser(state, 0)?;
                if state.check(&TokenKind::Delimiter(Delimiter::RBracket)) {
                    state.consume();
                } else {
                    return Err(parse_error(state, "expected ']'"));
                }

                left = Expr {
                    kind: ExprKind::Index(Box::new(left), Box::new(expr)),
                    span: state.current_offset(),
                }
            }
            TokenKind::Delimiter(Delimiter::LParen) => {
                // 调用 func(a,b,c)
                // 这是一个后缀表达式，只会捕获 func_name(arg1,arg2)这种情况
                // 函数声明不在这里处理
                let bp = 150;
                if bp <= min_bp {
                    break;
                }
                state.consume();
                let mut argvs: Vec<Expr> = Vec::new();
                loop {
                    if state.check(&TokenKind::Delimiter(Delimiter::RParen)) {
                        state.consume();
                        break;
                    } // 参数列表可以为空
                    let expr = pratt_parser(state, 0)?;
                    argvs.push(expr);
                    if state.check(&TokenKind::Delimiter(Delimiter::RParen)) {
                        state.consume();
                        break;
                    } else if state.check(&TokenKind::Delimiter(Delimiter::Comma)) {
                        state.consume();
                    } else {
                        return Err(parse_error(state, "expected ',' or ')'"));
                    }
                }
                left = Expr {
                    kind: ExprKind::Call(Box::new(left), argvs),
                    span: state.current_offset(),
                }
            }
            // 区间优先级故意放得比算术/比较更低，这样 `1 + 2..10` 会先形成左边的完整表达式。
            TokenKind::Delimiter(Delimiter::DotDot) => {
                let bp = 10;
                if bp <= min_bp {
                    break;
                }
                state.consume();
                let end = pratt_parser(state, bp)?;
                left = Expr {
                    kind: ExprKind::Range(RangeExpr {
                        start: Box::new(left),
                        end: Box::new(end),
                        inclusive: false,
                    }),
                    span: state.current_offset(),
                };
            }
            // inclusive range，和 `..` 一样走表达式层；`for` 再把它特判成 ForRange 语句。
            TokenKind::Delimiter(Delimiter::DotDotEq) => {
                let bp = 10;
                if bp <= min_bp {
                    break;
                }
                state.consume();
                let end = pratt_parser(state, bp)?;
                left = Expr {
                    kind: ExprKind::Range(RangeExpr {
                        start: Box::new(left),
                        end: Box::new(end),
                        inclusive: true,
                    }),
                    span: state.current_offset(),
                };
            }
            _ => break,
        }
    }
    Ok(left)
}

fn parse_lambda(
    state: &mut TokenStream<'_>,
    params: Vec<String>,
    span: usize,
) -> Result<Expr, ParseError> {
    if state.check(&TokenKind::Delimiter(Delimiter::LBrace)) {
        let body = expect_block(state, "lambda")?;

        Ok(Expr {
            kind: ExprKind::FuncLiteral { params, body },
            span,
        })
    } else {
        let expr = parse_expr_in(state)?;

        let body = vec![Stmt {
            kind: StmtKind::Return { value: Some(expr) },
            span,
        }];
        Ok(Expr {
            kind: ExprKind::FuncLiteral { params, body },
            span,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::parse_expr;
    use crate::ecscript::ast::{Expr, ExprKind, InfixOper, Literal, PrefixOper, Stmt, StmtKind};
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
}
