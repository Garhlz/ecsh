//! ShellWord 展开：将 lexer 产出的 fragment AST 展开为可执行 argv。
//!
//! 四种嵌入语法只在执行时展开，不在 lexer/parser 阶段做死：
//!   - $VAR    → 先查脚本作用域，不存在则 fallback std::env::var
//!   - ${expr} → 调用 ecscript evaluator 求值，标量转字符串
//!   - $(cmd)  → 通过 /bin/sh -c 执行 shell 命令，捕获 stdout
//!   - ${...arr} → 对 Array 做 argv spread

use glob::{MatchOptions, glob_with};
use std::process::{Command as ProcessCommand, Stdio};

use crate::ecscript::value::Value;
use crate::ecscript::{eval_expr_src, repr_value};
use crate::types::{
    Command, OutputRedirection, Redirection, ShellResult, ShellState, ShellWord, WordFragment,
};

pub struct ExpandEnv<'a> {
    pub script_env: &'a crate::ecscript::env::Environment<'a>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct ExpandedWord {
    text: String,
    glob_pattern: String,
    has_glob: bool,
}

impl ExpandedWord {
    fn empty() -> Self {
        Self {
            text: String::new(),
            glob_pattern: String::new(),
            has_glob: false,
        }
    }

    fn literal(text: String) -> Self {
        Self {
            glob_pattern: glob::Pattern::escape(&text),
            text,
            has_glob: false,
        }
    }

    fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

/// 把带 fragment 的命令展开成只含ShellWord WordFragment::Lit 的可执行命令。
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
/// 注意：program 允许通过 `${...arr}` 之类展开成多个 argv；
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
    let mut result = vec![ExpandedWord::empty()];

    for frag in &word.fragments {
        match frag {
            WordFragment::Lit(s) => append_fragment(&mut result, s, true),
            WordFragment::QuotedLit(s) => append_fragment(&mut result, s, false),
            WordFragment::Var(name) => {
                let val = if let Ok(v) = env.script_env.get(name, 0) {
                    value_to_string(&v)
                } else {
                    std::env::var(name).unwrap_or_default()
                };
                append_fragment(&mut result, &val, false);
            }
            WordFragment::Cmd(src) => {
                let output = expand_cmd(src)?;
                append_fragment(&mut result, &output, false);
            }
            WordFragment::Expr { src, spread: false } => {
                let value = eval_expr_src(src, env.script_env)?;
                match &value {
                    Value::Array(_) | Value::Object(_) => {
                        return Err(
                            "cannot inline array/object — use to_json() or ${...arr} to spread"
                                .into(),
                        );
                    }
                    _ => append_fragment(&mut result, &value_to_string(&value), false),
                }
            }
            WordFragment::Expr { src, spread: true } => {
                let value = eval_expr_src(src, env.script_env)?;
                let Value::Array(arr) = value else {
                    return Err("${...arr} expects an Array".into());
                };

                let items: Vec<ExpandedWord> = arr
                    .borrow()
                    .iter()
                    .map(|value| ExpandedWord::literal(value_to_string(value)))
                    .collect();

                splice_spread(&mut result, items);
            }
        }
    }

    expand_tilde_prefix(word, &mut result);
    expand_globs(result)
}

