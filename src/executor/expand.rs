//! ShellWord 展开：将 lexer 产出的 fragment AST 展开为可执行 argv。
//!
//! 四种嵌入语法只在执行时展开，不在 lexer/parser 阶段做死：
//!   - $VAR    → 先查脚本作用域，不存在则 fallback std::env::var
//!   - ${VAR}  → 只查 std::env::var
//!   - $(cmd)  → 通过 /bin/sh -c 执行 shell 命令，捕获 stdout
//!   - $[expr] → 调用 ecscript evaluator 求值，标量转字符串

use std::process::{Command as ProcessCommand, Stdio};

use crate::ecscript::value::Value;
use crate::ecscript::{eval_expr_src, repr_value};
use crate::types::{
    Command, OutputRedirection, Redirection, ShellResult, ShellState, ShellWord, WordFragment,
};

pub struct ExpandEnv<'a> {
    pub script_env: &'a crate::ecscript::env::Environment<'a>,
}

/// 把带 fragment 的命令展开成只含字面量 `ShellWord` 的可执行命令。
///
/// 程序名和参数可以展开成多个 argv；重定向路径必须恰好展开成一个字符串。
pub fn expand_command(command: &Command, state: &ShellState) -> ShellResult<Command> {
    let env = ExpandEnv {
        script_env: &state.script_env,
    };
    let argv = expand_argv(command, &env)?;
    let (program, args) = split_argv(argv)?;

    let stdin = match &command.redirection.stdin {
        Some(path) => Some(ShellWord::lit(expand_single_word(
            path,
            &env,
            "stdin redirection",
        )?)),
        None => None,
    };

    let stdout = match &command.redirection.stdout {
        Some(OutputRedirection::Truncate(path)) => Some(OutputRedirection::Truncate(
            ShellWord::lit(expand_single_word(path, &env, "stdout redirection")?),
        )),
        Some(OutputRedirection::Append(path)) => Some(OutputRedirection::Append(ShellWord::lit(
            expand_single_word(path, &env, "stdout redirection")?,
        ))),
        None => None,
    };

    Ok(Command {
        program: ShellWord::lit(program),
        args: args.into_iter().map(ShellWord::lit).collect(),
        redirection: Redirection { stdin, stdout },
    })
}

/// 将命令头和参数统一展开为扁平 argv。
///
/// 注意：program 允许通过 `$[...arr]` 之类展开成多个 argv；
/// 调用方需要再把第一个元素拆成真正的程序名。
pub fn expand_argv(command: &Command, env: &ExpandEnv<'_>) -> ShellResult<Vec<String>> {
    let mut argv = Vec::new();

    let program = expand_shell_word(&command.program, env)?;
    argv.extend(program);

    for arg in &command.args {
        let expanded = expand_shell_word(arg, env)?;
        argv.extend(expanded);
    }

    Ok(argv)
}

fn split_argv(mut argv: Vec<String>) -> ShellResult<(String, Vec<String>)> {
    if argv.is_empty() {
        return Err("command head expansion produced no words".into());
    }

    let program = argv.remove(0);
    if program.is_empty() {
        return Err("command head expansion produced an empty program name".into());
    }

    Ok((program, argv))
}

/// 用在 redirection 等必须得到单个字符串的位置。
fn expand_single_word(word: &ShellWord, env: &ExpandEnv<'_>, context: &str) -> ShellResult<String> {
    let mut words = expand_shell_word(word, env)?;
    match words.len() {
        0 => Err(format!("{} expansion produced no words", context).into()),
        1 => Ok(words.pop().expect("single expanded word should exist")),
        _ => Err(format!("{} expansion produced multiple words", context).into()),
    }
}

