use std::{cell::RefCell, collections::HashMap, rc::Rc};

use crate::ecscript::{
    ast::{Expr, ExprKind, InfixOper, Literal, PrefixOper, RangeExpr, Stmt},
    builtin::run_builtin,
    env::Environment,
    error::{EvalResult, RuntimeError, RuntimeErrorKind},
    value::Value,
};
#[derive(Debug, Clone, PartialEq)]
pub enum ExecFlow {
    Normal,
    Break(usize),
    Continue(usize),
    // Return(Value),
}

pub fn eval_script(stmts: &[Stmt], env: &Environment<'_>) -> EvalResult<ExecFlow> {
    for stmt in stmts {
        match eval_stmt(stmt, env)? {
            ExecFlow::Normal => continue,
            ExecFlow::Break(span) => {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::BreakOutsideLoop,
                    "break outside loop",
                ));
            }
            ExecFlow::Continue(span) => {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::ContinueOutsideLoop,
                    "continue outside loop",
                ));
            }
        }
    }
    Ok(ExecFlow::Normal)
}

/// 求值单条语句。
///
///
/// 有能力改变控制流的语句自己返回，否则用全局的Normal
/// 循环语句消费break/continue，其他都只是透传
/// 写完之后重构多次
pub fn eval_stmt(stmt: &Stmt, env: &Environment<'_>) -> Result<ExecFlow, RuntimeError> {
    match stmt {
        Stmt::Let { name, expr, span } => {
            let value = eval_expr(expr, env)?;
            env.insert(name.clone(), value, *span)?;
        }
        Stmt::Assign { target, expr, span } => {
            let value = eval_expr(expr, env)?;
            env.set(target, value, *span)?
        }
        Stmt::ExprStmt { expr, .. } => {
            eval_expr(expr, env)?;
        }
        Stmt::Block { stmts, .. } => return eval_block(stmts, env),
        Stmt::If {
            cond,
            then_body,
            else_body,
            span,
        } => {
            let cond_var = expect_bool(eval_expr(cond, env)?, *span, "if condition")?;
            if cond_var {
                return eval_block(then_body, env);
            } else {
                return eval_block(else_body, env);
            }
        }
        Stmt::While { cond, body, span } => loop {
            let cond_var = expect_bool(eval_expr(cond, env)?, *span, "while condition")?;
            if !cond_var {
                break;
            }
            match eval_block(body, env)? {
                // 捕捉透传上来的控制流，控制外层循环的状态
                ExecFlow::Break(_) => break,
                ExecFlow::Continue(_) => continue,
                ExecFlow::Normal => {}
            }
        },
        Stmt::ForIn {
            var,
            iterable,
            body,
            span,
        } => {
            let coll = eval_expr(iterable, env)?;
            // 支持对数组和对象（键）的遍历
            match coll {
                Value::Array(arr) => {
                    let items: Vec<Value> = arr.borrow().clone();
                    // 先拍平迭代快照，避免循环体再次借用同一个 RefCell 时触发运行时借用冲突。
                    for value in items {
                        let new_env = Environment::new_child(env);
                        new_env.insert(var.clone(), value, *span)?;
                        // 依然捕获消费eval_block透传上来的控制流
                        match eval_block(body, &new_env)? {
                            ExecFlow::Break(_) => break,
                            ExecFlow::Continue(_) => continue,
                            ExecFlow::Normal => {}
                        }
                    }
                }
                Value::Object(obj) => {
                    let mut keys: Vec<String> = obj.borrow().keys().cloned().collect();
                    keys.sort(); // 排序之后稳定遍历
                    for key in keys {
                        let new_env = Environment::new_child(env);
                        new_env.insert(var.clone(), Value::String(key), *span)?;
                        match eval_block(body, &new_env)? {
                            ExecFlow::Break(_) => break,
                            ExecFlow::Continue(_) => continue,
                            ExecFlow::Normal => {}
                        }
                    }
                }
                other => {
                    return Err(RuntimeError::new(
                        *span,
                        RuntimeErrorKind::TypeMismatch,
                        format!(
                            "for-in iterable must be Array or Object, got {}",
                            other.type_name()
                        ),
                    ));
                }
            }
        }
        Stmt::ForRange {
            var,
            range,
            body,
            span,
        } => {
            let RangeExpr {
                start,
                end,
                inclusive,
            } = range;
            let start = expect_int(eval_expr(start, env)?, *span, "for range start")?;
            let end = expect_int(eval_expr(end, env)?, *span, "for range end")?;
            // `start..end` 和 `start..=end` 的具体迭代器类型不同，这里先统一擦成 trait object。
            let iterator: Box<dyn Iterator<Item = i64>> = if *inclusive {
                Box::new(start..=end)
            } else {
                Box::new(start..end)
            };
            for i in iterator {
                let new_env = Environment::new_child(env);
                new_env.insert(var.clone(), Value::Int(i), *span)?;
                match eval_block(body, &new_env)? {
                    ExecFlow::Break(_) => break,
                    ExecFlow::Continue(_) => continue,
                    ExecFlow::Normal => {}
                }
            }
        }
        Stmt::Break { span } => {
            return Ok(ExecFlow::Break(*span));
        }
        Stmt::Continue { span } => {
            return Ok(ExecFlow::Continue(*span));
        }
    }
    Ok(ExecFlow::Normal)
}

fn eval_block(stmts: &[Stmt], env: &Environment<'_>) -> Result<ExecFlow, RuntimeError> {
    let new_env = Environment::new_child(env);
    for stmt in stmts {
        match eval_stmt(stmt, &new_env)? {
            ExecFlow::Normal => continue,
            flow => return Ok(flow),
        };
    }
    Ok(ExecFlow::Normal)
}

