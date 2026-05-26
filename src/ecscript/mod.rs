mod ast;
mod builtin;
pub mod env;
pub mod error;
mod eval;
mod func;
mod io_state;
pub mod lexer;
mod parser;
mod pratt;
pub mod value;

use std::{
    fs,
    path::{Path, PathBuf},
};

pub use self::ast::Stmt;
pub use self::env::Environment;
pub use self::error::{ParseError, RuntimeError, RuntimeErrorKind};
pub use self::eval::{eval_script, eval_top_level_script};
pub use self::value::{Value, display_value, repr_value};

#[derive(Debug)]
pub enum InterpreterError {
    Parse(ParseError),
    Runtime(RuntimeError),
}

pub enum ScriptFileError {
    Read {
        path: PathBuf,
        err: std::io::Error,
    },
    Script {
        source: String,
        err: InterpreterError,
    },
}

impl ScriptFileError {
    pub fn format_for_user(&self) -> String {
        match self {
            ScriptFileError::Read { path, err } => {
                format!("failed to read script file '{}': {}", path.display(), err)
            }
            ScriptFileError::Script { source, err } => err.format_with_source(source),
        }
    }
}

impl InterpreterError {
    pub fn format_with_source(&self, src: &str) -> String {
        match self {
            InterpreterError::Parse(err) => err.format_with_source(src),
            InterpreterError::Runtime(err) => err.format_with_source(src),
        }
    }
}

impl std::fmt::Display for InterpreterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InterpreterError::Parse(err) => err.fmt(f),
            InterpreterError::Runtime(err) => err.fmt(f),
        }
    }
}

impl std::error::Error for InterpreterError {}

impl From<ParseError> for InterpreterError {
    fn from(value: ParseError) -> Self {
        InterpreterError::Parse(value)
    }
}

impl From<RuntimeError> for InterpreterError {
    fn from(value: RuntimeError) -> Self {
        InterpreterError::Runtime(value)
    }
}

pub struct Interpreter {
    env: Environment<'static>,
}

impl Default for Interpreter {
    fn default() -> Self {
        Self::new()
    }
}

impl Interpreter {
    pub fn new() -> Self {
        Self {
            env: Environment::new(),
        }
    }

    pub fn run(&self, src: &str) -> Result<(), InterpreterError> {
        let tokens = lexer::tokenize(src)?;
        let stmts = parser::parse_script(&tokens)?;
        eval::eval_script(&stmts, &self.env)?;
        Ok(())
    }

    pub fn eval_expr(&self, src: &str) -> Result<Value, InterpreterError> {
        let tokens = lexer::tokenize(src)?;
        let expr = pratt::parse_expr(&tokens)?;
        let value = eval::eval_expr(&expr, &self.env)?;
        Ok(value)
    }
}

pub fn reset_repl_output_state() {
    io_state::reset_output_state();
}

pub fn repl_output_needs_newline() -> bool {
    io_state::output_needs_newline()
}

/// tokenize → parse → eval 一步到位
pub fn eval_expr_src(src: &str, env: &Environment<'_>) -> error::EvalResult<Value> {
    let tokens = lexer::tokenize(src).map_err(|e| {
        // 把 lexer 的 String error 转成 RuntimeError
        RuntimeError::new(0, error::RuntimeErrorKind::ParseInExpr, e.message)
    })?;

    let expr = pratt::parse_expr(&tokens).map_err(|e| {
        RuntimeError::new(e.offset, error::RuntimeErrorKind::ParseInExpr, e.message)
    })?;

    let value = eval::eval_expr(&expr, env)?;

    Ok(value)
}

pub fn run_script_source(src: &str, env: &Environment<'_>) -> Result<(), InterpreterError> {
    let tokens = lexer::tokenize(src)?;
    let stmts = parser::parse_script(&tokens)?;
    eval::eval_script(&stmts, env)?;
    Ok(())
}

pub fn run_script_file(
    path: impl AsRef<Path>,
    env: &Environment<'_>,
) -> Result<(), ScriptFileError> {
    let path = path.as_ref();
    let source = fs::read_to_string(path).map_err(|err| ScriptFileError::Read {
        path: path.to_path_buf(),
        err,
    })?;
    run_script_source(&source, env).map_err(|err| ScriptFileError::Script { source, err })
}

pub fn parse_top_level_script(src: &str) -> Option<Result<Vec<Stmt>, ParseError>> {
    let starts_with_keyword = starts_with_top_level_keyword(src);
    let tokens = match lexer::tokenize(src) {
        Ok(tokens) => tokens,
        Err(err) => {
            return if starts_with_keyword {
                Some(Err(err))
            } else {
                None
            };
        }
    };
    if let Some(lexer::Token {
        kind: lexer::TokenKind::Keyword(keyword),
        ..
    }) = tokens
        .iter()
        .find(|token| !matches!(token.kind, lexer::TokenKind::Newline))
        && keyword.is_top_level()
    {
        return Some(parser::parse_script(&tokens));
    }

    let mut stream = pratt::TokenStream::new(&tokens);
    stream.skip_newlines();
    if !is_assign_target(&mut stream) {
        return None;
    }

    if matches!(
        &stream.peek().kind,
        lexer::TokenKind::Delimiter(
            lexer::Delimiter::LParen
                | lexer::Delimiter::Eq
                | lexer::Delimiter::PlusEq
                | lexer::Delimiter::MinusEq
                | lexer::Delimiter::StarEq
                | lexer::Delimiter::SlashEq
                | lexer::Delimiter::PercentEq
        )
    ) {
        // 这里期望读入的是完整的多行语句
        return Some(parser::parse_script(&tokens));
    }

    None
}

fn starts_with_top_level_keyword(src: &str) -> bool {
    let trimmed = src.trim_start();
    [
        "let", "if", "while", "for", "continue", "break", "func", "return",
    ]
    .into_iter()
    .any(|keyword| {
        trimmed
            .strip_prefix(keyword)
            .is_some_and(|rest| rest.is_empty() || !matches!(rest.chars().next(), Some(ch) if ch == '_' || ch.is_ascii_alphanumeric()))
    })
}

fn is_assign_target(stream: &mut pratt::TokenStream<'_>) -> bool {
    if !matches!(stream.peek().kind, lexer::TokenKind::Identifier(_)) {
        return false;
    }
    stream.consume();

    if stream.check(&lexer::TokenKind::Delimiter(lexer::Delimiter::Dot)) {
        stream.consume();
        if matches!(stream.peek().kind, lexer::TokenKind::Identifier(_)) {
            stream.consume();
            return true;
        }
        return false;
    }

    if stream.check(&lexer::TokenKind::Delimiter(lexer::Delimiter::LBracket)) {
        let mut depth = 1;
        loop {
            match &stream.peek().kind {
                lexer::TokenKind::Delimiter(lexer::Delimiter::LBracket) => {
                    depth += 1;
                    stream.consume();
                }
                lexer::TokenKind::Delimiter(lexer::Delimiter::RBracket) => {
                    depth -= 1;
                    stream.consume();
                    if depth == 0 {
                        return true;
                    }
                }
                lexer::TokenKind::EOF => return false,
                _ => stream.consume(),
            }
        }
    }

    true
}
