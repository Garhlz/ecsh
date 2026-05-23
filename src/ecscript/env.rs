use crate::ecscript::{
    error::{EvalResult, RuntimeError, RuntimeErrorKind},
    value::{Binding, Slot, Value},
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
pub struct Environment<'a> {
    vars: RefCell<HashMap<String, Binding>>,
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
    pub fn find_root(&'a self) -> &'a Environment<'a> {
        if self.parent.is_some() {
            self.parent.unwrap().find_root()
        } else {
            self
        }
    }
    // 当前层环境没有，插入；有则报错
    // let 语句。会遮蔽父环境的变量名
    pub fn insert(&self, name: String, value: Binding, span: usize) -> EvalResult<()> {
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
        if let Some(value) = self.vars.borrow().get(name) {
            match value {
                Binding::Direct(value) => Ok(value.clone()),
                Binding::Shared(slot) => Ok(slot.borrow().clone()),
            }
        } else {
            if let Some(parent) = self.parent {
                parent.get(name, span)
            } else if let Some(builtin) = crate::ecscript::builtin::lookup_builtin(name) {
                Ok(Value::Builtin(builtin))
            } else {
                Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::UndefinedVariable,
                    format!("undefined variable '{}'", name),
                ))
            }
        }
    }

    /// 重新赋值——沿作用域链查找变量并更新。
    ///
    /// 只处理简单的变量名赋值（`x = value`）。
    /// 字段赋值（`obj.name = value`）和索引赋值（`arr[i] = value`）
    /// 由 eval 层的 `assign_target` 函数处理，避免环境层反向依赖求值层。
    pub fn set(&self, name: &str, value: Value, span: usize) -> EvalResult<()> {
        enum Found {
            Direct,
            Shared(Slot),
        }

        // 先在读借用下探测类型，clone Slot（只增加引用计数），然后立刻 drop 读借用。
        // 这样后续的 borrow_mut 不会与活跃的 borrow 冲突。
        let found = {
            let vars = self.vars.borrow();
            vars.get(name).map(|b| match b {
                Binding::Direct(..) => Found::Direct,
                Binding::Shared(slot) => Found::Shared(slot.clone()),
            })
        };

        match found {
            Some(Found::Direct) => {
                self.vars
                    .borrow_mut()
                    .insert(name.to_string(), Binding::Direct(value));
                Ok(())
            }
            Some(Found::Shared(slot)) => {
                *slot.borrow_mut() = value;
                Ok(())
            }
            None => {
                // 尝试递归从父环境中寻找
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

    pub fn capture_upvalue(&self, name: &str, span: usize) -> Option<Slot> {
        if self.parent.is_none() {
            // 当前层已经是root层，不进行变量提升
            return None;
        }

        // 先在读借用下探测类型，clone Slot（只增加引用计数），然后立刻 drop 读借用。
        // 这样后续的 borrow_mut 不会与活跃的 borrow 冲突。
        let found_value = {
            let vars = self.vars.borrow();
            vars.get(name).map(|b| match b {
                Binding::Direct(value) => Binding::Direct(value.clone()),
                Binding::Shared(slot) => Binding::Shared(slot.clone()),
            })
        };

        match found_value {
            Some(Binding::Direct(value)) => {
                let slot = Rc::new(RefCell::new(value));
                self.vars
                    .borrow_mut()
                    .insert(name.to_string(), Binding::Shared(slot.clone())); //slot.clone()只增加引用计数
                Some(slot)
            }
            Some(Binding::Shared(slot)) => Some(slot.clone()),
            None => {
                if let Some(parent) = self.parent {
                    parent.capture_upvalue(name, span)
                } else {
                    // 已经是root环境，在任何函数中都可以访问，不需要提升到堆上
                    None
                }
            }
        }
    }
}
