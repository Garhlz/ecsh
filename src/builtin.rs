//! 内置命令定义与实现。
//!
//! builtin 命令在 shell 自身进程执行（不 fork），因此能修改 shell 的工作目录、环境变量等状态。
//! 注意：jobs / fg / bg 虽然在命令表中注册，但实际执行由 executor 层负责，
//! run_builtin 对它们返回 None，交由特殊路径处理。

use crate::diagnostics::print_error;
use crate::ecscript::{
    Environment, ModuleLoader, repl_output_needs_newline, reset_repl_output_state,
    run_script_file_with_ctx,
};
use crate::extensions::{HookName, after_cd_context, new_extensions, run_hooks};
use crate::types::Command;
use crate::types::{CommandFlow, CommandStatus, ShellState};
use nix::unistd::isatty;
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

const ANSI_RESET: &str = "\x1b[0m";
const ANSI_HOT_PINK: &str = "\x1b[1;38;5;201m";
const ANSI_NEON_GREEN: &str = "\x1b[1;38;5;120m";
const ANSI_SUN_YELLOW: &str = "\x1b[1;38;5;226m";
const ANSI_ELECTRIC_CYAN: &str = "\x1b[1;38;5;51m";
const ANSI_WARM_ORANGE: &str = "\x1b[1;38;5;214m";

/// 内置命令的种类。每种命令对应一段独立的执行逻辑。
#[derive(Clone, Copy)]
pub enum BuiltinKind {
    Help,
    Exit,
    Cd,
    Pwd,
    Env,
    Export,
    Unset,
    Clear,
    Status,
    Jobs,
    Fg,
    Bg,
    Alias,
    Unalias,
    Trap,
    Type,
    Which,
    History,
    Source,
    ReloadRc,
}

/// 将命令名映射到 BuiltinKind。
///
/// 所有内置命令名称只在这里维护一份，避免分散在各个 match 语句中。
pub fn builtin_kind(command: &Command) -> Option<BuiltinKind> {
    match command.program.as_lit_str().unwrap_or("") {
        "help" => Some(BuiltinKind::Help),
        "exit" => Some(BuiltinKind::Exit),
        "cd" => Some(BuiltinKind::Cd),
        "pwd" => Some(BuiltinKind::Pwd),
        "env" => Some(BuiltinKind::Env),
        "export" => Some(BuiltinKind::Export),
        "unset" => Some(BuiltinKind::Unset),
        "clear" => Some(BuiltinKind::Clear),
        "status" => Some(BuiltinKind::Status),
        "jobs" => Some(BuiltinKind::Jobs),
        "fg" => Some(BuiltinKind::Fg),
        "bg" => Some(BuiltinKind::Bg),
        "alias" => Some(BuiltinKind::Alias),
        "unalias" => Some(BuiltinKind::Unalias),
        "trap" => Some(BuiltinKind::Trap),
        "type" => Some(BuiltinKind::Type),
        "which" => Some(BuiltinKind::Which),
        "history" => Some(BuiltinKind::History),
        "source" => Some(BuiltinKind::Source),
        "." => Some(BuiltinKind::Source),
        "reload_rc" => Some(BuiltinKind::ReloadRc),
        _ => None,
    }
}

pub const BUILTIN_NAMES: &[&str] = &[
    "help",
    "exit",
    "cd",
    "pwd",
    "env",
    "export",
    "unset",
    "clear",
    "status",
    "jobs",
    "fg",
    "bg",
    "alias",
    "unalias",
    "trap",
    "type",
    "which",
    "history",
    "source",
    ".",
    "reload_rc",
];

/// 判断某个内置命令是否可以出现在管道中。
///
/// 只允许纯输出型（只读、不修改 shell 状态）的内置命令进入管道。
/// cd / export / unset 这类会修改 shell 状态，必须由 shell 自身执行，不能放在子进程里。
pub fn is_builtin_allowed_in_pipeline(kind: BuiltinKind) -> bool {
    matches!(
        kind,
        BuiltinKind::Help | BuiltinKind::Pwd | BuiltinKind::Env | BuiltinKind::Status
    )
}

