use crate::ecscript::ast::{InfixOper, PrefixOper};

use crate::ecscript::error::ParseError;

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Int(i64),
    Float(f64),
    String(String),
    True,
    False,
    Nil,
    Identifier(String),
    Operator(Operator),
    Delimiter(Delimiter),
    Newline,
    EOF,
    CommandLiteral(String),

    // 保留字
    Keyword(Keyword),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Keyword {
    Let,
    If,
    Else,
    While,
    For,
    In,
    Continue,
    Break,
    Func,
    Return,
    Cmd,
    Use,
    Pub,
    As,
}

impl Keyword {
    pub fn is_top_level(&self) -> bool {
        matches!(
            self,
            Keyword::Let
                | Keyword::If
                | Keyword::While
                | Keyword::For
                | Keyword::Continue
                | Keyword::Break
                | Keyword::Func
                | Keyword::Return
                | Keyword::Use
                | Keyword::Pub
        )
    }
}

#[derive(Debug, Clone, PartialEq, Copy)]
pub enum Operator {
    // 只知道字面类型，不知道含义 / 相对意义（前缀还是中缀）
    // 算术运算符
    Plus,    // +
    Minus,   // -
    Star,    // *
    Slash,   // /
    Percent, // %

    // 比较运算符
    EqEq,  // ==
    NotEq, // !=
    Lt,    // <
    Gt,    // >
    LtEq,  // <=
    GtEq,  // >=

    // 逻辑运算符
    AndAnd, // &&
    OrOr,   // ||
    Bang,   // !

    PipeForward, // |>
}

impl Operator {
    pub fn lexeme(self) -> &'static str {
        match self {
            Operator::Plus => "+",
            Operator::Minus => "-",
            Operator::Star => "*",
            Operator::Slash => "/",
            Operator::Percent => "%",
            Operator::EqEq => "==",
            Operator::NotEq => "!=",
            Operator::Lt => "<",
            Operator::Gt => ">",
            Operator::LtEq => "<=",
            Operator::GtEq => ">=",
            Operator::AndAnd => "&&",
            Operator::OrOr => "||",
            Operator::Bang => "!",
            Operator::PipeForward => "|>",
        }
    }

    pub fn prefix_info(self) -> Option<(u8, PrefixOper)> {
        match self {
            Operator::Bang => Some((130, PrefixOper::Not)),
            Operator::Minus => Some((130, PrefixOper::Neg)),
            _ => None,
        }
    }

    pub fn infix_info(self) -> Option<(u8, u8, InfixOper)> {
        match self {
            Operator::Plus => Some((60, 60, InfixOper::Add)),
            Operator::Minus => Some((60, 60, InfixOper::Sub)),
            Operator::Star => Some((80, 80, InfixOper::Mul)),
            Operator::Slash => Some((80, 80, InfixOper::Div)),
            Operator::Percent => Some((80, 80, InfixOper::Mod)),

            Operator::EqEq => Some((40, 40, InfixOper::Eq)),
            Operator::NotEq => Some((40, 40, InfixOper::Ne)),
            Operator::Lt => Some((40, 40, InfixOper::Lt)),
            Operator::Gt => Some((40, 40, InfixOper::Gt)),
            Operator::LtEq => Some((40, 40, InfixOper::Le)),
            Operator::GtEq => Some((40, 40, InfixOper::Ge)),

            Operator::AndAnd => Some((30, 30, InfixOper::And)),
            Operator::OrOr => Some((20, 20, InfixOper::Or)),

            Operator::PipeForward => Some((10, 10, InfixOper::PipeForward)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Copy)]
pub enum Delimiter {
    // 分隔符，也就是在标识符命名中不合法的那些
    LParen,    // (
    RParen,    // )
    LBrace,    // {
    RBrace,    // }
    LBracket,  // [
    RBracket,  // ]
    Comma,     // ,
    Dot,       // .
    Semicolon, // ;
    DotDot,    // ..
    DotDotEq,  // ..=
    Eq,        // =
    PlusEq,    // +=
    MinusEq,   // -=
    StarEq,    // *=
    SlashEq,   // /=
    PercentEq, // %=
    Colon,     // :
    FatArrow,  // =>
}

impl Delimiter {
    pub fn lexeme(self) -> &'static str {
        match self {
            Delimiter::LParen => "(",
            Delimiter::RParen => ")",
            Delimiter::LBrace => "{",
            Delimiter::RBrace => "}",
            Delimiter::LBracket => "[",
            Delimiter::RBracket => "]",
            Delimiter::Comma => ",",
            Delimiter::Dot => ".",
            Delimiter::Semicolon => ";",
            Delimiter::DotDot => "..",
            Delimiter::DotDotEq => "..=",
            Delimiter::Eq => "=",
            Delimiter::PlusEq => "+=",
            Delimiter::MinusEq => "-=",
            Delimiter::StarEq => "*=",
            Delimiter::SlashEq => "/=",
            Delimiter::PercentEq => "%=",
            Delimiter::Colon => ":",
            Delimiter::FatArrow => "=>",
        }
    }
}

