use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::Path,
    rc::Rc,
};

use crate::ecscript::{
    ast::{Stmt, StmtKind},
    env::Environment,
    error::{EvalResult, RuntimeError, RuntimeErrorKind},
    func::free_vars,
    module::{ModuleLoader, load_module},
    value::{Binding, Function, Value},
};

use super::{
    EvalContext, ExecFlow, assign_target, eval_compound_assign, eval_expr_with_ctx, expect_bool,
    expect_int, resolve_assign_target,
};

/// 按顺序执行脚本语句，并返回最终执行流。
///
/// 这层用于普通脚本求值；顶层若出现未被循环或函数消费的
/// `break` / `continue` / `return`，会在更外层被转换成运行时错误。
pub fn eval_script(stmts: &[Stmt], env: &Environment<'_>) -> EvalResult<ExecFlow> {
    eval_script_with_ctx(stmts, env, None)
}

pub fn eval_script_with_ctx(
    stmts: &[Stmt],
    env: &Environment<'_>,
    shell_state: Option<&crate::types::ShellState>,
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
    shell_state: Option<&crate::types::ShellState>,
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
/// 大多数语句执行完成后返回 `ExecFlow::Normal`。
/// `break` / `continue` / `return` 不在这里直接报错，而是先编码成
/// `ExecFlow` 向上传递，再由循环、函数或顶层入口决定是否消费。
pub fn eval_stmt(stmt: &Stmt, env: &Environment<'_>) -> Result<ExecFlow, RuntimeError> {
    eval_stmt_with_ctx(stmt, env, EvalContext::plain(None, None, None, None))
}

/// 在独立模块环境中执行脚本，并把 `pub` 绑定收集成模块对象返回。
pub fn eval_module(stmts: &[Stmt]) -> EvalResult<Value> {
    eval_module_in_dir(stmts, None, None)
}

pub(crate) fn eval_module_in_dir(
    stmts: &[Stmt],
    current_module_dir: Option<&Path>,
    module_loader: Option<&ModuleLoader>,
) -> EvalResult<Value> {
    let env = Environment::new();
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
        if let Ok(value) = env.get(&name, 0) {
            object.insert(name, value);
        }
    }
    Ok(Value::Object(Rc::new(RefCell::new(object))))
}

pub(crate) fn eval_stmt_with_ctx(
    stmt: &Stmt,
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
            let start = expect_int(
                eval_expr_with_ctx(&range.start, env, ctx)?,
                span,
                "for range start",
            )?;
            let end = expect_int(
                eval_expr_with_ctx(&range.end, env, ctx)?,
                span,
                "for range end",
            )?;
            let iterator: Box<dyn Iterator<Item = i64>> = if range.inclusive {
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
            let free_set = free_vars(Some(name), params, body)?;

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
        StmtKind::Break => return Ok(ExecFlow::Break(span)),
        StmtKind::Continue => return Ok(ExecFlow::Continue(span)),
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

pub(super) fn record_module_export(ctx: EvalContext<'_>, name: &str) {
    if let Some(exports) = ctx.module_exports {
        exports.borrow_mut().insert(name.to_string());
    }
}

pub(super) fn eval_block_with_ctx(
    stmts: &[Stmt],
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
