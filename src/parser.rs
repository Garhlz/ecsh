use crate::lexer::tokenize;
use crate::types::{
    Command, OutputRedirection, ParsedLine, Pipeline, Redirection, ShellState, Token,
};

pub fn parse_line(line: &str, state: &ShellState) -> Result<ParsedLine, String> {
    let tokens = tokenize(line, state)?;
    parse_tokens(&tokens)
}

// 将 token 流转成当前 shell 的语法结构。
// 现在支持普通命令、管道，以及 `&&` / `||` 组成的递归控制流 AST。
fn parse_tokens(tokens: &[Token]) -> Result<ParsedLine, String> {
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

        // && 和 || 按同一优先级、左结合处理。这里从右侧找到最后一个逻辑操作符
        // 作为拆分点，使 `a || b && c` 解析成 `(a || b) && c`。
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

    if tokens.iter().any(|tok| matches!(tok, Token::Pipe)) {
        let pipeline = parse_pipeline(tokens)?;
        Ok(ParsedLine::Pipeline(pipeline))
    } else {
        // 单条最小指令，直接转换
        let command = parse_tokens_to_command(tokens)?;
        Ok(ParsedLine::Command(command))
    }
}

// 解析一段不包含管道和逻辑操作符的 token 序列。
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
    // 不断消费参数迭代器，同时识别重定向操作符及其路径参数。
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
            // 非重定向 token 作为普通参数保留。
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

fn next_redirection_path<'a>(
    words: &mut impl Iterator<Item = &'a Token>,
    operator: &str,
) -> Result<String, String> {
    // 重定向符之后必须跟随一个普通词作为路径。
    let Some(Token::Word(path)) = words.next() else {
        return Err(format!("missing filename after {}", operator));
    };
    Ok(path.to_string())
}
