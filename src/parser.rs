use crate::types::{Command, ParsedLine, Pipeline};

// 对 shell 而言，空命令不是错误。用户直接按下 Enter 时，应进入下一轮提示符。
fn parse_args(line: &str) -> Option<Command> {
    let mut iter = line.split_whitespace().map(|word| word.to_string());
    let program = iter.next()?;
    let args = iter.collect();
    Some(Command { program, args })
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
        match parse_args(line) {
            Some(command) => Ok(Some(ParsedLine::Command(command))),
            None => Ok(None),
        }
    }
}

fn parse_pipeline(line: &str) -> Result<Option<Pipeline>, String> {
    if !line.contains('|') {
        return Ok(None);
    }

    let commands = line
        .split('|')
        .map(str::trim)
        // 如果 parse_args 返回 None ，说明某一段命令为空。
        .map(|part| parse_args(part).ok_or_else(|| "empty command in pipeline".to_string()))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Some(Pipeline { commands }))
}
