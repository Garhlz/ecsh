//! 语法分析：将 Token 序列解析为 AST（ParsedLine）。
//!
//! 采用递归下降解析，运算符优先级从低到高：
//!   1. `;`（最低，顺序执行）
//!   2. `&&` / `||`（逻辑控制流）
//!   3. `|`（管道）
//!   4. 单条命令
//!
//! 所有二元操作符采用左结合，通过 rposition 从右向左找到分割点来递归拆分。
//! 错误统一通过 `ParseError` 返回。

use crate::ecscript::Environment;
use crate::ecscript::error::ParseError;
use crate::ecscript::value::CommandValue;
use crate::lexer::tokenize;
use crate::types::{
    Command, OutputRedirection, ParsedJob, ParsedLine, Pipeline, Redirection, ShellState,
    ShellWord, Token,
};
use std::collections::HashMap;
use std::rc::Rc;

fn err(msg: impl Into<String>) -> ParseError {
    ParseError::new(0, msg)
}

/// 对外入口，将用户输入的一行文本解析为 ParsedJob。
pub fn parse_line(line: &str, state: &ShellState) -> Result<ParsedJob, ParseError> {
    let tokens = tokenize(line, state)?;
    let tokens = expand_alias_tokens(tokens, state, 0)?;
    parse_input(line, &tokens)
}

fn expand_alias_tokens(
    mut tokens: Vec<Token>,
    state: &ShellState,
    depth: usize,
) -> Result<Vec<Token>, ParseError> {
    const MAX_ALIAS_EXPANSION_DEPTH: usize = 8;
    if depth >= MAX_ALIAS_EXPANSION_DEPTH {
        return Err(err("alias expansion exceeded maximum depth"));
    }

    let Some(Token::Word(word)) = tokens.first() else {
        return Ok(tokens);
    };
    let Some(name) = word.as_lit_str() else {
        return Ok(tokens);
    };
    let Some(alias) = state.aliases.get(name) else {
        return Ok(tokens);
    };

    let mut expanded = tokenize(alias, state)?;
    expanded.extend(tokens.drain(1..));
    expand_alias_tokens(expanded, state, depth + 1)
}