impl TokenKind {
    pub fn describe(&self) -> String {
        match self {
            TokenKind::Int(_) => "integer literal".to_string(),
            TokenKind::Float(_) => "float literal".to_string(),
            TokenKind::String(_) => "string literal".to_string(),
            TokenKind::True => "keyword 'true'".to_string(),
            TokenKind::False => "keyword 'false'".to_string(),
            TokenKind::Nil => "keyword 'nil'".to_string(),
            TokenKind::Identifier(name) => format!("identifier '{}'", name),
            TokenKind::Operator(operator) => format!("operator '{}'", operator.lexeme()),
            TokenKind::Delimiter(delimiter) => format!("'{}'", delimiter.lexeme()),
            TokenKind::Newline => "newline".to_string(),
            TokenKind::EOF => "end of input".to_string(),
            TokenKind::CommandLiteral(_) => "command literal".to_string(),
            TokenKind::Keyword(Keyword::Let) => "keyword 'let'".to_string(),
            TokenKind::Keyword(Keyword::If) => "keyword 'if'".to_string(),
            TokenKind::Keyword(Keyword::Else) => "keyword 'else'".to_string(),
            TokenKind::Keyword(Keyword::While) => "keyword 'while'".to_string(),
            TokenKind::Keyword(Keyword::For) => "keyword 'for'".to_string(),
            TokenKind::Keyword(Keyword::In) => "keyword 'in'".to_string(),
            TokenKind::Keyword(Keyword::Continue) => "keyword 'continue'".to_string(),
            TokenKind::Keyword(Keyword::Break) => "keyword 'break'".to_string(),
            TokenKind::Keyword(Keyword::Func) => "keyword 'func'".to_string(),
            TokenKind::Keyword(Keyword::Return) => "keyword 'return'".to_string(),
            TokenKind::Keyword(Keyword::Cmd) => "keyword 'cmd'".to_string(),
            TokenKind::Keyword(Keyword::Use) => "keyword 'use'".to_string(),
            TokenKind::Keyword(Keyword::As) => "keyword 'as'".to_string(),
            TokenKind::Keyword(Keyword::Pub) => "keyword 'pub'".to_string(),
        }
    }

    pub fn can_start_expr(&self) -> bool {
        match self {
            TokenKind::Int(_)
            | TokenKind::Float(_)
            | TokenKind::String(_)
            | TokenKind::True
            | TokenKind::False
            | TokenKind::Nil
            | TokenKind::Identifier(_)
            | TokenKind::CommandLiteral(_) => true,
            TokenKind::Operator(operator) => operator.prefix_info().is_some(),
            TokenKind::Delimiter(Delimiter::LParen | Delimiter::LBracket | Delimiter::LBrace) => {
                true
            }
            _ => false,
        }
    }
}

fn push_token(tokens: &mut Vec<Token>, kind: TokenKind, start: usize, end: usize) {
    tokens.push(Token { kind, start, end });
}