/// 最小 tilde 展开：只处理字面量开头的 `~` / `~/...`。
///
/// 这里刻意不展开：
/// - `~user`
/// - 由 `${expr}` / `$(cmd)` 生成的 `~`
fn expand_tilde_prefix(word: &ShellWord, result: &mut [ExpandedWord]) {
    let Some(WordFragment::Lit(first)) = word.fragments.first() else {
        return;
    };
    if !(first == "~" || first.starts_with("~/")) {
        return;
    }
    let Ok(home) = std::env::var("HOME") else {
        return;
    };
    let Some(first_word) = result.first_mut() else {
        return;
    };

    if first_word.text == "~" {
        first_word.text = home.clone();
        first_word.glob_pattern = glob::Pattern::escape(&home);
        first_word.has_glob = false;
    } else if let Some(suffix) = first_word.text.strip_prefix("~/") {
        let suffix = suffix.to_string();
        let pattern_suffix = first_word
            .glob_pattern
            .strip_prefix("~/")
            .unwrap_or(&suffix)
            .to_string();
        first_word.text = format!("{home}/{suffix}");
        first_word.glob_pattern = format!("{}/{}", glob::Pattern::escape(&home), pattern_suffix);
    }
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
fn append_fragment(result: &mut Vec<ExpandedWord>, text: &str, allow_glob: bool) {
    if let Some(last) = result.last_mut() {
        last.text.push_str(text);
        if allow_glob {
            last.has_glob |= contains_glob_meta(text);
            last.glob_pattern.push_str(text);
        } else {
            last.glob_pattern.push_str(&glob::Pattern::escape(text));
        }
    } else {
        let mut word = ExpandedWord::empty();
        word.text.push_str(text);
        if allow_glob {
            word.has_glob = contains_glob_meta(text);
            word.glob_pattern.push_str(text);
        } else {
            word.glob_pattern.push_str(&glob::Pattern::escape(text));
        }
        result.push(word);
    }
}

/// 把 `${...arr}` 展开的多个元素缝到当前 word 上。
///
/// 例如 `pre$[...["a", "b"]]` 会变成 `["prea", "b"]`。
fn splice_spread(result: &mut Vec<ExpandedWord>, items: Vec<ExpandedWord>) {
    if result.len() == 1 && result[0].is_empty() {
        *result = items;
        return;
    }

    let prefix = result.pop().unwrap_or_default();
    let mut iter = items.into_iter();
    let mut first = prefix;
    if let Some(item) = iter.next() {
        first.text.push_str(&item.text);
        first.glob_pattern.push_str(&item.glob_pattern);
        first.has_glob |= item.has_glob;
    }
    result.push(first);
    result.extend(iter);
}

fn expand_globs(words: Vec<ExpandedWord>) -> ShellResult<Vec<String>> {
    let mut expanded = Vec::new();

    for word in words {
        if !word.has_glob {
            expanded.push(word.text);
            continue;
        }

        let options = MatchOptions {
            case_sensitive: true,
            require_literal_separator: true,
            require_literal_leading_dot: !glob_component_starts_with_literal_dot(
                &word.glob_pattern,
            ),
        };
        let Ok(paths) = glob_with(&word.glob_pattern, options) else {
            expanded.push(word.text);
            continue;
        };
        let mut matches = paths.filter_map(Result::ok).collect::<Vec<_>>();
        if matches.is_empty() {
            expanded.push(word.text);
            continue;
        }

        matches.sort();
        expanded.extend(
            matches
                .into_iter()
                .map(|path| path.to_string_lossy().into_owned()),
        );
    }

    Ok(expanded)
}

fn contains_glob_meta(text: &str) -> bool {
    text.contains('*') || text.contains('?') || text.contains('[')
}

fn glob_component_starts_with_literal_dot(pattern: &str) -> bool {
    let component = pattern.rsplit('/').next().unwrap_or(pattern);
    component.starts_with('.') || component.starts_with(r"\.")
}

#[cfg(test)]
mod tests {
    use super::{ExpandEnv, expand_command, expand_shell_word};
    use crate::ecscript::env::Environment;
    use crate::ecscript::value::{Binding, Value};
    use crate::extensions::new_extensions;
    use crate::types::{
        Command, CommandStatus, OutputRedirection, Redirection, ShellState, ShellWord, WordFragment,
    };
    use std::collections::HashMap;
    use std::fs;
    use std::path::PathBuf;
    use std::rc::Rc;

    fn state() -> ShellState {
        ShellState {
            last_status: CommandStatus::success(),
            interactive: false,
            shell_pgid: None,
            shell_terminal_fd: None,
            jobs: Vec::new(),
            next_job_id: 1,
            current_fg_pgid: None,
            script_env: Rc::new(Environment::new()),
            aliases: HashMap::new(),
            traps: HashMap::new(),
            command_history: Vec::new(),
            extensions: new_extensions(),
            module_loader: None,
        }
    }

    fn unique_temp_dir(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("ecsh-expand-{}-{}", std::process::id(), name));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
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
                WordFragment::Expr {
                    src: format!(r#"env("{var_name}")"#),
                    spread: false,
                },
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
    fn expands_tilde_to_home_for_literal_word() {
        let state = state();
        let old_home = std::env::var_os("HOME");
        unsafe { std::env::set_var("HOME", "/tmp/ecsh-home") };

        let word = ShellWord::lit("~/demo");
        let env = ExpandEnv {
            script_env: &state.script_env,
        };
        let expanded = expand_shell_word(&word, &env).unwrap();
        assert_eq!(expanded, vec!["/tmp/ecsh-home/demo"]);

        match old_home {
            Some(value) => unsafe { std::env::set_var("HOME", value) },
            None => unsafe { std::env::remove_var("HOME") },
        }
    }

    #[test]
    fn does_not_expand_tilde_user_form() {
        let state = state();
        let word = ShellWord::lit("~elaine");
        let env = ExpandEnv {
            script_env: &state.script_env,
        };
        let expanded = expand_shell_word(&word, &env).unwrap();
        assert_eq!(expanded, vec!["~elaine"]);
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
    fn env_builtin_missing_stringifies_nil_in_shell_word() {
        let state = state();
        let word = ShellWord {
            fragments: vec![WordFragment::Expr {
                src: r#"env("ECSH_NONEXISTENT_VAR")"#.into(),
                spread: false,
            }],
        };
        let env = ExpandEnv {
            script_env: &state.script_env,
        };
        let expanded = expand_shell_word(&word, &env).unwrap();
        assert_eq!(expanded, vec!["nil"]);
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

    #[test]
    fn expands_unquoted_glob_matches_in_sorted_order() {
        let state = state();
        let dir = unique_temp_dir("sorted-glob");
        fs::write(dir.join("b.txt"), "").unwrap();
        fs::write(dir.join("a.txt"), "").unwrap();
        fs::write(dir.join("skip.log"), "").unwrap();

        let word = ShellWord::lit(format!("{}/*.txt", dir.display()));
        let env = ExpandEnv {
            script_env: &state.script_env,
        };
        let expanded = expand_shell_word(&word, &env).unwrap();
        assert_eq!(
            expanded,
            vec![
                dir.join("a.txt").to_string_lossy().into_owned(),
                dir.join("b.txt").to_string_lossy().into_owned(),
            ]
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn leaves_unmatched_glob_literal_unchanged() {
        let state = state();
        let dir = unique_temp_dir("unmatched-glob");
        let pattern = format!("{}/*.missing", dir.display());
        let word = ShellWord::lit(pattern.clone());
        let env = ExpandEnv {
            script_env: &state.script_env,
        };

        assert_eq!(expand_shell_word(&word, &env).unwrap(), vec![pattern]);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn quoted_glob_meta_does_not_expand() {
        let state = state();
        let dir = unique_temp_dir("quoted-glob");
        fs::write(dir.join("a.txt"), "").unwrap();
        let word = ShellWord {
            fragments: vec![
                WordFragment::Lit(format!("{}/", dir.display())),
                WordFragment::QuotedLit("*.txt".into()),
            ],
        };
        let env = ExpandEnv {
            script_env: &state.script_env,
        };

        assert_eq!(
            expand_shell_word(&word, &env).unwrap(),
            vec![format!("{}/*.txt", dir.display())]
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn expansion_results_do_not_trigger_second_pass_glob() {
        let state = state();
        let dir = unique_temp_dir("dynamic-glob");
        fs::write(dir.join("a.txt"), "").unwrap();
        let pattern = format!("{}/*.txt", dir.display());
        let var_name = "ECSH_GLOB_PATTERN";
        unsafe { std::env::set_var(var_name, &pattern) };
        let word = ShellWord {
            fragments: vec![WordFragment::Var(var_name.into())],
        };
        let env = ExpandEnv {
            script_env: &state.script_env,
        };

        assert_eq!(expand_shell_word(&word, &env).unwrap(), vec![pattern]);

        unsafe { std::env::remove_var(var_name) };
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn unquoted_glob_does_not_match_dotfiles_without_literal_dot() {
        let state = state();
        let dir = unique_temp_dir("dot-glob");
        fs::write(dir.join(".hidden"), "").unwrap();
        fs::write(dir.join("shown"), "").unwrap();
        let env = ExpandEnv {
            script_env: &state.script_env,
        };

        let plain =
            expand_shell_word(&ShellWord::lit(format!("{}/*", dir.display())), &env).unwrap();
        assert_eq!(
            plain,
            vec![dir.join("shown").to_string_lossy().into_owned()]
        );

        let dot =
            expand_shell_word(&ShellWord::lit(format!("{}/.*", dir.display())), &env).unwrap();
        assert!(dot.contains(&dir.join(".hidden").to_string_lossy().into_owned()));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn redirection_glob_reports_multiple_words() {
        let state = state();
        let dir = unique_temp_dir("redir-glob");
        fs::write(dir.join("a.txt"), "").unwrap();
        fs::write(dir.join("b.txt"), "").unwrap();
        let command = Command {
            program: ShellWord::lit("cat"),
            args: vec![],
            redirection: Redirection {
                stdin: Some(ShellWord::lit(format!("{}/*.txt", dir.display()))),
                stdout: None,
            },
        };

        let err = expand_command(&command, &state).unwrap_err();
        assert_eq!(
            err.to_string(),
            "stdin redirection expansion produced multiple words"
        );

        let _ = fs::remove_dir_all(dir);
    }
}
