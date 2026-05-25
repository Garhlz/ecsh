//! 词法分析：将一行输入字符串拆成 Token 序列。
//!
//! 状态机有三种模式：Normal（正常）、SingleQuoted（单引号）、DoubleQuoted（双引号）。
//! 空格和操作符在 Normal 模式下作为 token 边界；引号内所有内容保留为字面量。
//! 错误统一通过 `ParseError` 返回，携带字节偏移和续行感知标志。

use crate::ecscript::error::ParseError;
use crate::types::{LexerStatus, ShellState, ShellWord, Token, WordFragment};

/// 将一行输入拆分为 Token 序列，错误携带字节偏移。
pub fn tokenize(line: &str, _state: &ShellState) -> Result<Vec<Token>, ParseError> {
    let mut chars = line.chars().peekable();
    let mut lexer_status = LexerStatus::Normal;

    let mut lit_buffer: String = String::new();
    let mut fragments: Vec<WordFragment> = Vec::new();
    let mut tokens = Vec::new();
    let mut offset: usize = 0;

    while let Some(ch) = chars.next() {
        offset += ch.len_utf8();
        match lexer_status {
            // ── Normal 模式：操作符和空白有特殊意义 ──
            LexerStatus::Normal => match ch {
                ch if ch.is_whitespace() => {
                    flush_word(&mut lit_buffer, &mut fragments, &mut tokens);
                }

                '|' => {
                    flush_word(&mut lit_buffer, &mut fragments, &mut tokens);
                    let kind = if chars.next_if_eq(&'|').is_some() {
                        Token::OrIf
                    } else {
                        Token::Pipe
                    };
                    tokens.push(kind);
                }
                '&' => {
                    flush_word(&mut lit_buffer, &mut fragments, &mut tokens);
                    let kind = if chars.next_if_eq(&'&').is_some() {
                        Token::AndIf
                    } else {
                        Token::Ampersand
                    };
                    tokens.push(kind);
                }
                '<' => {
                    flush_word(&mut lit_buffer, &mut fragments, &mut tokens);
                    tokens.push(Token::RedirectionIn);
                }
                '>' => {
                    flush_word(&mut lit_buffer, &mut fragments, &mut tokens);
                    let kind = if chars.next_if_eq(&'>').is_some() {
                        Token::RedirectionAppend
                    } else {
                        Token::RedirectionTruncate
                    };
                    tokens.push(kind);
                }
                '$' => handle_dollar(&mut chars, &mut lit_buffer, &mut fragments, &mut offset)?,
                '\'' => {
                    lexer_status = LexerStatus::SingleQuoted;
                }
                '\"' => {
                    lexer_status = LexerStatus::DoubleQuoted;
                }
                ';' => {
                    flush_word(&mut lit_buffer, &mut fragments, &mut tokens);
                    tokens.push(Token::Semicolon);
                }
                '\\' => {
                    if let Some(ch) = chars.next() {
                        offset += ch.len_utf8();
                        lit_buffer.push(ch);
                    } else {
                        return Err(ParseError::new(offset, "trailing backslash"));
                    }
                }
                _ => {
                    lit_buffer.push(ch);
                }
            },

            // ── 单引号模式：所有字符按字面量处理，不被识别为操作符 ──
            LexerStatus::SingleQuoted => match ch {
                '\'' => {
                    lexer_status = LexerStatus::Normal;
                }
                _ => lit_buffer.push(ch),
            },

            // ── 双引号模式：保留空白和操作符字面量，但 $ 和 \" 仍会被处理 ──
            LexerStatus::DoubleQuoted => match ch {
                '"' => lexer_status = LexerStatus::Normal,
                '$' => handle_dollar(&mut chars, &mut lit_buffer, &mut fragments, &mut offset)?,
                '\\' => {
                    if let Some(ch) = chars.next() {
                        // 目前只能转义这三个字符
                        if matches!(ch, '"' | '$' | '\\') {
                            lit_buffer.push(ch);
                        } else {
                            lit_buffer.push('\\');
                            lit_buffer.push(ch);
                        }
                    } else {
                        return Err(ParseError::new(
                            offset,
                            "trailing backslash in double quotes",
                        ));
                    }
                }
                _ => lit_buffer.push(ch),
            },
        }
    }

    // 输入结束但引号未闭合,报错。
    if let LexerStatus::DoubleQuoted = lexer_status {
        return Err(ParseError::incomplete(offset, "unterminated double quote"));
    }
    if let LexerStatus::SingleQuoted = lexer_status {
        return Err(ParseError::incomplete(offset, "unterminated single quote"));
    }

    flush_word(&mut lit_buffer, &mut fragments, &mut tokens);
    Ok(tokens)
}

