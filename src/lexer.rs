use crate::types::{LexerStatus, ShellState, Token};

pub fn tokenize(line: &str, state: &ShellState) -> Result<Vec<Token>, String> {
    let mut chars = line.chars().peekable();
    let mut lexer_status = LexerStatus::Normal;
    let mut acc_word: String = String::new();
    let mut result = Vec::new();

    while let Some(ch) = chars.next() {
        match lexer_status {
            LexerStatus::Normal => match ch {
                ch if ch.is_whitespace() => {
                    flush_word(&mut result, &mut acc_word);
                }
                '|' => match chars.peek().copied() {
                    None => {
                        flush_word(&mut result, &mut acc_word);
                        result.push(Token::Pipe);
                    }
                    Some('|') => {
                        // `peek()` 只查看第二个 `|`，这里再消费它，形成 `||` token。
                        let _ = chars.next();
                        flush_word(&mut result, &mut acc_word);
                        result.push(Token::OrIf);
                    }
                    Some(_) => {
                        flush_word(&mut result, &mut acc_word);
                        result.push(Token::Pipe);
                    }
                },
                '&' => match chars.peek().copied() {
                    Some('&') => {
                        let _ = chars.next();
                        flush_word(&mut result, &mut acc_word);
                        result.push(Token::AndIf);
                    }
                    _ => {
                        // 单个 `&` 通常表示后台执行。当前还未实现这一语义，
                        // 因此这里直接返回错误，避免把它误当成普通字符。
                        return Err("single '&' is not supported yet".to_string());
                    }
                },
                '<' => {
                    // 当前只支持 `<`。`<<` here-doc 需要额外的读取规则，暂不处理。
                    flush_word(&mut result, &mut acc_word);
                    result.push(Token::RedirectionIn);
                }
                '>' => match chars.peek().copied() {
                    None => {
                        flush_word(&mut result, &mut acc_word);
                        result.push(Token::RedirectionTruncate);
                    }
                    Some('>') => {
                        let _ = chars.next();
                        flush_word(&mut result, &mut acc_word);
                        result.push(Token::RedirectionAppend);
                    }
                    Some(_) => {
                        flush_word(&mut result, &mut acc_word);
                        result.push(Token::RedirectionTruncate);
                    }
                },
                '$' => handle_dollar(&mut chars, &mut acc_word, state)?,
                '\'' => {
                    // 引号只改变当前词内部的解释规则，不会自动结束当前词。
                    // 因此 `a"b"c` 应被解析成一个 Word("abc")。
                    lexer_status = LexerStatus::SingleQuoted;
                }
                '\"' => {
                    lexer_status = LexerStatus::DoubleQuoted;
                }
                ';' => {
                    // 只有Normal状态下才是操作符
                    flush_word(&mut result, &mut acc_word);
                    result.push(Token::Semicolon);
                }
                '\\' => {
                    match chars.peek().copied() {
                        None => {
                            // 单独一个结尾反斜杠，在最小实现中直接报错
                            // TODO 完整 shell 里可能会把 \newline 当作行续接
                            return Err("trailing backslash".to_string());
                        }
                        Some(ch) => {
                            // // 反斜杠会吞掉下一个字符，并把它作为普通字符加入当前 word。
                            let _ = chars.next();
                            acc_word.push(ch);
                        }
                    }
                }
                _ => {
                    acc_word.push(ch);
                }
            },
            // 单引号内所有字符都按字面量处理，不进行变量展开，也不识别操作符。
            LexerStatus::SingleQuoted => match ch {
                '\'' => {
                    lexer_status = LexerStatus::Normal;
                }
                _ => acc_word.push(ch),
            },

            // 双引号内保留空白和操作符字面量，但仍支持当前最小变量展开。
            LexerStatus::DoubleQuoted => match ch {
                '\"' => lexer_status = LexerStatus::Normal,
                '$' => handle_dollar(&mut chars, &mut acc_word, state)?,
                '\\' => match chars.peek().copied() {
                    None => {
                        return Err("trailing backslash in double quotes".to_string());
                    }
                    Some(ch) if matches!(ch, '\"' | '$' | '\\') => {
                        // 这几个特殊符号失去特殊含义
                        let _ = chars.next(); // 消费掉
                        acc_word.push(ch);
                    }
                    Some(ch) => {
                        let _ = chars.next();
                        acc_word.push('\\');
                        acc_word.push(ch);
                    }
                },
                _ => acc_word.push(ch),
            },
        }
    }

    // EOF 时仍处于引号状态，说明用户输入缺少对应的右引号。
    if let LexerStatus::DoubleQuoted = lexer_status {
        return Err("unterminated double quote".to_string());
    }
    if let LexerStatus::SingleQuoted = lexer_status {
        return Err("unterminated single quote".to_string());
    }

    flush_word(&mut result, &mut acc_word);
    Ok(result)
}

