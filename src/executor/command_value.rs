//! `cmd{ ... }` 命令值的最小执行桥。
//!
//! 这一层只负责：
//! - 复用 shell word 展开，把 `CommandInvocation` 变成可执行命令
//! - 用当前 shell/script 环境执行外部命令
//! - 返回结构化 `CommandResult`
//!
//! 当前刻意不复用 shell 顶层控制流：
//! - 不支持 `&&` / `||` / `;` / `&`
//! - 只支持单命令中的纯输出 shell builtin
//! - pipeline 已支持，但 builtin 仍然不进入命令值 pipeline

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, OwnedFd};
use std::path::PathBuf;
use std::process::{Command as ProcessCommand, Stdio};
use std::thread;
use std::time::Instant;

use crate::builtin::{builtin_kind, is_builtin_allowed_in_pipeline, run_builtin};
use crate::ecscript::value::{CommandInvocation, CommandResult, CommandValue};
use crate::types::{Command, OutputRedirection, Pipeline, ShellResult, ShellState};

use super::builtins::run_builtin_with_redirection;
use super::expand;

/// 经过 shell word 展开后的可执行命令。
///
/// `CommandInvocation` 里仍然保留 `${expr}` / `${...arr}` 这类延迟展开片段；
/// 真正执行前先收口成这个结构，后续 stdio / spawn 逻辑只处理字面量路径和 argv。
#[derive(Clone, Debug)]
struct ExpandedCommandInvocation {
    command: ExpandedCommandValue,
    cwd_override: Option<PathBuf>,
    env_override: Option<HashMap<String, String>>,
    stdin_override: Option<String>,
}

