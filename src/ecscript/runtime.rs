use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::ecscript::{
    Environment,
    error::{self, ParseError, RuntimeError},
    eval, io_state, lexer, module, parser, pratt,
    value::Value,
};

/// 解释器执行过程中可能出现的两类错误。
#[derive(Debug)]
pub enum InterpreterError {
    Parse(ParseError),
    Runtime(RuntimeError),
}

/// 脚本文件模式下可能出现的错误：读取失败或执行失败。
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
    /// 将错误格式化为用户可读的消息。
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
    /// 附带源码文本，输出带行列号和上下文的错误信息。
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

/// 持有顶层 `Environment` 的解释器实例，适合交互式 REPL 等场景复用环境。
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

    /// 执行一段完整的 ecscript 脚本：tokenize → parse → eval。
    pub fn run(&self, src: &str) -> Result<(), InterpreterError> {
        let tokens = lexer::tokenize(src)?;
        let stmts = parser::parse_script(&tokens)?;
        eval::eval_script(&stmts, &self.env)?;
        Ok(())
    }

    /// 只求值一个表达式（非语句），返回 `Value`。
    pub fn eval_expr(&self, src: &str) -> Result<Value, InterpreterError> {
        let tokens = lexer::tokenize(src)?;
        let expr = pratt::parse_expr(&tokens)?;
        let value = eval::eval_expr(&expr, &self.env)?;
        Ok(value)
    }
}

/// 重置 REPL 的输出状态（换行标记等）。
pub fn reset_repl_output_state() {
    io_state::reset_output_state();
}

/// REPL 输出是否需要补一个换行。
pub fn repl_output_needs_newline() -> bool {
    io_state::output_needs_newline()
}

/// tokenize → parse → eval 一步到位求值表达式，返回 `Value`。
///
/// 与 `Interpreter::eval_expr` 的区别是接受任意 `Environment` 而非 `'static`，
/// 且将 lex/parse 错误统一转为 `RuntimeError`。
pub fn eval_expr_src(src: &str, env: &Environment<'_>) -> error::EvalResult<Value> {
    let tokens = lexer::tokenize(src)
        .map_err(|e| RuntimeError::new(0, error::RuntimeErrorKind::ParseInExpr, e.message))?;

    let expr = pratt::parse_expr(&tokens).map_err(|e| {
        RuntimeError::new(e.offset, error::RuntimeErrorKind::ParseInExpr, e.message)
    })?;

    let value = eval::eval_expr(&expr, env)?;

    Ok(value)
}

pub fn run_script_source(src: &str, env: &Environment<'_>) -> Result<(), InterpreterError> {
    run_script_source_with_stdin(src, env, None)
}

/// 执行一段脚本源码，可选传入一份模拟标准输入的文本快照。
///
/// `stdin_text` 只服务于 `stdin()` / `read_lines()` 这类内置函数，
/// 不会自动转成命令桥的 `stdin_override`。
pub fn run_script_source_with_stdin(
    src: &str,
    env: &Environment<'_>,
    stdin_text: Option<&str>,
) -> Result<(), InterpreterError> {
    let tokens = lexer::tokenize(src)?;
    let stmts = parser::parse_script(&tokens)?;
    let ctx = eval::EvalContext::plain(None, stdin_text, None, None);
    eval::eval_script_with_io_ctx(&stmts, env, ctx)?;
    Ok(())
}

pub fn run_script_file(
    path: impl AsRef<Path>,
    env: &Environment<'_>,
) -> Result<(), ScriptFileError> {
    run_script_file_with_stdin(path, env, None)
}

/// 读取并执行脚本文件，可选传入模拟标准输入的文本快照。
///
/// 文件模式下的 `stdin()` / `read_lines()` 通过此方式消费管道输入或重定向输入。
/// 同时传入文件所在目录以支持 `use` 相对路径模块导入。
pub fn run_script_file_with_stdin(
    path: impl AsRef<Path>,
    env: &Environment<'_>,
    stdin_text: Option<&str>,
) -> Result<(), ScriptFileError> {
    let path = path.as_ref();
    let source = fs::read_to_string(path).map_err(|err| ScriptFileError::Read {
        path: path.to_path_buf(),
        err,
    })?;
    let tokens = lexer::tokenize(&source).map_err(|err| ScriptFileError::Script {
        source: source.clone(),
        err: InterpreterError::Parse(err),
    })?;
    let stmts = parser::parse_script(&tokens).map_err(|err| ScriptFileError::Script {
        source: source.clone(),
        err: InterpreterError::Parse(err),
    })?;
    let loader = module::ModuleLoader::new();
    let ctx = eval::EvalContext::plain(None, stdin_text, path.parent(), Some(&loader));
    eval::eval_script_with_io_ctx(&stmts, env, ctx)
        .map(|_| ())
        .map_err(|err| ScriptFileError::Script {
            source,
            err: InterpreterError::Runtime(err),
        })
}
