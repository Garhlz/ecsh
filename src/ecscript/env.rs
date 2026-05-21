use crate::ecscript::{
    error::{EvalResult, RuntimeError, RuntimeErrorKind},
    value::Value,
};
use std::cell::RefCell;
use std::collections::HashMap;
pub struct Environment<'a> {
    vars: RefCell<HashMap<String, Value>>,
    parent: Option<&'a Environment<'a>>,
}

impl<'a> Environment<'a> {
    pub fn new() -> Self {
        Self {
            vars: RefCell::new(HashMap::new()),
            parent: None,
        }
    }

    pub fn new_child(parent: &'a Environment<'a>) -> Self {
        Self {
            vars: RefCell::new(HashMap::new()),
            parent: Some(parent),
        }
    }

    // 当前层环境没有，插入；有则报错
    pub fn insert(&self, name: String, value: Value, span: usize) -> EvalResult<()> {
        if self.vars.borrow().contains_key(&name) {
            Err(RuntimeError::new(
                span,
                RuntimeErrorKind::DuplicateVariable,
                format!("variable '{}' already defined in this scope", name),
            ))
        } else {
            self.vars.borrow_mut().insert(name, value);
            Ok(())
        }
    }

    pub fn get(&self, name: &str, span: usize) -> EvalResult<Value> {
        if let Some(value) = self.vars.borrow().get(name).cloned() {
            Ok(value)
        } else {
            if let Some(parent) = self.parent {
                parent.get(name, span)
            } else {
                Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::UndefinedVariable,
                    format!("undefined variable '{}'", name),
                ))
            }
        }
    }

    pub fn set(&self, name: &str, value: Value, span: usize) -> EvalResult<()> {
        if self.vars.borrow().contains_key(name) {
            self.vars.borrow_mut().insert(name.to_string(), value);
            Ok(())
        } else {
            if let Some(parent) = self.parent {
                parent.set(name, value, span)
            } else {
                Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::UndefinedVariable,
                    format!("undefined variable '{}'", name),
                ))
            }
        }
    }
}