#[derive(Clone, Debug)]
enum ExpandedCommandValue {
    Simple(Command),
    Pipeline(Pipeline),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExecutionMode {
    InheritedTerminal,
    Captured,
}

/// 继承当前终端执行命令值。
///
/// 当前返回 `CommandResult`，上层 `run(cmd)` builtin 可以再决定：
/// - 成功时返回 `nil`
/// - 非零退出时提升成语言错误
pub fn run_command_invocation(
    invocation: &CommandInvocation,
    state: &ShellState,
) -> ShellResult<CommandResult> {
    let expanded = expand_invocation(invocation, state)?;
    execute_command_invocation(&expanded, state, ExecutionMode::InheritedTerminal)
}

/// 捕获 stdout/stderr 执行命令值。
///
/// 这里不把非零退出码视为错误；是否把失败提升成语言错误由更上层 API 决定。
pub fn capture_command_invocation(
    invocation: &CommandInvocation,
    state: &ShellState,
) -> ShellResult<CommandResult> {
    let expanded = expand_invocation(invocation, state)?;
    execute_command_invocation(&expanded, state, ExecutionMode::Captured)
}

/// 把命令值里的 shell word 片段展开成真正可执行的命令结构。
///
/// 这一层只负责两件事：
/// - 复用现有 shell 展开规则，把 `ShellWord` 收口成字面量 argv / 重定向路径
/// - 把命令值上的 override 字段转成后续 `std::process::Command` 需要的数据
///
/// 当前接受单命令和 pipeline，但 shell builtin 仍然不在命令值桥接范围内。
fn expand_invocation(
    invocation: &CommandInvocation,
    state: &ShellState,
) -> ShellResult<ExpandedCommandInvocation> {
    let command = match &invocation.command {
        CommandValue::Simple(command) => {
            ExpandedCommandValue::Simple(expand::expand_command(command, state)?)
        }
        CommandValue::Pipeline(pipeline) => ExpandedCommandValue::Pipeline(Pipeline {
            commands: pipeline
                .commands
                .iter()
                .map(|command| expand::expand_command(command, state))
                .collect::<ShellResult<Vec<_>>>()?,
        }),
    };

    validate_expanded_command(&command)?;

    Ok(ExpandedCommandInvocation {
        command,
        cwd_override: invocation.cwd_override.as_ref().map(PathBuf::from),
        env_override: invocation
            .env_override
            .as_ref()
            .map(|vars| vars.iter().map(|(k, v)| (k.clone(), v.clone())).collect()),
        stdin_override: invocation.stdin_override.clone(),
    })
}

fn validate_expanded_command(command: &ExpandedCommandValue) -> ShellResult<()> {
    match command {
        // 单命令允许进入一小撮纯输出 builtin；会修改 shell 状态的 builtin 仍然留在 shell 自身语义里。
        ExpandedCommandValue::Simple(command) => {
            if let Some(kind) = builtin_kind(command)
                && !is_builtin_allowed_in_pipeline(kind)
            {
                return Err("only pure-output shell built-ins are supported by run/capture".into());
            }
        }
        ExpandedCommandValue::Pipeline(pipeline) => {
            validate_pipeline_redirection(pipeline)?;
            for command in &pipeline.commands {
                if builtin_kind(command).is_some() {
                    return Err(
                        "shell built-in commands are not supported in command-value pipelines yet"
                            .into(),
                    );
                }
            }
        }
    }
    Ok(())
}

/// 按执行模式运行已经完成展开的单命令。
///
/// 这里故意不关心 shell 顶层控制流，只做：
/// - 配置 argv / cwd / env / stdio
/// - spawn 子进程
/// - 可选写入 stdin_override
/// - 根据模式收集退出状态和输出
fn execute_command_invocation(
    invocation: &ExpandedCommandInvocation,
    state: &ShellState,
    mode: ExecutionMode,
) -> ShellResult<CommandResult> {
    match &invocation.command {
        ExpandedCommandValue::Simple(command) => execute_simple_command(
            command,
            state,
            invocation.cwd_override.as_deref(),
            invocation.env_override.as_ref(),
            invocation.stdin_override.as_deref(),
            mode,
        ),
        ExpandedCommandValue::Pipeline(pipeline) => execute_pipeline(
            pipeline,
            state,
            invocation.cwd_override.as_deref(),
            invocation.env_override.as_ref(),
            invocation.stdin_override.as_deref(),
            mode,
        ),
    }
}

fn execute_simple_command(
    command: &Command,
    state: &ShellState,
    cwd_override: Option<&std::path::Path>,
    env_override: Option<&HashMap<String, String>>,
    stdin_override: Option<&str>,
    mode: ExecutionMode,
) -> ShellResult<CommandResult> {
    if let Some(kind) = builtin_kind(command) {
        return match mode {
            ExecutionMode::InheritedTerminal => run_simple_builtin(command, state),
            ExecutionMode::Captured => capture_simple_builtin(command, state, kind),
        };
    }

    let mut process = ProcessCommand::new(
        command
            .program
            .as_lit_str()
            .ok_or("expanded command program must be a literal")?,
    );

    process.args(
        command
            .args
            .iter()
            .map(|arg| {
                arg.as_lit_str()
                    .ok_or("expanded command arg must be a literal")
            })
            .collect::<Result<Vec<_>, _>>()?,
    );

    if let Some(cwd) = cwd_override {
        process.current_dir(cwd);
    }
    if let Some(env) = env_override {
        process.envs(env);
    }

    apply_stdio(command, stdin_override, &mut process, mode)?;

    let start = Instant::now();
    let mut child = process.spawn()?;

    if let Some(stdin) = stdin_override {
        if let Some(mut child_stdin) = child.stdin.take() {
            child_stdin.write_all(stdin.as_bytes())?;
        }
    }

    match mode {
        ExecutionMode::InheritedTerminal => {
            let status = child.wait()?;
            let duration_ms = start.elapsed().as_millis();
            Ok(CommandResult {
                code: status.code().unwrap_or(128),
                signal: exit_signal(&status),
                stdout: String::new(),
                stderr: String::new(),
                duration_ms,
            })
        }
        ExecutionMode::Captured => {
            let output = child.wait_with_output()?;
            let duration_ms = start.elapsed().as_millis();
            Ok(CommandResult {
                code: output.status.code().unwrap_or(128),
                signal: exit_signal(&output.status),
                stdout: String::from_utf8(output.stdout)?,
                stderr: String::from_utf8(output.stderr)?,
                duration_ms,
            })
        }
    }
}

fn execute_pipeline(
    pipeline: &Pipeline,
    _state: &ShellState,
    cwd_override: Option<&std::path::Path>,
    env_override: Option<&HashMap<String, String>>,
    stdin_override: Option<&str>,
    mode: ExecutionMode,
) -> ShellResult<CommandResult> {
    let start = Instant::now();
    let mut previous_stdout = None;
    let mut stdin_writer = None;
    let mut children = Vec::new();
    let mut stderr_readers = Vec::new();
    let mut final_stdout = None;

    for (index, command) in pipeline.commands.iter().enumerate() {
        let is_first = index == 0;
        let is_last = index == pipeline.commands.len() - 1;

        let mut process = ProcessCommand::new(
            command
                .program
                .as_lit_str()
                .ok_or("expanded command program must be a literal")?,
        );
        process.args(
            command
                .args
                .iter()
                .map(|arg| {
                    arg.as_lit_str()
                        .ok_or("expanded command arg must be a literal")
                })
                .collect::<Result<Vec<_>, _>>()?,
        );

        if let Some(cwd) = cwd_override {
            process.current_dir(cwd);
        }
        if let Some(env) = env_override {
            process.envs(env);
        }

        if let Some(stdout) = previous_stdout.take() {
            process.stdin(Stdio::from(stdout));
        } else if is_first {
            if stdin_override.is_some() {
                process.stdin(Stdio::piped());
            } else if let Some(path) = &command.redirection.stdin {
                process
                    .stdin(Stdio::from(File::open(path.as_lit_str().ok_or(
                        "expanded stdin redirection path must be a literal",
                    )?)?));
            } else {
                process.stdin(Stdio::inherit());
            }
        }

        if is_last {
            if mode == ExecutionMode::Captured {
                process.stdout(Stdio::piped());
            } else if let Some(stdout) = &command.redirection.stdout {
                match stdout {
                    OutputRedirection::Truncate(path) => {
                        process
                            .stdout(Stdio::from(File::create(path.as_lit_str().ok_or(
                                "expanded stdout redirection path must be a literal",
                            )?)?));
                    }
                    OutputRedirection::Append(path) => {
                        let file =
                            std::fs::OpenOptions::new()
                                .create(true)
                                .append(true)
                                .open(path.as_lit_str().ok_or(
                                    "expanded stdout redirection path must be a literal",
                                )?)?;
                        process.stdout(Stdio::from(file));
                    }
                }
            } else {
                process.stdout(Stdio::inherit());
            }
        } else {
            process.stdout(Stdio::piped());
        }

        if mode == ExecutionMode::Captured {
            process.stderr(Stdio::piped());
        } else {
            process.stderr(Stdio::inherit());
        }

        let mut child = process.spawn()?;

        if is_first {
            stdin_writer = child.stdin.take();
        }
        if !is_last {
            previous_stdout = child.stdout.take();
        } else if mode == ExecutionMode::Captured {
            final_stdout = child.stdout.take();
        }

        if mode == ExecutionMode::Captured
            && let Some(stderr) = child.stderr.take()
        {
            stderr_readers.push(thread::spawn(move || -> std::io::Result<Vec<u8>> {
                let mut reader = stderr;
                let mut buf = Vec::new();
                reader.read_to_end(&mut buf)?;
                Ok(buf)
            }));
        }

        children.push(child);
    }

    if let Some(stdin) = stdin_override
        && let Some(mut writer) = stdin_writer.take()
    {
        writer.write_all(stdin.as_bytes())?;
    }

    let mut final_status = None;
    for child in &mut children {
        final_status = Some(child.wait()?);
    }
    let final_status = final_status.ok_or("pipeline must contain at least one command")?;

    let stdout = if let Some(stdout) = final_stdout {
        let mut reader = stdout;
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf)?;
        String::from_utf8(buf)?
    } else {
        String::new()
    };

