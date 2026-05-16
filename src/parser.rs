//! 语法分析：将 Token 序列解析为 AST（ParsedLine）。
//!
//! 采用递归下降解析，运算符优先级从低到高：
//!   1. `;`（最低，顺序执行）
//!   2. `&&` / `||`（逻辑控制流）
//!   3. `|`（管道）
//!   4. 单条命令
//!
//! 所有二元操作符采用左结合，通过 rposition 从右向左找到分割点来递归拆分。

use crate::lexer::tokenize;
use crate::types::{
    Command, OutputRedirection, ParsedJob, ParsedLine, Pipeline, Redirection, ShellState, Token,
};

/// 对外入口：将用户输入的一行文本解析为 ParsedJob。
///
/// 流程：tokenize → parse_input → ParsedJob
/// 返回的 ParsedJob 包含语法结构、前后台标志和命令原文。
pub fn parse_line(line: &str, state: &ShellState) -> Result<ParsedJob, String> {
    let tokens = tokenize(line, state)?;
    parse_input(line, &tokens)
}

/// 解析整行 token 序列，处理 `&` 后台标志并生成 ParsedJob。
///
/// 规则：
///   - 行尾 `&` → 整条命令/管道在后台运行
///   - 教学版限制：`&` 必须在行尾，且只支持单命令或管道
///   - 不支持 `true && echo ok &` 这类复合控制流后台运行
fn parse_input(line: &str, tokens: &[Token]) -> Result<ParsedJob, String> {
    let command_line = line.trim().to_string();

    if let Some(pos) = tokens
        .iter()
        .position(|tok| matches!(tok, Token::Ampersand))
    {
        if pos != tokens.len() - 1 {
            return Err("background '&' is only supported at the end of a command".to_string());
        }

        let body_tokens = &tokens[..pos];
        if body_tokens.is_empty() {
            return Err("missing command before &".to_string());
        }

        let parsed = parse_tokens(body_tokens)?;
        if !matches!(parsed, ParsedLine::Command(_) | ParsedLine::Pipeline(_)) {
            return Err(
                "background execution is only supported for a single command or pipeline"
                    .to_string(),
            );
        }

        return Ok(ParsedJob {
            line: parsed,
            background: true,
            command_line,
        });
    }

    Ok(ParsedJob {
        line: parse_tokens(tokens)?,
        background: false,
        command_line,
    })
}

/// 递归下降解析 token 序列：按优先级从低到高找分割点。
///
/// 算法：对每种运算符，用 rposition 从右向左找到分割点（保证左结合），
/// 递归处理左右两边，构建二元 AST 节点。
/// 越先处理的操作符优先级越低（因为在 AST 中是上层节点）。
fn parse_tokens(tokens: &[Token]) -> Result<ParsedLine, String> {
    // ── 优先级 1：`;`（最低）──
    if let Some(pos) = tokens
        .iter()
        .rposition(|tok| matches!(tok, Token::Semicolon))
    {
        let (left, rest) = tokens.split_at(pos);
        let (_op, right) = rest
            .split_first()
            .ok_or_else(|| "missing command around ;".to_string())?;

        if left.is_empty() {
            return Err("missing command before ;".to_string());
        }
        if right.is_empty() {
            return Err("missing command after ;".to_string());
        }

        let left_parsed = parse_tokens(left)?;
        let right_parsed = parse_tokens(right)?;
        return Ok(ParsedLine::Sequence(
            Box::new(left_parsed),
            Box::new(right_parsed),
        ));
    }

    // ── 优先级 2：`&&` / `||` ──
    // 同为左结合、同一优先级。从最右找到分割点，
    // 使 `a || b && c` 解析成 `(a || b) && c`。
    if let Some(pos) = tokens
        .iter()
        .rposition(|tok| matches!(tok, Token::AndIf | Token::OrIf))
    {
        let op = tokens
            .get(pos)
            .ok_or_else(|| "missing operator".to_string())?;
        let op_text = match op {
            Token::AndIf => "&&",
            Token::OrIf => "||",
            _ => unreachable!("operator position should only match && or ||"),
        };

        let (left, rest) = tokens.split_at(pos);
        let (_op, right) = rest
            .split_first()
            .ok_or_else(|| format!("missing command around {}", op_text))?;

        if left.is_empty() {
            return Err(format!("missing command before {}", op_text));
        }
        if right.is_empty() {
            return Err(format!("missing command after {}", op_text));
        }

        let left_parsed = parse_tokens(left)?;
        let right_parsed = parse_tokens(right)?;

        if matches!(op, Token::AndIf) {
            return Ok(ParsedLine::AndThen(
                Box::new(left_parsed),
                Box::new(right_parsed),
            ));
        } else if matches!(op, Token::OrIf) {
            return Ok(ParsedLine::OrElse(
                Box::new(left_parsed),
                Box::new(right_parsed),
            ));
        }
    }

    // ── 优先级 3：`|`（管道）──
    if tokens.iter().any(|tok| matches!(tok, Token::Pipe)) {
        let pipeline = parse_pipeline(tokens)?;
        Ok(ParsedLine::Pipeline(pipeline))
    } else {
        // ── 基础情况：单条命令 ──
        let command = parse_tokens_to_command(tokens)?;
        Ok(ParsedLine::Command(command))
    }
}

