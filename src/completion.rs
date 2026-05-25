use crate::builtin::BUILTIN_NAMES;
use rustyline::completion::{Completer, FilenameCompleter, Pair};
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::history::DefaultHistory;
use rustyline::validate::Validator;
use rustyline::{Context, Editor, Helper, Result as RustylineResult};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

pub type EcshEditor = Editor<EcshHelper, DefaultHistory>;

pub fn new_editor() -> RustylineResult<EcshEditor> {
    let mut editor = Editor::<EcshHelper, DefaultHistory>::new()?;
    editor.set_helper(Some(EcshHelper::default()));
    Ok(editor)
}

#[derive(Default)]
pub struct EcshHelper {
    files: FilenameCompleter,
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