/// 执行内置命令（不包括 jobs/fg/bg）。
///
/// 返回 `Option<CommandFlow>`：
///   - Some(flow) → 已执行，返回控制流和状态码
///   - None       → 是 jobs/fg/bg，需要 executor 层特殊处理
pub fn run_builtin(command: &Command, state: &mut ShellState) -> Option<CommandFlow> {
    let kind = builtin_kind(command)?;

    match kind {
        BuiltinKind::Help => {
            print_help();
            Some(CommandFlow::Continue(CommandStatus::success()))
        }
        BuiltinKind::Exit => Some(CommandFlow::Exit(CommandStatus::success())),
        BuiltinKind::Cd => Some(CommandFlow::Continue(run_cd(command, state))),
        BuiltinKind::Pwd => Some(CommandFlow::Continue(run_pwd())),
        BuiltinKind::Env => Some(CommandFlow::Continue(run_env())),
        BuiltinKind::Export => Some(CommandFlow::Continue(run_export(command))),
        BuiltinKind::Unset => Some(CommandFlow::Continue(run_unset(command))),
        BuiltinKind::Clear => Some(CommandFlow::Continue(run_clear())),
        BuiltinKind::Status => Some(CommandFlow::Continue(run_status(state))),
        BuiltinKind::Alias => Some(CommandFlow::Continue(run_alias(command, state))),
        BuiltinKind::Unalias => Some(CommandFlow::Continue(run_unalias(command, state))),
        BuiltinKind::Trap => Some(CommandFlow::Continue(run_trap(command, state))),
        BuiltinKind::Type => Some(CommandFlow::Continue(run_type(command, state))),
        BuiltinKind::Which => Some(CommandFlow::Continue(run_which(command, state))),
        BuiltinKind::History => Some(CommandFlow::Continue(run_history(command, state))),
        BuiltinKind::Source => Some(CommandFlow::Continue(run_source(command, state))),
        BuiltinKind::ReloadRc => Some(CommandFlow::Continue(run_reload_rc(command, state))),
        // jobs / fg / bg 需要访问作业表和前台等待逻辑，
        // 由 executor 层统一处理，避免 builtin 模块反向依赖 executor。
        BuiltinKind::Jobs | BuiltinKind::Fg | BuiltinKind::Bg => None,
    }
}

// ── 基础 builtin ───────────────────────────────────────────────────

/// `help` 命令：打印所有内置命令的简单说明。
pub fn print_help() {
    print_help_title();
    println!("ecsh builtins:");
    println!("  help - show this help message");
    println!("  cd - change current working directory");
    println!("  pwd - print working directory");
    println!("  exit - exit the shell");
    println!("  env - print environment variables");
    println!("  export KEY=value - set environment variable");
    println!("  unset KEY - remove environment variable");
    println!("  clear - clear the terminal screen");
    println!("  status - print last command status");
    println!("  jobs - list background and stopped jobs");
    println!("  fg %N - move job N to the foreground");
    println!("  bg %N - resume job N in the background");
    println!("  alias [NAME='VALUE'] - define or show aliases");
    println!("  unalias NAME ... - remove aliases");
    println!("  trap [CMD SIGNAL] - register EXIT/INT trap");
    println!("  type NAME ... - describe how each command name resolves");
    println!("  which NAME ... - print resolved command path or shell resolution");
    println!("  history - show command history");
    println!("  source FILE / . FILE - run an .ecs file in the current shell script environment");
    println!("  reload_rc - reload ~/.ecshrc with a fresh ecscript module/runtime registry");
}

/// 打印 help 标题行。每个词用不同颜色，重定向到文件时自动去掉颜色。
fn print_help_title() {
    let use_color = isatty(io::stdout()).unwrap_or(false);

    println!(
        "{}ecsh{} - {}Elaine{} {}&{} {}Cornelia's{} {}shell{}",
        color_prefix(use_color, ANSI_HOT_PINK),
        color_prefix(use_color, ANSI_RESET),
        color_prefix(use_color, ANSI_NEON_GREEN),
        color_prefix(use_color, ANSI_RESET),
        color_prefix(use_color, ANSI_SUN_YELLOW),
        color_prefix(use_color, ANSI_RESET),
        color_prefix(use_color, ANSI_ELECTRIC_CYAN),
        color_prefix(use_color, ANSI_RESET),
        color_prefix(use_color, ANSI_WARM_ORANGE),
        color_prefix(use_color, ANSI_RESET),
    );
}

/// 根据 use_color 开关决定输出颜色代码还是空字符串。
fn color_prefix<'a>(use_color: bool, color: &'a str) -> &'a str {
    if use_color { color } else { "" }
}

