use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::Path,
    rc::Rc,
};

use crate::ecscript::{
    ast::{
        AssignTarget, CompoundAssignOp, Expr, ExprKind, InfixOper, Literal, PrefixOper, RangeExpr,
        Stmt, StmtKind,
    },
    builtin::run_builtin,
    env::Environment,
    error::{EvalResult, RuntimeError, RuntimeErrorKind},
    func::{call_function, free_vars},
    module::{ModuleLoader, load_module},
    value::{Binding, BuiltinContext, CommandInvocation, Function, Value},
};
use crate::types::ShellState;
#[derive(Debug, Clone, PartialEq)]
pub enum ExecFlow {
    Normal,
    Break(usize),
    Continue(usize),
    Return { value: Option<Value>, span: usize },
}

#[derive(Clone, Copy)]
pub(crate) struct EvalContext<'a> {
    pub(crate) shell_state: Option<&'a ShellState>,
    pub(crate) stdin_text: Option<&'a str>,
    pub(crate) current_module_dir: Option<&'a Path>,
    pub(crate) module_loader: Option<&'a ModuleLoader>,
    module_exports: Option<&'a RefCell<HashSet<String>>>,
}

impl<'a> EvalContext<'a> {
    pub(crate) fn plain(
        shell_state: Option<&'a ShellState>,
        stdin_text: Option<&'a str>,
        current_module_dir: Option<&'a Path>,
        module_loader: Option<&'a ModuleLoader>,
    ) -> Self {
        Self {
            shell_state,
            stdin_text,
            current_module_dir,
            module_loader,
            module_exports: None,
        }
    }

    fn for_module(
        exports: &'a RefCell<HashSet<String>>,
        current_module_dir: Option<&'a Path>,
        module_loader: Option<&'a ModuleLoader>,
    ) -> Self {
        Self {
            shell_state: None,
            stdin_text: None,
            current_module_dir,
            module_loader,
            module_exports: Some(exports),
        }
    }
}

/// 把解析好的语句ast解析运行，返回执行流
pub fn eval_script(stmts: &[Stmt], env: &Environment<'_>) -> EvalResult<ExecFlow> {
    eval_script_with_ctx(stmts, env, None)
}

pub fn eval_script_with_ctx(
    stmts: &[Stmt],
    env: &Environment<'_>,
    shell_state: Option<&ShellState>,
) -> EvalResult<ExecFlow> {
    let ctx = EvalContext::plain(shell_state, None, None, None);
    eval_script_with_io_ctx(stmts, env, ctx)
}

pub(crate) fn eval_script_with_io_ctx(
    stmts: &[Stmt],
    env: &Environment<'_>,
    ctx: EvalContext<'_>,
) -> EvalResult<ExecFlow> {
    for stmt in stmts {
        match eval_stmt_with_ctx(stmt, env, ctx)? {
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
            ExecFlow::Return { span, .. } => {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::ReturnOutsideFunction,
                    "return outside function",
                ));
            }
        }
    }
    Ok(ExecFlow::Normal)
}

/// 执行顶层脚本，并在最后一条语句是表达式语句时返回它的值。
pub fn eval_top_level_script(stmts: &[Stmt], env: &Environment<'_>) -> EvalResult<Option<Value>> {
    eval_top_level_script_with_ctx(stmts, env, None)
}

pub fn eval_top_level_script_with_ctx(
    stmts: &[Stmt],
    env: &Environment<'_>,
    shell_state: Option<&ShellState>,
) -> EvalResult<Option<Value>> {
    let ctx = EvalContext::plain(shell_state, None, None, None);
    eval_top_level_script_with_io_ctx(stmts, env, ctx)
}

pub(crate) fn eval_top_level_script_with_io_ctx(
    stmts: &[Stmt],
    env: &Environment<'_>,
    ctx: EvalContext<'_>,
) -> EvalResult<Option<Value>> {
    let Some((last, prefix)) = stmts.split_last() else {
        return Ok(None);
    };

    for stmt in prefix {
        match eval_stmt_with_ctx(stmt, env, ctx)? {
            ExecFlow::Normal => {}
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
            ExecFlow::Return { span, .. } => {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::ReturnOutsideFunction,
                    "return outside function",
                ));
            }
        }
    }

    match &last.kind {
        StmtKind::ExprStmt { expr } => Ok(Some(eval_expr_with_ctx(expr, env, ctx)?)),
        _ => match eval_stmt_with_ctx(last, env, ctx)? {
            ExecFlow::Normal => Ok(None),
            ExecFlow::Break(span) => Err(RuntimeError::new(
                span,
                RuntimeErrorKind::BreakOutsideLoop,
                "break outside loop",
            )),
            ExecFlow::Continue(span) => Err(RuntimeError::new(
                span,
                RuntimeErrorKind::ContinueOutsideLoop,
                "continue outside loop",
            )),
            ExecFlow::Return { span, .. } => Err(RuntimeError::new(
                span,
                RuntimeErrorKind::ReturnOutsideFunction,
                "return outside function",
            )),
        },
    }
}

/// 求值单条语句。
///
/// 有能力改变控制流的语句自己返回，否则用全局的Normal
/// 循环语句消费break/continue，其他都只是透传
pub fn eval_stmt(
    stmt: &crate::ecscript::ast::Stmt,
    env: &Environment<'_>,
    // captures: Option<Rc<Function>>,
) -> Result<ExecFlow, RuntimeError> {
    eval_stmt_with_ctx(stmt, env, EvalContext::plain(None, None, None, None))
}

