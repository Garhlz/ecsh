use crate::ecscript::{
    env::Environment,
    error::{RuntimeError, RuntimeErrorKind},
    eval::{ExecFlow, eval_stmt},
    value::{Function, Value},
};
use std::rc::Rc;
pub fn call_function(
    func: Rc<Function>,
    params_value: &Vec<Value>,
    env: &Environment<'_>,
    span: usize,
) -> Result<Option<Value>, RuntimeError> {
    let func_env = Environment::new_child(env.find_root());
    if func.params.len() != params_value.len() {
        return Err(RuntimeError::new(
            span,
            RuntimeErrorKind::ArityMismatch,
            "param number error",
        ));
    }

    let name = (*func).name.clone().unwrap();

    func_env.insert(name, Value::Function(func.clone()), span)?;

    for (param_name, param_value) in func.params.iter().zip(params_value.iter()) {
        func_env.insert(param_name.clone(), param_value.clone(), span)?;
    }

    for stmt in &(*func).stmts {
        match eval_stmt(stmt, &func_env)? {
            ExecFlow::Return { value, .. } => {
                return Ok(value);
            }
            _ => continue,
        };
    }
    return Ok(None);
}
