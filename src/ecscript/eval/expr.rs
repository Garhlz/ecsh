use std::{cell::RefCell, collections::HashMap, rc::Rc};

use crate::ecscript::{
    ast::{Expr, ExprKind, InfixOper, Literal, PrefixOper, RangeExpr},
    builtin::run_builtin,
    env::Environment,
    error::{EvalResult, RuntimeError, RuntimeErrorKind},
    func::{call_function, free_vars},
    value::{BuiltinContext, CommandInvocation, Function, Value},
};

use super::EvalContext;

pub fn eval_expr(expr: &Expr, env: &Environment<'_>) -> EvalResult<Value> {
    eval_expr_with_ctx(expr, env, EvalContext::plain(None, None, None, None))
}

pub(super) fn eval_expr_with_ctx(
    expr: &Expr,
    env: &Environment<'_>,
    ctx: EvalContext<'_>,
) -> EvalResult<Value> {
    // 表达式求值始终围绕“当前词法环境 + 额外运行时上下文”展开。
    // 其中环境负责变量解析，上下文补充 shell、stdin、模块目录等外围信息。
    let span = expr.span;
    match &expr.kind {
        ExprKind::Literal(lit) => match lit {
            Literal::Nil => Ok(Value::Nil),
            Literal::Bool(b) => Ok(Value::Bool(*b)),
            Literal::Int(i) => Ok(Value::Int(*i)),
            Literal::Float(f) => Ok(Value::Float(*f)),
            Literal::String(s) => Ok(Value::String(s.clone())),
        },
        ExprKind::Variable(name) => env.get(name, span),
        ExprKind::Prefix(oper, right) => match oper {
            PrefixOper::Neg => {
                let val = eval_expr_with_ctx(right, env, ctx)?;
                if let Value::Int(int_val) = val {
                    Ok(Value::Int(-int_val))
                } else if let Value::Float(float_val) = val {
                    Ok(Value::Float(-float_val))
                } else {
                    Err(RuntimeError::new(
                        span,
                        RuntimeErrorKind::TypeMismatch,
                        format!("cannot negate {}", val.type_name()),
                    ))
                }
            }
            PrefixOper::Not => {
                let val = eval_expr_with_ctx(right, env, ctx)?;
                if let Value::Bool(bool_val) = val {
                    Ok(Value::Bool(!bool_val))
                } else {
                    Err(RuntimeError::new(
                        span,
                        RuntimeErrorKind::TypeMismatch,
                        format!("cannot apply '!' to {}", val.type_name()),
                    ))
                }
            }
        },
        ExprKind::Infix(left, oper, right) => {
            let left_val = eval_expr_with_ctx(left, env, ctx)?;
            match oper {
                // `&&` / `||` 需要短路语义，因此右侧表达式不能提前求值。
                InfixOper::And => eval_and_short_circuit(left_val, right, env, span, ctx),
                InfixOper::Or => eval_or_short_circuit(left_val, right, env, span, ctx),
                _ => {
                    let right_val = eval_expr_with_ctx(right, env, ctx)?;
                    match oper {
                        InfixOper::Add => eval_add(left_val, right_val, span),
                        InfixOper::Sub => eval_sub(left_val, right_val, span),
                        InfixOper::Mul => eval_mul(left_val, right_val, span),
                        InfixOper::Div => eval_div(left_val, right_val, span),
                        InfixOper::Mod => eval_mod(left_val, right_val, span),
                        InfixOper::Eq => eval_eq(left_val, right_val, span),
                        InfixOper::Ne => eval_ne(left_val, right_val, span),
                        InfixOper::Lt => eval_lt(left_val, right_val, span),
                        InfixOper::Gt => eval_gt(left_val, right_val, span),
                        InfixOper::Le => eval_le(left_val, right_val, span),
                        InfixOper::Ge => eval_ge(left_val, right_val, span),
                        InfixOper::And | InfixOper::Or => unreachable!(),
                        _ => unreachable!(),
                    }
                }
            }
        }
        ExprKind::Array(vec_expr) => {
            let mut values = Vec::new();
            for expr in vec_expr {
                let val = eval_expr_with_ctx(expr, env, ctx)?;
                values.push(val);
            }
            Ok(Value::Array(Rc::new(RefCell::new(values))))
        }
        ExprKind::Object(hashmap_expr) => {
            let mut hash_map = HashMap::new();
            for (name, value) in hashmap_expr {
                let right_val = eval_expr_with_ctx(value, env, ctx)?;
                hash_map.insert(name.clone(), right_val);
            }
            Ok(Value::Object(Rc::new(RefCell::new(hash_map))))
        }
        ExprKind::Index(base, index_expr) => {
            let base_val = eval_expr_with_ctx(base, env, ctx)?;
            let index_val = eval_expr_with_ctx(index_expr, env, ctx)?;

            match (base_val, index_val) {
                (Value::Array(arr), Value::Int(i)) => {
                    let idx = crate::ecscript::value::validate_array_index(
                        i,
                        arr.borrow().len(),
                        false,
                        span,
                    )?;
                    arr.borrow().get(idx).cloned().ok_or_else(|| {
                        RuntimeError::new(
                            span,
                            RuntimeErrorKind::IndexOutOfBounds,
                            format!(
                                "array index {} out of bounds for length {}",
                                i,
                                arr.borrow().len()
                            ),
                        )
                    })
                }
                (Value::Object(obj), Value::String(k)) => {
                    if obj.borrow().contains_key(&k) {
                        obj.borrow().get(&k).cloned().ok_or_else(|| {
                            RuntimeError::new(
                                span,
                                RuntimeErrorKind::NonExistentField,
                                format!("object has no field '{}'", k),
                            )
                        })
                    } else {
                        Err(RuntimeError::new(
                            span,
                            RuntimeErrorKind::NonExistentField,
                            format!("object has no field '{}'", k),
                        ))
                    }
                }
                (Value::Array(_), other) => Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!("array index must be Int, got {}", other.type_name()),
                )),
                (Value::Object(_), other) => Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!("object index must be String, got {}", other.type_name()),
                )),
                (other, index) => Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!(
                        "cannot index {} with {}",
                        other.type_name(),
                        index.type_name()
                    ),
                )),
            }
        }
        ExprKind::Field(obj, name) => {
            let obj_val = eval_expr_with_ctx(obj, env, ctx)?;
            let Value::Object(obj) = obj_val else {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!("cannot access field '{}' on {}", name, obj_val.type_name()),
                ));
            };

            obj.borrow().get(name).cloned().ok_or_else(|| {
                RuntimeError::new(
                    span,
                    RuntimeErrorKind::NonExistentField,
                    format!("object has no field '{}'", name),
                )
            })
        }
        ExprKind::Call(name_expr, args_expr) => {
            let callee = eval_expr_with_ctx(name_expr, env, ctx)?;
            let mut args = Vec::new();
            for arg_expr in args_expr {
                let arg = eval_expr_with_ctx(arg_expr, env, ctx)?;
                args.push(arg);
            }

            match callee {
                Value::Function(func) => {
                    if let Some(value) = call_function(func, &args, env, span)? {
                        Ok(value)
                    } else {
                        Ok(Value::Nil)
                    }
                }
                Value::Builtin(builtin) => run_builtin(
                    builtin,
                    args,
                    span,
                    BuiltinContext {
                        shell_state: ctx.shell_state,
                        env,
                        stdin_text: ctx.stdin_text,
                    },
                ),
                other => Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::NotCallable,
                    format!("{} is not callable", other.type_name()),
                )),
            }
        }
        ExprKind::Range(RangeExpr { .. }) => Err(RuntimeError::new(
            span,
            RuntimeErrorKind::TypeMismatch,
            "range expressions are only valid in for loops; use range(start, end)",
        )),
        ExprKind::FuncLiteral { params, body } => {
            let mut captures = HashMap::new();

            // 匿名函数字面量与命名函数声明共享同一套捕获策略：
            // 先静态收集自由变量，再向定义点环境申请 upvalue 槽位。
            let free_set = free_vars(None, params, body)?;

            for name in free_set {
                if let Some(slot) = env.capture_upvalue(&name, span) {
                    captures.insert(name, slot);
                }
            }

            let func = Function {
                name: None,
                params: params.clone(),
                stmts: body.clone(),
                captures,
            };

            Ok(Value::Function(Rc::new(func)))
        }
        ExprKind::CommandLiteral(command) => Ok(Value::Command(CommandInvocation {
            command: command.clone(),
            cwd_override: None,
            env_override: None,
            stdin_override: None,
        })),
    }
}