/// 在独立模块环境中执行脚本，并把 `pub` 绑定收集成模块对象返回。
///
/// 这层只负责：
/// - 复用普通语句执行逻辑
/// - 记录哪些绑定被声明为 `pub`
/// - 在执行结束后从模块环境中取出最终值
///
/// 它暂时不负责：
/// - 文件读取
/// - 路径解析
/// - 模块缓存
/// - `use ... as ...` 语法
pub fn eval_module(stmts: &[Stmt]) -> EvalResult<Value> {
    eval_module_in_dir(stmts, None, None)
}

pub(crate) fn eval_module_in_dir(
    stmts: &[Stmt],
    current_module_dir: Option<&Path>,
    module_loader: Option<&ModuleLoader>,
) -> EvalResult<Value> {
    let env = Environment::new();
    // 在eval_stmt_with_ctx函数中，带有pub的let/func语句会向ctx中携带的export 集合插入相应的变量名
    let exports = RefCell::new(HashSet::new());
    let ctx = EvalContext::for_module(&exports, current_module_dir, module_loader);

    for stmt in stmts {
        match eval_stmt_with_ctx(stmt, &env, ctx)? {
            ExecFlow::Normal => {}
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
            ExecFlow::Return { span, .. } => {
                return Err(RuntimeError::new(
                    span,
                    RuntimeErrorKind::ReturnOutsideFunction,
                    "return outside function",
                ));
            }
        }
    }

    let mut object = HashMap::new();
    for name in exports.into_inner() {
        // 从eval之后的env中获取这个name最后对应的值，如果是函数，已经捕获了提升了的自由变量
        if let Ok(value) = env.get(&name, 0) {
            object.insert(name, value);
        }
    }
    Ok(Value::Object(Rc::new(RefCell::new(object))))
}

fn eval_stmt_with_ctx(
    stmt: &crate::ecscript::ast::Stmt,
    env: &Environment<'_>,
    ctx: EvalContext<'_>,
) -> Result<ExecFlow, RuntimeError> {
    let span = stmt.span;
    match &stmt.kind {
        StmtKind::Let { name, expr, public } => {
            let value = eval_expr_with_ctx(expr, env, ctx)?;
            env.insert(name.clone(), Binding::Direct(value), span)?;
            if *public {
                record_module_export(ctx, name);
            }
        }
        StmtKind::Assign { target, expr } => {
            let value = eval_expr_with_ctx(expr, env, ctx)?;
            assign_target(target, value, env, span, ctx)?
        }
        StmtKind::CompoundAssign { target, op, expr } => {
            let target = resolve_assign_target(target, env, span, ctx)?;
            let left = target.load(span)?;
            let right = eval_expr_with_ctx(expr, env, ctx)?;
            let value = eval_compound_assign(*op, left, right, span)?;
            target.store(value, span)?;
        }
        StmtKind::ExprStmt { expr } => {
            eval_expr_with_ctx(expr, env, ctx)?;
        }
        StmtKind::Block { stmts } => return eval_block_with_ctx(stmts, env, ctx),
        StmtKind::If {
            cond,
            then_body,
            else_body,
        } => {
            let cond_var = expect_bool(eval_expr_with_ctx(cond, env, ctx)?, span, "if condition")?;
            if cond_var {
                return eval_block_with_ctx(then_body, env, ctx);
            } else {
                return eval_block_with_ctx(else_body, env, ctx);
            }
        }
        StmtKind::While { cond, body } => loop {
            let cond_var =
                expect_bool(eval_expr_with_ctx(cond, env, ctx)?, span, "while condition")?;
            if !cond_var {
                break;
            }
            match eval_block_with_ctx(body, env, ctx)? {
                ExecFlow::Break(_) => break,
                ExecFlow::Continue(_) => continue,
                ExecFlow::Normal => {}
                whole @ ExecFlow::Return { .. } => return Ok(whole),
            }
        },
        StmtKind::ForIn {
            var,
            iterable,
            body,
        } => {
            let coll = eval_expr_with_ctx(iterable, env, ctx)?;
            match coll {
                Value::Array(arr) => {
                    let items: Vec<Value> = arr.borrow().clone();
                    for value in items {
                        let new_env = Environment::new_child(env);
                        new_env.insert(var.clone(), Binding::Direct(value), span)?;
                        match eval_block_with_ctx(body, &new_env, ctx)? {
                            ExecFlow::Break(_) => break,
                            ExecFlow::Continue(_) => continue,
                            ExecFlow::Normal => {}
                            whole @ ExecFlow::Return { .. } => return Ok(whole),
                        }
                    }
                }
                Value::Object(obj) => {
                    let mut keys: Vec<String> = obj.borrow().keys().cloned().collect();
                    keys.sort();
                    for key in keys {
                        let new_env = Environment::new_child(env);
                        new_env.insert(var.clone(), Binding::Direct(Value::String(key)), span)?;
                        match eval_block_with_ctx(body, &new_env, ctx)? {
                            ExecFlow::Break(_) => break,
                            ExecFlow::Continue(_) => continue,
                            ExecFlow::Normal => {}
                            whole @ ExecFlow::Return { .. } => return Ok(whole),
                        }
                    }
                }
                other => {
                    return Err(RuntimeError::new(
                        span,
                        RuntimeErrorKind::TypeMismatch,
                        format!(
                            "for-in iterable must be Array or Object, got {}",
                            other.type_name()
                        ),
                    ));
                }
            }
        }
        StmtKind::ForRange { var, range, body } => {
            let RangeExpr {
                start,
                end,
                inclusive,
            } = range;
            let start = expect_int(
                eval_expr_with_ctx(start, env, ctx)?,
                span,
                "for range start",
            )?;
            let end = expect_int(eval_expr_with_ctx(end, env, ctx)?, span, "for range end")?;
            let iterator: Box<dyn Iterator<Item = i64>> = if *inclusive {
                Box::new(start..=end)
            } else {
                Box::new(start..end)
            };
            for i in iterator {
                let new_env = Environment::new_child(env);
                new_env.insert(var.clone(), Binding::Direct(Value::Int(i)), span)?;
                match eval_block_with_ctx(body, &new_env, ctx)? {
                    ExecFlow::Break(_) => break,
                    ExecFlow::Continue(_) => continue,
                    ExecFlow::Normal => {}
                    whole @ ExecFlow::Return { .. } => return Ok(whole),
                }
            }
        }
        StmtKind::FuncDeclare {
            name,
            params,
            body,
            public,
        } => {
            let mut captures = HashMap::new();

            // 解析ast收集自由变量
            let free_set = free_vars(Some(name), params, body)?;

            // 提升自由变量
            for name in free_set {
                if let Some(slot) = env.capture_upvalue(&name, span) {
                    captures.insert(name, slot);
                }
            }

            let func = Function {
                name: Some(name.clone()),
                params: params.clone(),
                stmts: body.clone(),
                captures,
            };

            let func_val = Value::Function(Rc::new(func));

            env.insert(name.clone(), Binding::Direct(func_val), span)?;
            if *public {
                record_module_export(ctx, name);
            }
        }
        StmtKind::Use { path, alias } => {
            let module = load_module(path, span, ctx)?;
            env.insert(alias.clone(), Binding::Direct(module), span)?;
        }

        StmtKind::Break => {
            return Ok(ExecFlow::Break(span));
        }
        StmtKind::Continue => {
            return Ok(ExecFlow::Continue(span));
        }
        StmtKind::Return { value } => {
            if let Some(return_expr) = value {
                let return_value = eval_expr_with_ctx(return_expr, env, ctx)?;
                return Ok(ExecFlow::Return {
                    value: Some(return_value),
                    span,
                });
            } else {
                return Ok(ExecFlow::Return { value: None, span });
            }
        }
    }
    Ok(ExecFlow::Normal)
}