fn expect_bool(value: Value, span: usize, context: &str) -> EvalResult<bool> {
    match value {
        Value::Bool(b) => Ok(b),
        other => Err(RuntimeError::new(
            span,
            RuntimeErrorKind::TypeMismatch,
            format!("{context} must be Bool, got {}", other.type_name()),
        )),
    }
}

fn expect_int(value: Value, span: usize, context: &str) -> EvalResult<i64> {
    match value {
        Value::Int(i) => Ok(i),
        other => Err(RuntimeError::new(
            span,
            RuntimeErrorKind::TypeMismatch,
            format!("{context} must be Int, got {}", other.type_name()),
        )),
    }
}

pub fn eval_expr(expr: &Expr, env: &Environment) -> EvalResult<Value> {
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
                let val = eval_expr(right, env)?;
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
                let val = eval_expr(right, env)?;
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
            let left_val = eval_expr(left, env)?;
            match oper {
                // 这里加入了逻辑运算的短路设定
                InfixOper::And => eval_and_short_circuit(left_val, right, env, span),
                InfixOper::Or => eval_or_short_circuit(left_val, right, env, span),
                _ => {
                    let right_val = eval_expr(right, env)?;
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
                    }
                }
            }
        }
        ExprKind::Array(vec_expr) => {
            let mut values = Vec::new();
            for expr in vec_expr {
                let val = eval_expr(expr, env)?;
                values.push(val);
            }
            let arr_val = Value::Array(Rc::new(RefCell::new(values)));
            Ok(arr_val)
        }
        ExprKind::Object(hashmap_expr) => {
            let mut hash_map = HashMap::new();
            for (name, value) in hashmap_expr {
                let right_val = eval_expr(value, env)?;
                hash_map.insert(name.clone(), right_val);
            }
            Ok(Value::Object(Rc::new(RefCell::new(hash_map))))
        }
        ExprKind::Index(base, index_expr) => {
            let base_val = eval_expr(base, env)?;
            let index_val = eval_expr(index_expr, env)?;

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
            let obj_val = eval_expr(obj, env)?;
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
            let callee = eval_expr(name_expr, env)?;
            let mut args = Vec::new();
            for arg_expr in args_expr {
                let arg = eval_expr(arg_expr, env)?;
                args.push(arg);
            }

            match callee {
                Value::Builtin(builtin) => run_builtin(builtin, args, span),
                other => Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::NotCallable,
                    format!("{} is not callable", other.type_name()),
                )),
            }
        }
        ExprKind::Range(RangeExpr {
            start,
            end,
            inclusive,
        }) => {
            let start = expect_int(eval_expr(start, env)?, span, "range start")?;
            let end = expect_int(eval_expr(end, env)?, span, "range end")?;

            // 转为数组
            if *inclusive {
                let vec: Vec<Value> = (start..=end).map(|val| Value::Int(val)).collect();
                Ok(Value::Array(Rc::new(RefCell::new(vec))))
            } else {
                let vec: Vec<Value> = (start..end).map(|val| Value::Int(val)).collect();
                Ok(Value::Array(Rc::new(RefCell::new(vec))))
            }
        }
    }
}

fn eval_add(left: Value, right: Value, span: usize) -> EvalResult<Value> {
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

fn eval_sub(left: Value, right: Value, span: usize) -> EvalResult<Value> {
    match (&left, &right) {
        (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a - b)),
        (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 - b)),
        (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a - *b as f64)),
        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a - b)),
        _ => Err(RuntimeError::new(
            span,
            RuntimeErrorKind::TypeMismatch,
            format!(
                "cannot subtract {} and {}",
                left.type_name(),
                right.type_name()
            ),
        )),
    }
}

fn eval_mul(left: Value, right: Value, span: usize) -> EvalResult<Value> {
    match (&left, &right) {
        (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a * b)),
        (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 * b)),
        (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a * *b as f64)),
        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a * b)),
        _ => Err(RuntimeError::new(
            span,
            RuntimeErrorKind::TypeMismatch,
            format!(
                "cannot multiply {} and {}",
                left.type_name(),
                right.type_name()
            ),
        )),
    }
}

