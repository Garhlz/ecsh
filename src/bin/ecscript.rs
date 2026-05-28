use ecsh::{
    diagnostics::print_error,
    ecscript::{
        Environment, Interpreter, InterpreterError, Value, error::ParseError,
        repl_output_needs_newline, repr_value, reset_repl_output_state,
        run_script_file_with_stdin,
    },
};
use rustyline::{DefaultEditor, error::ReadlineError};
use std::{
    env,
    io::{self, IsTerminal, Read},
    process,
};

fn main() {
    process::exit(match run() {
        Ok(()) => 0,
        Err(CliError::Usage(message)) => {
            eprintln!("{}", message);
            2
        }
        Err(CliError::Script { source, err }) => {
            print_error(err.format_with_source(&source));
            1
        }
        Err(CliError::Other(message)) => {
            print_error(message);
            1
        }
    });
}

enum InputMode {
    Repl,
    Stdin,
    Eval(String),
    File(String),
}

enum CliError {
    Usage(String),
    Script {
        source: String,
        err: InterpreterError,
    },
    Other(String),
}

fn run() -> Result<(), CliError> {
    match parse_args()? {
        InputMode::Repl => run_repl(),
        InputMode::Stdin => {
            let mut source = String::new();
            io::stdin()
                .read_to_string(&mut source)
                .map_err(|err| CliError::Other(format!("failed to read stdin: {}", err)))?;
            run_source(&source)
        }
        InputMode::Eval(source) => run_source(&source),
        InputMode::File(path) => {
            let stdin_text = if io::stdin().is_terminal() {
                None
            } else {
                let mut text = String::new();
                io::stdin()
                    .read_to_string(&mut text)
                    .map_err(|err| CliError::Other(format!("failed to read stdin: {}", err)))?;
                Some(text)
            };
            let env = Environment::new();
            run_script_file_with_stdin(&path, &env, stdin_text.as_deref()).map_err(|err| {
                match err {
                    ecsh::ecscript::ScriptFileError::Read { path, err } => {
                        CliError::Other(format!("failed to read '{}': {}", path.display(), err))
                    }
                    ecsh::ecscript::ScriptFileError::Script { source, err } => {
                        CliError::Script { source, err }
                    }
                }
            })
        }
    }
}

fn parse_args() -> Result<InputMode, CliError> {
    let mut args = env::args().skip(1);
    let Some(first) = args.next() else {
        return Ok(if io::stdin().is_terminal() {
            InputMode::Repl
        } else {
            InputMode::Stdin
        });
    };

    match first.as_str() {
        "-h" | "--help" => Err(CliError::Usage(usage())),
        "-e" | "--eval" => {
            let source = args.next().ok_or_else(|| {
                CliError::Usage("missing source after -e/--eval\n\n".to_string() + &usage())
            })?;
            if args.next().is_some() {
                return Err(CliError::Usage(
                    "unexpected extra arguments after -e/--eval\n\n".to_string() + &usage(),
                ));
            }
            Ok(InputMode::Eval(source))
        }
        path => {
            if args.next().is_some() {
                return Err(CliError::Usage(
                    "expected at most one script path\n\n".to_string() + &usage(),
                ));
            }
            Ok(InputMode::File(path.to_string()))
        }
    }
}

fn run_source(source: &str) -> Result<(), CliError> {
    let interpreter = Interpreter::new();
    interpreter.run(source).map_err(|err| CliError::Script {
        source: source.to_string(),
        err,
    })
}

fn history_path() -> Option<std::path::PathBuf> {
    dirs_next::data_dir().map(|mut p| {
        p.push("ecscript");
        std::fs::create_dir_all(&p).ok();
        p.push("history");
        p
    })
}

fn run_repl() -> Result<(), CliError> {
    let interpreter = Interpreter::new();
    let mut editor = DefaultEditor::new()
        .map_err(|err| CliError::Other(format!("failed to start REPL: {}", err)))?;

    if let Some(ref path) = history_path() {
        let _ = editor.load_history(path);
    }
    let mut buffer = String::new();

    println!("ecscript REPL  (type :quit to exit, :help for more)");

    loop {
        let prompt = if buffer.is_empty() { ">>> " } else { "... " };
        match editor.readline(prompt) {
            Ok(line) => {
                let command = line.trim();
                if buffer.is_empty() && command.is_empty() {
                    continue;
                }
                if buffer.is_empty() {
                    match command {
                        ":quit" | ":q" => break,
                        ":help" | ":h" => {
                            println!("{}", repl_help());
                            continue;
                        }
                        ":clear" => {
                            print!("\x1B[2J\x1B[H");
                            continue;
                        }
                        _ if command.starts_with(':') => {
                            println!(
                                "unknown command '{}', type :help for available commands",
                                command
                            );
                            continue;
                        }
                        _ => {}
                    }
                }

                if !buffer.is_empty() {
                    buffer.push('\n');
                }
                buffer.push_str(&line);
                reset_repl_output_state();

                match eval_repl_buffer(&interpreter, &buffer) {
                    ReplEval::Incomplete => continue,
                    ReplEval::Expr(value) => {
                        editor.add_history_entry(buffer.as_str()).map_err(|err| {
                            CliError::Other(format!("failed to save history entry: {}", err))
                        })?;
                        if should_echo_repl_value(&value) {
                            println!("{}", repr_value(&value));
                        } else if repl_output_needs_newline() {
                            println!();
                        }
                        buffer.clear();
                    }
                    ReplEval::Stmt => {
                        editor.add_history_entry(buffer.as_str()).map_err(|err| {
                            CliError::Other(format!("failed to save history entry: {}", err))
                        })?;
                        if repl_output_needs_newline() {
                            println!();
                        }
                        buffer.clear();
                    }
                    ReplEval::Error(err) => {
                        if repl_output_needs_newline() {
                            println!();
                        }
                        print_error(err.format_with_source(&buffer));
                        buffer.clear();
                    }
                }
            }
            Err(ReadlineError::Interrupted) => {
                println!("^C");
                buffer.clear();
            }
            Err(ReadlineError::Eof) => break,
            Err(err) => {
                return Err(CliError::Other(format!("REPL input error: {}", err)));
            }
        }
    }

    if let Some(ref path) = history_path() {
        let _ = editor.save_history(path);
    }
    Ok(())
}

