use crate::ecscript::{
    ast::AssignTarget,
    builtin::lookup_builtin,
    error::{EvalResult, RuntimeError, RuntimeErrorKind},
    eval::eval_expr,
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
    pub fn find_root(&'a self) -> &'a Environment<'a> {
        if self.parent.is_some() {
            self.parent.unwrap().find_root()
        } else {
            self
        }
    }
    // 当前层环境没有，插入；有则报错
    // let 语句。会遮蔽父环境的变量名
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
            // builtin 只作为“查不到变量时”的兜底，这样 `let len = 1;` 可以自然遮蔽内置名。
            // 当前builtin类型只可能是从环境中查找失败得到的
            } else if let Some(builtin) = lookup_builtin(name) {
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

    // 重新赋值语句
    pub fn set(&self, target: &AssignTarget, value: Value, span: usize) -> EvalResult<()> {
        match target {
            AssignTarget::Name(name) => {
                if self.vars.borrow().contains_key(name) {
                    self.vars.borrow_mut().insert(name.to_string(), value);
                    Ok(())
                } else {
                    // 如果有父环境，沿着父指针向上寻找
                    if let Some(parent) = self.parent {
                        parent.set(target, value, span)
                    } else {
                        Err(RuntimeError::new(
                            span,
                            RuntimeErrorKind::UndefinedVariable,
                            format!("undefined variable '{}'", name),
                        ))
                    }
                }
            }
            /*
            AssignTarget::Field / Index
            这是“对某个已经求值出来的容器做原地修改”，不需要再单独找名字所在作用域。
            因为 base/object 先 eval_expr(...)，里面如果有变量读取，本来就会通过 env.get()自动沿作用域链查找。
            */
            // 处理arr[i] = some_expr 这种类型的
            // 还有object["key"] 这种情况
            AssignTarget::Index {
                object: base,
                index: index_expr,
            } => {
                let base_val = eval_expr(base, &self)?;
                let index_val = eval_expr(index_expr, &self)?;

                match (base_val, index_val) {
                    (Value::Array(arr), Value::Int(i)) => {
                        let idx = crate::ecscript::value::validate_array_index(
                            i,
                            arr.borrow().len(),
                            false,
                            span,
                        )?;
                        arr.borrow_mut()[idx] = value;
                        Ok(())
                    }
                    (Value::Object(obj), Value::String(k)) => {
                        // 这里是不管原本是否存在都进行插入
                        obj.borrow_mut().insert(k, value);
                        Ok(())
                    }
                    (Value::Array(_), other) => Err(RuntimeError::new(
                        span,
                        RuntimeErrorKind::TypeMismatch,
                        format!(
                            "array assignment index must be Int, got {}",
                            other.type_name()
                        ),
                    )),
                    (Value::Object(_), other) => Err(RuntimeError::new(
                        span,
                        RuntimeErrorKind::TypeMismatch,
                        format!(
                            "object assignment index must be String, got {}",
                            other.type_name()
                        ),
                    )),
                    (other, index) => Err(RuntimeError::new(
                        span,
                        RuntimeErrorKind::TypeMismatch,
                        format!(
                            "cannot assign through index on {} with {}",
                            other.type_name(),
                            index.type_name()
                        ),
                    )),
                }
            }
            // 处理 obj.name = some_expr
            AssignTarget::Field { object: obj, field } => {
                let base_val = eval_expr(obj, &self)?;
                if let Value::Object(obj) = base_val {
                    obj.borrow_mut().insert(field.clone(), value);
                    Ok(())
                } else {
                    Err(RuntimeError::new(
                        span,
                        RuntimeErrorKind::TypeMismatch,
                        format!(
                            "cannot assign field '{}' on {}",
                            field,
                            base_val.type_name()
                        ),
                    ))
                }
            }
        }
    }
}
