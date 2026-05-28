use crate::ecscript::{
    ast::{CompoundAssignOp, Stmt, StmtKind, expr_to_assign_target},
    error::ParseError,
    lexer::{Delimiter, Keyword, Token, TokenKind},
    pratt::{TokenStream, parse_expr_in},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AssignmentKind {
    Simple,
    Compound(CompoundAssignOp),
}

pub fn parse_script(tokens: &[Token]) -> Result<Vec<Stmt>, ParseError> {
    let mut state = TokenStream::new(tokens);
    let mut result_stmts: Vec<Stmt> = Vec::new();
    state.skip_newlines();
    while !state.check(&TokenKind::EOF) {
        let cur_stmt = parse_stmt(&mut state)?;
        result_stmts.push(cur_stmt);
        state.skip_newlines();
    }
    Ok(result_stmts)
}

/// 把token流转换成单一语句
fn parse_stmt(state: &mut TokenStream<'_>) -> Result<Stmt, ParseError> {
    match state.peek().kind.clone() {
        TokenKind::Keyword(keyword) => match keyword {
            Keyword::Let => parse_let(state, false),
            Keyword::If => parse_if(state),
            Keyword::While => parse_while(state),
            Keyword::For => parse_for(state),
            Keyword::Use => parse_use(state),
            Keyword::Break => parse_break(state),
            Keyword::Continue => parse_continue(state),
            Keyword::Return => parse_return(state),
            Keyword::Func => parse_func(state, false),
            Keyword::Pub => {
                state.consume();
                if matches!(state.peek().kind, TokenKind::Keyword(Keyword::Let)) {
                    parse_let(state, true)
                } else if matches!(state.peek().kind, TokenKind::Keyword(Keyword::Func)) {
                    parse_func(state, true)
                } else {
                    Err(ParseError::new(
                        state.current_offset(),
                        "expect 'let' or 'func' after 'pub'",
                    ))
                }
            }
            _ => Err(ParseError::new(
                state.current_offset(),
                "unexpected 'in' or 'else' at first place of top level".to_string(),
            )),
        },

        TokenKind::Delimiter(Delimiter::RBrace) => Err(ParseError::new(
            state.current_offset(),
            "unexpected '}' at top level".to_string(),
        )),
        TokenKind::Delimiter(Delimiter::LBrace) => parse_block(state),

        _ => parse_assignment_or_expr_stmt(state),
    }
}

fn parse_assignment_or_expr_stmt(state: &mut TokenStream<'_>) -> Result<Stmt, ParseError> {
    let span = state.current_offset();
    let left_expr = parse_expr_in(state)?;
    if let Some(assignment_kind) = assignment_kind(&state.peek().kind) {
        let target = expr_to_assign_target(&left_expr).ok_or_else(|| {
            ParseError::new(
                left_expr.span,
                "invalid assignment target; expected variable, field access, or index access"
                    .to_string(),
            )
        })?;
        state.consume();
        let right_value = parse_expr_in(state)?;

        expect_semicolon(state)?;

        let kind = match assignment_kind {
            AssignmentKind::Simple => StmtKind::Assign {
                target,
                expr: right_value,
            },
            AssignmentKind::Compound(op) => StmtKind::CompoundAssign {
                target,
                op,
                expr: right_value,
            },
        };

        Ok(Stmt { kind, span })
    } else {
        expect_semicolon(state)?;
        Ok(Stmt {
            kind: StmtKind::ExprStmt { expr: left_expr },
            span,
        })
    }
}

fn assignment_kind(token_kind: &TokenKind) -> Option<AssignmentKind> {
    match token_kind {
        TokenKind::Delimiter(Delimiter::Eq) => Some(AssignmentKind::Simple),
        TokenKind::Delimiter(Delimiter::PlusEq) => {
            Some(AssignmentKind::Compound(CompoundAssignOp::Add))
        }
        TokenKind::Delimiter(Delimiter::MinusEq) => {
            Some(AssignmentKind::Compound(CompoundAssignOp::Sub))
        }
        TokenKind::Delimiter(Delimiter::StarEq) => {
            Some(AssignmentKind::Compound(CompoundAssignOp::Mul))
        }
        TokenKind::Delimiter(Delimiter::SlashEq) => {
            Some(AssignmentKind::Compound(CompoundAssignOp::Div))
        }
        TokenKind::Delimiter(Delimiter::PercentEq) => {
            Some(AssignmentKind::Compound(CompoundAssignOp::Mod))
        }
        _ => None,
    }
}

fn parse_let(state: &mut TokenStream<'_>, public: bool) -> Result<Stmt, ParseError> {
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
    Ok(Stmt {
        kind: StmtKind::Let {
            name,
            expr: right_value,
            public,
        },
        span,
    })
}

fn parse_use(state: &mut TokenStream<'_>) -> Result<Stmt, ParseError> {
    let span = state.current_offset();
    state.consume(); // consume 'use'

    let mut path = String::new();
    while !matches!(
        state.peek().kind,
        TokenKind::Keyword(Keyword::As) | TokenKind::EOF
    ) {
        path.push_str(&use_path_token_text(state.peek())?);
        state.consume();
    }

    if path.is_empty() {
        return Err(ParseError::new(
            state.current_offset(),
            "expected module path after 'use'",
        ));
    }

    if !matches!(state.peek().kind, TokenKind::Keyword(Keyword::As)) {
        return Err(ParseError::new(
            state.current_offset(),
            "expected 'as' after module path",
        ));
    }
    state.consume(); // consume 'as'

    let TokenKind::Identifier(alias) = state.peek().kind.clone() else {
        return Err(ParseError::new(
            state.current_offset(),
            format!(
                "expected alias identifier after 'as', found {}",
                state.peek().kind.describe()
            ),
        ));
    };
    state.consume();
    expect_semicolon(state)?;

    Ok(Stmt {
        kind: StmtKind::Use { path, alias },
        span,
    })
}

fn use_path_token_text(token: &Token) -> Result<String, ParseError> {
    match &token.kind {
        TokenKind::Identifier(name) => Ok(name.clone()),
        TokenKind::String(text) => Ok(text.clone()),
        TokenKind::Delimiter(Delimiter::Dot) => Ok(".".into()),
        TokenKind::Delimiter(Delimiter::DotDot) => Ok("..".into()),
        TokenKind::Operator(crate::ecscript::lexer::Operator::Slash) => Ok("/".into()),
        TokenKind::Operator(crate::ecscript::lexer::Operator::Minus) => Ok("-".into()),
        other => Err(ParseError::new(
            token.start,
            format!("invalid token {} in module path", other.describe()),
        )),
    }
}

fn parse_block(state: &mut TokenStream<'_>) -> Result<Stmt, ParseError> {
    let span = state.current_offset();
    state.consume(); // consume '{'
    let mut block_stmts: Vec<Stmt> = Vec::new();
    loop {
        state.skip_newlines();
        if matches!(state.peek().kind, TokenKind::Delimiter(Delimiter::RBrace)) {
            state.consume();
            break;
        }
        if matches!(state.peek().kind, TokenKind::EOF) {
            return Err(ParseError::incomplete(
                state.current_offset(),
                "unterminated block, expected '}' before end of input".to_string(),
            ));
        }
        let cur_stmt = parse_stmt(state)?;
        block_stmts.push(cur_stmt);
    }
    Ok(Stmt {
        kind: StmtKind::Block { stmts: block_stmts },
        span,
    })
}

fn expect_semicolon(state: &mut TokenStream<'_>) -> Result<(), ParseError> {
    // 分号现在只作为同一行拆分多语句的显式分隔符。
    if matches!(
        state.peek().kind,
        TokenKind::Delimiter(Delimiter::Semicolon) | TokenKind::Newline
    ) {
        while matches!(
            state.peek().kind,
            TokenKind::Delimiter(Delimiter::Semicolon) | TokenKind::Newline
        ) {
            state.consume();
        }
        return Ok(());
    }
    if matches!(
        state.peek().kind,
        TokenKind::EOF | TokenKind::Delimiter(Delimiter::RBrace)
    ) {
        return Ok(());
    }
    let next = &state.peek().kind;
    let message = if next.can_start_expr() {
        format!(
            "expected operator or ';' after expression, found {}",
            next.describe()
        )
    } else {
        format!("expected ';' after statement, found {}", next.describe())
    };
    Err(ParseError::new(state.current_offset(), message))
}

fn parse_if(state: &mut TokenStream<'_>) -> Result<Stmt, ParseError> {
    let span = state.current_offset();
    state.consume();
    let cond = parse_expr_in(state)?;
    let then_body = expect_block(state, "if")?;

    if state.check(&TokenKind::Keyword(Keyword::Else)) {
        state.consume();
        let else_body = if state.check(&TokenKind::Keyword(Keyword::If)) {
            vec![parse_if(state)?]
        } else {
            expect_block(state, "else")?
        };

        Ok(Stmt {
            kind: StmtKind::If {
                cond,
                then_body,
                else_body,
            },
            span,
        })
    } else {
        Ok(Stmt {
            kind: StmtKind::If {
                cond,
                then_body,
                else_body: Vec::new(),
            },
            span,
        })
    }
}

fn parse_while(state: &mut TokenStream<'_>) -> Result<Stmt, ParseError> {
    let span = state.current_offset();
    state.consume();
    let cond = parse_expr_in(state)?;
    let body = expect_block(state, "while")?;
    Ok(Stmt {
        kind: StmtKind::While { cond, body },
        span,
    })
}

fn parse_for(state: &mut TokenStream<'_>) -> Result<Stmt, ParseError> {
    let span = state.current_offset();
    state.consume();
    let TokenKind::Identifier(var) = state.peek().kind.clone() else {
        return Err(ParseError::new(
            state.current_offset(),
            format!(
                "expected variable name after for, found {}",
                state.peek().kind.describe()
            ),
        ));
    };
    state.consume();

    let TokenKind::Keyword(Keyword::In) = state.peek().kind.clone() else {
        return Err(ParseError::new(
            state.current_offset(),
            format!(
                "expected `in` after for, found {}",
                state.peek().kind.describe()
            ),
        ));
    };
    state.consume();

    let expr = parse_expr_in(state)?;
    match expr.kind {
        crate::ecscript::ast::ExprKind::Range(range) => {
            let body = expect_block(state, "for")?;
            Ok(Stmt {
                kind: StmtKind::ForRange { var, range, body },
                span,
            })
        }
        _ => {
            let body = expect_block(state, "for")?;
            Ok(Stmt {
                kind: StmtKind::ForIn {
                    var,
                    iterable: expr,
                    body,
                },
                span,
            })
        }
    }
}

/// 这里parse出来的是函数声明的语句
fn parse_func(state: &mut TokenStream<'_>, public: bool) -> Result<Stmt, ParseError> {
    let span = state.current_offset();
    state.consume(); // "func" token
    let TokenKind::Identifier(name) = state.peek().kind.clone() else {
        return Err(ParseError::new(
            state.current_offset(),
            format!(
                "expected function variable name after func, found {}",
                state.peek().kind.describe()
            ),
        ));
    };
    state.consume(); // 函数名
    if !state.check(&TokenKind::Delimiter(Delimiter::LParen)) {
        return Err(ParseError::new(
            state.current_offset(),
            format!(
                "expected '(' after function name, found {}",
                state.peek().kind.describe()
            ),
        ));
    }
    state.consume(); // 左括号
    let mut params = Vec::new();

    loop {
        // 括号中可以为空
        if matches!(state.peek().kind, TokenKind::Delimiter(Delimiter::RParen)) {
            state.consume();
            break;
        }
        // 注意这里parse出来的是函数定义语句 func test(a,b){}
        let TokenKind::Identifier(name) = state.peek().kind.clone() else {
            return Err(ParseError::new(
                state.current_offset(),
                "expected param name string in function declare params",
            ));
        };
        state.consume();
        params.push(name);
        if matches!(state.peek().kind, TokenKind::Delimiter(Delimiter::RParen)) {
            state.consume();
            break;
        } else if matches!(state.peek().kind, TokenKind::Delimiter(Delimiter::Comma)) {
            state.consume();
        } else {
            return Err(ParseError::new(
                state.current_offset(),
                "expected comma of right paren in function params",
            ));
        }
    }

    let body = expect_block(state, "func")?;

    Ok(Stmt {
        kind: StmtKind::FuncDeclare {
            name,
            params,
            body,
            public,
        },
        span,
    })
}

fn parse_break(state: &mut TokenStream<'_>) -> Result<Stmt, ParseError> {
    let span = state.current_offset();
    state.consume();
    expect_semicolon(state)?;
    Ok(Stmt {
        kind: StmtKind::Break,
        span,
    })
}

fn parse_continue(state: &mut TokenStream<'_>) -> Result<Stmt, ParseError> {
    let span = state.current_offset();
    state.consume();
    expect_semicolon(state)?;
    Ok(Stmt {
        kind: StmtKind::Continue,
        span,
    })
}

fn parse_return(state: &mut TokenStream<'_>) -> Result<Stmt, ParseError> {
    let span = state.current_offset();
    state.consume();
    if matches!(
        state.peek().kind,
        TokenKind::Delimiter(Delimiter::Semicolon)
            | TokenKind::Newline
            | TokenKind::Delimiter(Delimiter::RBrace)
            | TokenKind::EOF
    ) {
        while matches!(
            state.peek().kind,
            TokenKind::Delimiter(Delimiter::Semicolon) | TokenKind::Newline
        ) {
            state.consume();
        }
        Ok(Stmt {
            kind: StmtKind::Return { value: None },
            span,
        })
    } else {
        let return_expr = parse_expr_in(state)?;
        expect_semicolon(state)?;
        Ok(Stmt {
            kind: StmtKind::Return {
                value: Some(return_expr),
            },
            span,
        })
    }
}

pub fn expect_block(state: &mut TokenStream<'_>, name: &str) -> Result<Vec<Stmt>, ParseError> {
    state.skip_newlines();
    if !state.check(&TokenKind::Delimiter(Delimiter::LBrace)) {
        let incomplete = matches!(state.peek().kind, TokenKind::EOF);
        let mk = if incomplete {
            ParseError::incomplete
        } else {
            ParseError::new
        };
        return Err(mk(
            state.current_offset(),
            format!(
                "expected '{{' after {name}, found {}",
                state.peek().kind.describe()
            ),
        ));
    }
    let Stmt {
        kind: StmtKind::Block { stmts: body, .. },
        ..
    } = parse_block(state)?
    else {
        return Err(ParseError::new(
            state.current_offset(),
            format!("expected block after {name}"),
        ));
    };
    Ok(body)
}
#[cfg(test)]
mod tests {
    use super::parse_script;
    use crate::ecscript::{
        ast::{AssignTarget, CompoundAssignOp, Expr, ExprKind, InfixOper, Literal, Stmt, StmtKind},
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

    fn lit_bool(value: bool) -> Expr {
        Expr {
            kind: ExprKind::Literal(Literal::Bool(value)),
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
            vec![Stmt {
                kind: StmtKind::Let {
                    name: "x".into(),
                    expr: lit_int(42),
                    public: false,
                },
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

    #[test]
    fn accepts_missing_semicolon_at_eof() {
        assert_eq!(
            parse_src("let x = 42"),
            vec![Stmt {
                kind: StmtKind::Let {
                    name: "x".into(),
                    expr: lit_int(42),
                    public: false,
                },
                span: 0
            }]
        );
    }

    // ── assign 语句 ──────────────────────────────────────

    #[test]
    fn parses_assign_statement() {
        assert_eq!(
            parse_src("x = 10;"),
            vec![Stmt {
                kind: StmtKind::Assign {
                    target: AssignTarget::Name("x".into()),
                    expr: lit_int(10),
                },
                span: 0
            }]
        );
    }

    #[test]
    fn parses_assign_with_expression() {
        assert_eq!(
            parse_src("y = 1 + 2;"),
            vec![Stmt {
                kind: StmtKind::Assign {
                    target: AssignTarget::Name("y".into()),
                    expr: expr_add(lit_int(1), lit_int(2)),
                },
                span: 0,
            }]
        );
    }

    #[test]
    fn parses_field_assign_statement() {
        let stmts = parse_src("obj.name = 10;");
        assert_eq!(stmts.len(), 1);
        match &stmts[0].kind {
            StmtKind::Assign {
                target: AssignTarget::Field { object, field },
                expr,
            } => {
                assert_eq!(*object, var("obj"));
                assert_eq!(field, "name");
                assert_eq!(*expr, lit_int(10));
            }
            other => panic!("expected field assign, got {:?}", other),
        }
    }

    #[test]
    fn parses_index_assign_statement() {
        let stmts = parse_src("arr[i] = 10;");
        assert_eq!(stmts.len(), 1);
        match &stmts[0].kind {
            StmtKind::Assign {
                target: AssignTarget::Index { object, index },
                expr,
            } => {
                assert_eq!(*object, var("arr"));
                assert_eq!(*index, var("i"));
                assert_eq!(*expr, lit_int(10));
            }
            other => panic!("expected index assign, got {:?}", other),
        }
    }

    #[test]
    fn parses_compound_assign_statement() {
        assert_eq!(
            parse_src("x += 10;"),
            vec![Stmt {
                kind: StmtKind::CompoundAssign {
                    target: AssignTarget::Name("x".into()),
                    op: CompoundAssignOp::Add,
                    expr: lit_int(10),
                },
                span: 1
            }]
        );
    }

    #[test]
    fn parses_field_compound_assign_statement() {
        let stmts = parse_src("obj.name *= 10;");
        assert_eq!(stmts.len(), 1);
        match &stmts[0].kind {
            StmtKind::CompoundAssign {
                target: AssignTarget::Field { object, field },
                op,
                expr,
            } => {
                assert_eq!(*object, var("obj"));
                assert_eq!(field, "name");
                assert_eq!(*op, CompoundAssignOp::Mul);
                assert_eq!(*expr, lit_int(10));
            }
            other => panic!("expected field compound assign, got {:?}", other),
        }
    }

    #[test]
    fn parses_index_compound_assign_statement() {
        let stmts = parse_src("arr[i] %= 10;");
        assert_eq!(stmts.len(), 1);
        match &stmts[0].kind {
            StmtKind::CompoundAssign {
                target: AssignTarget::Index { object, index },
                op,
                expr,
            } => {
                assert_eq!(*object, var("arr"));
                assert_eq!(*index, var("i"));
                assert_eq!(*op, CompoundAssignOp::Mod);
                assert_eq!(*expr, lit_int(10));
            }
            other => panic!("expected index compound assign, got {:?}", other),
        }
    }

    // ── 表达式语句 ───────────────────────────────────────

    #[test]
    fn parses_expr_stmt_literal() {
        assert_eq!(
            parse_src("42;"),
            vec![Stmt {
                kind: StmtKind::ExprStmt { expr: lit_int(42) },
                span: 0,
            }]
        );
    }

    #[test]
    fn parses_expr_stmt_variable() {
        assert_eq!(
            parse_src("x;"),
            vec![Stmt {
                kind: StmtKind::ExprStmt { expr: var("x") },
                span: 0,
            }]
        );
    }

    #[test]
    fn parses_expr_stmt_with_identifier_prefix() {
        assert_eq!(
            parse_src("x + 1;"),
            vec![Stmt {
                kind: StmtKind::ExprStmt {
                    expr: expr_add(var("x"), lit_int(1)),
                },
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
            vec![Stmt {
                kind: StmtKind::Block { stmts: vec![] },
                span: 0,
            }]
        );
    }

    #[test]
    fn parses_block_with_statements() {
        let stmts = parse_src("{ let x = 1; x = 2; 3; }");
        assert_eq!(stmts.len(), 1);
        if let StmtKind::Block { stmts, .. } = &stmts[0].kind {
            assert_eq!(stmts.len(), 3);
            assert!(matches!(stmts[0].kind, StmtKind::Let { .. }));
            assert!(matches!(stmts[1].kind, StmtKind::Assign { .. }));
            assert!(matches!(stmts[2].kind, StmtKind::ExprStmt { .. }));
        } else {
            panic!("expected block");
        }
    }

    #[test]
    fn parses_nested_blocks() {
        let stmts = parse_src("{ { 1; } }");
        assert_eq!(stmts.len(), 1);
        if let StmtKind::Block { stmts, .. } = &stmts[0].kind {
            assert_eq!(stmts.len(), 1);
            if let StmtKind::Block { stmts: inner, .. } = &stmts[0].kind {
                assert_eq!(inner.len(), 1);
                assert!(matches!(inner[0].kind, StmtKind::ExprStmt { .. }));
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
        assert!(matches!(stmts[0].kind, StmtKind::Let { .. }));
        assert!(matches!(stmts[1].kind, StmtKind::Let { .. }));
    }

    #[test]
    fn parses_newline_separated_statements() {
        let stmts = parse_src("let x = 1\nlet y = 2\nx + y\n");
        assert_eq!(stmts.len(), 3);
        assert!(matches!(stmts[0].kind, StmtKind::Let { .. }));
        assert!(matches!(stmts[1].kind, StmtKind::Let { .. }));
        assert!(matches!(stmts[2].kind, StmtKind::ExprStmt { .. }));
    }

    #[test]
    fn parses_two_assigns() {
        let stmts = parse_src("x = 1; y = 2;");
        assert_eq!(stmts.len(), 2);
        assert!(matches!(stmts[0].kind, StmtKind::Assign { .. }));
        assert!(matches!(stmts[1].kind, StmtKind::Assign { .. }));
    }

    #[test]
    fn accepts_missing_final_semicolon() {
        let stmts = parse_src("let x = 1");
        assert_eq!(stmts.len(), 1);
        assert!(matches!(stmts[0].kind, StmtKind::Let { .. }));
    }

    #[test]
    fn accepts_missing_final_semicolon_in_block() {
        let stmts = parse_src("{ 1 }");
        assert_eq!(stmts.len(), 1);
        assert!(matches!(stmts[0].kind, StmtKind::Block { .. }));
    }

    #[test]
    fn reports_adjacent_keyword_after_number_expression() {
        assert_parse_error(
            "42 true;",
            7,
            "expected operator or ';' after expression, found keyword 'true'",
        );
    }

    #[test]
    fn reports_adjacent_string_after_number_expression() {
        assert_parse_error(
            "42\"hi\";",
            6,
            "expected operator or ';' after expression, found string literal",
        );
    }

    #[test]
    fn reports_assign_missing_rhs() {
        assert_parse_error("x = ;", 5, "expected expression, found ';'");
    }

    #[test]
    fn reports_compound_assign_missing_rhs() {
        assert_parse_error("x += ;", 6, "expected expression, found ';'");
    }

    #[test]
    fn reports_invalid_assignment_target() {
        assert_parse_error(
            "1 + 2 = 3;",
            3,
            "invalid assignment target; expected variable, field access, or index access",
        );
    }

    #[test]
    fn reports_unexpected_top_level_rbrace() {
        assert_parse_error("}", 1, "unexpected '}' at top level");
    }

    #[test]
    fn parses_postfix_after_numeric_literal_without_requiring_whitespace() {
        let stmts = parse_src("1[0]; 1.foo; 1(2);");
        assert_eq!(stmts.len(), 3);
        assert!(matches!(stmts[0].kind, StmtKind::ExprStmt { .. }));
        assert!(matches!(stmts[1].kind, StmtKind::ExprStmt { .. }));
        assert!(matches!(stmts[2].kind, StmtKind::ExprStmt { .. }));
    }

    #[test]
    fn records_let_statement_span_at_keyword() {
        let stmts = parse_src("let x = 42;");
        assert_eq!(stmts[0].span, 3);
    }

    #[test]
    fn records_assign_statement_span_at_identifier() {
        let stmts = parse_src("x = 42;");
        assert_eq!(stmts[0].span, 1);
    }

    #[test]
    fn records_compound_assign_statement_span_at_identifier() {
        let stmts = parse_src("x += 42;");
        assert_eq!(stmts[0].span, 1);
    }

    #[test]
    fn records_expr_statement_span_at_expression_start() {
        let stmts = parse_src("-1 + 2;");
        assert_eq!(stmts[0].span, 1);
    }

    #[test]
    fn records_block_statement_span_at_left_brace() {
        let stmts = parse_src("{ 1; }");
        assert_eq!(stmts[0].span, 1);
    }

    #[test]
    fn reports_double_semicolon() {
        assert_parse_error(";;", 1, "expected expression, found ';'");
    }

    // ── 控制流 ────────────────────────────────────────────

    #[test]
    fn parses_if_without_else() {
        assert_eq!(
            parse_src("if true { 1; }"),
            vec![Stmt {
                kind: StmtKind::If {
                    cond: lit_bool(true),
                    then_body: vec![Stmt {
                        kind: StmtKind::ExprStmt { expr: lit_int(1) },
                        span: 0,
                    }],
                    else_body: vec![],
                },
                span: 0,
            }]
        );
    }

    #[test]
    fn parses_if_else() {
        assert_eq!(
            parse_src("if true { 1; } else { 2; }"),
            vec![Stmt {
                kind: StmtKind::If {
                    cond: lit_bool(true),
                    then_body: vec![Stmt {
                        kind: StmtKind::ExprStmt { expr: lit_int(1) },
                        span: 0,
                    }],
                    else_body: vec![Stmt {
                        kind: StmtKind::ExprStmt { expr: lit_int(2) },
                        span: 0,
                    }],
                },
                span: 0,
            }]
        );
    }

    #[test]
    fn parses_if_else_if_chain() {
        let stmts = parse_src("if false { 1; } else if true { 2; } else { 3; }");
        assert_eq!(stmts.len(), 1);
    }

    #[test]
    fn parses_while_loop() {
        let stmts = parse_src("while true { 1; }");
        assert_eq!(stmts.len(), 1);
        assert!(matches!(stmts[0].kind, StmtKind::While { .. }));
    }

    #[test]
    fn parses_use_with_relative_path_and_alias() {
        let stmts = parse_src("use ./foo.ecs as foo");
        assert_eq!(
            stmts,
            vec![Stmt {
                kind: StmtKind::Use {
                    path: "./foo.ecs".into(),
                    alias: "foo".into(),
                },
                span: 0,
            }]
        );
    }

    #[test]
    fn parses_for_range() {
        let stmts = parse_src("for i in 0..10 { 1; }");
        assert_eq!(stmts.len(), 1);
        assert!(matches!(stmts[0].kind, StmtKind::ForRange { .. }));
    }

    #[test]
    fn parses_for_inclusive_range() {
        let stmts = parse_src("for i in 0..=5 { 1; }");
        assert_eq!(stmts.len(), 1);
        assert!(matches!(stmts[0].kind, StmtKind::ForRange { .. }));
    }

    #[test]
    fn parses_for_in_array() {
        let stmts = parse_src("for v in a { 1; }");
        assert_eq!(stmts.len(), 1);
        assert!(matches!(stmts[0].kind, StmtKind::ForIn { .. }));
    }

    #[test]
    fn records_if_statement_span_at_keyword() {
        let stmts = parse_src("if true { 1; }");
        assert_eq!(stmts[0].span, 2);
    }

    #[test]
    fn records_while_statement_span_at_keyword() {
        let stmts = parse_src("while true { 1; }");
        assert_eq!(stmts[0].span, 5);
    }

    #[test]
    fn records_for_statement_span_at_keyword() {
        let stmts = parse_src("for i in 0..3 { 1; }");
        assert_eq!(stmts[0].span, 3);
    }

    #[test]
    fn records_func_statement_span_at_keyword() {
        let stmts = parse_src("func f() { return; }");
        assert_eq!(stmts[0].span, 4);
    }

    #[test]
    fn records_break_statement_span_at_keyword() {
        let stmts = parse_src("break;");
        assert_eq!(stmts[0].span, 5);
    }

    #[test]
    fn records_continue_statement_span_at_keyword() {
        let stmts = parse_src("continue;");
        assert_eq!(stmts[0].span, 8);
    }

    #[test]
    fn parses_break() {
        let stmts = parse_src("break;");
        assert_eq!(stmts.len(), 1);
        assert!(matches!(stmts[0].kind, StmtKind::Break));
    }

    #[test]
    fn parses_continue() {
        let stmts = parse_src("continue;");
        assert_eq!(stmts.len(), 1);
        assert!(matches!(stmts[0].kind, StmtKind::Continue));
    }

    #[test]
    fn parses_func_declare_with_params_and_return() {
        let stmts = parse_src("func add(a, b) { return a + b; }");
        assert_eq!(stmts.len(), 1);
        match &stmts[0].kind {
            StmtKind::FuncDeclare {
                name, params, body, ..
            } => {
                assert_eq!(name, "add");
                assert_eq!(params, &vec!["a".to_string(), "b".to_string()]);
                assert_eq!(body.len(), 1);
                match &body[0].kind {
                    StmtKind::Return {
                        value: Some(expr), ..
                    } => {
                        assert_eq!(*expr, expr_add(var("a"), var("b")));
                    }
                    other => panic!("expected return statement, got {:?}", other),
                }
            }
            other => panic!("expected function declaration, got {:?}", other),
        }
    }

    #[test]
    fn parses_func_declare_with_empty_params() {
        let stmts = parse_src("func ping() { return; }");
        assert_eq!(stmts.len(), 1);
        match &stmts[0].kind {
            StmtKind::FuncDeclare {
                name, params, body, ..
            } => {
                assert_eq!(name, "ping");
                assert!(params.is_empty());
                assert_eq!(body.len(), 1);
                assert!(matches!(body[0].kind, StmtKind::Return { value: None, .. }));
            }
            other => panic!("expected function declaration, got {:?}", other),
        }
    }

    #[test]
    fn parses_return_with_value() {
        let stmts = parse_src("return 1 + 2;");
        assert_eq!(stmts.len(), 1);
        match &stmts[0].kind {
            StmtKind::Return {
                value: Some(expr), ..
            } => {
                assert_eq!(*expr, expr_add(lit_int(1), lit_int(2)));
            }
            other => panic!("expected return with value, got {:?}", other),
        }
    }

    #[test]
    fn parses_return_without_value() {
        let stmts = parse_src("return;");
        assert_eq!(stmts.len(), 1);
        assert!(matches!(stmts[0].kind, StmtKind::Return { value: None }));
    }

    #[test]
    fn reports_unterminated_if_without_block() {
        assert_parse_error(
            "if true 1;",
            9,
            "expected '{' after if, found integer literal",
        );
    }

    #[test]
    fn reports_while_without_block() {
        assert_parse_error(
            "while true 1;",
            12,
            "expected '{' after while, found integer literal",
        );
    }

    #[test]
    fn reports_for_without_block() {
        assert_parse_error(
            "for i in 0..3 1;",
            15,
            "expected '{' after for, found integer literal",
        );
    }
}
