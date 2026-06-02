use crate::builtin::BUILTIN_NAMES;
use crate::diagnostics::print_error;
use crate::extensions::{invoke_bind_callback, key_to_event, parse_key_string, resolve_completion};
use crate::types::ShellState;
use rustyline::completion::{Completer, FilenameCompleter, Pair};
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::history::DefaultHistory;
use rustyline::validate::Validator;
use rustyline::{
    Cmd, ConditionalEventHandler, Context, Editor, Event, EventContext, EventHandler, Helper,
    RepeatCount, Result as RustylineResult,
};
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::rc::Rc;

pub type EcshEditor = Editor<EcshHelper, DefaultHistory>;

thread_local! {
    static BIND_SHELL_STATE: RefCell<Option<ShellState>> = const { RefCell::new(None) };
}

pub fn new_editor() -> RustylineResult<EcshEditor> {
    let mut editor = Editor::<EcshHelper, DefaultHistory>::new()?;
    editor.set_helper(Some(EcshHelper::default()));
    Ok(editor)
}

#[derive(Default)]
pub struct EcshHelper {
    files: FilenameCompleter,
    shell_state: Rc<RefCell<Option<ShellState>>>,
}

impl EcshHelper {
    pub fn sync_shell_state(&mut self, state: &ShellState) {
        *self.shell_state.borrow_mut() = Some(state.clone());
        BIND_SHELL_STATE.with(|slot| *slot.borrow_mut() = Some(state.clone()));
    }
}

/// A `ConditionalEventHandler` that dispatches to ecscript bind callbacks.
#[derive(Clone)]
pub struct BindDispatcher {
    key: String,
}

impl BindDispatcher {
    pub fn new(key: String) -> Self {
        Self { key }
    }
}

impl ConditionalEventHandler for BindDispatcher {
    fn handle(
        &self,
        evt: &Event,
        _n: RepeatCount,
        _positive: bool,
        ctx: &EventContext,
    ) -> Option<Cmd> {
        let Event::KeySeq(_) = evt else {
            return None;
        };
        let line = ctx.line().to_string();
        let cursor = ctx.pos();

        BIND_SHELL_STATE.with(|slot| {
            let state = slot.borrow();
            let state = state.as_ref()?;
            // If this key was removed from the config (e.g. via reload_rc),
            // fall through so rustyline applies its built-in default binding.
            if !state.extensions.borrow().key_bindings.contains_key(&self.key) {
                return None;
            }
            match with_cooked_tty(|| invoke_bind_callback(&self.key, &line, cursor, state)) {
                Ok(Some(cmd)) => Some(cmd),
                // A registered bind callback that returns nil has handled the key
                // and should suppress rustyline's built-in fallback binding.
                Ok(None) => Some(Cmd::Noop),
                Err(err) => {
                    print_error(err.format_with_source(""));
                    Some(Cmd::Noop)
                }
            }
        })
    }
}

#[cfg(unix)]
fn with_cooked_tty<T>(
    f: impl FnOnce() -> Result<T, crate::ecscript::RuntimeError>,
) -> Result<T, crate::ecscript::RuntimeError> {
    use nix::sys::termios::{
        self, ControlFlags, InputFlags, LocalFlags, SetArg, SpecialCharacterIndices as SCI,
    };

    let stdin = std::io::stdin();
    let original = termios::tcgetattr(&stdin).map_err(|err| {
        crate::ecscript::RuntimeError::new(
            0,
            crate::ecscript::RuntimeErrorKind::IoError,
            format!("failed to read tty mode: {err}"),
        )
    })?;

    let mut cooked = original.clone();
    cooked.input_flags |= InputFlags::BRKINT | InputFlags::ICRNL | InputFlags::IXON;
    cooked.control_flags |= ControlFlags::CS8;
    cooked.local_flags |=
        LocalFlags::ECHO | LocalFlags::ICANON | LocalFlags::IEXTEN | LocalFlags::ISIG;
    cooked.control_chars[SCI::VMIN as usize] = 1;
    cooked.control_chars[SCI::VTIME as usize] = 0;

    termios::tcsetattr(&stdin, SetArg::TCSADRAIN, &cooked).map_err(|err| {
        crate::ecscript::RuntimeError::new(
            0,
            crate::ecscript::RuntimeErrorKind::IoError,
            format!("failed to switch tty to cooked mode: {err}"),
        )
    })?;

    // Guard restores original terminal attributes even if f() panics.
    struct CookedGuard {
        stdin: std::io::Stdin,
        original: termios::Termios,
    }
    impl Drop for CookedGuard {
        fn drop(&mut self) {
            let _ = termios::tcsetattr(&self.stdin, SetArg::TCSADRAIN, &self.original);
        }
    }

    let _guard = CookedGuard {
        stdin: std::io::stdin(),
        original,
    };

    f()
}

#[cfg(not(unix))]
fn with_cooked_tty<T>(
    f: impl FnOnce() -> Result<T, crate::ecscript::RuntimeError>,
) -> Result<T, crate::ecscript::RuntimeError> {
    f()
}

pub fn sync_editor_shell_state(editor: &mut EcshEditor, state: &ShellState) {
    let Some(helper) = editor.helper_mut() else {
        return;
    };
    helper.sync_shell_state(state);

    let key_bindings = state
        .extensions
        .borrow()
        .key_bindings
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    for key in key_bindings {
        let Ok((keycode, modifiers)) = parse_key_string(&key, 0) else {
            continue;
        };
        editor.bind_sequence(
            key_to_event(keycode, modifiers),
            EventHandler::Conditional(Box::new(BindDispatcher::new(key))),
        );
    }
}