/// 将不含管道和逻辑操作符的 token 序列解析为单条 Command。
///
/// 处理内容：
///   - 第一个 Word 作为程序名
///   - 后续 Word 作为参数
///   - `<` / `>` / `>>` 作为重定向操作符，后跟路径
///   - 不允许重复重定向（如 `> a > b` 报错）
fn parse_tokens_to_command(tokens: &[Token]) -> Result<Command, String> {
    if tokens.is_empty() {
        return Err("empty command".to_string());
    }

    let mut words = tokens.iter();
    let Some(Token::Word(program)) = words.next() else {
        return Err("command must start with a word".to_string());
    };
    let program = program.to_string();
    let mut args = Vec::new();
    let mut redirection = Redirection::default();

    while let Some(tok) = words.next() {
        match tok {
            Token::RedirectionIn => {
                let path = next_redirection_path(&mut words, "<")?;
                if redirection.stdin.is_some() {
                    return Err("duplicate stdin redirection".to_string());
                }
                redirection.stdin = Some(path);
            }
            Token::RedirectionTruncate => {
                let path = next_redirection_path(&mut words, ">")?;
                if redirection.stdout.is_some() {
                    return Err("duplicate stdout redirection".to_string());
                }
                redirection.stdout = Some(OutputRedirection::Truncate(path));
            }
            Token::RedirectionAppend => {
                let path = next_redirection_path(&mut words, ">>")?;
                if redirection.stdout.is_some() {
                    return Err("duplicate stdout redirection".to_string());
                }
                redirection.stdout = Some(OutputRedirection::Append(path));
            }
            Token::Word(word) => args.push(word.to_string()),
            _ => return Err("unexpected token in command".to_string()),
        }
    }

    Ok(Command {
        program,
        args,
        redirection,
    })
}

/// 解析管道：按 `|` 分割 token，每段解析为一条 Command。
fn parse_pipeline(tokens: &[Token]) -> Result<Pipeline, String> {
    let commands = tokens
        .split(|tok| matches!(tok, Token::Pipe))
        .map(|segment| {
            if segment.is_empty() {
                return Err("empty command in pipeline".to_string());
            }
            parse_tokens_to_command(segment)
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Pipeline { commands })
}

/// 从 token 迭代器中取出下一个 Word 作为重定向路径。
///
/// 如果下一个 token 不是 Word，返回错误（如 `echo >` 缺少文件名）。
fn next_redirection_path<'a>(
    words: &mut impl Iterator<Item = &'a Token>,
    operator: &str,
) -> Result<String, String> {
    let Some(Token::Word(path)) = words.next() else {
        return Err(format!("missing filename after {}", operator));
    };
    Ok(path.to_string())
}
