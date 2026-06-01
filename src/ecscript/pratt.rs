use crate::ecscript::error::ParseError;
use crate::parser::parse_command_literal;

use crate::ecscript::ast::{Expr, ExprKind, InfixOper, Literal, RangeExpr, Stmt, StmtKind};
use crate::ecscript::lexer::{Delimiter, Token, TokenKind};
use crate::ecscript::parser::expect_block;

/// 构造解析错误：若当前 token 是 EOF 则记为 incomplete，否则为普通错误。
///
/// 区分 incomplete 的意义在于，调用方可以据此决定是请求续行还是直接报错。
fn parse_error(state: &TokenStream<'_>, message: impl Into<String>) -> ParseError {
    let offset = state.current_offset();
    let msg = message.into();
    if matches!(state.peek().kind, TokenKind::EOF) {
        ParseError::incomplete(offset, msg)
    } else {
        ParseError::new(offset, msg)
    }
}
/// 词法单元流，提供带缓存位置的 peek/consume/save/load 操作。
///
/// Pratt 解析过程中通过 `save`/`load` 实现回溯，用于区分 `(expr)` 和 `(params) => body` 等歧义语法。
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

    pub fn skip_newlines(&mut self) {
        while matches!(self.peek().kind, TokenKind::Newline) {
            self.consume();
        }
    }

    // pub fn peek_n(&self, n: usize) -> Option<&Token> {
    //     self.tokens.get(self.pos + n)
    // }
    // pub fn check_next(&self, kind: &TokenKind) -> bool {
    //     self.peek_n(1).is_some_and(|token| token.kind == *kind)
    // }

    pub fn check(&self, kind: &TokenKind) -> bool {
        self.peek().kind == *kind
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
    state.skip_newlines();
    let expr = pratt_parser(&mut state, 0)?;
    state.skip_newlines();
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
    pratt_parser(state, 0)
}

/// Pratt（top-down operator precedence）解析器核心。
///
/// 算法思想：每个 token 都可携带前缀和中缀两种解析行为，通过 `min_bp`（最小绑定力）
/// 控制结合性与优先级。`min_bp` 越高，越晚被当前层级"抢走"，从而形成更深的子树。
///
/// 整体分两阶段：
/// 1. **前缀** — 消费开头 token（字面量、变量、前缀运算符、括号/数组/对象等）。
/// 2. **中缀循环** — 只要当前 token 的左侧绑定力 > `min_bp`，就消费它并递归解析右侧。
fn pratt_parser(state: &mut TokenStream<'_>, min_bp: u8) -> Result<Expr, ParseError> {
    state.skip_newlines();
    let mut left: Expr;

    let prefix_span = state.current_offset();
    // ── 前缀位置：根据当前 token 类型构造表达式左端 ──
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

        TokenKind::CommandLiteral(src) => {
            let command = parse_command_literal(&src)
                .map_err(|err| ParseError::new(prefix_span, err.message))?;
            left = Expr {
                kind: ExprKind::CommandLiteral(command),
                span: prefix_span,
            };
            state.consume();
        }

        // ── 前缀运算符：如 `-x`、`!flag` ──
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

        // ── `(` 两种语义：分组提升优先级，或 lambda 参数列表 ──
        // 先尝试按 `(params) => body` 解析，失败则回退为普通分组 `(expr)`。
        TokenKind::Delimiter(Delimiter::LParen) => {
            let span = state.current_offset();
            state.consume();
            state.skip_newlines();

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
                            state.skip_newlines();
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
                state.skip_newlines();
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

        // ── 数组字面量：`[elem, elem, ...]` ──
        TokenKind::Delimiter(Delimiter::LBracket) => {
            state.consume();
            state.skip_newlines();
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
                state.skip_newlines();

                if state.check(&TokenKind::Delimiter(Delimiter::Comma)) {
                    state.consume();
                    state.skip_newlines();
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

        // ── 对象字面量：`{key: value, ...}`，key 支持标识符或字符串 ──
        TokenKind::Delimiter(Delimiter::LBrace) => {
            state.consume();
            state.skip_newlines();
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
                    state.skip_newlines();
                } else {
                    return Err(ParseError::new(
                        state.current_offset(),
                        format!("expected ':', found {}", state.peek().kind.describe()),
                    ));
                }

                let value = pratt_parser(state, 0)?;
                state.skip_newlines();

                obj.push((key, value));

                if state.check(&TokenKind::Delimiter(Delimiter::Comma)) {
                    state.consume();
                    state.skip_newlines();
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

    // ── 中缀循环：只要当前 token 的左结合力 > min_bp 就继续 ──
    // 左结合运算符会在左侧累积深度，右结合则通过提高 right_bp 实现。
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

                    if matches!(infix_oper, InfixOper::PipeForward) {
                        left = desugar_pipe_forward(left, right, op_span)?;
                    } else {
                        left = Expr {
                            kind: ExprKind::Infix(Box::new(left), infix_oper, Box::new(right)),
                            span: op_span,
                        };
                    }
                } else {
                    break;
                }
            }
            // ── 后缀/中缀位置：字段访问 `.name`、索引 `[i]`、调用 `(args)`、区间 `..`、运算符 ──
            TokenKind::Delimiter(Delimiter::Dot) => {
                // `.` 对象字段访问，是左结合，优先级很高
                let bp = 150;
                if bp <= min_bp {
                    break;
                }
                state.consume();
                state.skip_newlines();
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
                state.skip_newlines();
                let expr = pratt_parser(state, 0)?;
                state.skip_newlines();
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
                state.skip_newlines();
                let mut argvs: Vec<Expr> = Vec::new();
                loop {
                    if state.check(&TokenKind::Delimiter(Delimiter::RParen)) {
                        state.consume();
                        break;
                    } // 参数列表可以为空
                    let expr = pratt_parser(state, 0)?;
                    argvs.push(expr);
                    state.skip_newlines();
                    if state.check(&TokenKind::Delimiter(Delimiter::RParen)) {
                        state.consume();
                        break;
                    } else if state.check(&TokenKind::Delimiter(Delimiter::Comma)) {
                        state.consume();
                        state.skip_newlines();
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

/// 解析 lambda 函数体。
///
/// 支持两种体语法：
/// - **块体** `{ stmt; ... }`：直接作为函数体语句块。
/// - **表达式体** `=> expr`：自动包装为 `return expr;` 语句块。
fn parse_lambda(
    state: &mut TokenStream<'_>,
    params: Vec<String>,
    span: usize,
) -> Result<Expr, ParseError> {
    state.skip_newlines();
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

/// 将管道转发 `left |> f(args...)` 脱糖为普通调用 `f(left, args...)`。
///
/// 右侧必须是 Call 表达式，否则报错。脱糖在解析阶段完成，后续阶段无需特殊处理。
fn desugar_pipe_forward(left: Expr, right: Expr, span: usize) -> Result<Expr, ParseError> {
    if let ExprKind::Call(name, mut args) = right.kind {
        args.insert(0, left);
        let desugar_call = ExprKind::Call(name, args);
        Ok(Expr {
            kind: desugar_call,
            span,
        })
    } else {
        Err(ParseError::new(
            span,
            "|> expects a call expression on the right-hand side",
        ))
    }
}

#[cfg(test)]
#[path = "pratt_tests.rs"]
mod tests;