impl Helper for EcshHelper {}
impl Hinter for EcshHelper {
    type Hint = String;
}
impl Highlighter for EcshHelper {}
impl Validator for EcshHelper {}

impl Completer for EcshHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        ctx: &Context<'_>,
    ) -> RustylineResult<(usize, Vec<Pair>)> {
        let prefix = &line[..pos];
        let word_start = prefix
            .char_indices()
            .rev()
            .find(|(_, ch)| ch.is_whitespace())
            .map(|(idx, ch)| idx + ch.len_utf8())
            .unwrap_or(0);
        let first_word = prefix[..word_start].trim().is_empty();
        let current = &prefix[word_start..];

        if let Some((start, candidates)) = self.scripted_candidates(line, current, word_start) {
            return Ok((start, candidates));
        }

        if first_word && !current.contains('/') {
            let start = word_start;
            let candidates = command_candidates(current);
            if !candidates.is_empty() {
                return Ok((start, candidates));
            }
        }

        self.files.complete(line, pos, ctx)
    }
}

impl EcshHelper {
    fn scripted_candidates(
        &self,
        line: &str,
        current: &str,
        word_start: usize,
    ) -> Option<(usize, Vec<Pair>)> {
        let guard = self.shell_state.borrow();
        let state = guard.as_ref()?;
        let (argv, arg_index) = split_argv(line, current);
        let command = argv.first()?.clone();

        match resolve_completion(state, &command, line, current, argv, arg_index) {
            Ok(Some(items)) => {
                if items.is_empty() {
                    // Scripted handler ran but produced no candidates; fall through to
                    // non-scripted completion so that builtin/file candidates can appear.
                    return None;
                }
                Some((
                    word_start,
                    items
                        .into_iter()
                        .map(|item| {
                            let display = item.display.unwrap_or_else(|| item.value.clone());
                            Pair {
                                display,
                                replacement: item.value,
                            }
                        })
                        .collect(),
                ))
            }
            Ok(None) => None,
            Err(_err) => {
                // resolve_completion already handles error printing internally
                None
            }
        }
    }
}

fn command_candidates(prefix: &str) -> Vec<Pair> {
    let mut names = BTreeSet::new();
    for builtin in BUILTIN_NAMES {
        if builtin.starts_with(prefix) {
            names.insert((*builtin).to_string());
        }
    }

    if let Ok(path) = std::env::var("PATH") {
        for dir in path.split(':') {
            let dir = Path::new(dir);
            let Ok(entries) = fs::read_dir(dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let Ok(file_type) = entry.file_type() else {
                    continue;
                };
                if !file_type.is_file() {
                    continue;
                }
                let name = entry.file_name();
                let Some(name) = name.to_str() else {
                    continue;
                };
                if name.starts_with(prefix) {
                    names.insert(name.to_string());
                }
            }
        }
    }

    names
        .into_iter()
        .map(|name| Pair {
            display: name.clone(),
            replacement: name,
        })
        .collect()
}

fn split_argv(line: &str, current: &str) -> (Vec<String>, usize) {
    let mut argv = line
        .split_whitespace()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let ends_with_space = line.chars().last().is_some_and(char::is_whitespace);
    if ends_with_space {
        argv.push(String::new());
    }

    let arg_index = argv.len().saturating_sub(1);
    if let Some(last) = argv.last_mut() {
        *last = current.to_string();
    } else {
        argv.push(current.to_string());
    }
    (argv, arg_index)
}

#[cfg(test)]
mod tests {
    use super::EcshHelper;
    use crate::ecscript::{
        Environment, eval_top_level_script_with_ctx, lexer, parse_top_level_script,
    };
    use crate::extensions::new_extensions;
    use crate::types::{CommandStatus, ShellState};
    use rustyline::Context;
    use rustyline::completion::Completer;
    use rustyline::history::DefaultHistory;
    use std::collections::HashMap;
    use std::rc::Rc;

    fn state() -> ShellState {
        ShellState {
            last_status: CommandStatus::success(),
            interactive: true,
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

    fn register(src: &str, state: &ShellState) {
        let _ = lexer::tokenize(src).unwrap();
        let stmts = parse_top_level_script(src).unwrap().unwrap();
        eval_top_level_script_with_ctx(&stmts, &state.script_env, Some(state)).unwrap();
    }

    #[test]
    fn scripted_completion_returns_structured_items() {
        let state = state();
        register(
            r#"
complete("git", (ctx) => {
    return [
        { value: "status", display: "git status" }
    ];
})
"#,
            &state,
        );

        let mut helper = EcshHelper::default();
        helper.sync_shell_state(&state);
        let history = DefaultHistory::new();
        let ctx = Context::new(&history);
        let (_, candidates) = helper.complete("git st", 6, &ctx).unwrap();

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].replacement, "status");
        assert_eq!(candidates[0].display, "git status");
    }

    #[test]
    fn scripted_completion_falls_back_on_error() {
        let state = state();
        register(
            r#"
complete("git", (ctx) => {
    return 1;
})
"#,
            &state,
        );

        let mut helper = EcshHelper::default();
        helper.sync_shell_state(&state);
        let history = DefaultHistory::new();
        let ctx = Context::new(&history);
        let (_, candidates) = helper.complete("git", 3, &ctx).unwrap();

        assert!(
            candidates
                .iter()
                .any(|candidate| candidate.replacement == "git")
        );
    }
}