/// `cd [dir]` 命令：更改当前工作目录。
///
/// 不传参数时默认切换到 HOME 环境变量指向的目录。
/// 使用 `std::env::set_current_dir` 操作系统调用更改进程的工作目录。
fn run_cd(command: &Command, state: &ShellState) -> CommandStatus {
    if command.args.len() > 1 {
        print_error("cd: too many arguments");
        return CommandStatus::failure();
    }

    let dir: String = if command.args.is_empty() {
        match std::env::var("HOME") {
            Ok(home) => home,
            Err(_) => {
                print_error("cd: HOME not set");
                return CommandStatus::failure();
            }
        }
    } else {
        command.args[0].as_lit_str().unwrap_or("").to_string()
    };

    if let Err(err) = set_current_dir_with_hooks(&dir, state) {
        print_error(format!("cd: {}", err));
        return CommandStatus::failure();
    }

    CommandStatus::success()
}

pub(crate) fn set_current_dir_with_hooks(dir: &str, state: &ShellState) -> Result<(), String> {
    let old_cwd = std::env::current_dir()
        .map(|cwd| cwd.to_string_lossy().into_owned())
        .unwrap_or_else(|_| ".".to_string());
    std::env::set_current_dir(dir).map_err(|err| format!("{}: {}", dir, err))?;
    let new_cwd = std::env::current_dir()
        .map(|cwd| cwd.to_string_lossy().into_owned())
        .unwrap_or_else(|_| ".".to_string());
    sync_pwd_env(&old_cwd, &new_cwd);

    let already_running = {
        let mut ext = state.extensions.borrow_mut();
        if ext.after_cd_reentry {
            true
        } else {
            ext.after_cd_reentry = true;
            false
        }
    };

    if !already_running {
        run_hooks(HookName::AfterCd, after_cd_context(old_cwd, new_cwd), state);
        state.extensions.borrow_mut().after_cd_reentry = false;
    }

    Ok(())
}

fn sync_pwd_env(old_cwd: &str, new_cwd: &str) {
    unsafe {
        std::env::set_var("OLDPWD", old_cwd);
        std::env::set_var("PWD", new_cwd);
    }
}

/// `pwd` 命令：打印当前工作目录。
///
/// 使用 `std::env::current_dir()` 获取当前进程的工作目录。
fn run_pwd() -> CommandStatus {
    match std::env::current_dir() {
        Ok(path) => {
            println!("{}", path.display());
            CommandStatus::success()
        }
        Err(err) => {
            print_error(format!("pwd: {}", err));
            CommandStatus::failure()
        }
    }
}

/// `env` 命令：打印所有环境变量（KEY=value 格式，每行一个）。
fn run_env() -> CommandStatus {
    for (key, value) in std::env::vars() {
        println!("{}={}", key, value);
    }

    CommandStatus::success()
}

/// `export KEY=value` 命令：设置环境变量。
///
/// 当前只支持 `export KEY=value` 格式（没有单独的 `export NAME` 列出模式）。
/// `std::env::set_var` 是 unsafe 的：Rust 标准库标记为 unsafe 因为多线程下可能产生数据竞争。
/// ecsh 是单线程程序，所以这里实际是安全的。
fn run_export(command: &Command) -> CommandStatus {
    if command.args.len() != 1 {
        print_error("export: usage: export KEY=value");
        return CommandStatus::failure();
    }

    let arg = command.args[0].as_lit_str().unwrap_or("");
    let Some((key, value)) = arg.split_once('=') else {
        print_error("export: usage: export KEY=value");
        return CommandStatus::failure();
    };

    if key.is_empty() {
        print_error("export: usage: export KEY=value");
        return CommandStatus::failure();
    }

    if !is_valid_env_key(key) {
        print_error(format!("export: invalid variable name: {}", key));
        return CommandStatus::failure();
    }

    unsafe { std::env::set_var(key, value) };
    CommandStatus::success()
}

