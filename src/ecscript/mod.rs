mod ast;
mod builtin;
mod env;
pub mod error;
mod eval;
mod func;
mod io_state;
mod lexer;
mod parser;
mod pratt;
mod value;

use self::{
    env::Environment,
    error::{ParseError, RuntimeError},
};

pub use self::value::{Value, display_value, repr_value};

#[derive(Debug)]
pub enum InterpreterError {
    Parse(ParseError),
    Runtime(RuntimeError),
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
