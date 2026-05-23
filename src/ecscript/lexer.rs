use crate::ecscript::ast::{InfixOper, PrefixOper};

use crate::ecscript::error::ParseError;

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
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
    EOF,

    // 保留字
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
            TokenKind::EOF => "end of input".to_string(),
            TokenKind::Let => "keyword 'let'".to_string(),
            TokenKind::If => "keyword 'if'".to_string(),
            TokenKind::Else => "keyword 'else'".to_string(),
            TokenKind::While => "keyword 'while'".to_string(),
            TokenKind::For => "keyword 'for'".to_string(),
            TokenKind::In => "keyword 'in'".to_string(),
            TokenKind::Continue => "keyword 'continue'".to_string(),
            TokenKind::Break => "keyword 'break'".to_string(),
            TokenKind::Func => "keyword 'func'".to_string(),
            TokenKind::Return => "keyword 'return'".to_string(),
        }
    }
}

pub fn tokenize(src: &str) -> Result<Vec<Token>, ParseError> {
    let mut chars = src.chars().peekable();
    let mut buf = String::new();
    let mut tokens = Vec::new();
    let mut offset: usize = 0; // 当前扫描位置的字节偏移

    while let Some(ch) = chars.next() {
        match ch {
            ch if ch.is_whitespace() => {
                offset += ch.len_utf8();
            }

            // integer/float
            ch if ch.is_ascii_digit() => {
                offset += ch.len_utf8();
                buf.push(ch);
                let mut is_int = true;
                while let Some(next_ch) = chars.peek().copied() {
                    // 这里提前看一个不消费
                    match next_ch {
                        next_ch if next_ch.is_ascii_digit() => {
                            let _ = chars.next(); // 显式消费
                            offset += next_ch.len_utf8();
                            buf.push(next_ch);
                        }
                        '.' if is_int => {
                            // 因为这里'.'是没有消费的，而且要看'.'的下一个
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
                // TODO 会不会有123ab 1.23ab这种错误被我忽视？但是123*这种情况又不算错
                // 遇到非数字或者字符串结束
                let number_text = std::mem::take(&mut buf);
                // 夺取所有权并且置空
                if is_int {
                    tokens.push(Token {
                        kind: TokenKind::Int(number_text.parse().unwrap()),
                        end: offset,
                    })
                } else {
                    tokens.push(Token {
                        kind: TokenKind::Float(number_text.parse().unwrap()),
                        end: offset,
                    })
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
                    "nil" => tokens.push(Token {
                        kind: TokenKind::Nil,
                        end: offset,
                    }),
                    "true" => tokens.push(Token {
                        kind: TokenKind::True,
                        end: offset,
                    }),
                    "false" => tokens.push(Token {
                        kind: TokenKind::False,
                        end: offset,
                    }),

                    // 保留字
                    "let" => tokens.push(Token {
                        kind: TokenKind::Let,
                        end: offset,
                    }),
                    "if" => tokens.push(Token {
                        kind: TokenKind::If,
                        end: offset,
                    }),
                    "else" => tokens.push(Token {
                        kind: TokenKind::Else,
                        end: offset,
                    }),
                    "while" => tokens.push(Token {
                        kind: TokenKind::While,
                        end: offset,
                    }),
                    "for" => tokens.push(Token {
                        kind: TokenKind::For,
                        end: offset,
                    }),
                    "in" => tokens.push(Token {
                        kind: TokenKind::In,
                        end: offset,
                    }),
                    "continue" => tokens.push(Token {
                        kind: TokenKind::Continue,
                        end: offset,
                    }),
                    "break" => tokens.push(Token {
                        kind: TokenKind::Break,
                        end: offset,
                    }),
                    "func" => tokens.push(Token {
                        kind: TokenKind::Func,
                        end: offset,
                    }),
                    "return" => tokens.push(Token {
                        kind: TokenKind::Return,
                        end: offset,
                    }),
                    _ => tokens.push(Token {
                        kind: TokenKind::Identifier(ident),
                        end: offset,
                    }),
                }
            }

            // string
            '\"' => {
                offset += ch.len_utf8();
                let mut is_close = false;
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
                                return Err(ParseError::new(
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
                    tokens.push(Token {
                        kind: TokenKind::String(string_text),
                        end: offset,
                    })
                } else {
                    return Err(ParseError::new(
                        offset,
                        "unterminated string literal".to_string(),
                    ));
                }
            }

            // delimiter
            '(' => {
                offset += ch.len_utf8();
                tokens.push(Token {
                    kind: TokenKind::Delimiter(Delimiter::LParen),
                    end: offset,
                })
            }
            ')' => {
                offset += ch.len_utf8();
                tokens.push(Token {
                    kind: TokenKind::Delimiter(Delimiter::RParen),
                    end: offset,
                })
            }
            '{' => {
                offset += ch.len_utf8();
                tokens.push(Token {
                    kind: TokenKind::Delimiter(Delimiter::LBrace),
                    end: offset,
                })
            }
            '}' => {
                offset += ch.len_utf8();
                tokens.push(Token {
                    kind: TokenKind::Delimiter(Delimiter::RBrace),
                    end: offset,
                })
            }
            '[' => {
                offset += ch.len_utf8();
                tokens.push(Token {
                    kind: TokenKind::Delimiter(Delimiter::LBracket),
                    end: offset,
                })
            }
            ']' => {
                offset += ch.len_utf8();
                tokens.push(Token {
                    kind: TokenKind::Delimiter(Delimiter::RBracket),
                    end: offset,
                })
            }
            ',' => {
                offset += ch.len_utf8();
                tokens.push(Token {
                    kind: TokenKind::Delimiter(Delimiter::Comma),
                    end: offset,
                })
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
                    tokens.push(Token { kind, end: offset });
                } else if let Some(next_ch) = chars.next_if(|next_ch| (*next_ch).is_ascii_digit()) {
                    // .123这种浮点数
                    offset += next_ch.len_utf8();
                    buf.push_str("0.");
                    buf.push(next_ch);
                    while let Some(next_ch) = chars.next_if(|next_ch| (*next_ch).is_ascii_digit()) {
                        // 这样就不需要自己手动先peek再消费了
                        offset += next_ch.len_utf8();
                        buf.push(next_ch);
                    }
                    // TODO 会不会有.123abc这种错误被我忽视？
                    let number_text = std::mem::take(&mut buf);
                    tokens.push(Token {
                        kind: TokenKind::Float(number_text.parse().unwrap()),
                        end: offset,
                    })
                } else {
                    tokens.push(Token {
                        kind: TokenKind::Delimiter(Delimiter::Dot),
                        end: offset,
                    });
                }
            }
            ';' => {
                offset += ch.len_utf8();
                tokens.push(Token {
                    kind: TokenKind::Delimiter(Delimiter::Semicolon),
                    end: offset,
                })
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
                tokens.push(Token { kind, end: offset });
            }

            // oper
            '+' => {
                offset += ch.len_utf8();
                tokens.push(Token {
                    kind: TokenKind::Operator(Operator::Plus),
                    end: offset,
                })
            }
            '-' => {
                offset += ch.len_utf8();
                tokens.push(Token {
                    kind: TokenKind::Operator(Operator::Minus),
                    end: offset,
                })
            }
            '*' => {
                offset += ch.len_utf8();
                tokens.push(Token {
                    kind: TokenKind::Operator(Operator::Star),
                    end: offset,
                })
            }
            '/' => {
                offset += ch.len_utf8();
                tokens.push(Token {
                    kind: TokenKind::Operator(Operator::Slash),
                    end: offset,
                })
            }
            '%' => {
                offset += ch.len_utf8();
                tokens.push(Token {
                    kind: TokenKind::Operator(Operator::Percent),
                    end: offset,
                })
            }
            ':' => {
                offset += ch.len_utf8();
                tokens.push(Token {
                    kind: TokenKind::Delimiter(Delimiter::Colon),
                    end: offset,
                })
            }
            '&' => {
                offset += ch.len_utf8();
                if chars.next_if_eq(&'&').is_some() {
                    offset += '&'.len_utf8();
                    tokens.push(Token {
                        kind: TokenKind::Operator(Operator::AndAnd),
                        end: offset,
                    })
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
                    tokens.push(Token {
                        kind: TokenKind::Operator(Operator::OrOr),
                        end: offset,
                    })
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
                tokens.push(Token { kind, end: offset });
            }
            '<' => {
                offset += ch.len_utf8();
                let kind = if chars.next_if_eq(&'=').is_some() {
                    offset += '='.len_utf8();
                    TokenKind::Operator(Operator::LtEq)
                } else {
                    TokenKind::Operator(Operator::Lt)
                };
                tokens.push(Token { kind, end: offset });
            }
            '>' => {
                offset += ch.len_utf8();
                let kind = if chars.next_if_eq(&'=').is_some() {
                    offset += '='.len_utf8();
                    TokenKind::Operator(Operator::GtEq)
                } else {
                    TokenKind::Operator(Operator::Gt)
                };
                tokens.push(Token { kind, end: offset });
            }
            other => {
                return Err(ParseError::new(
                    offset,
                    format!("unexpected character '{}'", other),
                ));
            }
        }
    }
    tokens.push(Token {
        kind: TokenKind::EOF,
        end: offset,
    });
    Ok(tokens)
}

#[cfg(test)]
mod tests {
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
    fn eq_eq_lexes_as_equality_operator() {
        assert_kinds(
            "== != <= >= && || ! =",
            vec![
                op(Operator::EqEq),
                op(Operator::NotEq),
                op(Operator::LtEq),
                op(Operator::GtEq),
                op(Operator::AndAnd),
                op(Operator::OrOr),
                op(Operator::Bang),
                delimiter(Delimiter::Eq),
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
    fn reports_unterminated_string() {
        assert_lex_error("\"abc", 4, "unterminated string literal");
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
}