/// 将当前累积的字面量提交到fragments数组中，并重置buffer。
/// 使用 `std::mem::take` 把 String 所有权移交给 Token，避免 clone。
fn flush_buffer(lit_buffer: &mut String, fragments: &mut Vec<WordFragment>) {
    if !lit_buffer.is_empty() {
        fragments.push(WordFragment::Lit(std::mem::take(lit_buffer)));
    }
}

// 同理，提交token
fn flush_word(lit_buffer: &mut String, fragments: &mut Vec<WordFragment>, tokens: &mut Vec<Token>) {
    flush_buffer(lit_buffer, fragments);
    if !fragments.is_empty() {
        tokens.push(Token::Word(ShellWord {
            fragments: std::mem::take(fragments),
        }));
    }
}

fn handle_dollar(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    lit_buffer: &mut String,
    fragments: &mut Vec<WordFragment>,
    offset: &mut usize,
) -> Result<(), ParseError> {
    // 进入之前`$`已经消费掉了
    match chars.peek().copied() {
        None => {
            // 行尾的孤 $，保留字面量。
            lit_buffer.push('$');
            Ok(())
        }
        Some('?') => {
            // 视为var
            let _ = chars.next();
            flush_buffer(lit_buffer, fragments);
            fragments.push(WordFragment::Var("?".to_string()));
            Ok(())
        }
        Some('{') => {
            flush_buffer(lit_buffer, fragments); // 把之前的字面量消费掉

            // 解析为EnvVar,不需要深度计数
            let _ = chars.next();

            match chars.next() {
                Some('}') => Err(ParseError::new(*offset, "empty variable name in braces")),
                Some(start) if start == '_' || start.is_ascii_alphabetic() => {
                    // 开始循环解析{}中的环境变量
                    let mut is_close = false;

                    let mut envvar_buffer = String::new();
                    envvar_buffer.push(start);

                    loop {
                        if let Some(succ) =
                            chars.next_if(|ch| *ch == '_' || ch.is_ascii_alphanumeric())
                        {
                            envvar_buffer.push(succ);
                        } else if chars.next_if_eq(&'}').is_some() {
                            is_close = true;
                            break;
                        } else {
                            break;
                        }
                    }

                    if !is_close {
                        return Err(ParseError::incomplete(
                            *offset,
                            "unterminated ${...} expansion",
                        ));
                    }

                    fragments.push(WordFragment::EnvVar(envvar_buffer));

                    Ok(())
                }
                Some(_) => Err(ParseError::new(*offset, "invalid variable name in braces")),
                None => Err(ParseError::incomplete(
                    *offset,
                    "unterminated ${...} expansion",
                )),
            }
        }
        Some('(') => {
            // 解析为 Cmd,需要深度计数（处理引号和转义）
            flush_buffer(lit_buffer, fragments); // 把之前的字面量消费掉

            let _ = chars.next();
            let mut depth = 1;

            let mut cmd_buffer = String::new();

            enum LoopState {
                Normal,
                InDoubleQuote,
                InSingleQuote,
            }
            let mut loop_state = LoopState::Normal;

            loop {
                match loop_state {
                    LoopState::Normal => match chars.next() {
                        Some(')') => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                            cmd_buffer.push(')');
                        }
                        Some('(') => {
                            depth += 1;
                            cmd_buffer.push('(');
                        }
                        Some('\\') => {
                            // 保留原始文本，留给之后的程序处理
                            cmd_buffer.push('\\');
                            let Some(nxt) = chars.next() else {
                                return Err(ParseError::incomplete(
                                    *offset,
                                    "unterminated $(...) expansion",
                                ));
                            };
                            cmd_buffer.push(nxt);
                        }
                        Some('"') => {
                            loop_state = LoopState::InDoubleQuote;
                            cmd_buffer.push('"');
                        }
                        Some('\'') => {
                            loop_state = LoopState::InSingleQuote;
                            cmd_buffer.push('\'');
                        }
                        Some(ch) => {
                            cmd_buffer.push(ch);
                        }
                        None => {
                            return Err(ParseError::incomplete(
                                *offset,
                                "unterminated $(...) expansion",
                            ));
                        }
                    },
                    // 1. 括号不再影响整个dollar表达式
                    // 2. 处理转义
                    // 3. 不嵌套处理$，将其视为普通char
                    LoopState::InDoubleQuote => match chars.next() {
                        Some('"') => {
                            loop_state = LoopState::Normal;
                            cmd_buffer.push('"');
                        }
                        Some('\\') => {
                            cmd_buffer.push('\\');
                            if let Some(nxt) = chars.next() {
                                cmd_buffer.push(nxt);
                            } else {
                                return Err(ParseError::incomplete(
                                    *offset,
                                    "unterminated $(...) expansion",
                                ));
                            };
                        }
                        Some(ch) => {
                            cmd_buffer.push(ch);
                        }
                        None => {
                            return Err(ParseError::incomplete(
                                *offset,
                                "unterminated $(...) expansion",
                            ));
                        }
                    },
                    LoopState::InSingleQuote => match chars.next() {
                        Some('\'') => {
                            loop_state = LoopState::Normal;
                            cmd_buffer.push('\'');
                        }

                        Some(ch) => {
                            cmd_buffer.push(ch);
                        }
                        None => {
                            return Err(ParseError::incomplete(
                                *offset,
                                "unterminated $(...) expansion",
                            ));
                        }
                    },
                }
            }

            fragments.push(WordFragment::Cmd(cmd_buffer));

            Ok(())
        }
        Some('[') => {
            // 解析为 ecscript Expr,需要深度计数（处理引号和转义）
            flush_buffer(lit_buffer, fragments); // 把之前的字面量消费掉

            let _ = chars.next();
            let mut depth = 1;

            let mut expr_buffer = String::new();

            let mut is_spread = false;
            let mut dot_cnt = 0;

            while chars.next_if_eq(&'.').is_some() {
                dot_cnt += 1;
            }

            if dot_cnt == 3 {
                is_spread = true;
            } else {
                for _ in 0..dot_cnt {
                    expr_buffer.push('.');
                }
            }

            enum LoopState {
                Normal,
                InDoubleQuote,
                InSingleQuote,
            }
            let mut loop_state = LoopState::Normal;

            loop {
                match loop_state {
                    LoopState::Normal => match chars.next() {
                        Some(']') => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                            expr_buffer.push(']');
                        }
                        Some('[') => {
                            depth += 1;
                            expr_buffer.push('[');
                        }
                        Some('\\') => {
                            // 保留原始文本，留给之后的程序处理
                            expr_buffer.push('\\');
                            let Some(nxt) = chars.next() else {
                                return Err(ParseError::incomplete(
                                    *offset,
                                    "unterminated $[...] expansion",
                                ));
                            };
                            expr_buffer.push(nxt);
                        }
                        Some('"') => {
                            loop_state = LoopState::InDoubleQuote;
                            expr_buffer.push('"');
                        }
                        Some('\'') => {
                            loop_state = LoopState::InSingleQuote;
                            expr_buffer.push('\'');
                        }
                        Some(ch) => {
                            expr_buffer.push(ch);
                        }
                        None => {
                            return Err(ParseError::incomplete(
                                *offset,
                                "unterminated $[...] expansion",
                            ));
                        }
                    },
                    // 1. 括号不再影响整个dollar表达式
                    // 2. 处理转义
                    // 3. 不嵌套处理$，将其视为普通char
                    LoopState::InDoubleQuote => match chars.next() {
                        Some('"') => {
                            loop_state = LoopState::Normal;
                            expr_buffer.push('"');
                        }
                        Some('\\') => {
                            expr_buffer.push('\\');
                            if let Some(nxt) = chars.next() {
                                expr_buffer.push(nxt);
                            } else {
                                return Err(ParseError::incomplete(
                                    *offset,
                                    "unterminated $[...] expansion",
                                ));
                            };
                        }
                        Some(ch) => {
                            expr_buffer.push(ch);
                        }
                        None => {
                            return Err(ParseError::incomplete(
                                *offset,
                                "unterminated $[...] expansion",
                            ));
                        }
                    },
                    LoopState::InSingleQuote => match chars.next() {
                        Some('\'') => {
                            loop_state = LoopState::Normal;
                            expr_buffer.push('\'');
                        }

                        Some(ch) => {
                            expr_buffer.push(ch);
                        }
                        None => {
                            return Err(ParseError::incomplete(
                                *offset,
                                "unterminated $[...] expansion",
                            ));
                        }
                    },
                }
            }

            fragments.push(WordFragment::Expr {
                src: expr_buffer,
                spread: is_spread,
            });

            Ok(())
        }
        Some(ch) => {
            if ch == '_' || ch.is_ascii_alphabetic() {
                // 解析为普通var， 也就是先匹配ecscript本地变量，然后fallback到环境变量
                // `$NAME` 使用最长匹配：一直读到第一个非变量名字符为止。

                flush_buffer(lit_buffer, fragments); // 先提交之前的字面量部分

                let _ = chars.next();
                let mut var_buffer = String::new();
                var_buffer.push(ch);

                loop {
                    if let Some(succ) = chars.next_if(|ch| *ch == '_' || ch.is_ascii_alphanumeric())
                    {
                        var_buffer.push(succ);
                    } else {
                        break;
                    }
                }

                fragments.push(WordFragment::Var(var_buffer));
            } else {
                // `$` 后不是当前支持的展开形式，保留字面量 `$`。
                lit_buffer.push('$');
            }
            Ok(())
        }
    }
}