// 将当前累计的词提交为 Token::Word，并把 acc_word 重置为空串。
// `std::mem::take` 会把原 String 的所有权移交给 Token，避免 clone。
fn flush_word(result: &mut Vec<Token>, acc_word: &mut String) {
    if !acc_word.is_empty() {
        result.push(Token::Word(std::mem::take(acc_word)));
    }
}

fn expand_variable(name: &str, state: &ShellState) -> String {
    if name == "?" {
        state.last_status.code.to_string()
    } else {
        // 对当前最小实现来说，未定义环境变量直接展开为空串，行为更接近常见 shell。
        std::env::var(name).unwrap_or_default()
    }
}

fn is_variable_name_start(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphabetic()
}

fn is_variable_name_continue(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

// 进入该函数时，外层已经消费了 `$`。
// 当前支持 `$?`、`$NAME`、`${NAME}`；未定义环境变量展开为空串。
fn handle_dollar(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    acc_word: &mut String,
    state: &ShellState,
) -> Result<(), String> {
    match chars.peek().copied() {
        None => {
            // 把这个 $ 加到前面的词中
            acc_word.push('$');
            // 让外层在真正遇到空白/操作符/输入结束时再统一 flush
            // flush_word(&mut result, &mut acc_word);
            Ok(())
        }
        Some('?') => {
            let _ = chars.next();
            let expanded_word = expand_variable("?", state);
            acc_word.push_str(&expanded_word);
            Ok(())
        }
        Some('{') => {
            // `${NAME}` 中花括号只是变量名边界，不会出现在展开结果中。
            let _ = chars.next(); // 消费掉左花括号

            match chars.peek().copied() {
                Some('}') => Err("empty variable name in braces".to_string()),
                Some(start) if is_variable_name_start(start) => {
                    // 当前只支持环境变量名形式：[A-Za-z_][A-Za-z0-9_]*。
                    let mut origin_word = String::new();
                    origin_word.push(start);
                    let _ = chars.next();
                    let mut flag = false;
                    while let Some(successor) = chars.peek().copied() {
                        if is_variable_name_continue(successor) {
                            let _ = chars.next();
                            origin_word.push(successor);
                        } else if successor == '}' {
                            let _ = chars.next();
                            flag = true;
                            break;
                        } else {
                            break;
                        }
                    }
                    if !flag {
                        return Err("unterminated ${...} expansion".to_string());
                    }
                    let expanded_word = expand_variable(&origin_word, state);
                    acc_word.push_str(expanded_word.as_str());
                    Ok(())
                }
                Some(_) => Err("invalid variable name in braces".to_string()),
                None => Err("unterminated ${...} expansion".to_string()),
            }
        }
        Some(start) => {
            if is_variable_name_start(start) {
                // `$NAME` 使用最长匹配：一直读到第一个非变量名字符为止。
                let mut origin_word = String::new();
                origin_word.push(start);
                let _ = chars.next();
                while let Some(successor) = chars.peek().copied() {
                    if is_variable_name_continue(successor) {
                        let _ = chars.next();
                        origin_word.push(successor);
                    } else {
                        // 不提前消费 不满足规则的字符
                        break;
                    }
                }
                let expanded_word = expand_variable(&origin_word, state);
                acc_word.push_str(expanded_word.as_str());
            } else {
                // `$` 后不是当前支持的展开形式时，保留字面量 `$`。
                acc_word.push('$');
            }
            Ok(())
        }
    }
}
