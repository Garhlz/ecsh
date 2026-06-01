use std::{cell::RefCell, collections::HashMap, rc::Rc};

use crate::ecscript::{
    ast::{AssignTarget, CompoundAssignOp},
    env::Environment,
    error::{EvalResult, RuntimeError, RuntimeErrorKind},
    value::Value,
};

use super::{EvalContext, eval_add, eval_div, eval_expr_with_ctx, eval_mod, eval_mul, eval_sub};

// 赋值语句会先把左值解析成“可读可写的位置”，再统一执行 load/store。
// 这样 `a[i] += 1` 之类的复合赋值只需要解析一次目标，避免副作用重复发生。
pub(super) enum ResolvedAssignTarget<'a> {
    Name {
        name: String,
        env: &'a Environment<'a>,
    },
    Field {
        object: Rc<RefCell<HashMap<String, Value>>>,
        field: String,
    },
    ArrayIndex {
        array: Rc<RefCell<Vec<Value>>>,
        index: usize,
    },
    ObjectIndex {
        object: Rc<RefCell<HashMap<String, Value>>>,
        key: String,
    },
}

impl<'a> ResolvedAssignTarget<'a> {
    pub(super) fn load(&self, span: usize) -> EvalResult<Value> {
        match self {
            ResolvedAssignTarget::Name { name, env } => env.get(name, span),
            ResolvedAssignTarget::Field { object, field } => {
                object.borrow().get(field).cloned().ok_or_else(|| {
                    RuntimeError::new(
                        span,
                        RuntimeErrorKind::NonExistentField,
                        format!("object has no field '{}'", field),
                    )
                })
            }
            ResolvedAssignTarget::ArrayIndex { array, index } => {
                array.borrow().get(*index).cloned().ok_or_else(|| {
                    RuntimeError::new(
                        span,
                        RuntimeErrorKind::IndexOutOfBounds,
                        format!(
                            "array index {} out of bounds for length {}",
                            index,
                            array.borrow().len()
                        ),
                    )
                })
            }
            ResolvedAssignTarget::ObjectIndex { object, key } => {
                object.borrow().get(key).cloned().ok_or_else(|| {
                    RuntimeError::new(
                        span,
                        RuntimeErrorKind::NonExistentField,
                        format!("object has no field '{}'", key),
                    )
                })
            }
        }
    }

    pub(super) fn store(&self, value: Value, span: usize) -> EvalResult<()> {
        match self {
            ResolvedAssignTarget::Name { name, env } => env.set(name, value, span),
            ResolvedAssignTarget::Field { object, field } => {
                object.borrow_mut().insert(field.clone(), value);
                Ok(())
            }
            ResolvedAssignTarget::ArrayIndex { array, index } => {
                array.borrow_mut()[*index] = value;
                Ok(())
            }
            ResolvedAssignTarget::ObjectIndex { object, key } => {
                object.borrow_mut().insert(key.clone(), value);
                Ok(())
            }
        }
    }
}

pub(super) fn expect_bool(value: Value, span: usize, context: &str) -> EvalResult<bool> {
    match value {
        Value::Bool(b) => Ok(b),
        other => Err(RuntimeError::new(
            span,
            RuntimeErrorKind::TypeMismatch,
            format!("{context} must be Bool, got {}", other.type_name()),
        )),
    }
}

pub(super) fn expect_int(value: Value, span: usize, context: &str) -> EvalResult<i64> {
    match value {
        Value::Int(i) => Ok(i),
        other => Err(RuntimeError::new(
            span,
            RuntimeErrorKind::TypeMismatch,
            format!("{context} must be Int, got {}", other.type_name()),
        )),
    }
}

pub(super) fn resolve_assign_target<'a>(
    target: &AssignTarget,
    env: &'a Environment<'a>,
    span: usize,
    ctx: EvalContext<'_>,
) -> EvalResult<ResolvedAssignTarget<'a>> {
    // 这里负责把语法层左值变成运行时可操作目标。
    // 名字绑定保留对环境的引用；字段和索引访问则先把容器及键位解析出来。
    match target {
        AssignTarget::Name(name) => Ok(ResolvedAssignTarget::Name {
            name: name.clone(),
            env,
        }),
        AssignTarget::Field { object, field } => {
            let base_val = eval_expr_with_ctx(object, env, ctx)?;
            let Value::Object(obj) = base_val else {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!(
                        "cannot assign field '{}' on {}",
                        field,
                        base_val.type_name()
                    ),
                ));
            };
            Ok(ResolvedAssignTarget::Field {
                object: obj,
                field: field.clone(),
            })
        }
        AssignTarget::Index { object, index } => {
            let base_val = eval_expr_with_ctx(object, env, ctx)?;
            let index_val = eval_expr_with_ctx(index, env, ctx)?;

            match (base_val, index_val) {
                (Value::Array(arr), Value::Int(i)) => {
                    let idx = crate::ecscript::value::validate_array_index(
                        i,
                        arr.borrow().len(),
                        false,
                        span,
                    )?;
                    Ok(ResolvedAssignTarget::ArrayIndex {
                        array: arr,
                        index: idx,
                    })
                }
                (Value::Object(obj), Value::String(k)) => Ok(ResolvedAssignTarget::ObjectIndex {
                    object: obj,
                    key: k,
                }),
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
    }
}

/// 执行赋值操作。
///
/// 将 eval_expr 逻辑留在 eval 层，env 只负责变量名的作用域查找，
/// 避免环境层反向依赖求值层。
pub(super) fn assign_target(
    target: &AssignTarget,
    value: Value,
    env: &Environment<'_>,
    span: usize,
    ctx: EvalContext<'_>,
) -> EvalResult<()> {
    let target = resolve_assign_target(target, env, span, ctx)?;
    target.store(value, span)
}

pub(super) fn eval_compound_assign(
    op: CompoundAssignOp,
    left: Value,
    right: Value,
    span: usize,
) -> EvalResult<Value> {
    match op {
        CompoundAssignOp::Add => eval_add(left, right, span),
        CompoundAssignOp::Sub => eval_sub(left, right, span),
        CompoundAssignOp::Mul => eval_mul(left, right, span),
        CompoundAssignOp::Div => eval_div(left, right, span),
        CompoundAssignOp::Mod => eval_mod(left, right, span),
    }
}