fn eval_div(left: Value, right: Value, span: usize) -> EvalResult<Value> {
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

fn eval_mod(left: Value, right: Value, span: usize) -> EvalResult<Value> {
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

fn eval_eq(left: Value, right: Value, span: usize) -> EvalResult<Value> {
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

fn eval_ne(left: Value, right: Value, span: usize) -> EvalResult<Value> {
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

fn eval_lt(left: Value, right: Value, span: usize) -> EvalResult<Value> {
    match (&left, &right) {
        (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a < b)),
        (Value::Int(a), Value::Float(b)) => Ok(Value::Bool((*a as f64) < *b)),
        (Value::Float(a), Value::Int(b)) => Ok(Value::Bool(*a < (*b as f64))),
        (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a < b)),
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

fn eval_gt(left: Value, right: Value, span: usize) -> EvalResult<Value> {
    match (&left, &right) {
        (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a > b)),
        (Value::Int(a), Value::Float(b)) => Ok(Value::Bool((*a as f64) > *b)),
        (Value::Float(a), Value::Int(b)) => Ok(Value::Bool(*a > (*b as f64))),
        (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a > b)),
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

fn eval_le(left: Value, right: Value, span: usize) -> EvalResult<Value> {
    match (&left, &right) {
        (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a <= b)),
        (Value::Int(a), Value::Float(b)) => Ok(Value::Bool((*a as f64) <= *b)),
        (Value::Float(a), Value::Int(b)) => Ok(Value::Bool(*a <= (*b as f64))),
        (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a <= b)),
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

fn eval_ge(left: Value, right: Value, span: usize) -> EvalResult<Value> {
    match (&left, &right) {
        (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a >= b)),
        (Value::Int(a), Value::Float(b)) => Ok(Value::Bool((*a as f64) >= *b)),
        (Value::Float(a), Value::Int(b)) => Ok(Value::Bool(*a >= (*b as f64))),
        (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a >= b)),
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

fn eval_and_short_circuit(
    left: Value,
    right: &Expr,
    env: &Environment<'_>,
    span: usize,
) -> EvalResult<Value> {
    match left {
        Value::Bool(false) => Ok(Value::Bool(false)),
        Value::Bool(true) => {
            let right = eval_expr(right, env)?;
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

fn eval_or_short_circuit(
    left: Value,
    right: &Expr,
    env: &Environment<'_>,
    span: usize,
) -> EvalResult<Value> {
    match left {
        Value::Bool(true) => Ok(Value::Bool(true)),
        Value::Bool(false) => {
            let right = eval_expr(right, env)?;
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

// ── 测试 ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::eval_expr;
    use crate::ecscript::{
        env::Environment,
        error::{RuntimeError, RuntimeErrorKind},
        lexer::tokenize,
        pratt::parse_expr,
        value::Value,
    };

    /// tokenize → parse → eval 一步到位
    fn eval_src(src: &str, env: &Environment) -> Result<Value, RuntimeError> {
        let tokens = tokenize(src).unwrap();
        let expr = parse_expr(&tokens).unwrap();
        eval_expr(&expr, env)
    }

    fn env_with(name: &str, val: Value) -> Environment<'_> {
        let env = Environment::new();
        env.insert(name.to_string(), val, 0).unwrap();
        env
    }

    // ── 字面量 ────────────────────────────────────────────

    #[test]
    fn eval_literal_nil() {
        let env = Environment::new();
        assert_eq!(eval_src("nil", &env), Ok(Value::Nil));
    }

    #[test]
    fn eval_literal_bool() {
        let env = Environment::new();
        assert_eq!(eval_src("true", &env), Ok(Value::Bool(true)));
        assert_eq!(eval_src("false", &env), Ok(Value::Bool(false)));
    }

    #[test]
    fn eval_literal_int() {
        let env = Environment::new();
        assert_eq!(eval_src("42", &env), Ok(Value::Int(42)));
    }

    #[test]
    fn eval_literal_float() {
        let env = Environment::new();
        assert_eq!(eval_src("2.5", &env), Ok(Value::Float(2.5)));
    }

    #[test]
    fn eval_literal_string() {
        let env = Environment::new();
        assert_eq!(
            eval_src("\"hello\"", &env),
            Ok(Value::String("hello".to_string()))
        );
    }

    #[test]
    fn eval_array_literal_allows_mixed_types() {
        let env = Environment::new();
        let value = eval_src("[1, \"x\", true]", &env).unwrap();
        let Value::Array(arr) = value else {
            panic!("expected array");
        };
        assert_eq!(
            *arr.borrow(),
            vec![Value::Int(1), Value::String("x".into()), Value::Bool(true)]
        );
    }

    #[test]
    fn eval_object_literal_uses_string_keys() {
        let env = Environment::new();
        let value = eval_src("{name: 1, \"age\": 2}", &env).unwrap();
        let Value::Object(obj) = value else {
            panic!("expected object");
        };
        let obj = obj.borrow();
        assert_eq!(obj.get("name"), Some(&Value::Int(1)));
        assert_eq!(obj.get("age"), Some(&Value::Int(2)));
    }

    // ── 变量读取 ──────────────────────────────────────────

    #[test]
    fn eval_variable_success() {
        let env = env_with("x", Value::Int(10));
        assert_eq!(eval_src("x", &env), Ok(Value::Int(10)));
    }

    #[test]
    fn eval_builtin_len_from_environment_fallback() {
        let env = Environment::new();
        assert_eq!(eval_src("len([1, 2, 3])", &env), Ok(Value::Int(3)));
    }

    #[test]
    fn eval_builtin_to_json_sorted_output() {
        let env = Environment::new();
        assert_eq!(
            eval_src("to_json({b: 2, a: 1})", &env),
            Ok(Value::String("{\"a\":1,\"b\":2}".into()))
        );
    }

    #[test]
    fn eval_undefined_variable() {
        let env = Environment::new();
        let err = eval_src("y", &env).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::UndefinedVariable);
        assert!(err.message.contains("y"));
    }

    #[test]
    fn eval_undefined_variable_has_span() {
        let env = Environment::new();
        let err = eval_src("y", &env).unwrap_err();
        // "y" 只有 1 个字节，end offset 是 1
        assert_eq!(err.offset, 1);
    }

    // ── 前缀运算符 ────────────────────────────────────────

    #[test]
    fn eval_prefix_neg_int() {
        let env = Environment::new();
        assert_eq!(eval_src("-5", &env), Ok(Value::Int(-5)));
    }

    #[test]
    fn eval_prefix_neg_float() {
        let env = Environment::new();
        assert_eq!(eval_src("-3.5", &env), Ok(Value::Float(-3.5)));
    }

    #[test]
    fn eval_prefix_neg_variable() {
        let env = env_with("n", Value::Int(7));
        assert_eq!(eval_src("-n", &env), Ok(Value::Int(-7)));
    }

    #[test]
    fn eval_prefix_not_true() {
        let env = Environment::new();
        assert_eq!(eval_src("!true", &env), Ok(Value::Bool(false)));
    }

    #[test]
    fn eval_prefix_not_false() {
        let env = Environment::new();
        assert_eq!(eval_src("!false", &env), Ok(Value::Bool(true)));
    }

    #[test]
    fn eval_prefix_neg_type_error() {
        let env = Environment::new();
        let err = eval_src("-\"hello\"", &env).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
    }

    #[test]
    fn eval_prefix_not_type_error() {
        let env = Environment::new();
        let err = eval_src("!42", &env).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
    }

    // ── 算术运算符 ────────────────────────────────────────

    #[test]
    fn eval_add_int() {
        let env = Environment::new();
        assert_eq!(eval_src("3 + 4", &env), Ok(Value::Int(7)));
    }

    #[test]
    fn eval_sub_int() {
        let env = Environment::new();
        assert_eq!(eval_src("10 - 3", &env), Ok(Value::Int(7)));
    }

    #[test]
    fn eval_mul_int() {
        let env = Environment::new();
        assert_eq!(eval_src("6 * 7", &env), Ok(Value::Int(42)));
    }

    #[test]
    fn eval_div_int() {
        let env = Environment::new();
        assert_eq!(eval_src("10 / 3", &env), Ok(Value::Int(3)));
    }

    #[test]
    fn eval_mod_int() {
        let env = Environment::new();
        assert_eq!(eval_src("10 % 3", &env), Ok(Value::Int(1)));
    }

    #[test]
    fn eval_mixed_int_float_promotes() {
        let env = Environment::new();
        assert_eq!(eval_src("1 + 2.5", &env), Ok(Value::Float(3.5)));
        assert_eq!(eval_src("2.5 - 1", &env), Ok(Value::Float(1.5)));
        assert_eq!(eval_src("3 * 2.0", &env), Ok(Value::Float(6.0)));
        assert_eq!(eval_src("5.0 / 2", &env), Ok(Value::Float(2.5)));
    }

    #[test]
    fn eval_add_string_concat() {
        let env = Environment::new();
        assert_eq!(
            eval_src("\"hello\" + \" world\"", &env),
            Ok(Value::String("hello world".to_string()))
        );
    }

    // ── 除零 ──────────────────────────────────────────────

    #[test]
    fn eval_div_by_zero_int() {
        let env = Environment::new();
        let err = eval_src("1 / 0", &env).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::DivisionByZero);
    }

    #[test]
    fn eval_div_by_zero_float() {
        let env = Environment::new();
        let err = eval_src("1.0 / 0.0", &env).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::DivisionByZero);
    }

    #[test]
    fn eval_mod_by_zero() {
        let env = Environment::new();
        let err = eval_src("5 % 0", &env).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::DivisionByZero);
    }

    // ── 比较运算符 ────────────────────────────────────────

    #[test]
    fn eval_eq_int() {
        let env = Environment::new();
        assert_eq!(eval_src("5 == 5", &env), Ok(Value::Bool(true)));
        assert_eq!(eval_src("5 == 3", &env), Ok(Value::Bool(false)));
    }

    #[test]
    fn eval_eq_int_float_promotes() {
        let env = Environment::new();
        assert_eq!(eval_src("5 == 5.0", &env), Ok(Value::Bool(true)));
    }

    #[test]
    fn eval_eq_nil() {
        let env = Environment::new();
        assert_eq!(eval_src("nil == nil", &env), Ok(Value::Bool(true)));
    }

    #[test]
    fn eval_ne_nil() {
        let env = Environment::new();
        assert_eq!(eval_src("nil != nil", &env), Ok(Value::Bool(false)));
    }

    #[test]
    fn eval_eq_bool() {
        let env = Environment::new();
        assert_eq!(eval_src("true == true", &env), Ok(Value::Bool(true)));
        assert_eq!(eval_src("true == false", &env), Ok(Value::Bool(false)));
    }

    #[test]
    fn eval_eq_string() {
        let env = Environment::new();
        assert_eq!(eval_src("\"a\" == \"a\"", &env), Ok(Value::Bool(true)));
        assert_eq!(eval_src("\"a\" == \"b\"", &env), Ok(Value::Bool(false)));
    }

    #[test]
    fn eval_lt_int() {
        let env = Environment::new();
        assert_eq!(eval_src("1 < 2", &env), Ok(Value::Bool(true)));
        assert_eq!(eval_src("2 < 1", &env), Ok(Value::Bool(false)));
    }

    #[test]
    fn eval_lt_int_float_promotes() {
        let env = Environment::new();
        assert_eq!(eval_src("1 < 2.0", &env), Ok(Value::Bool(true)));
    }

    #[test]
    fn eval_gt_int() {
        let env = Environment::new();
        assert_eq!(eval_src("2 > 1", &env), Ok(Value::Bool(true)));
        assert_eq!(eval_src("1 > 2", &env), Ok(Value::Bool(false)));
    }

    #[test]
    fn eval_le_ge_int() {
        let env = Environment::new();
        assert_eq!(eval_src("1 <= 1", &env), Ok(Value::Bool(true)));
        assert_eq!(eval_src("1 <= 0", &env), Ok(Value::Bool(false)));
        assert_eq!(eval_src("1 >= 1", &env), Ok(Value::Bool(true)));
        assert_eq!(eval_src("0 >= 1", &env), Ok(Value::Bool(false)));
    }

    #[test]
    fn eval_cross_type_comparison_error() {
        let env = Environment::new();
        let err = eval_src("1 == true", &env).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
    }

    // ── 逻辑运算符 ────────────────────────────────────────

    #[test]
    fn eval_and() {
        let env = Environment::new();
        assert_eq!(eval_src("true && true", &env), Ok(Value::Bool(true)));
        assert_eq!(eval_src("true && false", &env), Ok(Value::Bool(false)));
        assert_eq!(eval_src("false && true", &env), Ok(Value::Bool(false)));
    }

    #[test]
    fn eval_or() {
        let env = Environment::new();
        assert_eq!(eval_src("true || false", &env), Ok(Value::Bool(true)));
        assert_eq!(eval_src("false || false", &env), Ok(Value::Bool(false)));
    }

    #[test]
    fn eval_logical_type_error() {
        let env = Environment::new();
        assert!(eval_src("1 && true", &env).is_err());
        assert!(eval_src("false || 0", &env).is_err());
    }

    #[test]
    fn eval_and_short_circuits_on_false_left() {
        let env = Environment::new();
        assert_eq!(eval_src("false && missing", &env), Ok(Value::Bool(false)));
    }

    #[test]
    fn eval_or_short_circuits_on_true_left() {
        let env = Environment::new();
        assert_eq!(eval_src("true || missing", &env), Ok(Value::Bool(true)));
    }

    #[test]
    fn eval_and_evaluates_right_when_left_is_true() {
        let env = Environment::new();
        let err = eval_src("true && missing", &env).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::UndefinedVariable);
    }

    #[test]
    fn eval_or_evaluates_right_when_left_is_false() {
        let env = Environment::new();
        let err = eval_src("false || missing", &env).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::UndefinedVariable);
    }

    // ── 优先级 ────────────────────────────────────────────

    #[test]
    fn eval_mul_before_add() {
        let env = Environment::new();
        assert_eq!(eval_src("1 + 2 * 3", &env), Ok(Value::Int(7)));
    }

    #[test]
    fn eval_parens_override() {
        let env = Environment::new();
        assert_eq!(eval_src("(1 + 2) * 3", &env), Ok(Value::Int(9)));
    }

    #[test]
    fn eval_comparison_before_logical() {
        let env = Environment::new();
        assert_eq!(eval_src("1 < 2 && 3 > 0", &env), Ok(Value::Bool(true)));
    }

    #[test]
    fn eval_prefix_before_binary() {
        let env = Environment::new();
        assert_eq!(eval_src("-3 + 5", &env), Ok(Value::Int(2)));
    }

    #[test]
    fn eval_double_prefix() {
        let env = Environment::new();
        assert_eq!(eval_src("!!true", &env), Ok(Value::Bool(true)));
        assert_eq!(eval_src("!!false", &env), Ok(Value::Bool(false)));
    }

    // ── 复杂表达式 ────────────────────────────────────────

    #[test]
    fn eval_complex_arithmetic() {
        let env = Environment::new();
        // 1 + 2 * 3 - 4 / 2  = 1 + 6 - 2 = 5
        assert_eq!(eval_src("1 + 2 * 3 - 4 / 2", &env), Ok(Value::Int(5)));
    }

    #[test]
    fn eval_with_variables() {
        let env = env_with("a", Value::Int(3));
        env.insert("b".to_string(), Value::Int(4), 0).unwrap();
        assert_eq!(eval_src("a + b", &env), Ok(Value::Int(7)));
        assert_eq!(eval_src("a * b", &env), Ok(Value::Int(12)));
    }

    #[test]
    fn eval_nested_logical() {
        let env = Environment::new();
        // (true || false) && !false  = true && true = true
        assert_eq!(
            eval_src("(true || false) && !false", &env),
            Ok(Value::Bool(true))
        );
    }

    // ── span 传播 ─────────────────────────────────────────

    #[test]
    fn eval_type_error_has_correct_span() {
        let env = Environment::new();
        let err = eval_src("1 + \"x\"", &env).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
        assert_eq!(err.offset, 3);
    }

    #[test]
    fn eval_array_index_requires_int() {
        let env = Environment::new();
        let err = eval_src("[1][\"x\"]", &env).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
        assert_eq!(err.message, "array index must be Int, got String");
    }

    #[test]
    fn eval_non_indexable_base_reports_types() {
        let env = Environment::new();
        let err = eval_src("1[0]", &env).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
        assert_eq!(err.message, "cannot index Int with Int");
    }

    #[test]
    fn eval_missing_field_reports_field_name() {
        let env = Environment::new();
        let err = eval_src("{a: 1}.b", &env).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::NonExistentField);
        assert_eq!(err.message, "object has no field 'b'");
    }

    #[test]
    fn eval_field_on_non_object() {
        let env = Environment::new();
        let err = eval_src("1.name", &env).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
        assert_eq!(err.message, "cannot access field 'name' on Int");
    }

    #[test]
    fn eval_array_index_out_of_bounds() {
        let env = Environment::new();
        let err = eval_src("[1, 2][5]", &env).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::IndexOutOfBounds);
    }

    #[test]
    fn eval_array_index_negative() {
        let env = Environment::new();
        let err = eval_src("[1][-1]", &env).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::IndexOutOfBounds);
    }

    #[test]
    fn eval_object_index_string_reads_field() {
        let env = Environment::new();
        assert_eq!(eval_src("{a: 1}[\"a\"]", &env), Ok(Value::Int(1)));
    }

    #[test]
    fn eval_call_builtin_len() {
        let env = Environment::new();
        assert_eq!(eval_src("len([])", &env), Ok(Value::Int(0)));
        assert_eq!(eval_src("len([1, 2, 3])", &env), Ok(Value::Int(3)));
    }

    #[test]
    fn eval_call_builtin_push() {
        let env = env_with(
            "a",
            Value::Array(std::rc::Rc::new(std::cell::RefCell::new(vec![Value::Int(
                1,
            )]))),
        );
        let _ = eval_src("push(a, 2)", &env).unwrap();
        if let Ok(Value::Array(arr)) = env.get("a", 0) {
            assert_eq!(*arr.borrow(), vec![Value::Int(1), Value::Int(2)]);
        } else {
            panic!("expected array");
        }
    }
}

