use crate::types::{Command, OutputRedirection, ParsedLine, Pipeline, Redirection, ShellState};

// 对 shell 而言，空命令不是错误。用户直接按下 Enter 时，应进入下一轮提示符。
fn parse_args(line: &str, state: &ShellState) -> Result<Option<Command>, String> {
    // 先做一轮最小变量展开，再进入后续基于空白分隔的解析。
    // 这里必须先把展开结果收集到 Vec<String> 中，让这些 String 成为真正的数据拥有者。
    let expanded_words = line
        .split_whitespace()
        .map(|word| expand_word(word, state))
        .collect::<Vec<String>>();

    // 这里的 words 不是拥有数据，而是借用 expanded_words 中每个 String 的切片。
    let mut words = expanded_words.iter().map(String::as_str);
    let Some(program) = words.next() else {
        return Ok(None);
    };

    // program 当前只是借来的 &str，但 Command 结构需要自己拥有 program，因此这里再显式拷贝成新的 String。
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
pub fn parse_line(line: &str, state: &ShellState) -> Result<Option<ParsedLine>, String> {
    if line.contains('|') {
        match parse_pipeline(line, state) {
            Ok(None) => Ok(None),
            Ok(Some(pipeline)) => Ok(Some(ParsedLine::Pipeline(pipeline))),
            Err(err) => Err(err),
        }
    } else {
        parse_args(line, state).map(|command| command.map(ParsedLine::Command))
    }
}

fn parse_pipeline(line: &str, state: &ShellState) -> Result<Option<Pipeline>, String> {
    if !line.contains('|') {
        return Ok(None);
    }

    let commands = line
        .split('|')
        .map(str::trim)
        // 如果某一段解析为空命令，说明管道中存在非法的空段。
        .map(|part| {
            parse_args(part, state)
                .and_then(|command| command.ok_or_else(|| "empty command in pipeline".to_string()))
        })
        // collect 可以把 Iterator<Result<Command, String>> 收集成 Result<Vec<Command>, String>。
        // 只要其中任何一段解析失败，整个 pipeline 解析就会提前返回 Err。
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Some(Pipeline { commands }))
}

// 变量展开/参数展开的处理阶段。这应该是一种相当常见的范式
pub fn expand_word(word: &str, state: &ShellState) -> String {
    // 这里按“词内扫描”的方式处理变量，而不是要求整个 token 恰好等于变量名。
    // 因此现在可以支持：
    //   prefix-$HOME
    //   $HOME/file
    //   ${HOME}
    // 仍然暂不处理引号和更完整的 shell 词法规则。
    let mut expanded = String::new();
    // `peekable()` 会给普通迭代器增加“先偷看下一个元素，再决定是否消费”的能力。
    // 变量展开里经常要先看 `$` 后面是什么，因此这里很合适。
    let mut chars = word.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '$' {
            expanded.push(ch);
            continue;
        }

        // `peek()` 返回的是 `Option<&char>`，因为它只是“借用地偷看”下一个字符。
        // 这里立刻用 `copied()` 把 `&char` 变成 `char`，这样后面的 match 就可以直接按值匹配字符字面量，而不必继续处理引用层级。
        match chars.peek().copied() {
            // `$?` 读取 shell 自己维护的上一条命令状态码。
            Some('?') => {
                // 前面的 `peek()` 只看不吃；这里再调用一次 `next()`，才真正把 `?` 从输入流里消费掉，避免它后面又被当成普通字符处理。
                chars.next();
                // `to_string()` 会创建一个临时 String；`push_str()` 只是在这次调用期间读取它的内容并拷贝进 `expanded`，不会把这个借用保存到外面，因此是安全的。
                expanded.push_str(&state.last_status.code.to_string());
            }
            // `${NAME}` 形式使用显式的花括号边界，便于处理前后缀拼接。
            Some('{') => {
                chars.next();
                let mut name = String::new();

                // 持续消费字符，直到读到右花括号
                while let Some(next_ch) = chars.next() {
                    if next_ch == '}' {
                        break;
                    }
                    name.push(next_ch);
                }

                if name.is_empty() {
                    expanded.push_str(&"${}".to_string());
                } else {
                    expanded.push_str(&expand_variable(&name, state));
                }
            }
            // `$NAME` 读取环境变量；变量名规则沿用前面 export/unset 的那套风格。
            Some(next_ch) if is_variable_name_start(next_ch) => {
                let mut name = String::new();
                // 这里的 `unwrap()` 是有根据的：只有在前面的 `peek()` 已经确认存在合法首字符时，
                // 才会进入这个分支，因此再次 `next()` 不会得到 None。
                name.push(chars.next().unwrap());

                while let Some(next_ch) = chars.peek().copied() {
                    if !is_variable_name_continue(next_ch) {
                        break;
                    }
                    // 这里采用“先看一眼，再决定是否消费”的最长匹配策略。
                    // 只要下一个字符仍然符合变量名规则，就把它真正取出来并追加到 `name`。
                    name.push(chars.next().unwrap());
                }

                expanded.push_str(&expand_variable(&name, state));
            }
            // 如果 `$` 后面不是当前支持的变量形式，就保留字面量 `$`。
            _ => expanded.push('$'),
        }
    }

    expanded
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