// ── 算术运算 ──────────────────────────────────────────────────────────

/// 为纯数值算术运算符生成求值函数。
///
/// 适用于 `-` `*` 等只有 Int/Float 语义、无额外特殊逻辑的运算符。
///
/// 不适用于：
///   - `+`（还有字符串拼接语义，需手写额外分支）
///   - `/`（需要除零检查，需手写前置守卫）
///   - `%`（只接受 Int×Int，不接受 Float，需手写）
macro_rules! impl_arith {
    ($name:ident, $op:tt, $desc:literal) => {
        pub(super) fn $name(left: Value, right: Value, span: usize) -> EvalResult<Value> {
            match (&left, &right) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a $op b)),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 $op b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a $op *b as f64)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a $op b)),
                _ => Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!("cannot {} {} and {}", $desc, left.type_name(), right.type_name()),
                )),
            }
        }
    };
}

impl_arith!(eval_sub, -, "subtract");
impl_arith!(eval_mul, *, "multiply");

pub(super) fn eval_add(left: Value, right: Value, span: usize) -> EvalResult<Value> {
    match (&left, &right) {
        (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a + b)),
        (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 + b)),
        (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a + *b as f64)),
        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
        (Value::String(a), Value::String(b)) => Ok(Value::String(format!("{}{}", a, b))),
        _ => Err(RuntimeError::new(
            span,
            RuntimeErrorKind::TypeMismatch,
            format!("cannot add {} and {}", left.type_name(), right.type_name()),
        )),
    }
}