// ── 语句测试 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod stmt_tests {
    use super::{ExecFlow, eval_expr, eval_script};
    use crate::ecscript::{
        ast::{AssignTarget, Expr, ExprKind, Literal, Stmt},
        env::Environment,
        error::{RuntimeError, RuntimeErrorKind},
        lexer::tokenize,
        parser::parse_script,
        pratt::parse_expr,
        value::Value,
    };

    fn eval_script_src(src: &str, env: &Environment<'_>) -> Result<ExecFlow, RuntimeError> {
        let tokens = tokenize(src).unwrap();
        let stmts = parse_script(&tokens).unwrap();
        eval_script(&stmts, env)
    }

    fn lit_int(n: i64) -> Expr {
        Expr {
            kind: ExprKind::Literal(Literal::Int(n)),
            span: 0,
        }
    }

    // ── let 语句 ──────────────────────────────────────────

    #[test]
    fn eval_let_inserts_variable() {
        let env = Environment::new();
        eval_script_src("let x = 42;", &env).unwrap();
        assert_eq!(env.get("x", 0), Ok(Value::Int(42)));
    }

    #[test]
    fn eval_let_duplicate_in_same_scope() {
        let env = Environment::new();
        let err = eval_script_src("let x = 1; let x = 2;", &env).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::DuplicateVariable);
        assert!(err.message.contains("x"));
    }

    #[test]
    fn eval_block_duplicate_in_same_scope() {
        let env = Environment::new();
        let err = eval_script_src("{ let y = 1; let y = 2; }", &env).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::DuplicateVariable);
        assert!(err.message.contains("y"));
    }

    // ── assign 语句 ───────────────────────────────────────

    #[test]
    fn eval_assign_updates_variable() {
        let env = Environment::new();
        eval_script_src("let x = 10; x = 20;", &env).unwrap();
        assert_eq!(env.get("x", 0), Ok(Value::Int(20)));
    }

    #[test]
    fn eval_assign_undeclared_variable() {
        let env = Environment::new();
        let err = eval_script_src("x = 5;", &env).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::UndefinedVariable);
    }

    #[test]
    fn eval_block_assign_undeclared_variable() {
        let env = Environment::new();
        let err = eval_script_src("{ y = 5; }", &env).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::UndefinedVariable);
        assert!(err.message.contains("y"));
    }

    #[test]
    fn eval_assign_requires_existing_variable() {
        let env = Environment::new();
        let stmts = vec![Stmt::Assign {
            target: AssignTarget::Name("x".into()),
            expr: lit_int(5),
            span: 0,
        }];
        let err = eval_script(&stmts, &env).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::UndefinedVariable);
    }

    // ── 表达式语句 ────────────────────────────────────────

    #[test]
    fn eval_expr_stmt_discards_value() {
        let env = Environment::new();
        let result = eval_script_src("42;", &env);
        assert!(result.is_ok());
    }

    // ── block 作用域 ──────────────────────────────────────

    #[test]
    fn eval_block_new_scope_let_does_not_leak() {
        let env = Environment::new();
        eval_script_src("let x = 1; { let y = 2; }", &env).unwrap();
        assert_eq!(env.get("x", 0), Ok(Value::Int(1)));
        assert_eq!(
            env.get("y", 0).unwrap_err().kind,
            RuntimeErrorKind::UndefinedVariable
        );
    }

    #[test]
    fn eval_block_reads_outer_variables() {
        let env = Environment::new();
        eval_script_src("let x = 10;", &env).unwrap();
        // x is visible from inside the block (via parent chain)
        let env_child = Environment::new_child(&env);
        let tokens = tokenize("x").unwrap();
        let expr = parse_expr(&tokens).unwrap();
        assert_eq!(eval_expr(&expr, &env_child), Ok(Value::Int(10)));
    }

    #[test]
    fn eval_block_assigns_outer_variable() {
        let env = Environment::new();
        eval_script_src("let x = 1; { x = 10; }", &env).unwrap();
        assert_eq!(env.get("x", 0), Ok(Value::Int(10)));
    }

    #[test]
    fn eval_block_let_shadows_outer() {
        let env = Environment::new();
        eval_script_src("let x = 1; { let x = 2; }", &env).unwrap();
        // outer x unchanged after block
        assert_eq!(env.get("x", 0), Ok(Value::Int(1)));
    }

    // ── eval_script 多语句 ────────────────────────────────

    #[test]
    fn eval_script_multiple_statements() {
        let env = Environment::new();
        eval_script_src("let x = 3; let y = x + 1;", &env).unwrap();
        assert_eq!(env.get("x", 0), Ok(Value::Int(3)));
        assert_eq!(env.get("y", 0), Ok(Value::Int(4)));
    }

    #[test]
    fn eval_script_returns_normal() {
        let env = Environment::new();
        let flow = eval_script_src("let x = 1;", &env).unwrap();
        assert_eq!(flow, ExecFlow::Normal);
    }

    #[test]
    fn eval_script_error_stops_execution() {
        let env = Environment::new();
        let err = eval_script_src("let x = 1; y; x = 2;", &env).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::UndefinedVariable);
        // let x = 1 executed, but "x = 2" did not (stopped at unknown variable y)
        assert_eq!(env.get("x", 0), Ok(Value::Int(1)));
    }

    #[test]
    fn eval_builtin_shadowing_reports_not_callable() {
        let env = Environment::new();
        let err = eval_script_src("let len = 1; len([1]);", &env).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::NotCallable);
        assert_eq!(err.message, "Int is not callable");
    }

    // ── 字段 / 索引赋值 ───────────────────────────────────

    #[test]
    fn eval_field_assign_writes_to_object() {
        let env = Environment::new();
        eval_script_src("let o = {name: \"e\"}; o.name = \"x\";", &env).unwrap();
        let Value::Object(obj) = env.get("o", 0).unwrap() else {
            panic!("expected object");
        };
        assert_eq!(
            obj.borrow().get("name").cloned(),
            Some(Value::String("x".into()))
        );
    }

    #[test]
    fn eval_index_assign_writes_to_array() {
        let env = Environment::new();
        eval_script_src("let a = [1, 2, 3]; a[0] = 99;", &env).unwrap();
        let Value::Array(arr) = env.get("a", 0).unwrap() else {
            panic!("expected array");
        };
        assert_eq!(
            *arr.borrow(),
            vec![Value::Int(99), Value::Int(2), Value::Int(3)]
        );
    }

    #[test]
    fn eval_index_assign_writes_to_object() {
        let env = Environment::new();
        eval_script_src("let o = {}; o[\"key\"] = 42;", &env).unwrap();
        let Value::Object(obj) = env.get("o", 0).unwrap() else {
            panic!("expected object");
        };
        assert_eq!(obj.borrow().get("key").cloned(), Some(Value::Int(42)));
    }

    #[test]
    fn eval_index_assign_out_of_bounds() {
        let env = Environment::new();
        let err = eval_script_src("let a = [1]; a[5] = 2;", &env).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::IndexOutOfBounds);
    }

    // ── 内置函数 via script ───────────────────────────────

    #[test]
    fn eval_builtin_push_via_script() {
        let env = Environment::new();
        eval_script_src("let a = [1]; push(a, 2);", &env).unwrap();
        let Value::Array(arr) = env.get("a", 0).unwrap() else {
            panic!("expected array");
        };
        assert_eq!(*arr.borrow(), vec![Value::Int(1), Value::Int(2)]);
    }

    #[test]
    fn eval_builtin_pop_via_script() {
        let env = Environment::new();
        eval_script_src("let a = [1, 2]; let x = pop(a);", &env).unwrap();
        let Value::Array(arr) = env.get("a", 0).unwrap() else {
            panic!("expected array");
        };
        assert_eq!(*arr.borrow(), vec![Value::Int(1)]);
        assert_eq!(env.get("x", 0), Ok(Value::Int(2)));
    }

    #[test]
    fn eval_builtin_insert_via_script() {
        let env = Environment::new();
        eval_script_src("let a = [1, 3]; insert(a, 1, 2);", &env).unwrap();
        let Value::Array(arr) = env.get("a", 0).unwrap() else {
            panic!("expected array");
        };
        assert_eq!(
            *arr.borrow(),
            vec![Value::Int(1), Value::Int(2), Value::Int(3)]
        );
    }

    #[test]
    fn eval_builtin_remove_via_script() {
        let env = Environment::new();
        eval_script_src("let a = [1, 99, 2]; let x = remove(a, 1);", &env).unwrap();
        let Value::Array(arr) = env.get("a", 0).unwrap() else {
            panic!("expected array");
        };
        assert_eq!(*arr.borrow(), vec![Value::Int(1), Value::Int(2)]);
        assert_eq!(env.get("x", 0), Ok(Value::Int(99)));
    }

    #[test]
    fn eval_builtin_keys_via_script() {
        let env = Environment::new();
        eval_script_src("let o = {b: 2, a: 1}; let k = keys(o);", &env).unwrap();
        let Value::Array(keys) = env.get("k", 0).unwrap() else {
            panic!("expected array");
        };
        assert_eq!(
            *keys.borrow(),
            vec![Value::String("a".into()), Value::String("b".into())]
        );
    }

    // ── 控制流 ────────────────────────────────────────────

    #[test]
    fn eval_if_then_true_branch() {
        let env = Environment::new();
        eval_script_src("let x = 0; if true { x = 1; }", &env).unwrap();
        assert_eq!(env.get("x", 0), Ok(Value::Int(1)));
    }

    #[test]
    fn eval_if_else_false_branch() {
        let env = Environment::new();
        eval_script_src("let x = 0; if false { x = 1; } else { x = 2; }", &env).unwrap();
        assert_eq!(env.get("x", 0), Ok(Value::Int(2)));
    }

    #[test]
    fn eval_if_else_if_chain() {
        let env = Environment::new();
        eval_script_src(
            "let x = 0; if false { x = 1; } else if true { x = 2; } else { x = 3; }",
            &env,
        )
        .unwrap();
        assert_eq!(env.get("x", 0), Ok(Value::Int(2)));
    }

    #[test]
    fn eval_if_condition_must_be_bool() {
        let env = Environment::new();
        let err = eval_script_src("if 1 { 0; }", &env).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
        assert_eq!(err.offset, 2);
        assert_eq!(err.message, "if condition must be Bool, got Int");
    }

    #[test]
    fn eval_while_loop_iterates() {
        let env = Environment::new();
        eval_script_src("let x = 0; while x < 3 { x = x + 1; }", &env).unwrap();
        assert_eq!(env.get("x", 0), Ok(Value::Int(3)));
    }

    #[test]
    fn eval_while_skips_when_condition_false() {
        let env = Environment::new();
        eval_script_src("let x = 0; while false { x = 1; }", &env).unwrap();
        assert_eq!(env.get("x", 0), Ok(Value::Int(0)));
    }

    #[test]
    fn eval_while_break() {
        let env = Environment::new();
        eval_script_src(
            "let x = 0; while x < 10 { x = x + 1; if x == 3 { break; } }",
            &env,
        )
        .unwrap();
        assert_eq!(env.get("x", 0), Ok(Value::Int(3)));
    }

    #[test]
    fn eval_while_continue_skips_iteration() {
        let env = Environment::new();
        eval_script_src(
            "let x = 0; let y = 0; while x < 3 { x = x + 1; if x == 2 { continue; } y = y + 1; }",
            &env,
        )
        .unwrap();
        assert_eq!(env.get("x", 0), Ok(Value::Int(3)));
        assert_eq!(env.get("y", 0), Ok(Value::Int(2)));
    }

    #[test]
    fn eval_while_condition_error_is_specific() {
        let env = Environment::new();
        let err = eval_script_src("while 1 { 0; }", &env).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
        assert_eq!(err.offset, 5);
        assert_eq!(err.message, "while condition must be Bool, got Int");
    }

    #[test]
    fn eval_for_range_exclusive() {
        let env = Environment::new();
        eval_script_src("let s = 0; for i in 0..3 { s = s + i; }", &env).unwrap();
        assert_eq!(env.get("s", 0), Ok(Value::Int(3))); // 0 + 1 + 2
    }

    #[test]
    fn eval_for_range_inclusive() {
        let env = Environment::new();
        eval_script_src("let s = 0; for i in 0..=3 { s = s + i; }", &env).unwrap();
        assert_eq!(env.get("s", 0), Ok(Value::Int(6))); // 0 + 1 + 2 + 3
    }

    #[test]
    fn eval_for_in_array() {
        let env = Environment::new();
        eval_script_src(
            "let a = [10, 20, 30]; let s = 0; for v in a { s = s + v; }",
            &env,
        )
        .unwrap();
        assert_eq!(env.get("s", 0), Ok(Value::Int(60)));
    }

    #[test]
    fn eval_for_in_object_keys() {
        let env = Environment::new();
        eval_script_src(
            "let o = {b: 2, a: 1}; let k = \"\"; for key in o { k = k + key; }",
            &env,
        )
        .unwrap();
        assert_eq!(env.get("k", 0), Ok(Value::String("ab".into())));
    }

    #[test]
    fn eval_for_in_array_uses_snapshot_when_body_mutates_source() {
        let env = Environment::new();
        eval_script_src(
            "let a = [1, 2]; let s = 0; for v in a { s = s + v; push(a, 10); }",
            &env,
        )
        .unwrap();
        assert_eq!(env.get("s", 0), Ok(Value::Int(3)));
        let Value::Array(arr) = env.get("a", 0).unwrap() else {
            panic!("expected array");
        };
        assert_eq!(
            *arr.borrow(),
            vec![Value::Int(1), Value::Int(2), Value::Int(10), Value::Int(10)]
        );
    }

    #[test]
    fn eval_for_in_non_iterable_reports_type() {
        let env = Environment::new();
        let err = eval_script_src("for x in 1 { 0; }", &env).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
        assert_eq!(err.offset, 3);
        assert_eq!(
            err.message,
            "for-in iterable must be Array or Object, got Int"
        );
    }

    #[test]
    fn eval_for_range_start_error_is_specific() {
        let env = Environment::new();
        let err = eval_script_src("for i in true..3 { 0; }", &env).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
        assert_eq!(err.offset, 3);
        assert_eq!(err.message, "for range start must be Int, got Bool");
    }

    #[test]
    fn eval_for_range_end_error_is_specific() {
        let env = Environment::new();
        let err = eval_script_src("for i in 0..false { 0; }", &env).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
        assert_eq!(err.offset, 3);
        assert_eq!(err.message, "for range end must be Int, got Bool");
    }

    #[test]
    fn eval_for_break_inside_loop() {
        let env = Environment::new();
        eval_script_src(
            "let s = 0; for i in 0..10 { if i == 3 { break; } s = s + i; }",
            &env,
        )
        .unwrap();
        assert_eq!(env.get("s", 0), Ok(Value::Int(3))); // 0 + 1 + 2
    }

    #[test]
    fn eval_for_continue_skips_iteration() {
        let env = Environment::new();
        eval_script_src(
            "let s = 0; for i in 0..5 { if i == 2 { continue; } s = s + i; }",
            &env,
        )
        .unwrap();
        assert_eq!(env.get("s", 0), Ok(Value::Int(8))); // 0 + 1 + 3 + 4
    }

    #[test]
    fn eval_break_outside_loop_reports_error() {
        let env = Environment::new();
        let err = eval_script_src("break;", &env).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::BreakOutsideLoop);
        assert_eq!(err.offset, 5);
        assert_eq!(err.message, "break outside loop");
    }

    #[test]
    fn eval_continue_outside_loop_reports_error() {
        let env = Environment::new();
        let err = eval_script_src("continue;", &env).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::ContinueOutsideLoop);
        assert_eq!(err.offset, 8);
        assert_eq!(err.message, "continue outside loop");
    }

    #[test]
    fn eval_nested_while_break_only_inner() {
        let env = Environment::new();
        eval_script_src(
            "let x = 0; let y = 0; while x < 3 { x = x + 1; y = 0; while y < 3 { y = y + 1; if y == 2 { break; } } }",
            &env,
        )
        .unwrap();
        assert_eq!(env.get("x", 0), Ok(Value::Int(3)));
    }
}