fn record_module_export(ctx: EvalContext<'_>, name: &str) {
    if let Some(exports) = ctx.module_exports {
        exports.borrow_mut().insert(name.to_string());
    }
}

fn eval_block_with_ctx(
    stmts: &[crate::ecscript::ast::Stmt],
    env: &Environment<'_>,
    ctx: EvalContext<'_>,
) -> Result<ExecFlow, RuntimeError> {
    let new_env = Environment::new_child(env);
    for stmt in stmts {
        match eval_stmt_with_ctx(stmt, &new_env, ctx)? {
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

enum ResolvedAssignTarget<'a> {
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
    fn load(&self, span: usize) -> EvalResult<Value> {
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

    fn store(&self, value: Value, span: usize) -> EvalResult<()> {
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

fn resolve_assign_target<'a>(
    target: &AssignTarget,
    env: &'a Environment<'a>,
    span: usize,
    ctx: EvalContext<'_>,
) -> EvalResult<ResolvedAssignTarget<'a>> {
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
fn assign_target(
    target: &AssignTarget,
    value: Value,
    env: &Environment<'_>,
    span: usize,
    ctx: EvalContext<'_>,
) -> EvalResult<()> {
    let target = resolve_assign_target(target, env, span, ctx)?;
    target.store(value, span)
}

fn eval_compound_assign(
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

pub fn eval_expr(expr: &Expr, env: &Environment<'_>) -> EvalResult<Value> {
    eval_expr_with_ctx(expr, env, EvalContext::plain(None, None, None, None))
}

fn eval_expr_with_ctx(
    expr: &Expr,
    env: &Environment<'_>,
    ctx: EvalContext<'_>,
) -> EvalResult<Value> {
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
            let arr_val = Value::Array(Rc::new(RefCell::new(values)));
            Ok(arr_val)
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
                        return Ok(value);
                    } else {
                        return Ok(Value::Nil);
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

            // 解析ast收集自由变量
            let free_set = free_vars(None, params, body)?;

            // 提升自由变量
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
///
/// # 用法
///
/// ```ignore
/// // 生成 fn eval_sub(left: Value, right: Value, span: usize) -> EvalResult<Value>
/// impl_arith!(eval_sub, -, "subtract");
///
/// // 生成 fn eval_mul(left: Value, right: Value, span: usize) -> EvalResult<Value>
/// impl_arith!(eval_mul, *, "multiply");
/// ```
///
/// # 展开结果
///
/// 以 `impl_arith!(eval_sub, -, "subtract")` 为例，展开后等价于：
///
/// ```ignore
/// fn eval_sub(left: Value, right: Value, span: usize) -> EvalResult<Value> {
///     match (&left, &right) {
///         (Value::Int(a), Value::Int(b))   => Ok(Value::Int(a - b)),
///         (Value::Int(a), Value::Float(b))  => Ok(Value::Float(*a as f64 - b)),
///         (Value::Float(a), Value::Int(b))  => Ok(Value::Float(a - *b as f64)),
///         (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a - b)),
///         _ => Err(RuntimeError::new(span, RuntimeErrorKind::TypeMismatch,
///             format!("cannot subtract {} and {}", left.type_name(), right.type_name()))),
///     }
/// }
/// ```
///
/// 自动处理 Int×Int → Int 以及 Int/Float 混合 → Float 的类型提升。
macro_rules! impl_arith {
    ($name:ident, $op:tt, $desc:literal) => {
        fn $name(left: Value, right: Value, span: usize) -> EvalResult<Value> {
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

// eval_sub 和 eval_mul 没有特殊逻辑，直接由宏生成
impl_arith!(eval_sub, -, "subtract");
impl_arith!(eval_mul, *, "multiply");

/// eval_add 有字符串拼接的额外语义，不能直接用 impl_arith! 生成。
/// 先尝试数值运算，如果类型不匹配再检查 String 拼接。
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

/// eval_div 需要除零检查，不能直接用 impl_arith! 生成。
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

/// eval_mod 只接受 Int×Int，且需要除零检查。
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

// ── 比较运算 ──────────────────────────────────────────────────────────

/// 为有序比较运算符生成求值函数。
///
/// 适用于 `<` `>` `<=` `>=`，它们只接受数值类型（Int/Float），返回 Bool。
/// 四个函数的结构完全相同，仅运算符不同。
///
/// 不适用于：
///   - `==` / `!=`（还接受 Nil/Bool/String 的比较，需手写）
///
/// # 用法
///
/// ```ignore
/// // 生成 fn eval_lt(left: Value, right: Value, span: usize) -> EvalResult<Value>
/// impl_ord_cmp!(eval_lt, <, "compare");
///
/// // 生成 fn eval_ge(left: Value, right: Value, span: usize) -> EvalResult<Value>
/// impl_ord_cmp!(eval_ge, >=, "compare");
/// ```
///
/// # 展开结果
///
/// 以 `impl_ord_cmp!(eval_lt, <, "compare")` 为例，展开后等价于：
///
/// ```ignore
/// fn eval_lt(left: Value, right: Value, span: usize) -> EvalResult<Value> {
///     match (&left, &right) {
///         (Value::Int(a), Value::Int(b))    => Ok(Value::Bool(a < b)),
///         (Value::Int(a), Value::Float(b))  => Ok(Value::Bool((*a as f64) < *b)),
///         (Value::Float(a), Value::Int(b))  => Ok(Value::Bool(*a < (*b as f64))),
///         (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a < b)),
///         _ => Err(RuntimeError::new(span, RuntimeErrorKind::TypeMismatch,
///             format!("cannot compare {} and {}", left.type_name(), right.type_name()))),
///     }
/// }
/// ```
macro_rules! impl_ord_cmp {
    ($name:ident, $op:tt, $desc:literal) => {
        fn $name(left: Value, right: Value, span: usize) -> EvalResult<Value> {
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

/// eval_eq 和 eval_ne 接受 Nil/Bool/String/数值 的比较，不能直接用 impl_ord_cmp!。
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

// ── 逻辑运算 ──────────────────────────────────────────────────────────

fn eval_and_short_circuit(
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

fn eval_or_short_circuit(
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

// ── 测试 ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::eval_expr;
    use crate::ecscript::{
        env::Environment,
        error::{RuntimeError, RuntimeErrorKind},
        lexer::tokenize,
        pratt::parse_expr,
        value::{Binding, Value},
    };

    /// tokenize → parse → eval 一步到位
    fn eval_src(src: &str, env: &Environment) -> Result<Value, RuntimeError> {
        let tokens = tokenize(src).unwrap();
        let expr = parse_expr(&tokens).unwrap();
        eval_expr(&expr, env)
    }

    fn env_with(name: &str, val: Value) -> Environment<'_> {
        let env = Environment::new();
        env.insert(name.to_string(), Binding::Direct(val), 0)
            .unwrap();
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
        assert_eq!(eval_src("1 + 2 * 3 - 4 / 2", &env), Ok(Value::Int(5)));
    }

    #[test]
    fn eval_with_variables() {
        let env = env_with("a", Value::Int(3));
        env.insert("b".to_string(), Binding::Direct(Value::Int(4)), 0)
            .unwrap();
        assert_eq!(eval_src("a + b", &env), Ok(Value::Int(7)));
        assert_eq!(eval_src("a * b", &env), Ok(Value::Int(12)));
    }

    #[test]
    fn eval_nested_logical() {
        let env = Environment::new();
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
    use super::{
        EvalContext, ExecFlow, ModuleLoader, eval_expr, eval_module, eval_script,
        eval_script_with_io_ctx,
    };
    use crate::ecscript::{
        ast::{AssignTarget, Expr, ExprKind, InfixOper, Literal, Stmt, StmtKind},
        env::Environment,
        error::{RuntimeError, RuntimeErrorKind},
        lexer::tokenize,
        parser::parse_script,
        pratt::parse_expr,
        value::Value,
    };
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
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

    fn temp_script_dir(name: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("ecsh-{name}-{nanos}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    // ── let 语句 ──────────────────────────────────────────

    #[test]
    fn eval_let_inserts_variable() {
        let env = Environment::new();
        eval_script_src("let x = 42;", &env).unwrap();
        assert_eq!(env.get("x", 0), Ok(Value::Int(42)));
    }

    #[test]
    fn eval_module_collects_pub_bindings_into_object() {
        let stmts = vec![
            Stmt {
                kind: StmtKind::Let {
                    name: "name".into(),
                    expr: Expr {
                        kind: ExprKind::Literal(Literal::String("ecs".into())),
                        span: 0,
                    },
                    public: true,
                },
                span: 0,
            },
            Stmt {
                kind: StmtKind::FuncDeclare {
                    name: "add".into(),
                    params: vec!["a".into(), "b".into()],
                    body: vec![Stmt {
                        kind: StmtKind::Return {
                            value: Some(Expr {
                                kind: ExprKind::Infix(
                                    Box::new(Expr {
                                        kind: ExprKind::Variable("a".into()),
                                        span: 0,
                                    }),
                                    InfixOper::Add,
                                    Box::new(Expr {
                                        kind: ExprKind::Variable("b".into()),
                                        span: 0,
                                    }),
                                ),
                                span: 0,
                            }),
                        },
                        span: 0,
                    }],
                    public: true,
                },
                span: 0,
            },
        ];
        let module = eval_module(&stmts).unwrap();
        let Value::Object(module) = module else {
            panic!("expected object");
        };
        let module = module.borrow();
        assert_eq!(module.get("name"), Some(&Value::String("ecs".into())));
        assert!(matches!(module.get("add"), Some(Value::Function(_))));
    }

    #[test]
    fn eval_module_keeps_private_bindings_internal() {
        let stmts = vec![
            Stmt {
                kind: StmtKind::Let {
                    name: "hidden".into(),
                    expr: lit_int(1),
                    public: false,
                },
                span: 0,
            },
            Stmt {
                kind: StmtKind::Let {
                    name: "visible".into(),
                    expr: Expr {
                        kind: ExprKind::Infix(
                            Box::new(Expr {
                                kind: ExprKind::Variable("hidden".into()),
                                span: 0,
                            }),
                            InfixOper::Add,
                            Box::new(lit_int(1)),
                        ),
                        span: 0,
                    },
                    public: true,
                },
                span: 0,
            },
        ];
        let module = eval_module(&stmts).unwrap();
        let Value::Object(module) = module else {
            panic!("expected object");
        };
        let module = module.borrow();
        assert_eq!(module.get("visible"), Some(&Value::Int(2)));
        assert!(!module.contains_key("hidden"));
    }

    #[test]
    fn eval_script_use_imports_module_object_from_relative_path() {
        let dir = temp_script_dir("module-import");
        let module_path = dir.join("foo.ecs");
        fs::write(
            &module_path,
            "let hidden = 1\npub let visible = hidden + 1\n",
        )
        .unwrap();

        let env = Environment::new();
        let tokens = tokenize("use ./foo.ecs as foo\nlet value = foo.visible\n").unwrap();
        let stmts = parse_script(&tokens).unwrap();
        let loader = ModuleLoader::new();
        let ctx = EvalContext::plain(None, None, Some(&dir), Some(&loader));
        eval_script_with_io_ctx(&stmts, &env, ctx).unwrap();

        assert_eq!(env.get("value", 0), Ok(Value::Int(2)));
        let Value::Object(foo) = env.get("foo", 0).unwrap() else {
            panic!("expected imported module object");
        };
        assert_eq!(foo.borrow().get("visible"), Some(&Value::Int(2)));
        assert!(!foo.borrow().contains_key("hidden"));
    }

    #[test]
    fn eval_script_use_reuses_cached_module_object() {
        let dir = temp_script_dir("module-cache");
        let module_path = dir.join("foo.ecs");
        fs::write(&module_path, "pub let xs = []\n").unwrap();

        let env = Environment::new();
        let tokens = tokenize(
            "use ./foo.ecs as a\nuse ./foo.ecs as b\npush(a.xs, 1)\nlet size = len(b.xs)\n",
        )
        .unwrap();
        let stmts = parse_script(&tokens).unwrap();
        let loader = ModuleLoader::new();
        let ctx = EvalContext::plain(None, None, Some(&dir), Some(&loader));
        eval_script_with_io_ctx(&stmts, &env, ctx).unwrap();

        assert_eq!(env.get("size", 0), Ok(Value::Int(1)));
    }

    #[test]
    fn eval_script_use_reports_circular_import() {
        let dir = temp_script_dir("module-cycle");
        fs::write(dir.join("a.ecs"), "use ./b.ecs as b\npub let a = 1\n").unwrap();
        fs::write(dir.join("b.ecs"), "use ./a.ecs as a\npub let b = 1\n").unwrap();

        let env = Environment::new();
        let tokens = tokenize("use ./a.ecs as a\n").unwrap();
        let stmts = parse_script(&tokens).unwrap();
        let loader = ModuleLoader::new();
        let ctx = EvalContext::plain(None, None, Some(&dir), Some(&loader));
        let err = eval_script_with_io_ctx(&stmts, &env, ctx).unwrap_err();

        assert_eq!(err.kind, RuntimeErrorKind::CircularReference);
        assert!(err.message.contains("circular module import detected"));
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
        let stmts = vec![Stmt {
            kind: StmtKind::Assign {
                target: AssignTarget::Name("x".into()),
                expr: lit_int(5),
            },
            span: 0,
        }];
        let err = eval_script(&stmts, &env).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::UndefinedVariable);
    }

    #[test]
    fn eval_compound_assign_updates_variable() {
        let env = Environment::new();
        eval_script_src("let x = 10; x += 20;", &env).unwrap();
        assert_eq!(env.get("x", 0), Ok(Value::Int(30)));
    }

    #[test]
    fn eval_compound_assign_uses_string_concatenation_for_plus_eq() {
        let env = Environment::new();
        eval_script_src("let s = \"ec\"; s += \"script\";", &env).unwrap();
        assert_eq!(env.get("s", 0), Ok(Value::String("ecscript".into())));
    }

    #[test]
    fn eval_compound_assign_requires_existing_variable() {
        let env = Environment::new();
        let err = eval_script_src("x += 1;", &env).unwrap_err();
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

    #[test]
    fn eval_field_compound_assign_reads_then_writes_object_field() {
        let env = Environment::new();
        eval_script_src("let o = {count: 1}; o.count += 2;", &env).unwrap();
        let Value::Object(obj) = env.get("o", 0).unwrap() else {
            panic!("expected object");
        };
        assert_eq!(obj.borrow().get("count").cloned(), Some(Value::Int(3)));
    }

    #[test]
    fn eval_index_compound_assign_reads_then_writes_array_element() {
        let env = Environment::new();
        eval_script_src("let a = [1, 2, 3]; a[1] *= 5;", &env).unwrap();
        let Value::Array(arr) = env.get("a", 0).unwrap() else {
            panic!("expected array");
        };
        assert_eq!(
            *arr.borrow(),
            vec![Value::Int(1), Value::Int(10), Value::Int(3)]
        );
    }

    #[test]
    fn eval_compound_assign_resolves_target_only_once() {
        let env = Environment::new();
        eval_script_src(
            "let calls = 0; func next_idx() { calls += 1; return 0; } let a = [1]; a[next_idx()] += 2;",
            &env,
        )
        .unwrap();
        assert_eq!(env.get("calls", 0), Ok(Value::Int(1)));
        let Value::Array(arr) = env.get("a", 0).unwrap() else {
            panic!("expected array");
        };
        assert_eq!(*arr.borrow(), vec![Value::Int(3)]);
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
        assert_eq!(env.get("s", 0), Ok(Value::Int(3)));
    }

    #[test]
    fn eval_for_range_inclusive() {
        let env = Environment::new();
        eval_script_src("let s = 0; for i in 0..=3 { s = s + i; }", &env).unwrap();
        assert_eq!(env.get("s", 0), Ok(Value::Int(6)));
    }

    #[test]
    fn eval_range_expression_reports_use_builtin() {
        let env = Environment::new();
        let err = eval_script_src("let xs = 1..3", &env).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::TypeMismatch);
        assert_eq!(
            err.message,
            "range expressions are only valid in for loops; use range(start, end)"
        );
    }

    #[test]
    fn eval_builtin_range_returns_closed_interval() {
        let env = Environment::new();
        eval_script_src("let xs = range(1, 3)", &env).unwrap();
        let Value::Array(values) = env.get("xs", 0).unwrap() else {
            panic!("expected array");
        };
        assert_eq!(
            *values.borrow(),
            vec![Value::Int(1), Value::Int(2), Value::Int(3)]
        );
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
        assert_eq!(env.get("s", 0), Ok(Value::Int(3)));
    }

    #[test]
    fn eval_for_continue_skips_iteration() {
        let env = Environment::new();
        eval_script_src(
            "let s = 0; for i in 0..5 { if i == 2 { continue; } s = s + i; }",
            &env,
        )
        .unwrap();
        assert_eq!(env.get("s", 0), Ok(Value::Int(8)));
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

    #[test]
    fn eval_func_call_returns_value() {
        let env = Environment::new();
        eval_script_src("func add(a, b) { return a + b; } let x = add(1, 2);", &env).unwrap();
        assert_eq!(env.get("x", 0), Ok(Value::Int(3)));
    }

    #[test]
    fn eval_func_return_without_value_becomes_nil() {
        let env = Environment::new();
        eval_script_src("func noop() { return; } let x = noop();", &env).unwrap();
        assert_eq!(env.get("x", 0), Ok(Value::Nil));
    }

    #[test]
    fn eval_func_return_inside_loop_exits_function() {
        let env = Environment::new();
        eval_script_src(
            "func first() { while true { return 7; } } let x = first();",
            &env,
        )
        .unwrap();
        assert_eq!(env.get("x", 0), Ok(Value::Int(7)));
    }

    #[test]
    fn eval_func_uses_global_not_caller_local_scope() {
        let env = Environment::new();
        eval_script_src(
            "let x = 1; let y = 0; func read_x() { return x; } { let x = 2; y = read_x(); }",
            &env,
        )
        .unwrap();
        assert_eq!(env.get("y", 0), Ok(Value::Int(1)));
    }

    #[test]
    fn eval_func_arity_mismatch_reports_error() {
        let env = Environment::new();
        let err = eval_script_src("func add(a, b) { return a + b; } add(1);", &env).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::ArityMismatch);
    }

    #[test]
    fn eval_return_outside_function_reports_error() {
        let env = Environment::new();
        let err = eval_script_src("return 1;", &env).unwrap_err();
        assert_eq!(err.kind, RuntimeErrorKind::ReturnOutsideFunction);
        assert_eq!(err.message, "return outside function");
    }

    #[test]
    fn eval_lambda_expression_body_returns_value() {
        let env = Environment::new();
        eval_script_src("let f = (x) => x + 1; let y = f(2);", &env).unwrap();
        assert_eq!(env.get("y", 0), Ok(Value::Int(3)));
    }

    #[test]
    fn eval_lambda_block_body_returns_value() {
        let env = Environment::new();
        eval_script_src(
            "let f = (a, b) => { return a + b; }; let y = f(1, 2);",
            &env,
        )
        .unwrap();
        assert_eq!(env.get("y", 0), Ok(Value::Int(3)));
    }

    #[test]
    fn eval_lambda_without_return_becomes_nil() {
        let env = Environment::new();
        eval_script_src("let f = () => {}; let y = f();", &env).unwrap();
        assert_eq!(env.get("y", 0), Ok(Value::Nil));
    }

    #[test]
    fn eval_lambda_uses_global_not_caller_local_scope() {
        let env = Environment::new();
        eval_script_src(
            "let x = 1; let y = 0; let f = () => x; { let x = 2; y = f(); }",
            &env,
        )
        .unwrap();
        assert_eq!(env.get("y", 0), Ok(Value::Int(1)));
    }

    #[test]
    fn smoke_lambda_can_be_called_immediately() {
        let env = Environment::new();
        eval_script_src("let y = ((x) => x + 1)(2);", &env).unwrap();
        assert_eq!(env.get("y", 0), Ok(Value::Int(3)));
    }

    #[test]
    fn smoke_lambda_can_live_in_object_field_and_be_called() {
        let env = Environment::new();
        eval_script_src("let ops = {inc: (x) => x + 1}; let y = ops.inc(4);", &env).unwrap();
        assert_eq!(env.get("y", 0), Ok(Value::Int(5)));
    }

    #[test]
    fn smoke_named_function_can_return_lambda_value() {
        let env = Environment::new();
        eval_script_src(
            "func make_inc() { return (x) => x + 1; } let inc = make_inc(); let y = inc(5);",
            &env,
        )
        .unwrap();
        assert_eq!(env.get("y", 0), Ok(Value::Int(6)));
    }

    #[test]
    fn smoke_lambda_supports_higher_order_calls() {
        let env = Environment::new();
        eval_script_src(
            "let twice = (f, x) => f(f(x)); let inc = (x) => x + 1; let y = twice(inc, 3);",
            &env,
        )
        .unwrap();
        assert_eq!(env.get("y", 0), Ok(Value::Int(5)));
    }

    #[test]
    fn smoke_lambda_block_body_can_run_control_flow() {
        let env = Environment::new();
        eval_script_src(
            "let sum_to = (n) => { let s = 0; for i in 0..=n { s = s + i; } return s; }; let y = sum_to(3);",
            &env,
        )
        .unwrap();
        assert_eq!(env.get("y", 0), Ok(Value::Int(6)));
    }

    #[test]
    fn eval_lambda_closure_prefers_captured_local_over_global() {
        let env = Environment::new();
        eval_script_src(
            "let x = 100; let f = 0; { let x = 1; f = () => x; } let y = f();",
            &env,
        )
        .unwrap();
        assert_eq!(env.get("y", 0), Ok(Value::Int(1)));
    }

    #[test]
    fn eval_lambda_closure_writes_back_captured_local_across_calls() {
        let env = Environment::new();
        eval_script_src(
            "func make_counter() { let x = 0; return () => { x = x + 1; return x; }; } let counter = make_counter(); let a = counter(); let b = counter();",
            &env,
        )
        .unwrap();
        assert_eq!(env.get("a", 0), Ok(Value::Int(1)));
        assert_eq!(env.get("b", 0), Ok(Value::Int(2)));
    }

    #[test]
    fn eval_sibling_closures_share_same_captured_slot() {
        let env = Environment::new();
        eval_script_src(
            "func make_pair() { let x = 0; let inc = () => { x = x + 1; return x; }; let get = () => x; return {inc: inc, get: get}; } let pair = make_pair(); let a = pair.inc(); let b = pair.inc(); let c = pair.get();",
            &env,
        )
        .unwrap();
        assert_eq!(env.get("a", 0), Ok(Value::Int(1)));
        assert_eq!(env.get("b", 0), Ok(Value::Int(2)));
        assert_eq!(env.get("c", 0), Ok(Value::Int(2)));
    }

    #[test]
    fn eval_inner_named_function_can_escape_with_capture() {
        let env = Environment::new();
        eval_script_src(
            "func outer() { let x = 7; func inner() { return x; } return inner; } let f = outer(); let y = f();",
            &env,
        )
        .unwrap();
        assert_eq!(env.get("y", 0), Ok(Value::Int(7)));
    }

    #[test]
    fn eval_lambda_reads_global_late_from_root() {
        let env = Environment::new();
        eval_script_src("let x = 1; let f = () => x; x = 2; let y = f();", &env).unwrap();
        assert_eq!(env.get("y", 0), Ok(Value::Int(2)));
    }

    #[test]
    fn eval_recursive_factorial() {
        let env = Environment::new();
        eval_script_src(
            "func fact(n) { if n <= 1 { return 1; } return n * fact(n - 1); } let r = fact(5);",
            &env,
        )
        .unwrap();
        assert_eq!(env.get("r", 0), Ok(Value::Int(120)));
    }

    #[test]
    fn eval_make_counter_closure() {
        let env = Environment::new();
        eval_script_src(
            "func make_counter() { let x = 0; return () => { x = x + 1; return x; }; } let c1 = make_counter(); let c2 = make_counter(); let a = c1(); let b = c1(); let d = c2();",
            &env,
        )
        .unwrap();
        assert_eq!(env.get("a", 0), Ok(Value::Int(1)));
        assert_eq!(env.get("b", 0), Ok(Value::Int(2)));
        assert_eq!(env.get("d", 0), Ok(Value::Int(1)));
    }

    // ── 闭包边界 / 刁钻场景 ─────────────────────────────────

    #[test]
    fn eval_closure_captures_mutable_container() {
        let env = Environment::new();
        eval_script_src(
            "let arr = [1, 2]; let add_to_arr = (x) => { push(arr, x); }; add_to_arr(3); add_to_arr(4);",
            &env,
        )
        .unwrap();
        let Value::Array(a) = env.get("arr", 0).unwrap() else {
            panic!("expected array");
        };
        assert_eq!(
            *a.borrow(),
            vec![Value::Int(1), Value::Int(2), Value::Int(3), Value::Int(4)]
        );
    }

    #[test]
    fn eval_closure_inside_loop_all_closures_share_slot() {
        let env = Environment::new();
        eval_script_src(
            "let a = []; let x = 0; while x < 3 { x = x + 1; let f = (n) => x + n; push(a, f(0)); }",
            &env,
        )
        .unwrap();
        let Value::Array(arr) = env.get("a", 0).unwrap() else {
            panic!("expected array");
        };
        assert_eq!(
            *arr.borrow(),
            vec![Value::Int(1), Value::Int(2), Value::Int(3)]
        );
    }

    #[test]
    fn eval_nested_closure_three_levels() {
        let env = Environment::new();
        eval_script_src(
            "let x = 1; func outer() { let y = 2; return () => x + y; } let f = outer(); let r = f();",
            &env,
        )
        .unwrap();
        assert_eq!(env.get("r", 0), Ok(Value::Int(3)));
    }

    #[test]
    fn eval_closure_returns_closure_chain() {
        let env = Environment::new();
        eval_script_src(
            "func a() { let x = 1; return () => x + 1; } let b = a(); let r = b();",
            &env,
        )
        .unwrap();
        assert_eq!(env.get("r", 0), Ok(Value::Int(2)));
    }

    #[test]
    fn eval_closure_shadowing_inner_let_does_not_capture() {
        let env = Environment::new();
        eval_script_src(
            "let x = 10; let f = () => { let x = 99; return x; }; let r = f();",
            &env,
        )
        .unwrap();
        assert_eq!(env.get("r", 0), Ok(Value::Int(99)));
        assert_eq!(env.get("x", 0), Ok(Value::Int(10)));
    }

    #[test]
    fn eval_closure_writeback_through_multiple_closures() {
        let env = Environment::new();
        eval_script_src(
            "let x = 0; let inc = () => { x = x + 1; }; let add2 = (n) => { x = x + n; }; inc(); inc(); add2(5);",
            &env,
        )
        .unwrap();
        assert_eq!(env.get("x", 0), Ok(Value::Int(7)));
    }

    #[test]
    fn eval_closure_captures_for_loop_variable_correctly() {
        let env = Environment::new();
        eval_script_src(
            "let fns = []; for i in 0..3 { let f = () => i; push(fns, f); } let r0 = fns[0](); let r1 = fns[1](); let r2 = fns[2]();",
            &env,
        )
        .unwrap();
        // Each closure captures the per-iteration i (because `let i` is in the for body scope)
        assert_eq!(env.get("r0", 0), Ok(Value::Int(0)));
        assert_eq!(env.get("r1", 0), Ok(Value::Int(1)));
        assert_eq!(env.get("r2", 0), Ok(Value::Int(2)));
    }

    #[test]
    fn eval_closure_captured_from_outer_named_function() {
        let env = Environment::new();
        eval_script_src(
            "func outer(n) { return () => n + 1; } let f = outer(5); let r = f();",
            &env,
        )
        .unwrap();
        assert_eq!(env.get("r", 0), Ok(Value::Int(6)));
    }

    #[test]
    fn eval_closure_captures_from_while_body() {
        let env = Environment::new();
        eval_script_src(
            "let x = 1; let f = () => 0; while x < 3 { f = () => x; x = x + 1; } let r = f();",
            &env,
        )
        .unwrap();
        assert_eq!(env.get("r", 0), Ok(Value::Int(3)));
    }

    #[test]
    fn eval_closure_assigned_to_object_field_and_called() {
        let env = Environment::new();
        eval_script_src(
            "let obj = {x: 0}; obj.inc = () => { obj.x = obj.x + 1; }; obj.inc(); obj.inc();",
            &env,
        )
        .unwrap();
        let Value::Object(o) = env.get("obj", 0).unwrap() else {
            panic!("expected object");
        };
        assert_eq!(o.borrow().get("x").cloned(), Some(Value::Int(2)));
    }

    #[test]
    fn eval_closure_captures_only_needed_variables_not_all() {
        let env = Environment::new();
        eval_script_src(
            "let a = 1; let b = 2; let c = 3; let f = () => a + c; let r = f();",
            &env,
        )
        .unwrap();
        assert_eq!(env.get("r", 0), Ok(Value::Int(4)));
        // b is not captured — make sure changes to b still visible as global
        eval_script_src("b = 99;", &env).unwrap();
        assert_eq!(env.get("b", 0), Ok(Value::Int(99)));
    }

    #[test]
    fn eval_anonymous_recursive_lambda_via_object() {
        let env = Environment::new();
        eval_script_src(
            "let fact_obj = {}; fact_obj.fact = (n) => { if n <= 1 { return 1; } return n * fact_obj.fact(n - 1); }; let r = fact_obj.fact(5);",
            &env,
        )
        .unwrap();
        assert_eq!(env.get("r", 0), Ok(Value::Int(120)));
    }
}
