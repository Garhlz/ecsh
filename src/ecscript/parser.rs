use crate::ecscript::{
    ast::Stmt,
    error::ParseError,
    lexer::{Delimiter, Token, TokenKind},
    pratt::{TokenStream, parse_expr_in},
};

pub fn parse_script(tokens: &[Token]) -> Result<Vec<Stmt>, ParseError> {
    let mut state = TokenStream::new(tokens);
    let mut result_stmts: Vec<Stmt> = Vec::new();
    while !state.check(&TokenKind::EOF) {
        let cur_stmt = parse_stmt(&mut state)?;
        result_stmts.push(cur_stmt);
    }
    Ok(result_stmts)
}

fn parse_stmt(state: &mut TokenStream<'_>) -> Result<Stmt, ParseError> {
    match state.peek().kind.clone() {
        TokenKind::Let => parse_let(state),
        TokenKind::Delimiter(Delimiter::RBrace) => Err(ParseError::new(
            state.current_offset(),
            "unexpected '}' at top level".to_string(),
        )),
        TokenKind::Identifier(_) if state.check_next(&TokenKind::Delimiter(Delimiter::Eq)) => {
            parse_assignment(state)
        }
        TokenKind::Delimiter(Delimiter::LBrace) => parse_block(state),
        // 否则尝试解析为表达式语句
        _ => parse_expr_stmt(state),
    }
}

fn parse_assignment(state: &mut TokenStream<'_>) -> Result<Stmt, ParseError> {
    let span = state.current_offset();
    if let TokenKind::Identifier(name) = state.peek().kind.clone() {
        state.consume();
        if !state.check(&TokenKind::Delimiter(Delimiter::Eq)) {
            return Err(ParseError::new(
                state.current_offset(),
                format!(
                    "expected '=' after '{}', found {}",
                    name,
                    state.peek().kind.describe()
                ),
            ));
        }
        state.consume();

        let right_value = parse_expr_in(state)?;
        expect_semicolon(state)?;
        Ok(Stmt::Assign {
            name,
            expr: right_value,
            span,
        })
    } else {
        Err(ParseError::new(
            state.current_offset(),
            format!(
                "expected variable name at start of assignment, found {}",
                state.peek().kind.describe()
            ),
        ))
    }
}

fn parse_let(state: &mut TokenStream<'_>) -> Result<Stmt, ParseError> {
    let span = state.current_offset();
    state.consume(); // consume Let
    let TokenKind::Identifier(name) = state.peek().kind.clone() else {
        return Err(ParseError::new(
            state.current_offset(),
            format!(
                "expected variable name after 'let', found {}",
                state.peek().kind.describe()
            ),
        ));
    };
    state.consume();
    if !state.check(&TokenKind::Delimiter(Delimiter::Eq)) {
        return Err(ParseError::new(
            state.current_offset(),
            format!(
                "expected '=' after 'let {}', found {}",
                name,
                state.peek().kind.describe()
            ),
        ));
    }
    state.consume();

    let right_value = parse_expr_in(state)?;
    expect_semicolon(state)?;
    Ok(Stmt::Let {
        name,
        expr: right_value,
        span,
    })
}

fn parse_block(state: &mut TokenStream<'_>) -> Result<Stmt, ParseError> {
    let span = state.current_offset();
    state.consume(); // consume '{'
    let mut block_stmts: Vec<Stmt> = Vec::new();
    loop {
        if matches!(state.peek().kind, TokenKind::Delimiter(Delimiter::RBrace)) {
            state.consume();
            break;
        }
        if matches!(state.peek().kind, TokenKind::EOF) {
            return Err(ParseError::new(
                state.current_offset(),
                "unterminated block, expected '}' before end of input".to_string(),
            ));
        }
        let cur_stmt = parse_stmt(state)?;
        block_stmts.push(cur_stmt);
    }
    Ok(Stmt::Block {
        stmts: block_stmts,
        span,
    })
}

