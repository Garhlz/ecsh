//! 词法分析：将一行 shell 输入拆成 `Token` 序列。
//!
//! lexer 只负责识别顶层操作符、引号边界和 shell word 的 fragment 结构；
//! `$...` 的实际求值继续延后到执行阶段。

use crate::ecscript::error::ParseError;
use crate::types::{LexerStatus, ShellState, ShellWord, Token, WordFragment};

/// 将一行输入拆成 token 序列。
///
/// 这里同时负责把普通单词收集成 `ShellWord`，并把 `$VAR`、`${VAR}`、
/// `$(cmd)`、`$[expr]` 等结构编码成 `WordFragment`。
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
            // Normal 模式下，空白和 shell 操作符会切开 token。
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

            // 单引号里完全按字面量处理，不再识别 `$` 或操作符。
            LexerStatus::SingleQuoted => match ch {
                '\'' => {
                    lexer_status = LexerStatus::Normal;
                }
                _ => lit_buffer.push(ch),
            },

            // 双引号保留字面量边界，但仍支持 `$` 展开和有限的反斜杠转义。
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

    if let LexerStatus::DoubleQuoted = lexer_status {
        return Err(ParseError::incomplete(offset, "unterminated double quote"));
    }
    if let LexerStatus::SingleQuoted = lexer_status {
        return Err(ParseError::incomplete(offset, "unterminated single quote"));
    }

    flush_word(&mut lit_buffer, &mut fragments, &mut tokens);
    Ok(tokens)
}

/// 把当前字面量缓冲区提交为一个 `Lit` fragment。
fn flush_buffer(lit_buffer: &mut String, fragments: &mut Vec<WordFragment>) {
    if !lit_buffer.is_empty() {
        fragments.push(WordFragment::Lit(std::mem::take(lit_buffer)));
    }
}

/// 结束当前 shell word，并把它提交为一个 `Token::Word`。
fn flush_word(lit_buffer: &mut String, fragments: &mut Vec<WordFragment>, tokens: &mut Vec<Token>) {
    flush_buffer(lit_buffer, fragments);
    if !fragments.is_empty() {
        tokens.push(Token::Word(ShellWord {
            fragments: std::mem::take(fragments),
        }));
    }
}

/// 解析 `$` 开头的 shell word fragment。
///
/// 支持的形式包括：
/// - `$VAR`
/// - `${VAR}`
/// - `$(cmd)`
/// - `$[expr]` / `$[...expr]`
///
/// 任何不属于这些形式的 `$` 都退化为普通字面量。
fn handle_dollar(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    lit_buffer: &mut String,
    fragments: &mut Vec<WordFragment>,
    offset: &mut usize,
) -> Result<(), ParseError> {
    match chars.peek().copied() {
        None => {
            lit_buffer.push('$');
            Ok(())
        }
        Some('?') => {
            // `$?` 走和普通变量一致的 fragment 表示，后面由执行阶段解释。
            let _ = chars.next();
            flush_buffer(lit_buffer, fragments);
            fragments.push(WordFragment::Var("?".to_string()));
            Ok(())
        }
        Some('{') => {
            // `${VAR}` 只表示环境变量，不参与脚本作用域 fallback。
            flush_buffer(lit_buffer, fragments);
            let _ = chars.next();

            match chars.next() {
                Some('}') => Err(ParseError::new(*offset, "empty variable name in braces")),
                Some(start) if start == '_' || start.is_ascii_alphabetic() => {
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
            // `$(...)` 需要自己维护括号深度，同时保留内部原始文本。
            flush_buffer(lit_buffer, fragments);
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
                    // 引号内部只维护引号自己的结束条件，不再额外解释括号或 `$`。
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
            // `$[...]` 复用和 `$(...)` 类似的扫描策略，但括号改成 `[]`。
            let _ = chars.next();
            flush_buffer(lit_buffer, fragments);
            let mut depth = 1;
            let mut expr_buffer = String::new();

            // 仅在最开头识别 `...` 作为 spread 标记，其余 `.` 都按源码保留。
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
                    // 引号内部不再参与 `[]` 深度计算，只等待对应引号闭合。
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
                // `$NAME` 采用最长匹配，执行阶段再做脚本变量优先的查找策略。
                flush_buffer(lit_buffer, fragments);
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
                lit_buffer.push('$');
            }
            Ok(())
        }
    }
}
