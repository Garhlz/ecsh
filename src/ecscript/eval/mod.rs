use std::{cell::RefCell, collections::HashSet, path::Path};

use crate::ecscript::{module::ModuleLoader, value::Value};
use crate::types::ShellState;

mod assign;
mod expr;
mod stmt;

use assign::{assign_target, eval_compound_assign, expect_bool, expect_int, resolve_assign_target};
pub use expr::eval_expr;
use expr::{eval_add, eval_div, eval_expr_with_ctx, eval_mod, eval_mul, eval_sub};
pub use stmt::{
    eval_module, eval_script, eval_script_with_ctx, eval_stmt, eval_top_level_script,
    eval_top_level_script_with_ctx,
};
pub(crate) use stmt::{eval_module_in_dir, eval_script_with_io_ctx};
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

// ── 测试 ────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests;
