use crate::ecscript::{Stmt, error::ParseError, lexer, parser, pratt};

/// 判断用户输入是顶层语句还是表达式。
///
/// REPL 需要区分两类输入：
/// - 表达式（如 `1 + 2`）：用 Pratt parser 解析后求值
/// - 顶层语句（如 `let x = 1`）：用递归下降 parser 解析为语句列表后执行
///
/// 若判断为顶层语句则返回 `Some(parse_result)`，否则返回 `None`
/// 由调用方按表达式路径处理。
///
/// 判断规则：
/// 1. 以顶层关键字开头 → 语句
/// 2. 以反引号命令字面量开头 → 语句
/// 3. 以 `ident =` / `ident.field =` / `ident[idx] =` 赋值形式开头 → 语句
/// 4. 其他 → 表达式
pub fn parse_top_level_script(src: &str) -> Option<Result<Vec<Stmt>, ParseError>> {
    let starts_with_keyword = starts_with_top_level_keyword(src);
    let tokens = match lexer::tokenize(src) {
        Ok(tokens) => tokens,
        Err(err) => {
            return if starts_with_keyword {
                Some(Err(err))
            } else {
                None
            };
        }
    };

    if let Some(lexer::Token {
        kind: lexer::TokenKind::Keyword(keyword),
        ..
    }) = tokens
        .iter()
        .find(|token| !matches!(token.kind, lexer::TokenKind::Newline))
        && keyword.is_top_level()
    {
        return Some(parser::parse_script(&tokens));
    }

    if matches!(
        tokens
            .iter()
            .find(|token| !matches!(token.kind, lexer::TokenKind::Newline))
            .map(|token| &token.kind),
        Some(lexer::TokenKind::CommandLiteral(_))
    ) {
        return Some(parser::parse_script(&tokens));
    }

    let mut stream = pratt::TokenStream::new(&tokens);
    stream.skip_newlines();
    if !is_assign_target(&mut stream) {
        return None;
    }

    if matches!(
        &stream.peek().kind,
        lexer::TokenKind::Delimiter(
            lexer::Delimiter::LParen
                | lexer::Delimiter::Eq
                | lexer::Delimiter::PlusEq
                | lexer::Delimiter::MinusEq
                | lexer::Delimiter::StarEq
                | lexer::Delimiter::SlashEq
                | lexer::Delimiter::PercentEq
        )
    ) {
        return Some(parser::parse_script(&tokens));
    }

    None
}

/// 检查源码是否以顶层关键字（let、if、while 等）开头。
fn starts_with_top_level_keyword(src: &str) -> bool {
    let trimmed = src.trim_start();
    [
        "let", "if", "while", "for", "continue", "break", "func", "return", "use", "pub",
    ]
    .into_iter()
    .any(|keyword| {
        trimmed
            .strip_prefix(keyword)
            .is_some_and(|rest| rest.is_empty() || !matches!(rest.chars().next(), Some(ch) if ch == '_' || ch.is_ascii_alphanumeric()))
    })
}

/// 消费可能的赋值/调用左侧目标，支持：
/// - `ident`
/// - `ident.field`
/// - `ident[idx]`
/// - `ident.field[idx].field` 这类链式后缀
///
/// 返回是否成功匹配。
fn is_assign_target(stream: &mut pratt::TokenStream<'_>) -> bool {
    if !matches!(stream.peek().kind, lexer::TokenKind::Identifier(_)) {
        return false;
    }
    stream.consume();

    loop {
        if stream.check(&lexer::TokenKind::Delimiter(lexer::Delimiter::Dot)) {
            stream.consume();
            if matches!(stream.peek().kind, lexer::TokenKind::Identifier(_)) {
                stream.consume();
                continue;
            }
            return false;
        }

        if stream.check(&lexer::TokenKind::Delimiter(lexer::Delimiter::LBracket)) {
            let mut depth = 1;
            stream.consume();
            loop {
                match &stream.peek().kind {
                    lexer::TokenKind::Delimiter(lexer::Delimiter::LBracket) => {
                        depth += 1;
                        stream.consume();
                    }
                    lexer::TokenKind::Delimiter(lexer::Delimiter::RBracket) => {
                        depth -= 1;
                        stream.consume();
                        if depth == 0 {
                            break;
                        }
                    }
                    lexer::TokenKind::EOF => return false,
                    _ => stream.consume(),
                }
            }
            continue;
        }

        return true;
    }
}

#[cfg(test)]
mod tests {
    use super::parse_top_level_script;

    fn parses_as_top_level_script(src: &str) -> bool {
        parse_top_level_script(src).is_some_and(|result| result.is_ok())
    }

    #[test]
    fn detects_index_assignment_as_script() {
        assert!(parses_as_top_level_script("a[2] = 1"));
    }

    #[test]
    fn detects_chained_index_assignment_as_script() {
        assert!(parses_as_top_level_script("e.b[1] = 200"));
    }

    #[test]
    fn detects_field_assignment_as_script() {
        assert!(parses_as_top_level_script("a.b = 1"));
    }

    #[test]
    fn leaves_bare_identifier_for_shell() {
        assert!(parse_top_level_script("a").is_none());
    }
}
