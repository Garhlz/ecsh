use crate::ecscript::{
    ast::{Expr, ExprKind, InfixOper, Literal, PrefixOper},
    env::Environment,
    error::{EvalResult, RuntimeError, RuntimeErrorKind},
    value::Value,
};

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
                InfixOper::And => eval_and(left_val, right_val, span),
                InfixOper::Or => eval_or(left_val, right_val, span),
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
            format!("cannot subtract {} and {}", left.type_name(), right.type_name()),
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
            format!("cannot multiply {} and {}", left.type_name(), right.type_name()),
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
            format!("cannot divide {} and {}", left.type_name(), right.type_name()),
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
            format!("cannot compute modulo of {} and {}", left.type_name(), right.type_name()),
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
            format!("cannot compare {} and {}", left.type_name(), right.type_name()),
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
            format!("cannot compare {} and {}", left.type_name(), right.type_name()),
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
            format!("cannot compare {} and {}", left.type_name(), right.type_name()),
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
            format!("cannot compare {} and {}", left.type_name(), right.type_name()),
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
            format!("cannot compare {} and {}", left.type_name(), right.type_name()),
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
            format!("cannot compare {} and {}", left.type_name(), right.type_name()),
        )),
    }
}

fn eval_and(left: Value, right: Value, span: usize) -> EvalResult<Value> {
    match (&left, &right) {
        (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(*a && *b)),
        _ => Err(RuntimeError::new(
            span,
            RuntimeErrorKind::TypeMismatch,
            format!("'&&' requires Bool operands, got {} and {}", left.type_name(), right.type_name()),
        )),
    }
}

fn eval_or(left: Value, right: Value, span: usize) -> EvalResult<Value> {
    match (&left, &right) {
        (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(*a || *b)),
        _ => Err(RuntimeError::new(
            span,
            RuntimeErrorKind::TypeMismatch,
            format!("'||' requires Bool operands, got {} and {}", left.type_name(), right.type_name()),
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

    fn env_with(name: &str, val: Value) -> Environment {
        let mut env = Environment::new();
        env.insert(name.to_string(), val);
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
        assert_eq!(eval_src("3.14", &env), Ok(Value::Float(3.14)));
    }

    #[test]
    fn eval_literal_string() {
        let env = Environment::new();
        assert_eq!(eval_src("\"hello\"", &env), Ok(Value::String("hello".to_string())));
    }

    // ── 变量读取 ──────────────────────────────────────────

    #[test]
    fn eval_variable_success() {
        let env = env_with("x", Value::Int(10));
        assert_eq!(eval_src("x", &env), Ok(Value::Int(10)));
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
        assert!(eval_src("true || 0", &env).is_err());
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
        let mut env = env_with("a", Value::Int(3));
        env.insert("b".to_string(), Value::Int(4));
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
        // "1" 在 offset 1，"+" 在 offset 3（但 infix span 是 op 的 offset=3）
        let err = eval_src("1 + \"x\"", &env).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
        assert_eq!(err.offset, 3); // '+' 的 end offset
    }
}