fn should_echo_repl_value(value: &Value) -> bool {
    !matches!(value, Value::Nil)
}

enum ReplEval {
    Incomplete,
    Expr(Value),
    Stmt,
    Error(InterpreterError),
}

fn eval_repl_buffer(interpreter: &Interpreter, src: &str) -> ReplEval {
    match interpreter.eval_expr(src) {
        Ok(value) => ReplEval::Expr(value),
        Err(err @ InterpreterError::Runtime(_)) => ReplEval::Error(err),
        Err(expr_err @ InterpreterError::Parse(_)) => {
            // If the expression parse itself is incomplete,
            // return Incomplete without trying the statement path
            if is_incomplete_repl_error(&expr_err) {
                return ReplEval::Incomplete;
            }
            match interpreter.run(src) {
                Ok(()) => ReplEval::Stmt,
                Err(err) if is_incomplete_repl_error(&err) => ReplEval::Incomplete,
                Err(err) => ReplEval::Error(err),
            }
        }
    }
}

fn is_incomplete_repl_error(err: &InterpreterError) -> bool {
    let InterpreterError::Parse(ParseError { incomplete, .. }) = err else {
        return false;
    };
    *incomplete
}

fn repl_help() -> String {
    "\
REPL commands:
  :quit, :q      Exit the REPL
  :help, :h      Show this help
  :clear         Clear the screen

Input rules:
  Expressions are evaluated and printed:  1 + 2  →  3
  Statements end with semicolon:          let x = 1;
  Multi-line input auto-continues:        if true {
  Lambda short form:                      (x) => x + 1
  Named function:                         func add(a, b) { return a + b; }

Builtins: len push pop insert remove keys values to_json print println"
        .to_string()
}

fn usage() -> String {
    "Usage:
  ecscript                 Start the interactive REPL when stdin is a terminal, otherwise read a full script from stdin
  ecscript <file.ecs>      Run a script file
  ecscript -e '<code>'     Evaluate a source string

REPL:
  :quit                    Leave the REPL"
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::{ReplEval, eval_repl_buffer, is_incomplete_repl_error, should_echo_repl_value};
    use ecsh::ecscript::{Interpreter, InterpreterError, Value, error::ParseError};

    #[test]
    fn repl_suppresses_nil_values() {
        assert!(!should_echo_repl_value(&Value::Nil));
    }

    #[test]
    fn repl_still_shows_non_nil_values() {
        assert!(should_echo_repl_value(&Value::Int(42)));
        assert!(should_echo_repl_value(&Value::String("hi".into())));
    }

    #[test]
    fn repl_treats_open_block_as_incomplete() {
        let err = InterpreterError::Parse(ParseError::incomplete(
            1,
            "unterminated block, expected '}' before end of input",
        ));
        assert!(is_incomplete_repl_error(&err));
    }

    #[test]
    fn repl_does_not_treat_missing_semicolon_as_incomplete() {
        let err = InterpreterError::Parse(ParseError::new(
            9,
            "expected ';' after statement, found end of input",
        ));
        assert!(!is_incomplete_repl_error(&err));
    }

    #[test]
    fn repl_classifies_expression_result() {
        let interpreter = Interpreter::new();
        match eval_repl_buffer(&interpreter, "1 + 1") {
            ReplEval::Expr(Value::Int(2)) => {}
            other => panic!(
                "expected expression result, got {:?}",
                other_variant(&other)
            ),
        }
    }

    #[test]
    fn repl_classifies_multiline_func_header_as_incomplete() {
        let interpreter = Interpreter::new();
        assert!(matches!(
            eval_repl_buffer(&interpreter, "func add(a, b)"),
            ReplEval::Incomplete
        ));
    }

    #[test]
    fn repl_continues_on_open_paren() {
        let interpreter = Interpreter::new();
        assert!(matches!(
            eval_repl_buffer(&interpreter, "(1 + 2"),
            ReplEval::Incomplete
        ));
    }

    #[test]
    fn repl_continues_on_open_array() {
        let interpreter = Interpreter::new();
        assert!(matches!(
            eval_repl_buffer(&interpreter, "[1, 2"),
            ReplEval::Incomplete
        ));
    }

    #[test]
    fn repl_continues_on_open_object() {
        let interpreter = Interpreter::new();
        assert!(matches!(
            eval_repl_buffer(&interpreter, "{a: 1"),
            ReplEval::Incomplete
        ));
    }

    #[test]
    fn repl_continues_on_let_without_value() {
        let interpreter = Interpreter::new();
        assert!(matches!(
            eval_repl_buffer(&interpreter, "let x ="),
            ReplEval::Incomplete
        ));
    }

    fn other_variant(eval: &ReplEval) -> &'static str {
        match eval {
            ReplEval::Incomplete => "Incomplete",
            ReplEval::Expr(_) => "Expr",
            ReplEval::Stmt => "Stmt",
            ReplEval::Error(_) => "Error",
        }
    }
}