pub fn tokenize(src: &str) -> Result<Vec<Token>, ParseError> {
    let mut chars = src.chars().peekable();
    let mut buf = String::new();
    let mut tokens = Vec::new();
    let mut offset: usize = 0; // 当前扫描位置的字节偏移

    // 单趟扫描：每次读取一个字符，根据首字符决定后续分支。
    // `offset` 始终记录已经消费的 UTF-8 字节数，用于报错定位。
    while let Some(ch) = chars.next() {
        let start = offset;
        match ch {
            '\n' => {
                offset += ch.len_utf8();
                push_token(&mut tokens, TokenKind::Newline, start, offset);
            }
            '\r' => {
                offset += ch.len_utf8();
            }
            ch if ch.is_whitespace() => {
                offset += ch.len_utf8();
            }

            // integer/float
            ch if ch.is_ascii_digit() => {
                offset += ch.len_utf8();
                buf.push(ch);
                let mut is_int = true;
                while let Some(next_ch) = chars.peek().copied() {
                    // 数字先尽量吃满；只有第一次遇到 `.` 时，才尝试把它解释成浮点数的小数点。
                    match next_ch {
                        next_ch if next_ch.is_ascii_digit() => {
                            let _ = chars.next(); // 显式消费
                            offset += next_ch.len_utf8();
                            buf.push(next_ch);
                        }
                        '.' if is_int => {
                            // 这里不能直接吞掉 `.`，因为 `1..2` 应该被拆成整数 + range，
                            // 只有 `.` 后面仍是数字时，才把当前字面量升级为浮点数。
                            let mut lookahead = chars.clone();
                            let _ = lookahead.next();

                            if let Some(after_dot) = lookahead.next() {
                                if after_dot.is_ascii_digit() {
                                    let _ = chars.next(); // 显式消费
                                    is_int = false;
                                    offset += '.'.len_utf8();
                                    buf.push('.');
                                } else {
                                    break;
                                }
                            } else {
                                break;
                            }
                        }
                        _ => break,
                    }
                }
                ensure_number_terminated(&mut chars, offset)?;
                // 遇到非数字或者字符串结束
                let number_text = std::mem::take(&mut buf);
                // 夺取所有权并且置空
                if is_int {
                    push_token(
                        &mut tokens,
                        TokenKind::Int(number_text.parse().unwrap()),
                        start,
                        offset,
                    )
                } else {
                    push_token(
                        &mut tokens,
                        TokenKind::Float(number_text.parse().unwrap()),
                        start,
                        offset,
                    )
                }
            }

            // raw string: r"..."
            'r' if chars.peek() == Some(&'"') => {
                offset += ch.len_utf8();
                let _ = chars.next();
                offset += '"'.len_utf8();

                let mut is_close = false;
                // 原始字符串不处理任何转义，直到遇到下一个 `"` 才结束。
                while let Some(next_ch) = chars.next() {
                    offset += next_ch.len_utf8();
                    if next_ch == '"' {
                        is_close = true;
                        break;
                    }
                    buf.push(next_ch);
                }

                let string_text = std::mem::take(&mut buf);
                if is_close {
                    push_token(&mut tokens, TokenKind::String(string_text), start, offset)
                } else {
                    return Err(ParseError::incomplete(
                        offset,
                        "unterminated raw string literal".to_string(),
                    ));
                }
            }

            // indentifier/reserved
            ch if ch.is_ascii_alphabetic() || ch == '_' => {
                offset += ch.len_utf8();
                buf.push(ch);
                while let Some(next_ch) =
                    chars.next_if(|next_ch| (*next_ch).is_ascii_alphanumeric() || (*next_ch) == '_')
                {
                    offset += next_ch.len_utf8();
                    buf.push(next_ch);
                }
                let ident = std::mem::take(&mut buf);
                match ident.as_str() {
                    "nil" => push_token(&mut tokens, TokenKind::Nil, start, offset),
                    "true" => push_token(&mut tokens, TokenKind::True, start, offset),
                    "false" => push_token(&mut tokens, TokenKind::False, start, offset),

                    // 保留字
                    "let" => {
                        push_token(&mut tokens, TokenKind::Keyword(Keyword::Let), start, offset)
                    }
                    "if" => push_token(&mut tokens, TokenKind::Keyword(Keyword::If), start, offset),
                    "else" => push_token(
                        &mut tokens,
                        TokenKind::Keyword(Keyword::Else),
                        start,
                        offset,
                    ),
                    "while" => push_token(
                        &mut tokens,
                        TokenKind::Keyword(Keyword::While),
                        start,
                        offset,
                    ),
                    "for" => {
                        push_token(&mut tokens, TokenKind::Keyword(Keyword::For), start, offset)
                    }
                    "in" => push_token(&mut tokens, TokenKind::Keyword(Keyword::In), start, offset),
                    "continue" => push_token(
                        &mut tokens,
                        TokenKind::Keyword(Keyword::Continue),
                        start,
                        offset,
                    ),
                    "break" => push_token(
                        &mut tokens,
                        TokenKind::Keyword(Keyword::Break),
                        start,
                        offset,
                    ),
                    "func" => push_token(
                        &mut tokens,
                        TokenKind::Keyword(Keyword::Func),
                        start,
                        offset,
                    ),
                    "return" => push_token(
                        &mut tokens,
                        TokenKind::Keyword(Keyword::Return),
                        start,
                        offset,
                    ),
                    "use" => {
                        push_token(&mut tokens, TokenKind::Keyword(Keyword::Use), start, offset)
                    }
                    "pub" => {
                        push_token(&mut tokens, TokenKind::Keyword(Keyword::Pub), start, offset)
                    }
                    "as" => push_token(&mut tokens, TokenKind::Keyword(Keyword::As), start, offset),
                    "cmd" => {
                        // `cmd` 后面紧跟 `{...}` 时，整个命令体会被折叠成一个 token；
                        // 否则它仍然只是普通关键字。
                        if let Some(command) = try_scan_command_literal(&mut chars, &mut offset)? {
                            push_token(
                                &mut tokens,
                                TokenKind::CommandLiteral(command),
                                start,
                                offset,
                            );
                        } else {
                            push_token(
                                &mut tokens,
                                TokenKind::Keyword(Keyword::Cmd),
                                start,
                                offset,
                            );
                        }
                    }
                    _ => push_token(&mut tokens, TokenKind::Identifier(ident), start, offset),
                }
            }

            // string
            '\"' => {
                offset += ch.len_utf8();
                let mut is_close = false;
                // 普通字符串支持少量转义，实际解码在词法阶段完成。
                while let Some(next_ch) = chars.next() {
                    offset += next_ch.len_utf8();
                    match next_ch {
                        '\\' => {
                            if let Some(escaped) = chars.next() {
                                offset += escaped.len_utf8();
                                match escaped {
                                    '\\' => buf.push('\\'),
                                    '"' => buf.push('"'),
                                    'n' => buf.push('\n'),
                                    't' => buf.push('\t'),
                                    other => {
                                        return Err(ParseError::new(
                                            offset,
                                            format!("unknown escape '\\{}'", other),
                                        ));
                                    }
                                }
                            } else {
                                return Err(ParseError::incomplete(
                                    offset,
                                    "unterminated escape at end of string".to_string(),
                                ));
                            }
                        }
                        '\"' => {
                            is_close = true;
                            break;
                        }
                        _ => {
                            buf.push(next_ch);
                        }
                    }
                }
                let string_text = std::mem::take(&mut buf);
                if is_close {
                    push_token(&mut tokens, TokenKind::String(string_text), start, offset)
                } else {
                    return Err(ParseError::incomplete(
                        offset,
                        "unterminated string literal".to_string(),
                    ));
                }
            }

            // delimiter
            '(' => {
                offset += ch.len_utf8();
                push_token(
                    &mut tokens,
                    TokenKind::Delimiter(Delimiter::LParen),
                    start,
                    offset,
                )
            }
            ')' => {
                offset += ch.len_utf8();
                push_token(
                    &mut tokens,
                    TokenKind::Delimiter(Delimiter::RParen),
                    start,
                    offset,
                )
            }
            '{' => {
                offset += ch.len_utf8();
                push_token(
                    &mut tokens,
                    TokenKind::Delimiter(Delimiter::LBrace),
                    start,
                    offset,
                )
            }
            '}' => {
                offset += ch.len_utf8();
                push_token(
                    &mut tokens,
                    TokenKind::Delimiter(Delimiter::RBrace),
                    start,
                    offset,
                )
            }
            '[' => {
                offset += ch.len_utf8();
                push_token(
                    &mut tokens,
                    TokenKind::Delimiter(Delimiter::LBracket),
                    start,
                    offset,
                )
            }
            ']' => {
                offset += ch.len_utf8();
                push_token(
                    &mut tokens,
                    TokenKind::Delimiter(Delimiter::RBracket),
                    start,
                    offset,
                )
            }
            ',' => {
                offset += ch.len_utf8();
                push_token(
                    &mut tokens,
                    TokenKind::Delimiter(Delimiter::Comma),
                    start,
                    offset,
                )
            }
            '.' => {
                offset += ch.len_utf8();
                if chars.next_if_eq(&'.').is_some() {
                    offset += '.'.len_utf8();
                    let kind = if chars.next_if_eq(&'=').is_some() {
                        offset += '='.len_utf8();
                        TokenKind::Delimiter(Delimiter::DotDotEq)
                    } else {
                        TokenKind::Delimiter(Delimiter::DotDot)
                    };
                    push_token(&mut tokens, kind, start, offset);
                } else if let Some(next_ch) = chars.next_if(|next_ch| (*next_ch).is_ascii_digit()) {
                    // 支持 `.123` 形式，并在语义上补成 `0.123`。
                    offset += next_ch.len_utf8();
                    buf.push_str("0.");
                    buf.push(next_ch);
                    while let Some(next_ch) = chars.next_if(|next_ch| (*next_ch).is_ascii_digit()) {
                        // 这样就不需要自己手动先peek再消费了
                        offset += next_ch.len_utf8();
                        buf.push(next_ch);
                    }
                    ensure_number_terminated(&mut chars, offset)?;
                    let number_text = std::mem::take(&mut buf);
                    push_token(
                        &mut tokens,
                        TokenKind::Float(number_text.parse().unwrap()),
                        start,
                        offset,
                    )
                } else {
                    push_token(
                        &mut tokens,
                        TokenKind::Delimiter(Delimiter::Dot),
                        start,
                        offset,
                    );
                }
            }
            ';' => {
                offset += ch.len_utf8();
                push_token(
                    &mut tokens,
                    TokenKind::Delimiter(Delimiter::Semicolon),
                    start,
                    offset,
                )
            }
            '=' => {
                offset += ch.len_utf8();
                let kind = if chars.next_if_eq(&'=').is_some() {
                    offset += '='.len_utf8();
                    TokenKind::Operator(Operator::EqEq)
                } else if chars.next_if_eq(&'>').is_some() {
                    offset += '>'.len_utf8();
                    TokenKind::Delimiter(Delimiter::FatArrow)
                } else {
                    TokenKind::Delimiter(Delimiter::Eq)
                };
                push_token(&mut tokens, kind, start, offset);
            }

            // oper
            '+' => {
                offset += ch.len_utf8();
                let kind = if chars.next_if_eq(&'=').is_some() {
                    offset += '='.len_utf8();
                    TokenKind::Delimiter(Delimiter::PlusEq)
                } else {
                    TokenKind::Operator(Operator::Plus)
                };
                push_token(&mut tokens, kind, start, offset)
            }
            '-' => {
                offset += ch.len_utf8();
                let kind = if chars.next_if_eq(&'=').is_some() {
                    offset += '='.len_utf8();
                    TokenKind::Delimiter(Delimiter::MinusEq)
                } else {
                    TokenKind::Operator(Operator::Minus)
                };
                push_token(&mut tokens, kind, start, offset)
            }
            '*' => {
                offset += ch.len_utf8();
                let kind = if chars.next_if_eq(&'=').is_some() {
                    offset += '='.len_utf8();
                    TokenKind::Delimiter(Delimiter::StarEq)
                } else {
                    TokenKind::Operator(Operator::Star)
                };
                push_token(&mut tokens, kind, start, offset)
            }
            '/' => {
                offset += ch.len_utf8();
                // `/` 同时承担除号、复合赋值和两种注释的入口，因此优先判注释。
                if chars.next_if_eq(&'/').is_some() {
                    while let Some(nxt) = chars.next() {
                        offset += nxt.len_utf8();
                        if nxt == '\n' {
                            break;
                        }
                    }
                } else if chars.next_if_eq(&'*').is_some() {
                    let mut is_close = false;
                    while let Some(nxt) = chars.next() {
                        offset += nxt.len_utf8();
                        if nxt == '*' && chars.next_if_eq(&'/').is_some() {
                            offset += '/'.len_utf8();
                            is_close = true;
                            break;
                        }
                    }
                    if !is_close {
                        return Err(ParseError::incomplete(offset, "unterminated block comment"));
                    }
                } else if chars.next_if_eq(&'=').is_some() {
                    offset += '='.len_utf8();
                    push_token(
                        &mut tokens,
                        TokenKind::Delimiter(Delimiter::SlashEq),
                        start,
                        offset,
                    )
                } else {
                    push_token(
                        &mut tokens,
                        TokenKind::Operator(Operator::Slash),
                        start,
                        offset,
                    )
                }
            }
            '%' => {
                offset += ch.len_utf8();
                let kind = if chars.next_if_eq(&'=').is_some() {
                    offset += '='.len_utf8();
                    TokenKind::Delimiter(Delimiter::PercentEq)
                } else {
                    TokenKind::Operator(Operator::Percent)
                };
                push_token(&mut tokens, kind, start, offset)
            }
            ':' => {
                offset += ch.len_utf8();
                push_token(
                    &mut tokens,
                    TokenKind::Delimiter(Delimiter::Colon),
                    start,
                    offset,
                )
            }
            '&' => {
                offset += ch.len_utf8();
                if chars.next_if_eq(&'&').is_some() {
                    offset += '&'.len_utf8();
                    push_token(
                        &mut tokens,
                        TokenKind::Operator(Operator::AndAnd),
                        start,
                        offset,
                    )
                } else {
                    return Err(ParseError::new(
                        offset,
                        "unexpected '&'; did you mean '&&'?".to_string(),
                    ));
                    // TODO 直接报错，之后处理&。报错文本之后打磨
                }
            }
            '|' => {
                offset += ch.len_utf8();
                if chars.next_if_eq(&'|').is_some() {
                    offset += '|'.len_utf8();
                    push_token(
                        &mut tokens,
                        TokenKind::Operator(Operator::OrOr),
                        start,
                        offset,
                    )
                } else if chars.next_if_eq(&'>').is_some() {
                    offset += '>'.len_utf8();
                    push_token(
                        &mut tokens,
                        TokenKind::Operator(Operator::PipeForward),
                        start,
                        offset,
                    )
                } else {
                    return Err(ParseError::new(
                        offset,
                        "unexpected '|'; did you mean '||'?".to_string(),
                    ));
                }
            }
            '!' => {
                offset += ch.len_utf8();
                let kind = if chars.next_if_eq(&'=').is_some() {
                    offset += '='.len_utf8();
                    TokenKind::Operator(Operator::NotEq)
                } else {
                    TokenKind::Operator(Operator::Bang)
                };
                push_token(&mut tokens, kind, start, offset);
            }
            '<' => {
                offset += ch.len_utf8();
                let kind = if chars.next_if_eq(&'=').is_some() {
                    offset += '='.len_utf8();
                    TokenKind::Operator(Operator::LtEq)
                } else {
                    TokenKind::Operator(Operator::Lt)
                };
                push_token(&mut tokens, kind, start, offset);
            }
            '>' => {
                offset += ch.len_utf8();
                let kind = if chars.next_if_eq(&'=').is_some() {
                    offset += '='.len_utf8();
                    TokenKind::Operator(Operator::GtEq)
                } else {
                    TokenKind::Operator(Operator::Gt)
                };
                push_token(&mut tokens, kind, start, offset);
            }
            other => {
                return Err(ParseError::new(
                    offset,
                    format!("unexpected character '{}'", other),
                ));
            }
        }
    }
    push_token(&mut tokens, TokenKind::EOF, offset, offset);
    Ok(tokens)
}