/// 展开单个 ShellWord。
///
/// 大多数 word 最终只会变成一个字符串；只有 spread fragment 会把它拆成多个 argv。
fn expand_shell_word(word: &ShellWord, env: &ExpandEnv<'_>) -> ShellResult<Vec<String>> {
    let mut result = vec![String::new()];

    for frag in &word.fragments {
        match frag {
            WordFragment::Lit(s) => append_fragment(&mut result, s),
            WordFragment::Var(name) => {
                let val = if let Ok(v) = env.script_env.get(name, 0) {
                    value_to_string(&v)
                } else {
                    std::env::var(name).unwrap_or_default()
                };
                append_fragment(&mut result, &val);
            }
            WordFragment::EnvVar(name) => {
                let val = std::env::var(name).unwrap_or_default();
                append_fragment(&mut result, &val);
            }
            WordFragment::Cmd(src) => {
                let output = expand_cmd(src)?;
                append_fragment(&mut result, &output);
            }
            WordFragment::Expr { src, spread: false } => {
                let value = eval_expr_src(src, env.script_env)?;
                match &value {
                    Value::Array(_) | Value::Object(_) => {
                        return Err(
                            "cannot inline array/object — use to_json() or $[...arr] to spread"
                                .into(),
                        );
                    }
                    _ => append_fragment(&mut result, &value_to_string(&value)),
                }
            }
            WordFragment::Expr { src, spread: true } => {
                let value = eval_expr_src(src, env.script_env)?;
                let Value::Array(arr) = value else {
                    return Err("$[...arr] expects an Array".into());
                };

                let items: Vec<String> = arr.borrow().iter().map(value_to_string).collect();

                splice_spread(&mut result, items);
            }
        }
    }

    Ok(result)
}

/// shell 展开里的字符串保持原样，其余值沿用 ecscript 的 repr 文本化规则。
fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        _ => repr_value(v),
    }
}

/// 通过 `/bin/sh -c` 做命令替换，只把 stdout 文本拼回当前 shell word。
///
/// 与普通 shell 的 `$(cmd)` 一样，这里不会因为子命令非零退出码而直接把外层展开判错；
/// 错误信息仍由子 shell 写到 stderr，展开阶段只消费 stdout。
fn expand_cmd(cmd_str: &str) -> ShellResult<String> {
    if cmd_str.as_bytes().contains(&0) {
        return Err("$(cmd) contains an interior NUL byte".into());
    }

    let output = ProcessCommand::new("/bin/sh")
        .arg("-c")
        .arg(cmd_str)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|err| format!("$(cmd): failed to start /bin/sh -c: {err}"))?
        .wait_with_output()
        .map_err(|err| format!("$(cmd): failed while waiting for /bin/sh -c: {err}"))?;

    let output = String::from_utf8(output.stdout)
        .map_err(|err| format!("$(cmd): stdout is not valid UTF-8: {err}"))?;
    Ok(output.trim_end_matches('\n').to_string())
}

/// 把一个普通 fragment 追加到当前正在构造的 argv 项上。
fn append_fragment(result: &mut Vec<String>, text: &str) {
    if let Some(last) = result.last_mut() {
        last.push_str(text);
    } else {
        result.push(text.to_string());
    }
}

/// 把 `$[...arr]` 展开的多个元素缝到当前 word 上。
///
/// 例如 `pre$[...["a", "b"]]` 会变成 `["prea", "b"]`。
fn splice_spread(result: &mut Vec<String>, items: Vec<String>) {
    if result.len() == 1 && result[0].is_empty() {
        *result = items;
        return;
    }

    let prefix = result.pop().unwrap_or_default();
    let mut iter = items.into_iter();
    let first = format!("{}{}", prefix, iter.next().unwrap_or_default());
    result.push(first);
    result.extend(iter);
}

#[cfg(test)]
mod tests {
    use super::{ExpandEnv, expand_command, expand_shell_word};
    use crate::ecscript::env::Environment;
    use crate::ecscript::value::{Binding, Value};
    use crate::types::{
        Command, CommandStatus, OutputRedirection, Redirection, ShellState, ShellWord, WordFragment,
    };

    fn state() -> ShellState {
        ShellState {
            last_status: CommandStatus::success(),
            interactive: false,
            shell_pgid: None,
            shell_terminal_fd: None,
            jobs: Vec::new(),
            next_job_id: 1,
            current_fg_pgid: None,
            script_env: Environment::new(),
        }
    }

    #[test]
    fn expands_program_head_into_program_and_args() {
        let state = state();
        let command = Command {
            program: ShellWord {
                fragments: vec![WordFragment::Expr {
                    src: r#"["echo", "from-head"]"#.to_string(),
                    spread: true,
                }],
            },
            args: vec![ShellWord::lit("tail")],
            redirection: Redirection::default(),
        };

        let expanded = expand_command(&command, &state).unwrap();
        assert_eq!(expanded.program, ShellWord::lit("echo"));
        assert_eq!(
            expanded.args,
            vec![ShellWord::lit("from-head"), ShellWord::lit("tail")]
        );
    }