    let mut stderr = String::new();
    for reader in stderr_readers {
        let buf = reader
            .join()
            .map_err(|_| "failed to join stderr reader thread")??;
        stderr.push_str(&String::from_utf8(buf)?);
    }

    Ok(CommandResult {
        code: final_status.code().unwrap_or(128),
        signal: exit_signal(&final_status),
        stdout,
        stderr,
        duration_ms: start.elapsed().as_millis(),
    })
}

fn run_simple_builtin(command: &Command, state: &ShellState) -> ShellResult<CommandResult> {
    let start = Instant::now();
    let mut child_state = builtin_bridge_state(state);
    let flow = run_builtin_with_redirection(command, &mut child_state)?;
    let status = match flow {
        crate::types::CommandFlow::Continue(status) | crate::types::CommandFlow::Exit(status) => {
            status
        }
    };

    Ok(CommandResult {
        code: status.code,
        signal: None,
        stdout: String::new(),
        stderr: String::new(),
        duration_ms: start.elapsed().as_millis(),
    })
}

fn capture_simple_builtin(
    command: &Command,
    state: &ShellState,
    _kind: crate::builtin::BuiltinKind,
) -> ShellResult<CommandResult> {
    let start = Instant::now();
    let stdout_path = temp_capture_path("stdout");
    let stderr_path = temp_capture_path("stderr");
    let stdout_file = File::create(&stdout_path)?;
    let stderr_file = File::create(&stderr_path)?;
    let saved_stdout = nix::unistd::dup(std::io::stdout())?;
    let saved_stderr = nix::unistd::dup(std::io::stderr())?;

    redirect_fd(stdout_file.as_raw_fd(), std::io::stdout().as_raw_fd())?;
    redirect_fd(stderr_file.as_raw_fd(), std::io::stderr().as_raw_fd())?;

    let mut child_state = builtin_bridge_state(state);
    let flow = run_builtin(command, &mut child_state)
        .ok_or("special shell built-ins are not supported by run/capture")?;
    std::io::stdout().flush()?;
    std::io::stderr().flush()?;
    let restore_stdout = nix::unistd::dup(&saved_stdout)?;
    let restore_stderr = nix::unistd::dup(&saved_stderr)?;
    nix::unistd::dup2_stdout(&restore_stdout)?;
    restore_stderr_fd(&restore_stderr)?;
    drop(stdout_file);
    drop(stderr_file);

    let status = match flow {
        crate::types::CommandFlow::Continue(status) | crate::types::CommandFlow::Exit(status) => {
            status
        }
    };
    let stdout = std::fs::read_to_string(&stdout_path)?;
    let stderr = std::fs::read_to_string(&stderr_path)?;
    let _ = std::fs::remove_file(&stdout_path);
    let _ = std::fs::remove_file(&stderr_path);

    Ok(CommandResult {
        code: status.code,
        signal: None,
        stdout,
        stderr,
        duration_ms: start.elapsed().as_millis(),
    })
}