pub(super) fn eval_div(left: Value, right: Value, span: usize) -> EvalResult<Value> {
    match (&left, &right) {
        (_, Value::Int(0) | Value::Float(0.0)) => Err(RuntimeError::new(
            span,
            RuntimeErrorKind::DivisionByZero,
            "division by zero",
        )),
        (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a / b)),
        (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 / b)),
        (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a / *b as f64)),
        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a / b)),
        _ => Err(RuntimeError::new(
            span,
            RuntimeErrorKind::TypeMismatch,
            format!(
                "cannot divide {} and {}",
                left.type_name(),
                right.type_name()
            ),
        )),
    }
}

pub(super) fn eval_mod(left: Value, right: Value, span: usize) -> EvalResult<Value> {
    match (&left, &right) {
        (_, Value::Int(0)) => Err(RuntimeError::new(
            span,
            RuntimeErrorKind::DivisionByZero,
            "modulo by zero",
        )),
        (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a % b)),
        _ => Err(RuntimeError::new(
            span,
            RuntimeErrorKind::TypeMismatch,
            format!(
                "cannot compute modulo of {} and {}",
                left.type_name(),
                right.type_name()
            ),
        )),
    }
}

// ── 比较运算 ──────────────────────────────────────────────────────────

macro_rules! impl_ord_cmp {
    ($name:ident, $op:tt, $desc:literal) => {
        pub(super) fn $name(left: Value, right: Value, span: usize) -> EvalResult<Value> {
            match (&left, &right) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a $op b)),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Bool((*a as f64) $op *b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Bool(*a $op (*b as f64))),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a $op b)),
                _ => Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!("cannot {} {} and {}", $desc, left.type_name(), right.type_name()),
                )),
            }
        }
    };
}

