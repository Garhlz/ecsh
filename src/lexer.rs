//! 词法分析：将一行输入字符串拆成 Token 序列。
//!
//! 状态机有三种模式：Normal（正常）、SingleQuoted（单引号）、DoubleQuoted（双引号）。
//! 空格和操作符在 Normal 模式下作为 token 边界；引号内所有内容保留为字面量。

use crate::types::{LexerStatus, ShellState, Token};

/// 将一行输入拆分为 Token 向量。
///
/// 处理流程：
///   - 按字符遍历，根据当前 LexerStatus 选择不同的处理规则
///   - 遇到空格或操作符时，将当前累积的词 flush 为 Token::Word
///   - `$` 触发变量展开：支持 `$?`、`$NAME`、`${NAME}`
///   - 引号切换 lexer 状态：`'` 进入 SingleQuoted，`"` 进入 DoubleQuoted
///   - `\` 转义下一个字符
///
/// 返回 Err 的情况：未闭合引号、单独的结尾反斜杠、非法的变量展开语法。
pub fn tokenize(line: &str, state: &ShellState) -> Result<Vec<Token>, String> {
    let mut chars = line.chars().peekable();
    let mut lexer_status = LexerStatus::Normal;
    let mut acc_word: String = String::new();
    let mut result = Vec::new();

    while let Some(ch) = chars.next() {
        match lexer_status {
            // ── Normal 模式：操作符和空白有特殊意义 ──
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
                        flush_word(&mut result, &mut acc_word);
                        result.push(Token::Ampersand);
                    }
                },
                '<' => {
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
                    lexer_status = LexerStatus::SingleQuoted;
                }
                '\"' => {
                    lexer_status = LexerStatus::DoubleQuoted;
                }
                ';' => {
                    flush_word(&mut result, &mut acc_word);
                    result.push(Token::Semicolon);
                }
                '\\' => {
                    match chars.peek().copied() {
                        None => {
                            return Err("trailing backslash".to_string());
                        }
                        Some(ch) => {
                            let _ = chars.next();
                            acc_word.push(ch);
                        }
                    }
                }
                _ => {
                    acc_word.push(ch);
                }
            },

            // ── 单引号模式：所有字符按字面量处理，不被识别为操作符 ──
            LexerStatus::SingleQuoted => match ch {
                '\'' => {
                    lexer_status = LexerStatus::Normal;
                }
                _ => acc_word.push(ch),
            },

            // ── 双引号模式：保留空白和操作符字面量，但 $ 和 \" 仍会被处理 ──
            LexerStatus::DoubleQuoted => match ch {
                '\"' => lexer_status = LexerStatus::Normal,
                '$' => handle_dollar(&mut chars, &mut acc_word, state)?,
                '\\' => match chars.peek().copied() {
                    None => {
                        return Err("trailing backslash in double quotes".to_string());
                    }
                    Some(ch) if matches!(ch, '\"' | '$' | '\\') => {
                        let _ = chars.next();
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

    // 输入结束但引号未闭合 → 报错。
    if let LexerStatus::DoubleQuoted = lexer_status {
        return Err("unterminated double quote".to_string());
    }
    if let LexerStatus::SingleQuoted = lexer_status {
        return Err("unterminated single quote".to_string());
    }

    flush_word(&mut result, &mut acc_word);
    Ok(result)
}

/// 将当前累积的词提交为 Token::Word，并重置 acc_word。
///
/// 使用 `std::mem::take` 把 String 所有权移交给 Token，避免 clone。
fn flush_word(result: &mut Vec<Token>, acc_word: &mut String) {
    if !acc_word.is_empty() {
        result.push(Token::Word(std::mem::take(acc_word)));
    }
}

/// 展开变量：`$?` 取上一条命令退出码；`$NAME` 取环境变量值。
///
/// 未定义的环境变量展开为空字符串（与 bash/zsh 行为一致）。
fn expand_variable(name: &str, state: &ShellState) -> String {
    if name == "?" {
        state.last_status.code.to_string()
    } else {
        std::env::var(name).unwrap_or_default()
    }
}

/// 检查字符是否可作为变量名的首字符：[A-Za-z_]。
fn is_variable_name_start(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphabetic()
}

/// 检查字符是否可作为变量名的非首字符：[A-Za-z0-9_]。
fn is_variable_name_continue(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

/// 处理 `$` 后面的变量展开语法。
///
/// 进入此函数时，外层已经消费了 `$`。
/// 支持的语法：
///   - `$?`      → 展开为上一条命令的退出码
///   - `$NAME`   → 最长匹配变量名，展开为环境变量值
///   - `${NAME}` → 花括号明确变量名边界，只展开 `${NAME}`，花括号不进入结果
///   - 其他       → `$` 保留为字面量
///
/// 未定义变量展开为空字符串。
fn handle_dollar(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    acc_word: &mut String,
    state: &ShellState,
) -> Result<(), String> {
    match chars.peek().copied() {
        None => {
            // 行尾的孤 $，保留字面量。
            acc_word.push('$');
            Ok(())
        }
        Some('?') => {
            let _ = chars.next();
            let expanded_word = expand_variable("?", state);
            acc_word.push_str(&expanded_word);
            Ok(())
        }
        Some('{') => {
            // `${NAME}` 中花括号只是变量名边界，不出现在展开结果中。
            let _ = chars.next();

            match chars.peek().copied() {
                Some('}') => Err("empty variable name in braces".to_string()),
                Some(start) if is_variable_name_start(start) => {
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
                        break;
                    }
                }
                let expanded_word = expand_variable(&origin_word, state);
                acc_word.push_str(expanded_word.as_str());
            } else {
                // `$` 后不是当前支持的展开形式，保留字面量 `$`。
                acc_word.push('$');
            }
            Ok(())
        }
    }
}
