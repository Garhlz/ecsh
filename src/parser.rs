use crate::types::{Command, OutputRedirection, ParsedLine, Pipeline, Redirection};

// 对 shell 而言，空命令不是错误。用户直接按下 Enter 时，应进入下一轮提示符。
fn parse_args(line: &str) -> Result<Option<Command>, String> {
    // 后续会持续消费迭代器，因此需要可变绑定。
    let mut words = line.split_whitespace();
    let Some(program) = words.next() else {
        return Ok(None);
    };

    let program = program.to_string();
    let mut args = Vec::new();
    let mut redirection = Redirection::default();

    // 不断消费参数迭代器，同时识别重定向操作符及其路径参数。
    while let Some(word) = words.next() {
        match word {
            "<" => {
                let path = next_redirection_path(&mut words, "<")?;
                if redirection.stdin.is_some() {
                    return Err("duplicate stdin redirection".to_string());
                }
                redirection.stdin = Some(path);
            }
            ">" => {
                let path = next_redirection_path(&mut words, ">")?;
                if redirection.stdout.is_some() {
                    return Err("duplicate stdout redirection".to_string());
                }
                redirection.stdout = Some(OutputRedirection::Truncate(path));
            }
            ">>" => {
                let path = next_redirection_path(&mut words, ">>")?;
                if redirection.stdout.is_some() {
                    return Err("duplicate stdout redirection".to_string());
                }
                redirection.stdout = Some(OutputRedirection::Append(path));
            }
            // 非重定向 token 作为普通参数保留。
            _ => args.push(word.to_string()),
        }
    }

    Ok(Some(Command {
        program,
        args,
        redirection,
    }))
}

fn next_redirection_path<'a>(
    words: &mut impl Iterator<Item = &'a str>,
    operator: &str,
) -> Result<String, String> {
    // 重定向符之后必须跟随一个路径 token。
    let Some(path) = words.next() else {
        return Err(format!("missing filename after {}", operator));
    };

    // 路径位置不能再次出现重定向符号。
    if is_redirection_operator(path) {
        return Err(format!("missing filename after {}", operator));
    }

    Ok(path.to_string())
}

fn is_redirection_operator(word: &str) -> bool {
    matches!(word, "<" | ">" | ">>")
}
// 将一行输入转换为当前支持的两种结构：普通命令或管道命令。
// 之后需进一步扩展
pub fn parse_line(line: &str) -> Result<Option<ParsedLine>, String> {
    if line.contains('|') {
        match parse_pipeline(line) {
            Ok(None) => Ok(None),
            Ok(Some(pipeline)) => Ok(Some(ParsedLine::Pipeline(pipeline))),
            Err(err) => Err(err),
        }
    } else {
        parse_args(line).map(|command| command.map(ParsedLine::Command))
    }
}

fn parse_pipeline(line: &str) -> Result<Option<Pipeline>, String> {
    if !line.contains('|') {
        return Ok(None);
    }

    let commands = line
        .split('|')
        .map(str::trim)
        // 如果某一段解析为空命令，说明管道中存在非法的空段。
        .map(|part| {
            parse_args(part)
                .and_then(|command| command.ok_or_else(|| "empty command in pipeline".to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Some(Pipeline { commands }))
}