    #[test]
    fn expands_var_with_script_scope_priority() {
        let state = state();
        let var_name = "ECSH_STAGE6_RUNTIME_VAR";
        state
            .script_env
            .insert(
                var_name.to_string(),
                Binding::Direct(Value::String("script".into())),
                0,
            )
            .unwrap();
        unsafe { std::env::set_var(var_name, "env") };

        let word = ShellWord {
            fragments: vec![
                WordFragment::Var(var_name.into()),
                WordFragment::Lit(":".into()),
                WordFragment::EnvVar(var_name.into()),
            ],
        };
        let env = ExpandEnv {
            script_env: &state.script_env,
        };

        let expanded = expand_shell_word(&word, &env).unwrap();
        assert_eq!(expanded, vec!["script:env"]);

        unsafe { std::env::remove_var(var_name) };
    }

    #[test]
    fn rejects_multiword_redirection_targets() {
        let state = state();
        let command = Command {
            program: ShellWord::lit("echo"),
            args: vec![],
            redirection: Redirection {
                stdin: None,
                stdout: Some(OutputRedirection::Truncate(ShellWord {
                    fragments: vec![WordFragment::Expr {
                        src: r#"["a", "b"]"#.to_string(),
                        spread: true,
                    }],
                })),
            },
        };

        let err = expand_command(&command, &state).unwrap_err();
        assert_eq!(
            err.to_string(),
            "stdout redirection expansion produced multiple words"
        );
    }

    #[test]
    fn lit_shellword_roundtrips_unchanged() {
        let state = state();
        let word = ShellWord::lit("hello");
        let env = ExpandEnv {
            script_env: &state.script_env,
        };
        assert_eq!(expand_shell_word(&word, &env).unwrap(), vec!["hello"]);
    }

    #[test]
    fn empty_var_falls_back_to_env() {
        let state = state();
        let word = ShellWord {
            fragments: vec![WordFragment::Var("ECSH_NONEXISTENT_VAR".into())],
        };
        let env = ExpandEnv {
            script_env: &state.script_env,
        };
        let expanded = expand_shell_word(&word, &env).unwrap();
        assert_eq!(expanded, vec![""]);
    }

    #[test]
    fn envvar_missing_gives_empty_string() {
        let state = state();
        let word = ShellWord {
            fragments: vec![WordFragment::EnvVar("ECSH_NONEXISTENT_VAR".into())],
        };
        let env = ExpandEnv {
            script_env: &state.script_env,
        };
        let expanded = expand_shell_word(&word, &env).unwrap();
        assert_eq!(expanded, vec![""]);
    }

    #[test]
    fn expr_with_compound_value_reports_error() {
        let state = state();
        let word = ShellWord {
            fragments: vec![WordFragment::Expr {
                src: r#"[1]"#.to_string(),
                spread: false,
            }],
        };
        let env = ExpandEnv {
            script_env: &state.script_env,
        };
        let err = expand_shell_word(&word, &env).unwrap_err();
        assert!(err.to_string().contains("cannot inline array"));
    }

    #[test]
    fn spread_lit_arg_splits_into_multiple() {
        let state = state();
        let word = ShellWord {
            fragments: vec![WordFragment::Expr {
                src: r#"["a", "b"]"#.to_string(),
                spread: true,
            }],
        };
        let env = ExpandEnv {
            script_env: &state.script_env,
        };
        let expanded = expand_shell_word(&word, &env).unwrap();
        assert_eq!(expanded, vec!["a", "b"]);
    }

    #[test]
    fn spread_splices_into_prefix_lit() {
        let state = state();
        let word = ShellWord {
            fragments: vec![
                WordFragment::Lit("pre-".into()),
                WordFragment::Expr {
                    src: r#"["a", "b"]"#.to_string(),
                    spread: true,
                },
            ],
        };
        let env = ExpandEnv {
            script_env: &state.script_env,
        };
        let expanded = expand_shell_word(&word, &env).unwrap();
        assert_eq!(expanded, vec!["pre-a", "b"]);
    }

    #[test]
    fn rejects_empty_program_head_expansion() {
        let state = state();
        let command = Command {
            program: ShellWord { fragments: vec![] },
            args: vec![],
            redirection: Redirection::default(),
        };
        let err = expand_command(&command, &state).unwrap_err();
        assert!(
            err.to_string().contains("command head"),
            "expected command head error, got: {err}"
        );
    }

    #[test]
    fn rejects_inline_array_in_expr() {
        let state = state();
        let word = ShellWord {
            fragments: vec![WordFragment::Expr {
                src: "[1]".to_string(),
                spread: false,
            }],
        };
        let env = ExpandEnv {
            script_env: &state.script_env,
        };
        let err = expand_shell_word(&word, &env).unwrap_err();
        assert!(err.to_string().contains("cannot inline array"));
    }
}
