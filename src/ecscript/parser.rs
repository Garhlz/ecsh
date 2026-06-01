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
    // 顶层按“语句序列”解析，换行在这里和 block 内都被当作天然分隔符跳过。
    while !state.check(&TokenKind::EOF) {
        let cur_stmt = parse_stmt(&mut state)?;
        result_stmts.push(cur_stmt);
        state.skip_newlines();
    }
    Ok(result_stmts)
}

/// 把token流转换成单一语句
fn parse_stmt(state: &mut TokenStream<'_>) -> Result<Stmt, ParseError> {
    // 这里先按首 token 做语句级分发；
    // 真正的表达式优先级和结合性都交给 Pratt parser 处理。
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
    // 这里故意先完整解析左侧表达式，再看下一个 token 是否是赋值符。
    // 这样 `obj.x = 1` / `arr[i] += 2` 能先复用表达式解析，再降格成可赋值目标。
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
    // 赋值符在语法层统一归一化，后续语义层无需再关心具体 token 形式。
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
    // `use` 路径不是普通表达式，而是把一串受限 token 直接拼成模块路径文本，
    // 直到遇到 `as` 为止，例如 `./foo-bar.ecs as foo`。
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
    // 这里显式列出允许出现在模块路径中的 token，避免误把任意表达式语法放进 `use`。
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
        // block 内允许空行；右花括号只负责结束当前 block，不会交给外层消费。
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
        // 连续的 `;` 或换行都会被吃掉；
        // 但如果一条语句一开始就是 `;`，那会在更早的位置报“expected expression”。
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
        // `else if` 不单独建语法节点，而是复用 `if` 语句递归嵌套进 else_body。
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
    // `for` 先统一解析 `in` 右侧表达式，再根据 AST 形状区分 range 迭代和普通 iterable。
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
        // 这里只接受最简单的参数名列表，不在语法层处理类型、默认值等更复杂形式。
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
    // `return` 的值是可选的：如果后面直接遇到语句终止符，就视为裸 return。
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
    // 控制流和函数体都强制要求 block，这样能避免单行语句体带来的悬挂歧义。
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
#[path = "parser_tests.rs"]
mod tests;
