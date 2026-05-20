use crate::ecscript::{
    error::{EvalResult, RuntimeError, RuntimeErrorKind},
    value::Value,
};
use std::collections::HashMap;
pub struct Environment {
    vars: HashMap<String, Value>,
}

impl Environment {
    pub fn new() -> Self {
        Self {
            vars: HashMap::new(),
        }
    }
    pub fn insert(&mut self, name: String, value: Value) {
        self.vars.insert(name, value);
    }
    pub fn get(&self, name: &str, span: usize) -> EvalResult<Value> {
        self.vars.get(name).cloned().ok_or_else(|| {
            RuntimeError::new(
                span,
                RuntimeErrorKind::UndefinedVariable,
                format!("undefined variable '{}'", name),
            )
        })
    }
}