/// 校验环境变量名是否合法：首字符为字母或下划线，后续为字母/数字/下划线。
pub(crate) fn is_valid_env_key(key: &str) -> bool {
    let mut chars = key.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphabetic() && first != '_' {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

/// `unset KEY` 命令：删除环境变量。
///
/// 先校验变量名合法性再调用 `std::env::remove_var`，避免非法变量名导致 panic。
fn run_unset(command: &Command) -> CommandStatus {
    if command.args.len() != 1 {
        print_usage("unset", "unset KEY");
        return CommandStatus::failure();
    }

    let key = command.args[0].as_lit_str().unwrap_or("");
    if !is_valid_env_key(key) {
        print_error(format!("unset: invalid variable name: {}", key));
        return CommandStatus::failure();
    }

    unsafe { std::env::remove_var(key) };
    CommandStatus::success()
}

// ── shell 交互 builtin ─────────────────────────────────────────────

/// `alias` 命令：定义、查看或列出别名。
///
/// 支持三种模式：
/// - `alias`
/// - `alias name=value`
/// - `alias name`
fn run_alias(command: &Command, state: &mut ShellState) -> CommandStatus {
    if command.args.is_empty() {
        let mut entries: Vec<_> = state.aliases.iter().collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        for (name, value) in entries {
            println!("alias {}='{}'", name, value);
        }
        return CommandStatus::success();
    }

    for arg in &command.args {
        let arg = arg.as_lit_str().unwrap_or("");
        if let Some((name, value)) = arg.split_once('=') {
            // 赋值形式直接写入 alias 表。
            if name.is_empty() {
                print_error("alias: empty alias name");
                return CommandStatus::failure();
            }
            state.aliases.insert(name.to_string(), value.to_string());
        } else if let Some(value) = state.aliases.get(arg) {
            // 非赋值形式表示查询已有 alias。
            println!("alias {}='{}'", arg, value);
        } else {
            print_error(format!("alias: no such alias: {}", arg));
            return CommandStatus::failure();
        }
    }

    CommandStatus::success()
}

/// `unalias` 命令：删除一个或多个别名。
fn run_unalias(command: &Command, state: &mut ShellState) -> CommandStatus {
    if command.args.is_empty() {
        print_usage("unalias", "unalias NAME ...");
        return CommandStatus::failure();
    }

    for arg in &command.args {
        let name = arg.as_lit_str().unwrap_or("");
        if state.aliases.remove(name).is_none() {
            print_error(format!("unalias: no such alias: {}", name));
            return CommandStatus::failure();
        }
    }

    CommandStatus::success()
}

/// `trap` 命令：查看、注册或删除 trap。
///
/// 当前只支持 `EXIT` 和 `INT`。
fn run_trap(command: &Command, state: &mut ShellState) -> CommandStatus {
    if command.args.is_empty() {
        let mut entries: Vec<_> = state.traps.iter().collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        for (signal, handler) in entries {
            println!("trap '{}' {}", handler, signal);
        }
        return CommandStatus::success();
    }

    // `trap - SIGNAL` 表示删除已有 trap。
    if command.args.len() == 2 && command.args[0].as_lit_str() == Some("-") {
        let Some(signal) = normalize_trap_name(command.args[1].as_lit_str().unwrap_or("")) else {
            print_error("trap: only EXIT and INT are supported");
            return CommandStatus::failure();
        };
        state.traps.remove(signal);
        return CommandStatus::success();
    }

    if command.args.len() != 2 {
        print_usage("trap", "trap 'command' EXIT|INT");
        return CommandStatus::failure();
    }

    // 普通注册形式：`trap 'command' SIGNAL`。
    let handler = command.args[0].as_lit_str().unwrap_or("");
    let Some(signal) = normalize_trap_name(command.args[1].as_lit_str().unwrap_or("")) else {
        print_error("trap: only EXIT and INT are supported");
        return CommandStatus::failure();
    };
    state.traps.insert(signal.to_string(), handler.to_string());
    CommandStatus::success()
}

/// 归一化 trap 名称，把外部输入折叠到内部键名。
fn normalize_trap_name(name: &str) -> Option<&'static str> {
    match name {
        "EXIT" => Some("EXIT"),
        "INT" | "SIGINT" => Some("INT"),
        _ => None,
    }
}

/// `type` 命令：解释一个名字在 shell 中会解析成什么。
fn run_type(command: &Command, state: &mut ShellState) -> CommandStatus {
    if command.args.is_empty() {
        print_usage("type", "type NAME ...");
        return CommandStatus::failure();
    }

    let mut ok = true;
    for arg in &command.args {
        let name = arg.as_lit_str().unwrap_or("");
        match describe_command(name, state) {
            Some(CommandDescription::Alias(value)) => {
                println!("{} is aliased to `{}`", name, value)
            }
            Some(CommandDescription::Builtin) => println!("{} is a shell builtin", name),
            Some(CommandDescription::ScriptCommand) => {
                println!("{} is an ecscript shell command", name)
            }
            Some(CommandDescription::External(path)) => println!("{} is {}", name, path.display()),
            None => {
                print_error(format!("type: not found: {}", name));
                ok = false;
            }
        }
    }

    if ok {
        CommandStatus::success()
    } else {
        CommandStatus::failure()
    }
}

/// `which` 命令：输出命令名最终解析到的路径或 shell 类型。
fn run_which(command: &Command, state: &mut ShellState) -> CommandStatus {
    if command.args.is_empty() {
        print_usage("which", "which NAME ...");
        return CommandStatus::failure();
    }

    let mut ok = true;
    for arg in &command.args {
        let name = arg.as_lit_str().unwrap_or("");
        match describe_command(name, state) {
            Some(CommandDescription::Alias(value)) => println!("alias {}='{}'", name, value),
            Some(CommandDescription::Builtin) => println!("{}: shell builtin", name),
            Some(CommandDescription::ScriptCommand) => {
                println!("{}: ecscript shell command", name)
            }
            Some(CommandDescription::External(path)) => println!("{}", path.display()),
            None => {
                print_error(format!("which: not found: {}", name));
                ok = false;
            }
        }
    }

    if ok {
        CommandStatus::success()
    } else {
        CommandStatus::failure()
    }
}

/// `history` 命令：打印当前 shell 会话的命令历史。
fn run_history(command: &Command, state: &mut ShellState) -> CommandStatus {
    if !command.args.is_empty() {
        print_usage("history", "history");
        return CommandStatus::failure();
    }

    for (idx, entry) in state.command_history.iter().enumerate() {
        println!("{:>5}  {}", idx + 1, entry);
    }
    CommandStatus::success()
}

/// `source FILE` / `. FILE`：在当前 shell 的 script_env 里执行一份 ecscript 文件。
fn run_source(command: &Command, state: &mut ShellState) -> CommandStatus {
    if command.args.len() != 1 {
        print_usage("source", "source FILE");
        return CommandStatus::failure();
    }

    let path = command.args[0].as_lit_str().unwrap_or("");
    if path.is_empty() {
        print_usage("source", "source FILE");
        return CommandStatus::failure();
    }

    reset_repl_output_state();
    match run_script_file_with_ctx(path, &state.script_env, state, None) {
        Ok(()) => {
            if repl_output_needs_newline() {
                println!();
            }
            CommandStatus::success()
        }
        Err(err) => {
            if repl_output_needs_newline() {
                println!();
            }
            print_error(err.format_for_user());
            CommandStatus::failure()
        }
    }
}

fn run_reload_rc(command: &Command, state: &mut ShellState) -> CommandStatus {
    if !command.args.is_empty() {
        print_usage("reload_rc", "reload_rc");
        return CommandStatus::failure();
    }

    reload_startup_rc(state)
}

pub fn load_startup_rc(state: &mut ShellState) {
    let Some(path) = startup_rc_path() else {
        return;
    };
    let status = run_rc_file(&path, state, false);
    if status.code != 0 {
        state.last_status = status;
    }
}

pub fn reload_startup_rc(state: &mut ShellState) -> CommandStatus {
    let Some(path) = startup_rc_path() else {
        print_error("reload_rc: ~/.ecshrc not found");
        return CommandStatus::failure();
    };
    run_rc_file(&path, state, true)
}

fn startup_rc_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let path = Path::new(&home).join(".ecshrc");
    path.exists().then_some(path)
}