fn builtin_bridge_state(state: &ShellState) -> ShellState {
    ShellState {
        last_status: state.last_status,
        interactive: false,
        shell_pgid: None,
        shell_terminal_fd: None,
        jobs: Vec::new(),
        next_job_id: 1,
        current_fg_pgid: None,
        script_env: crate::ecscript::env::Environment::new(),
        aliases: state.aliases.clone(),
        traps: HashMap::new(),
        command_history: state.command_history.clone(),
    }
}

fn temp_capture_path(suffix: &str) -> PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "ecsh-command-bridge-{}-{}-{}",
        std::process::id(),
        nonce,
        suffix
    ))
}

fn redirect_fd(from: i32, to: i32) -> ShellResult<()> {
    // SAFETY: `dup2` only duplicates raw file descriptors. Both descriptors come from
    // already-opened files/std streams in the current process.
    let rc = unsafe { libc::dup2(from, to) };
    if rc == -1 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

fn restore_stderr_fd(saved: &OwnedFd) -> ShellResult<()> {
    redirect_fd(saved.as_raw_fd(), std::io::stderr().as_raw_fd())
}

/// 根据调用模式和重定向规则配置子进程 stdio。
///
/// 优先级保持简单：
/// - `stdin_override` 高于 `< file`
/// - `Captured` 高于 `> file` / `>> file`
/// - 都没有时继承当前终端
fn apply_stdio(
    command: &Command,
    stdin_override: Option<&str>,
    process: &mut ProcessCommand,
    mode: ExecutionMode,
) -> ShellResult<()> {
    if stdin_override.is_some() {
        // 给即将启动的子进程创建一根匿名管道，并把子进程的 stdin 接到这根管道的读端
        process.stdin(Stdio::piped());
    } else if let Some(path) = &command.redirection.stdin {
        process
            .stdin(Stdio::from(File::open(path.as_lit_str().ok_or(
                "expanded stdin redirection path must be a literal",
            )?)?));
    } else {
        process.stdin(Stdio::inherit());
    }

    // 如果是capture()，覆盖标准输入输出
    if mode == ExecutionMode::Captured {
        process.stdout(Stdio::piped());
        process.stderr(Stdio::piped());
        return Ok(());
    }

    if let Some(stdout) = &command.redirection.stdout {
        match stdout {
            OutputRedirection::Truncate(path) => {
                process
                    .stdout(Stdio::from(File::create(path.as_lit_str().ok_or(
                        "expanded stdout redirection path must be a literal",
                    )?)?));
            }
            OutputRedirection::Append(path) => {
                let file = std::fs::OpenOptions::new().create(true).append(true).open(
                    path.as_lit_str()
                        .ok_or("expanded stdout redirection path must be a literal")?,
                )?;
                process.stdout(Stdio::from(file));
            }
        }
    } else {
        process.stdout(Stdio::inherit());
    }
    process.stderr(Stdio::inherit());
    Ok(())
}

fn validate_pipeline_redirection(pipeline: &Pipeline) -> ShellResult<()> {
    let n = pipeline.commands.len();
    for (i, command) in pipeline.commands.iter().enumerate() {
        if command.redirection.stdin.is_some() && i != 0 {
            return Err(format!(
                "{}: stdin redirection is only supported on the first pipeline command",
                command
            )
            .into());
        }
        if command.redirection.stdout.is_some() && i != n - 1 {
            return Err(format!(
                "{}: stdout redirection is only supported on the last pipeline command",
                command
            )
            .into());
        }
    }
    Ok(())
}

/// 提取子进程被信号终止时的信号号。
///
/// 非 Unix 平台上保留 `None`，避免把平台差异扩散到更上层。
#[cfg(unix)]
fn exit_signal(status: &std::process::ExitStatus) -> Option<i32> {
    use std::os::unix::process::ExitStatusExt;
    status.signal()
}

#[cfg(not(unix))]
fn exit_signal(_status: &std::process::ExitStatus) -> Option<i32> {
    None
}

#[cfg(test)]
mod tests {
    use super::{capture_command_invocation, run_command_invocation};
    use crate::ecscript::{
        env::Environment,
        value::{CommandInvocation, CommandValue},
    };
    use crate::types::{
        Command, CommandStatus, OutputRedirection, Pipeline, Redirection, ShellState, ShellWord,
    };
    use std::collections::{BTreeMap, HashMap};
    use std::fs;

    fn state() -> ShellState {
        ShellState {
            last_status: CommandStatus::success(),
            interactive: false,
            shell_pgid: None,
            shell_terminal_fd: None,
            jobs: Vec::new(),
            next_job_id: 1,
            current_fg_pgid: None,
            script_env: Environment::new(),
            aliases: HashMap::new(),
            traps: HashMap::new(),
            command_history: Vec::new(),
        }
    }

    fn simple_invocation(program: &str, args: &[&str]) -> CommandInvocation {
        CommandInvocation {
            command: CommandValue::Simple(Command {
                program: ShellWord::lit(program),
                args: args.iter().map(|arg| ShellWord::lit(*arg)).collect(),
                redirection: Redirection::default(),
            }),
            cwd_override: None,
            env_override: None,
            stdin_override: None,
        }
    }

    fn pipeline_invocation(commands: Vec<Command>) -> CommandInvocation {
        CommandInvocation {
            command: CommandValue::Pipeline(Pipeline { commands }),
            cwd_override: None,
            env_override: None,
            stdin_override: None,
        }
    }

    #[test]
    fn capture_collects_stdout_and_stderr() {
        let state = state();
        let invocation = simple_invocation("sh", &["-c", "printf foo; printf bar >&2"]);
        let result = capture_command_invocation(&invocation, &state).unwrap();
        assert_eq!(result.code, 0);
        assert_eq!(result.stdout, "foo");
        assert_eq!(result.stderr, "bar");
    }

    #[test]
    fn capture_applies_env_override() {
        let state = state();
        let mut invocation = simple_invocation("sh", &["-c", "printf %s \"$ECSH_CMD_TEST\""]);
        invocation.env_override = Some(BTreeMap::from([(
            "ECSH_CMD_TEST".to_string(),
            "override".to_string(),
        )]));
        let result = capture_command_invocation(&invocation, &state).unwrap();
        assert_eq!(result.stdout, "override");
    }

    #[test]
    fn run_honors_stdout_redirection() {
        let state = state();
        let output_path =
            std::env::temp_dir().join(format!("ecsh-cmd-run-{}.txt", std::process::id()));
        let mut invocation = simple_invocation("printf", &["hello"]);
        invocation.command = CommandValue::Simple(Command {
            program: ShellWord::lit("printf"),
            args: vec![ShellWord::lit("hello")],
            redirection: Redirection {
                stdin: None,
                stdout: Some(OutputRedirection::Truncate(ShellWord::lit(
                    output_path.to_string_lossy().to_string(),
                ))),
            },
        });
        let result = run_command_invocation(&invocation, &state).unwrap();
        assert_eq!(result.code, 0);
        assert_eq!(fs::read_to_string(&output_path).unwrap(), "hello");
        let _ = fs::remove_file(output_path);
    }

    #[test]
    fn capture_runs_pipeline_and_collects_final_stdout() {
        let state = state();
        let invocation = pipeline_invocation(vec![
            Command {
                program: ShellWord::lit("printf"),
                args: vec![ShellWord::lit("foo")],
                redirection: Redirection::default(),
            },
            Command {
                program: ShellWord::lit("tr"),
                args: vec![ShellWord::lit("o"), ShellWord::lit("O")],
                redirection: Redirection::default(),
            },
        ]);

        let result = capture_command_invocation(&invocation, &state).unwrap();
        assert_eq!(result.code, 0);
        assert_eq!(result.stdout, "fOO");
    }

    #[test]
    fn capture_collects_pipeline_stderr() {
        let state = state();
        let invocation = pipeline_invocation(vec![
            Command {
                program: ShellWord::lit("sh"),
                args: vec![
                    ShellWord::lit("-c"),
                    ShellWord::lit("printf left >&2; printf foo"),
                ],
                redirection: Redirection::default(),
            },
            Command {
                program: ShellWord::lit("sh"),
                args: vec![
                    ShellWord::lit("-c"),
                    ShellWord::lit("printf right >&2; tr o O"),
                ],
                redirection: Redirection::default(),
            },
        ]);

        let result = capture_command_invocation(&invocation, &state).unwrap();
        assert_eq!(result.stdout, "fOO");
        assert!(result.stderr.contains("left"));
        assert!(result.stderr.contains("right"));
    }
}