fn parse_expr_stmt(state: &mut TokenStream<'_>) -> Result<Stmt, ParseError> {
    let span = state.current_offset();
    let cur_expr = parse_expr_in(state)?;
    expect_semicolon(state)?;
    Ok(Stmt::ExprStmt {
        expr: cur_expr,
        span,
    })
}

fn expect_semicolon(state: &mut TokenStream<'_>) -> Result<(), ParseError> {
    if !state.check(&TokenKind::Delimiter(Delimiter::Semicolon)) {
        return Err(ParseError::new(
            state.current_offset(),
            format!(
                "expected ';' after statement, found {}",
                state.peek().kind.describe()
            ),
        ));
    }
    state.consume();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_script;
    use crate::ecscript::{
        ast::{Expr, ExprKind, InfixOper, Literal, Stmt},
        lexer::tokenize,
    };

    fn parse_src(src: &str) -> Vec<Stmt> {
        let tokens = tokenize(src).unwrap();
        parse_script(&tokens).unwrap()
    }

    fn assert_parse_error(src: &str, offset: usize, message: &str) {
        let tokens = tokenize(src).unwrap();
        let err = parse_script(&tokens).unwrap_err();
        assert_eq!(err.offset, offset);
        assert_eq!(err.message, message);
    }

    fn lit_int(n: i64) -> Expr {
        Expr {
            kind: ExprKind::Literal(Literal::Int(n)),
            span: 0,
        }
    }

    fn var(name: &str) -> Expr {
        Expr {
            kind: ExprKind::Variable(name.to_string()),
            span: 0,
        }
    }

    fn expr_add(left: Expr, right: Expr) -> Expr {
        Expr {
            kind: ExprKind::Infix(Box::new(left), InfixOper::Add, Box::new(right)),
            span: 0,
        }
    }

    // ── let 语句 ─────────────────────────────────────────

    #[test]
    fn parses_let_statement() {
        assert_eq!(
            parse_src("let x = 42;"),
            vec![Stmt::Let {
                name: "x".into(),
                expr: lit_int(42),
                span: 0
            }]
        );
    }

    #[test]
    fn reports_let_without_name() {
        assert_parse_error(
            "let = 5;",
            5,
            "expected variable name after 'let', found '='",
        );
    }

    #[test]
    fn reports_let_without_eq() {
        assert_parse_error(
            "let x 5;",
            7,
            "expected '=' after 'let x', found integer literal",
        );
    }

    // ── assign 语句 ──────────────────────────────────────

    #[test]
    fn parses_assign_statement() {
        assert_eq!(
            parse_src("x = 10;"),
            vec![Stmt::Assign {
                name: "x".into(),
                expr: lit_int(10),
                span: 0
            }]
        );
    }

    #[test]
    fn parses_assign_with_expression() {
        assert_eq!(
            parse_src("y = 1 + 2;"),
            vec![Stmt::Assign {
                name: "y".into(),
                expr: expr_add(lit_int(1), lit_int(2)),
                span: 0,
            }]
        );
    }

    // ── 表达式语句 ───────────────────────────────────────

    #[test]
    fn parses_expr_stmt_literal() {
        assert_eq!(
            parse_src("42;"),
            vec![Stmt::ExprStmt {
                expr: lit_int(42),
                span: 0,
            }]
        );
    }

    #[test]
    fn parses_expr_stmt_variable() {
        assert_eq!(
            parse_src("x;"),
            vec![Stmt::ExprStmt {
                expr: var("x"),
                span: 0,
            }]
        );
    }

    #[test]
    fn parses_expr_stmt_with_identifier_prefix() {
        assert_eq!(
            parse_src("x + 1;"),
            vec![Stmt::ExprStmt {
                expr: expr_add(var("x"), lit_int(1)),
                span: 0,
            }]
        );
    }

    // ── 错误：缺少分号 ───────────────────────────────────

    #[test]
    fn reports_missing_semicolon() {
        assert_parse_error(
            "let x = 1 let y = 2",
            13,
            "expected ';' after statement, found keyword 'let'",
        );
    }

    #[test]
    fn reports_missing_semicolon_in_block() {
        assert_parse_error(
            "{ let x = 1 let y = 2 }",
            15,
            "expected ';' after statement, found keyword 'let'",
        );
    }

    // ── block ────────────────────────────────────────────

    #[test]
    fn parses_empty_block() {
        assert_eq!(
            parse_src("{}"),
            vec![Stmt::Block {
                stmts: vec![],
                span: 0,
            }]
        );
    }

    #[test]
    fn parses_block_with_statements() {
        let stmts = parse_src("{ let x = 1; x = 2; 3; }");
        assert_eq!(stmts.len(), 1);
        if let Stmt::Block { ref stmts, .. } = stmts[0] {
            assert_eq!(stmts.len(), 3);
            assert!(matches!(stmts[0], Stmt::Let { .. }));
            assert!(matches!(stmts[1], Stmt::Assign { .. }));
            assert!(matches!(stmts[2], Stmt::ExprStmt { .. }));
        } else {
            panic!("expected block");
        }
    }

    #[test]
    fn parses_nested_blocks() {
        let stmts = parse_src("{ { 1; } }");
        assert_eq!(stmts.len(), 1);
        if let Stmt::Block { ref stmts, .. } = stmts[0] {
            assert_eq!(stmts.len(), 1);
            if let Stmt::Block {
                stmts: ref inner, ..
            } = stmts[0]
            {
                assert_eq!(inner.len(), 1);
                assert!(matches!(inner[0], Stmt::ExprStmt { .. }));
            } else {
                panic!("expected inner block");
            }
        } else {
            panic!("expected outer block");
        }
    }

    #[test]
    fn reports_unterminated_block() {
        assert_parse_error(
            "{",
            1,
            "unterminated block, expected '}' before end of input",
        );
    }

    // ── 多条语句 ─────────────────────────────────────────

    #[test]
    fn parses_multiple_statements() {
        let stmts = parse_src("let x = 1; let y = 2;");
        assert_eq!(stmts.len(), 2);
        assert!(matches!(stmts[0], Stmt::Let { .. }));
        assert!(matches!(stmts[1], Stmt::Let { .. }));
    }

    #[test]
    fn parses_two_assigns() {
        let stmts = parse_src("x = 1; y = 2;");
        assert_eq!(stmts.len(), 2);
        assert!(matches!(stmts[0], Stmt::Assign { .. }));
        assert!(matches!(stmts[1], Stmt::Assign { .. }));
    }

    #[test]
    fn reports_missing_final_semicolon() {
        assert_parse_error(
            "let x = 1",
            9,
            "expected ';' after statement, found end of input",
        );
    }

    #[test]
    fn reports_missing_final_semicolon_in_block() {
        assert_parse_error("{ 1 }", 5, "expected ';' after statement, found '}'");
    }

    #[test]
    fn reports_assign_missing_rhs() {
        assert_parse_error("x = ;", 5, "expected expression, found ';'");
    }

    #[test]
    fn reports_unexpected_top_level_rbrace() {
        assert_parse_error("}", 1, "unexpected '}' at top level");
    }

    #[test]
    fn records_let_statement_span_at_keyword() {
        let stmts = parse_src("let x = 42;");
        assert_eq!(stmts[0].span(), 3);
    }

    #[test]
    fn records_assign_statement_span_at_identifier() {
        let stmts = parse_src("x = 42;");
        assert_eq!(stmts[0].span(), 1);
    }

    #[test]
    fn records_expr_statement_span_at_expression_start() {
        let stmts = parse_src("-1 + 2;");
        assert_eq!(stmts[0].span(), 1);
    }

    #[test]
    fn records_block_statement_span_at_left_brace() {
        let stmts = parse_src("{ 1; }");
        assert_eq!(stmts[0].span(), 1);
    }

    #[test]
    fn reports_double_semicolon() {
        assert_parse_error(";;", 1, "expected expression, found ';'");
    }
}