fn run_rc_file(path: &Path, state: &mut ShellState, fresh_runtime: bool) -> CommandStatus {
    reset_repl_output_state();
    let result = if fresh_runtime {
        run_rc_file_with_fresh_runtime(path, state)
    } else {
        run_script_file_with_ctx(path, &state.script_env, state, None)
    };

    match result {
        Ok(()) => {
            if repl_output_needs_newline() {
                println!();
            }
            CommandStatus::success()
        }
        Err(err) => {
            if repl_output_needs_newline() {
                println!();
            }
            print_error(err.format_for_user());
            CommandStatus::failure()
        }
    }
}

fn run_rc_file_with_fresh_runtime(
    path: &Path,
    state: &mut ShellState,
) -> Result<(), crate::ecscript::ScriptFileError> {
    let mut staged = state.clone();
    staged.script_env = Rc::new(Environment::new());
    staged.extensions = new_extensions();
    staged.module_loader = Some(Rc::new(ModuleLoader::new()));

    run_script_file_with_ctx(path, &staged.script_env, &staged, None)?;

    state.script_env = staged.script_env;
    state.aliases = staged.aliases;
    state.traps = staged.traps;
    state.extensions = staged.extensions;
    state.module_loader = staged.module_loader;
    state.last_status = staged.last_status;
    Ok(())
}