/// 直接将cmd{}整体视为一个token
/// 常规处理单双引号、转义、大括号闭合
fn try_scan_command_literal(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    offset: &mut usize,
) -> Result<Option<String>, ParseError> {
    let mut probe = chars.clone();
    // 先探测，不消费真实输入；只有确认是 `cmd {` 结构后才进入命令体扫描。
    while let Some(next) = probe.peek().copied() {
        if matches!(next, ' ' | '\t') {
            let _ = probe.next();
        } else {
            break;
        }
    }
    if probe.next_if_eq(&'{').is_none() {
        return Ok(None);
    }

    while let Some(next) = chars.peek().copied() {
        if matches!(next, ' ' | '\t') {
            let _ = chars.next();
            *offset += next.len_utf8();
        } else {
            break;
        }
    }
    let _ = chars.next();
    *offset += '{'.len_utf8();

    let mut depth = 1usize;
    let mut body = String::new();

    // 命令字面量内部仍需要区分“普通态”和“引号态”，
    // 否则引号中的 `{` / `}` 会错误地影响大括号配对深度。
    enum ScanState {
        Normal,
        SingleQuoted,
        DoubleQuoted,
    }
    let mut state = ScanState::Normal;

    loop {
        match state {
            ScanState::Normal => match chars.next() {
                Some('{') => {
                    *offset += '{'.len_utf8();
                    depth += 1;
                    body.push('{');
                }
                Some('}') => {
                    *offset += '}'.len_utf8();
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    body.push('}');
                }
                Some('\'') => {
                    *offset += '\''.len_utf8();
                    state = ScanState::SingleQuoted;
                    body.push('\'');
                }
                Some('"') => {
                    *offset += '"'.len_utf8();
                    state = ScanState::DoubleQuoted;
                    body.push('"');
                }
                Some('\\') => {
                    // 普通态下保留反斜杠和其后的一个字符，避免误拆转义后的引号。
                    *offset += '\\'.len_utf8();
                    body.push('\\');
                    let Some(next) = chars.next() else {
                        return Err(ParseError::incomplete(
                            *offset,
                            "unterminated command literal",
                        ));
                    };
                    *offset += next.len_utf8();
                    body.push(next);
                }
                Some(ch) => {
                    *offset += ch.len_utf8();
                    body.push(ch);
                }
                None => {
                    return Err(ParseError::incomplete(
                        *offset,
                        "unterminated command literal",
                    ));
                }
            },
            ScanState::SingleQuoted => match chars.next() {
                Some('\'') => {
                    *offset += '\''.len_utf8();
                    state = ScanState::Normal;
                    body.push('\'');
                }
                Some(ch) => {
                    *offset += ch.len_utf8();
                    body.push(ch);
                }
                None => {
                    return Err(ParseError::incomplete(
                        *offset,
                        "unterminated command literal",
                    ));
                }
            },
            ScanState::DoubleQuoted => match chars.next() {
                Some('"') => {
                    *offset += '"'.len_utf8();
                    state = ScanState::Normal;
                    body.push('"');
                }
                Some('\\') => {
                    *offset += '\\'.len_utf8();
                    body.push('\\');
                    let Some(next) = chars.next() else {
                        return Err(ParseError::incomplete(
                            *offset,
                            "unterminated command literal",
                        ));
                    };
                    *offset += next.len_utf8();
                    body.push(next);
                }
                Some(ch) => {
                    *offset += ch.len_utf8();
                    body.push(ch);
                }
                None => {
                    return Err(ParseError::incomplete(
                        *offset,
                        "unterminated command literal",
                    ));
                }
            },
        }
    }

    Ok(Some(body))
}

fn ensure_number_terminated(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    offset: usize,
) -> Result<(), ParseError> {
    // 数字后面如果立刻跟标识符字符，会产生 `123abc` 这种歧义输入；
    // 这里直接在词法阶段拒绝，而不是留给语法阶段处理。
    if let Some(next_ch) = chars.peek().copied() {
        if next_ch.is_ascii_alphabetic() || next_ch == '_' {
            return Err(ParseError::new(
                offset,
                format!(
                    "invalid numeric literal; expected separator after number, found '{}'",
                    next_ch
                ),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "lexer_tests.rs"]
mod tests;
