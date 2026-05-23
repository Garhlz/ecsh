use crate::ecscript::{
    ast::{AssignTarget, Expr, ExprKind, RangeExpr, Stmt, StmtKind},
    env::Environment,
    error::{RuntimeError, RuntimeErrorKind},
    eval::{ExecFlow, eval_stmt},
    value::{Binding, Function, Value},
};
use std::collections::HashSet;
use std::rc::Rc;
pub fn call_function(
    func: Rc<Function>,
    params_value: &Vec<Value>,
    env: &Environment<'_>,
    span: usize,
) -> Result<Option<Value>, RuntimeError> {
    // 加了一层capture env，全都是slot
    let capture_env = Environment::new_child(env.find_root());

    for (name, slot) in &func.captures {
        capture_env.insert(name.clone(), Binding::Shared(slot.clone()), span)?;
    }

    // local env的父环境改为capture env
    let local_env = Environment::new_child(&capture_env);
    if func.params.len() != params_value.len() {
        return Err(RuntimeError::new(
            span,
            RuntimeErrorKind::ArityMismatch,
            "param number error",
        ));
    }

    // 如果是具名函数，才把函数变量绑定到当前环境中，支持递归
    if let Some(name) = (*func).name.clone() {
        local_env.insert(name, Binding::Direct(Value::Function(func.clone())), span)?;
    }

    for (param_name, param_value) in func.params.iter().zip(params_value.iter()) {
        local_env.insert(
            param_name.clone(),
            Binding::Direct(param_value.clone()),
            span,
        )?;
    }

    for stmt in &(*func).stmts {
        match eval_stmt(stmt, &local_env)? {
            ExecFlow::Return { value, .. } => {
                return Ok(value);
            }
            _ => continue,
        };
    }
    return Ok(None);
}

pub fn free_vars(
    name: Option<&str>,
    params: &Vec<String>,
    body: &Vec<Stmt>,
) -> Result<HashSet<String>, RuntimeError> {
    let mut free_set: HashSet<String> = HashSet::new();
    let mut scope_stack: Vec<HashSet<String>> = Vec::new();

    let mut cur_stack = HashSet::new();
    if let Some(name) = name {
        cur_stack.insert(name.to_string());
    }
    for param in params {
        cur_stack.insert(param.clone());
    }

    scope_stack.push(cur_stack); // 所有权转移进去了

    for stmt in body {
        visit_stmt(stmt, &mut scope_stack, &mut free_set);
    }

    Ok(free_set)
}

fn visit_stmt(stmt: &Stmt, stack: &mut Vec<HashSet<String>>, free_set: &mut HashSet<String>) {
    match &stmt.kind {
        StmtKind::Let { name, expr } => {
            visit_expr(expr, stack, free_set);
            declare(name, stack);
        }
        StmtKind::Assign { target, expr } => {
            match target {
                AssignTarget::Name(name) => {
                    upvalue(name, stack, free_set);
                }
                AssignTarget::Index { object, index } => {
                    visit_expr(object, stack, free_set);
                    visit_expr(index, stack, free_set);
                }
                AssignTarget::Field { object, .. } => {
                    visit_expr(object, stack, free_set);
                }
            }
            visit_expr(expr, stack, free_set);
        }
        StmtKind::ExprStmt { expr } => {
            visit_expr(expr, stack, free_set);
        }
        StmtKind::Block { stmts } => visit_block(stmts, stack, free_set),
        StmtKind::If {
            cond,
            then_body,
            else_body,
        } => {
            visit_expr(cond, stack, free_set);
            visit_block(then_body, stack, free_set);
            visit_block(else_body, stack, free_set);
        }
        StmtKind::While { cond, body } => {
            visit_expr(cond, stack, free_set);
            visit_block(body, stack, free_set);
        }
        StmtKind::ForIn {
            var,
            iterable,
            body,
        } => {
            visit_expr(iterable, stack, free_set);
            stack.push(HashSet::new());
            declare(var, stack);
            for stmt in body {
                visit_stmt(stmt, stack, free_set);
            }
            stack.pop();
        }
        StmtKind::ForRange { var, range, body } => {
            let RangeExpr { start, end, .. } = range;
            visit_expr(start, stack, free_set);
            visit_expr(end, stack, free_set);
            stack.push(HashSet::new());
            declare(var, stack);
            for stmt in body {
                visit_stmt(stmt, stack, free_set);
            }
            stack.pop();
        }
        StmtKind::Return { value } => {
            if let Some(expr) = value {
                visit_expr(expr, stack, free_set);
            }
        }
        StmtKind::FuncDeclare { name, .. } => {
            // 内层函数声明，实际上和let的意义一样
            declare(name, stack);
        }
        StmtKind::Break => {
            return;
        }
        StmtKind::Continue => {
            return;
        }
    }
}

fn visit_block(
    stmts: &Vec<Stmt>,
    // 这个类型不知道对不对
    stack: &mut Vec<HashSet<String>>,
    free_set: &mut HashSet<String>,
) {
    stack.push(HashSet::new());

    for stmt in stmts {
        visit_stmt(stmt, stack, free_set);
    }
    stack.pop();
}

fn visit_expr(expr: &Expr, stack: &Vec<HashSet<String>>, free_set: &mut HashSet<String>) {
    match &expr.kind {
        ExprKind::Variable(name) => {
            upvalue(name, stack, free_set);
        }
        ExprKind::Prefix(_, expr) => {
            visit_expr(expr, stack, free_set);
        }
        ExprKind::Infix(lexpr, _, rexpr) => {
            visit_expr(lexpr, stack, free_set);
            visit_expr(rexpr, stack, free_set);
        }
        ExprKind::Array(arr) => {
            for expr in arr {
                visit_expr(expr, stack, free_set);
            }
        }
        ExprKind::Object(obj) => {
            for (_, expr) in obj {
                visit_expr(expr, stack, free_set);
            }
        }
        ExprKind::Index(base, index) => {
            visit_expr(base, stack, free_set);
            visit_expr(index, stack, free_set);
        }
        ExprKind::Field(obj, _) => {
            visit_expr(obj, stack, free_set);
        }
        ExprKind::Call(name, params) => {
            visit_expr(name, stack, free_set);
            for param in params {
                visit_expr(param, stack, free_set);
            }
        }
        ExprKind::Range(RangeExpr { start, end, .. }) => {
            visit_expr(start, stack, free_set);
            visit_expr(end, stack, free_set);
        }
        ExprKind::FuncLiteral { .. } => {
            // 正在收集“外层函数”的自由变量
            // 内层 lambda 里的 x 不属于外层函数的自由变量，而属于内层 lambda 的自由变量
            return;
        }
        ExprKind::Literal(_) => {
            // 字面量当然无需理会
            return;
        }
    }
}

fn is_local(name: &str, stack: &Vec<HashSet<String>>) -> bool {
    for scope in stack.iter().rev() {
        if scope.contains(name) {
            return true;
        }
    }
    false
}

fn upvalue(name: &str, stack: &Vec<HashSet<String>>, free_set: &mut HashSet<String>) {
    if !is_local(name, stack) {
        free_set.insert(name.to_string());
    }
}

fn declare(name: &str, stack: &mut Vec<HashSet<String>>) {
    stack.last_mut().unwrap().insert(name.to_string());
}
