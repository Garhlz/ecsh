use crate::ecscript::{
    ast::{AssignTarget, Expr, ExprKind, RangeExpr, Stmt, StmtKind},
    env::Environment,
    error::{RuntimeError, RuntimeErrorKind},
    eval::{EvalContext, ExecFlow, eval_stmt_with_ctx},
    value::{Binding, Function, Value},
};
use std::collections::HashSet;
use std::rc::Rc;

/// 调用函数／闭包。
///
/// 构造的 environment 链为：
/// 根环境 → capture_env（被捕获的外部变量）→ local_env（参数和函数体局部变量）。
///
/// `capture_env` 挂到 `find_root()` 而非调用点当前环境，因为闭包的捕获关系在定义时就已经固定。
#[cfg(test)]
pub fn call_function(
    func: Rc<Function>,
    params_value: &Vec<Value>,
    env: &Environment<'_>,
    span: usize,
) -> Result<Option<Value>, RuntimeError> {
    call_function_with_eval_ctx(
        func,
        params_value.clone(),
        env,
        EvalContext::plain(None, None, None, None),
        "function",
        span,
    )
}

pub fn call_function_with_ctx(
    func: Rc<Function>,
    params_value: Vec<Value>,
    env: &Environment<'_>,
    shell_state: Option<&crate::types::ShellState>,
    stdin_text: Option<&str>,
    label: &str,
    span: usize,
) -> Result<Option<Value>, RuntimeError> {
    let cwd = shell_state.and_then(|_| std::env::current_dir().ok());
    let loader = shell_state.and_then(|state| state.module_loader.as_deref());
    call_function_with_eval_ctx(
        func,
        params_value,
        env,
        EvalContext::plain(shell_state, stdin_text, cwd.as_deref(), loader),
        label,
        span,
    )
}

pub(crate) fn call_function_with_eval_ctx(
    func: Rc<Function>,
    params_value: Vec<Value>,
    env: &Environment<'_>,
    ctx: EvalContext<'_>,
    label: &str,
    span: usize,
) -> Result<Option<Value>, RuntimeError> {
    let capture_env = Environment::new_child(env.find_root());

    for (name, slot) in &func.captures {
        capture_env.insert(name.clone(), Binding::Shared(slot.clone()), span)?;
    }

    let local_env = Environment::new_child(&capture_env);

    if func.params.len() != params_value.len() {
        return Err(RuntimeError::new(
            span,
            RuntimeErrorKind::ArityMismatch,
            format!(
                "{} expects {} arguments, got {}",
                label,
                func.params.len(),
                params_value.len()
            ),
        ));
    }

    // 具名函数将自身绑定到 local_env，支持递归调用
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
        match eval_stmt_with_ctx(stmt, &local_env, ctx)? {
            ExecFlow::Return { value, .. } => {
                return Ok(value);
            }
            _ => continue,
        };
    }

    Ok(None)
}

/// 收集函数体的自由变量（free variables）。
///
/// 用作用域栈 (`Vec<HashSet<String>>`) 追踪变量声明：进入块时 push，声明时插入栈顶，
/// 引用变量时从栈顶向栈底查找，找不到则加入 `free_set`。函数名和形参作为最外层作用域。
pub fn free_vars(
    name: Option<&str>,
    params: &Vec<String>,
    body: &Vec<Stmt>,
) -> Result<HashSet<String>, RuntimeError> {
    Ok(collect_free_vars(name, params, body))
}

fn collect_free_vars(name: Option<&str>, params: &[String], body: &[Stmt]) -> HashSet<String> {
    let mut free_set: HashSet<String> = HashSet::new();
    let mut scope_stack: Vec<HashSet<String>> = Vec::new();

    let mut cur_stack = HashSet::new();
    if let Some(name) = name {
        cur_stack.insert(name.to_string());
    }
    for param in params {
        cur_stack.insert(param.clone());
    }

    scope_stack.push(cur_stack);

    for stmt in body {
        visit_stmt(stmt, &mut scope_stack, &mut free_set);
    }

    free_set
}

fn visit_stmt(stmt: &Stmt, stack: &mut Vec<HashSet<String>>, free_set: &mut HashSet<String>) {
    match &stmt.kind {
        // let 声明：先访问右值表达式，再将变量名插入当前作用域
        StmtKind::Let { name, expr, .. } => {
            visit_expr(expr, stack, free_set);
            declare(name, stack);
        }

        // 赋值语句：左侧是引用（检查 upvalue），右侧是表达式
        StmtKind::Assign { target, expr } => {
            visit_assign_target(target, stack, free_set);
            visit_expr(expr, stack, free_set);
        }

        StmtKind::CompoundAssign { target, expr, .. } => {
            visit_assign_target(target, stack, free_set);
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

        // for-in：迭代对象在外部作用域，循环变量属于内部新作用域
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

        // 内层函数声明：函数名对外层来说是局部声明，不会成为自由变量。
        // 内层函数自己的自由变量在其编译时单独收集。
        StmtKind::FuncDeclare { name, .. } => {
            declare(name, stack);
        }

        StmtKind::Use { .. } => return,
        StmtKind::Break => return,
        StmtKind::Continue => return,
    }
}

/// 访问赋值语句左侧目标。
///
/// Name 是引用（检查 upvalue），Index/Field 则递归访问其子表达式。
fn visit_assign_target(
    target: &AssignTarget,
    stack: &[HashSet<String>],
    free_set: &mut HashSet<String>,
) {
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
}

/// 进入一个块作用域：push 新层，遍历语句，pop。
fn visit_block(stmts: &[Stmt], stack: &mut Vec<HashSet<String>>, free_set: &mut HashSet<String>) {
    stack.push(HashSet::new());

    for stmt in stmts {
        visit_stmt(stmt, stack, free_set);
    }

    stack.pop();
}

fn visit_expr(expr: &Expr, stack: &[HashSet<String>], free_set: &mut HashSet<String>) {
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

        // 内层 lambda 会在外层函数执行时创建，因此外层函数必须先保留
        // lambda 将来需要的非局部绑定。属于外层局部作用域的名字会被
        // upvalue 过滤，仍由 lambda 在创建时直接捕获。
        ExprKind::FuncLiteral { params, body } => {
            for name in collect_free_vars(None, params, body) {
                upvalue(&name, stack, free_set);
            }
        }

        // 命令字面量：不含变量引用
        ExprKind::CommandLiteral(_) => {
            return;
        }

        // 字面量：不含变量引用
        ExprKind::Literal(_) => {
            return;
        }
    }
}

/// 从栈顶向栈底逐层检查 name 是否已声明（支持变量遮蔽）。
fn is_local(name: &str, stack: &[HashSet<String>]) -> bool {
    for scope in stack.iter().rev() {
        if scope.contains(name) {
            return true;
        }
    }
    false
}

/// 若 name 不在作用域栈中，说明它来自外层作用域，记为自由变量（upvalue）。
fn upvalue(name: &str, stack: &[HashSet<String>], free_set: &mut HashSet<String>) {
    if !is_local(name, stack) {
        free_set.insert(name.to_string());
    }
}

/// 将变量名插入当前最内层作用域。
fn declare(name: &str, stack: &mut Vec<HashSet<String>>) {
    stack.last_mut().unwrap().insert(name.to_string());
}