fn parse_input(line: &str, tokens: &[Token]) -> Result<ParsedJob, ParseError> {
    let command_line = line.trim().to_string();

    if let Some(pos) = tokens
        .iter()
        .position(|tok| matches!(tok, Token::Ampersand))
    {
        if pos != tokens.len() - 1 {
            return Err(err(
                "background '&' is only supported at the end of a command",
            ));
        }
        let body_tokens = &tokens[..pos];
        if body_tokens.is_empty() {
            return Err(err("missing command before &"));
        }
        let parsed = parse_tokens(body_tokens)?;
        if !matches!(parsed, ParsedLine::Command(_) | ParsedLine::Pipeline(_)) {
            return Err(err(
                "background execution is only supported for a single command or pipeline",
            ));
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

fn parse_tokens(tokens: &[Token]) -> Result<ParsedLine, ParseError> {
    // ── 优先级 1：`;`（最低）──
    if let Some(pos) = tokens
        .iter()
        .rposition(|tok| matches!(tok, Token::Semicolon))
    {
        let (left, rest) = tokens.split_at(pos);
        let (_op, right) = rest
            .split_first()
            .ok_or_else(|| err("missing command around ;"))?;
        if left.is_empty() {
            return Err(err("missing command before ;"));
        }
        if right.is_empty() {
            return Err(err("missing command after ;"));
        }
        return Ok(ParsedLine::Sequence(
            Rc::new(parse_tokens(left)?),
            Rc::new(parse_tokens(right)?),
        ));
    }

    // ── 优先级 2：`&&` / `||` ──
    if let Some(pos) = tokens
        .iter()
        .rposition(|tok| matches!(tok, Token::AndIf | Token::OrIf))
    {
        let op = tokens.get(pos).ok_or_else(|| err("missing operator"))?;
        let op_text = match op {
            Token::AndIf => "&&",
            Token::OrIf => "||",
            _ => unreachable!(),
        };
        let (left, rest) = tokens.split_at(pos);
        let (_op, right) = rest
            .split_first()
            .ok_or_else(|| err(format!("missing command around {}", op_text)))?;
        if left.is_empty() {
            return Err(err(format!("missing command before {}", op_text)));
        }
        if right.is_empty() {
            return Err(err(format!("missing command after {}", op_text)));
        }
        let left_parsed = parse_tokens(left)?;
        let right_parsed = parse_tokens(right)?;
        if matches!(op, Token::AndIf) {
            return Ok(ParsedLine::AndThen(
                Rc::new(left_parsed),
                Rc::new(right_parsed),
            ));
        }
        return Ok(ParsedLine::OrElse(
            Rc::new(left_parsed),
            Rc::new(right_parsed),
        ));
    }

    // ── 优先级 3：`|`（管道）──
    if tokens.iter().any(|tok| matches!(tok, Token::Pipe)) {
        return Ok(ParsedLine::Pipeline(parse_pipeline(tokens)?));
    }

    // ── 基础情况：单条命令 ──
    Ok(ParsedLine::Command(parse_tokens_to_command(tokens)?))
}

/// 解析 `cmd{ ... }` 内部的受限 shell 命令字面量。
///
/// 当前支持单命令、重定向和 pipeline，不支持 `&&`、`||`、`;`、`&`。
pub fn parse_command_literal(src: &str) -> Result<CommandValue, ParseError> {
    let state = ShellState {
        last_status: crate::types::CommandStatus::success(),
        interactive: false,
        shell_pgid: None,
        shell_terminal_fd: None,
        jobs: Vec::new(),
        next_job_id: 1,
        current_fg_pgid: None,
        script_env: Environment::new(),
        aliases: HashMap::new(),
        traps: HashMap::new(),
        command_history: Vec::new(),
    };
    let tokens = tokenize(src, &state)?;
    if tokens.is_empty() {
        return Err(err("empty command literal"));
    }
    if tokens.iter().any(|tok| {
        matches!(
            tok,
            Token::AndIf | Token::OrIf | Token::Ampersand | Token::Semicolon
        )
    }) {
        return Err(err(
            "command literals do not support &&, ||, ;, or background &",
        ));
    }
    if tokens.iter().any(|tok| matches!(tok, Token::Pipe)) {
        return Ok(CommandValue::Pipeline(parse_pipeline(&tokens)?));
    }
    Ok(CommandValue::Simple(parse_tokens_to_command(&tokens)?))
}

fn parse_tokens_to_command(tokens: &[Token]) -> Result<Command, ParseError> {
    if tokens.is_empty() {
        return Err(err("empty command"));
    }
    let mut words = tokens.iter();
    let Some(Token::Word(program)) = words.next() else {
        return Err(err("command must start with a word"));
    };
    let mut args: Vec<ShellWord> = Vec::new();
    let mut redirection = Redirection::default();

    while let Some(tok) = words.next() {
        match tok {
            Token::RedirectionIn => {
                let path = next_redirection_path(&mut words, "<")?;
                if redirection.stdin.is_some() {
                    return Err(err("duplicate stdin redirection"));
                }
                redirection.stdin = Some(path);
            }
            Token::RedirectionTruncate => {
                let path = next_redirection_path(&mut words, ">")?;
                if redirection.stdout.is_some() {
                    return Err(err("duplicate stdout redirection"));
                }
                redirection.stdout = Some(OutputRedirection::Truncate(path));
            }
            Token::RedirectionAppend => {
                let path = next_redirection_path(&mut words, ">>")?;
                if redirection.stdout.is_some() {
                    return Err(err("duplicate stdout redirection"));
                }
                redirection.stdout = Some(OutputRedirection::Append(path));
            }
            Token::Word(word) => args.push(word.clone()),
            _ => return Err(err("unexpected token in command")),
        }
    }

    Ok(Command {
        program: program.clone(),
        args,
        redirection,
    })
}

fn parse_pipeline(tokens: &[Token]) -> Result<Pipeline, ParseError> {
    let commands = tokens
        .split(|tok| matches!(tok, Token::Pipe))
        .map(|segment| {
            if segment.is_empty() {
                return Err(err("empty command in pipeline"));
            }
            parse_tokens_to_command(segment)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Pipeline { commands })
}

fn next_redirection_path<'a>(
    words: &mut impl Iterator<Item = &'a Token>,
    operator: &str,
) -> Result<ShellWord, ParseError> {
    let Some(Token::Word(path)) = words.next() else {
        return Err(err(format!("missing filename after {}", operator)));
    };
    Ok(path.clone())
}