/// `type` / `which` 的内部统一解析结果。
enum CommandDescription<'a> {
    Alias(&'a str),
    Builtin,
    ScriptCommand,
    External(PathBuf),
}

/// 按 alias → builtin → ecscript command → PATH 的顺序解析一个命令名。
fn describe_command<'a>(name: &'a str, state: &'a ShellState) -> Option<CommandDescription<'a>> {
    if let Some(alias) = state.aliases.get(name) {
        return Some(CommandDescription::Alias(alias.as_str()));
    }
    if BUILTIN_NAMES.contains(&name) {
        return Some(CommandDescription::Builtin);
    }
    if state.extensions.borrow().script_commands.contains_key(name) {
        return Some(CommandDescription::ScriptCommand);
    }
    resolve_external_command(name).map(CommandDescription::External)
}

/// 按 shell 的常见规则在 PATH 中定位外部命令。
fn resolve_external_command(name: &str) -> Option<PathBuf> {
    if name.contains('/') {
        let path = PathBuf::from(name);
        return is_executable_file(&path).then_some(path);
    }

    let path = std::env::var("PATH").ok()?;
    for dir in path.split(':') {
        let candidate = Path::new(dir).join(name);
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }
    None
}

/// 判断一个路径是否指向可执行普通文件。
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
        && path
            .metadata()
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
}

/// `clear` 命令：清空终端屏幕。
///
/// ANSI 转义序列含义：
///   - \\x1b[2J → 清空整个屏幕
///   - \\x1b[3J → 清空 scrollback 缓冲区（不是所有终端都支持）
///   - \\x1b[H  → 光标移到左上角
fn run_clear() -> CommandStatus {
    print!("\x1b[2J\x1b[3J\x1b[H");
    match io::stdout().flush() {
        Ok(()) => CommandStatus::success(),
        Err(err) => {
            print_error(format!("clear: {}", err));
            CommandStatus::failure()
        }
    }
}

/// `status` 命令：打印上一条命令的退出码，供调试使用。
fn run_status(state: &mut ShellState) -> CommandStatus {
    println!("{}", state.last_status.code);
    CommandStatus::success()
}

/// 打印统一格式的 usage 错误。
fn print_usage(name: &str, usage: &str) {
    print_error(format!("{}: usage: {}", name, usage));
}

#[cfg(test)]
mod tests {
    use super::reload_startup_rc;
    use crate::ecscript::{Environment, ModuleLoader, Value, run_script_source};
    use crate::extensions::new_extensions;
    use crate::types::{CommandStatus, ShellState};
    use std::collections::HashMap;
    use std::rc::Rc;

    fn state() -> ShellState {
        ShellState {
            last_status: CommandStatus::success(),
            interactive: true,
            shell_pgid: None,
            shell_terminal_fd: None,
            jobs: Vec::new(),
            next_job_id: 1,
            current_fg_pgid: None,
            script_env: Rc::new(Environment::new()),
            aliases: HashMap::new(),
            traps: HashMap::new(),
            command_history: Vec::new(),
            extensions: new_extensions(),
            module_loader: Some(Rc::new(ModuleLoader::new())),
        }
    }

    #[test]
    fn reload_rc_keeps_old_script_env_on_failure() {
        let home = std::env::temp_dir().join(format!("ecsh-reload-rc-fail-{}", std::process::id()));
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(home.join(".ecshrc"), "let replacement = \n").unwrap();

        let old_home = std::env::var_os("HOME");
        unsafe { std::env::set_var("HOME", &home) };

        let mut shell = state();
        run_script_source("let sentinel = 1\n", &shell.script_env).unwrap();

        let status = reload_startup_rc(&mut shell);
        assert_eq!(status, CommandStatus::failure());
        assert_eq!(shell.script_env.get("sentinel", 0).unwrap(), Value::Int(1));
        assert!(shell.script_env.get("replacement", 0).is_err());

        match old_home {
            Some(value) => unsafe { std::env::set_var("HOME", value) },
            None => unsafe { std::env::remove_var("HOME") },
        }

        let _ = std::fs::remove_file(home.join(".ecshrc"));
        let _ = std::fs::remove_dir(home);
    }
}