impl_ord_cmp!(eval_lt, <, "compare");
impl_ord_cmp!(eval_gt, >, "compare");
impl_ord_cmp!(eval_le, <=, "compare");
impl_ord_cmp!(eval_ge, >=, "compare");

pub(super) fn eval_eq(left: Value, right: Value, span: usize) -> EvalResult<Value> {
    match (&left, &right) {
        (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a == b)),
        (Value::Int(a), Value::Float(b)) => Ok(Value::Bool(*a as f64 == *b)),
        (Value::Float(a), Value::Int(b)) => Ok(Value::Bool(*a == *b as f64)),
        (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a == b)),
        (Value::Nil, Value::Nil) => Ok(Value::Bool(true)),
        (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(a == b)),
        (Value::String(a), Value::String(b)) => Ok(Value::Bool(a == b)),
        _ => Err(RuntimeError::new(
            span,
            RuntimeErrorKind::TypeMismatch,
            format!(
                "cannot compare {} and {}",
                left.type_name(),
                right.type_name()
            ),
        )),
    }
}

pub(super) fn eval_ne(left: Value, right: Value, span: usize) -> EvalResult<Value> {
    match (&left, &right) {
        (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a != b)),
        (Value::Int(a), Value::Float(b)) => Ok(Value::Bool(*a as f64 != *b)),
        (Value::Float(a), Value::Int(b)) => Ok(Value::Bool(*a != *b as f64)),
        (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a != b)),
        (Value::Nil, Value::Nil) => Ok(Value::Bool(false)),
        (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(a != b)),
        (Value::String(a), Value::String(b)) => Ok(Value::Bool(a != b)),
        _ => Err(RuntimeError::new(
            span,
            RuntimeErrorKind::TypeMismatch,
            format!(
                "cannot compare {} and {}",
                left.type_name(),
                right.type_name()
            ),
        )),
    }
}

// ── 逻辑运算 ──────────────────────────────────────────────────────────

// 逻辑运算保持严格的 Bool 语义，并显式实现短路：
// 左侧已经决定结果时，右侧表达式不会被求值。
pub(super) fn eval_and_short_circuit(
    left: Value,
    right: &Expr,
    env: &Environment<'_>,
    span: usize,
    ctx: EvalContext<'_>,
) -> EvalResult<Value> {
    match left {
        Value::Bool(false) => Ok(Value::Bool(false)),
        Value::Bool(true) => {
            let right = eval_expr_with_ctx(right, env, ctx)?;
            match right {
                Value::Bool(value) => Ok(Value::Bool(value)),
                _ => Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!(
                        "'&&' requires Bool operands, got Bool and {}",
                        right.type_name()
                    ),
                )),
            }
        }
        _ => Err(RuntimeError::new(
            span,
            RuntimeErrorKind::TypeMismatch,
            format!("'&&' requires Bool left operand, got {}", left.type_name()),
        )),
    }
}

pub(super) fn eval_or_short_circuit(
    left: Value,
    right: &Expr,
    env: &Environment<'_>,
    span: usize,
    ctx: EvalContext<'_>,
) -> EvalResult<Value> {
    match left {
        Value::Bool(true) => Ok(Value::Bool(true)),
        Value::Bool(false) => {
            let right = eval_expr_with_ctx(right, env, ctx)?;
            match right {
                Value::Bool(value) => Ok(Value::Bool(value)),
                _ => Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::TypeMismatch,
                    format!(
                        "'||' requires Bool operands, got Bool and {}",
                        right.type_name()
                    ),
                )),
            }
        }
        _ => Err(RuntimeError::new(
            span,
            RuntimeErrorKind::TypeMismatch,
            format!("'||' requires Bool left operand, got {}", left.type_name()),
        )),
    }
}
