use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use crate::ecscript::{
    error::{EvalResult, RuntimeError, RuntimeErrorKind},
    eval::{EvalContext, eval_module_in_dir},
    value::Value,
};

/// 文件级模块加载器。
///
/// 负责：
/// - 解析 `use` 的文件路径
/// - 维护已初始化模块缓存
/// - 维护“正在加载”集合，检测循环导入
/// - 把模块文件交给 `eval_module_in_dir` 变成导出对象
///
/// 不负责：
/// - 解析 `use ... as ...` 语法
/// - 管理词法环境
/// - 决定 hook / completion / prompt 等扩展点
pub struct ModuleLoader {
    cache: RefCell<HashMap<PathBuf, Value>>,
    loading: RefCell<HashSet<PathBuf>>,
}

impl ModuleLoader {
    pub fn new() -> Self {
        Self {
            cache: RefCell::new(HashMap::new()),
            loading: RefCell::new(HashSet::new()),
        }
    }

    /// 加载一个模块并返回导出对象。
    ///
    /// 顺序固定为：
    /// 1. 把导入路径解析为规范化绝对路径
    /// 2. 命中缓存则直接复用
    /// 3. 如果正在加载，则报循环导入错误
    /// 4. 否则读取文件、parse 并执行模块
    /// 5. 把结果写入缓存
    pub(crate) fn load(
        &self,
        path: &str,
        current_module_dir: Option<&Path>,
        span: usize,
    ) -> EvalResult<Value> {
        let resolved = resolve_module_path(path, current_module_dir, span)?;

        if let Some(module) = self.cache.borrow().get(&resolved) {
            return Ok(module.clone());
        }

        if self.loading.borrow().contains(&resolved) {
            return Err(RuntimeError::new(
                span,
                RuntimeErrorKind::CircularReference,
                format!("circular module import detected: {}", resolved.display()),
            ));
        }

        self.loading.borrow_mut().insert(resolved.clone());
        let result = self.load_uncached(&resolved, span);
        self.loading.borrow_mut().remove(&resolved);

        let module = result?;
        self.cache
            .borrow_mut()
            .insert(resolved.clone(), module.clone());
        Ok(module)
    }

    /// 不经过缓存，直接把模块文件求值成导出对象。
    ///
    /// 模块内部如果继续 `use` 相对路径模块，仍然会复用当前 loader，
    /// 所以缓存和循环导入检测在整条导入链上都保持一致。
    fn load_uncached(&self, resolved: &Path, span: usize) -> EvalResult<Value> {
        let source = fs::read_to_string(resolved).map_err(|err| {
            RuntimeError::new(
                span,
                RuntimeErrorKind::IoError,
                format!("failed to read module '{}': {}", resolved.display(), err),
            )
        })?;
        let tokens = crate::ecscript::lexer::tokenize(&source).map_err(|err| {
            RuntimeError::new(
                span,
                RuntimeErrorKind::IoError,
                format!(
                    "failed to lex module '{}': {}",
                    resolved.display(),
                    err.message
                ),
            )
        })?;
        let stmts = crate::ecscript::parser::parse_script(&tokens).map_err(|err| {
            RuntimeError::new(
                span,
                RuntimeErrorKind::IoError,
                format!(
                    "failed to parse module '{}': {}",
                    resolved.display(),
                    err.message
                ),
            )
        })?;

        eval_module_in_dir(&stmts, resolved.parent(), Some(self))
    }
}

/// 从 evaluator 入口调用模块加载。
///
/// 这层只是把 `EvalContext` 中的 loader 和当前目录拿出来，
/// 不负责缓存策略本身。
pub(crate) fn load_module(path: &str, span: usize, ctx: EvalContext<'_>) -> EvalResult<Value> {
    let Some(loader) = ctx.module_loader else {
        return Err(RuntimeError::new(
            span,
            RuntimeErrorKind::IoError,
            "module imports require file-backed execution context",
        ));
    };
    loader.load(path, ctx.current_module_dir, span)
}

/// 解析 `use` 的模块路径并生成缓存 key。
///
/// 当前规则很克制：
/// - 绝对路径：直接规范化
/// - 相对路径：相对当前脚本文件目录解析
/// - 没有文件上下文：直接报错
fn resolve_module_path(
    path: &str,
    current_module_dir: Option<&Path>,
    span: usize,
) -> EvalResult<PathBuf> {
    let module_path = Path::new(path);
    let candidate = if module_path.is_absolute() {
        module_path.to_path_buf()
    } else {
        let Some(base_dir) = current_module_dir else {
            return Err(RuntimeError::new(
                span,
                RuntimeErrorKind::IoError,
                "module imports require file-backed execution context",
            ));
        };
        base_dir.join(module_path)
    };

    candidate.canonicalize().map_err(|err| {
        RuntimeError::new(
            span,
            RuntimeErrorKind::IoError,
            format!(
                "failed to resolve module '{}': {}",
                candidate.display(),
                err
            ),
        )
    })
}
