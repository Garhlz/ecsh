# MEMORY

This file summarizes the working context for continuing development of `ecsh`.
It is intended as handoff context for another AI assistant or future session.

## Project

- Repository: `/home/elaine/work/courses/szu_os/lab3/ecsh`
- Rust binary crate name: `ecsh`
- Purpose: a simple Unix-like shell for an operating systems lab.
- Main file: `src/main.rs`
- User language preference: Chinese explanations are preferred.
- The user is learning Rust and OS process APIs, and often asks for guidance instead of direct implementation.

## Commit Rules

Follow `AGENT.md`.

Commit message format:

```text
type(scope): concise summary

- Bullet list body
- Mention main changes
- Mention supported user-visible behavior
- Mention notable project/dependency/configuration changes
```

Avoid using multiple `git commit -m` body arguments for each bullet because that inserts blank lines between bullet items. Use one body argument containing newline-separated bullets.

Recent commits:

- `5a8d2d5 feat(shell): add clear built-in`
- `6f090fe feat(shell): add environment built-ins`
- `b12972b feat(shell): add cwd built-ins and project documentation`
- `a90bc89 feat(shell): implement basic command execution`

## Current Dependencies

`Cargo.toml` uses:

```toml
nix = { version = "0.31.3", features = ["process", "fs", "term"] }
```

Important `nix 0.31.3` APIs already inspected:

- `pipe() -> Result<(OwnedFd, OwnedFd)>`
- `dup2_stdin<Fd: AsFd>(fd: Fd) -> Result<()>`
- `dup2_stdout<Fd: AsFd>(fd: Fd) -> Result<()>`
- `execvp(filename: &CStr, args: &[S]) -> Result<Infallible>`
- `fork() -> Result<ForkResult>`
- `waitpid(child, None)?` is valid.
- `close<Fd: IntoRawFd>(fd: Fd) -> Result<()>`, but be careful with `OwnedFd` ownership and double close.

## Implemented Features

Current shell supports:

- Prompt and input loop.
- Whitespace-based argument parsing.
- External command execution with `fork`, `execvp`, and `waitpid`.
- Built-ins:
  - `help`
  - `exit`
  - `cd`
  - `pwd`
  - `env`
  - `export KEY=value`
  - `unset KEY`
  - `clear`
- `clear` uses ANSI escapes and skips `starting/ending` lifecycle messages.
- Environment variable names are validated with `[A-Za-z_][A-Za-z0-9_]*`.
- `export A=b=c` is intended to work by splitting only once at `=`.
- `unset NOT_EXIST` should not be an error, but invalid names like `unset 1=1` must not panic.
- Standard pipeline execution with `|` for external commands.
- `.gitignore` ignores `/target`, `.vscode/`, and `hello*`.

## Design Decisions So Far

### `Command`

```rust
struct Command {
    program: String,
    args: Vec<String>,
}
```

`args` intentionally does not include the program name. `argv[0]` is reconstructed during exec.

### Parsing

`parse_args(line: &str) -> Option<Command>`:

- Empty input is not an error.
- Empty input returns `None`.

For pipeline parsing, the recommended shape is:

```rust
fn parse_pipeline(line: &str) -> Result<Option<Pipeline>, String>
```

Semantics:

- `Ok(None)`: no pipe symbol, handle as normal command.
- `Ok(Some(pipeline))`: valid pipeline.
- `Err(err)`: pipe syntax was present but invalid, such as `|`, `ls |`, or `ls || grep`.

### Pipeline Syntax

Use standard shell pipe `|`, not lab text `||`.

Rationale:

- Standard Unix shell uses `|` for pipelines.
- `||` is logical OR in normal shell syntax.
- Current parser does not support quotes, so `echo "a|b"` will be split incorrectly. This is accepted as a first-version limitation.

### Pipeline Data Model

```rust
struct Pipeline {
    commands: Vec<Command>,
}
```

Use `commands`, not `programs`, because each element is a complete command.

### Built-ins in Pipeline

First version should not support built-ins in pipelines.

Reason:

- Pipeline elements normally execute in child processes.
- Built-ins such as `cd`, `export`, `unset`, and `exit` affect shell state and would not affect the parent shell if run in a child.
- Simpler first version: reject any pipeline containing a built-in.

Recommended helper:

```rust
fn is_builtin(command: &Command) -> bool
```

Use it both for execution dispatch and pipeline validation.

### External Execution Refactor

Do not call complete `run_external` from `run_pipeline`.

Reason:

- `run_external` forks and waits immediately.
- Pipeline must fork all children first, close pipe fds in parent, then wait for all children.

Better split:

- `build_c_argv(command) -> ShellResult<Vec<CString>>`
- `exec_external_or_exit(command) -> !`
- `run_external(command) -> ShellResult<()>` for the single-command path only.

Pipeline children should call `exec_external_or_exit(command)` after fd setup.

## Current Pipeline Status

The current `src/main.rs` implements standard `|` pipelines for external commands.

Already handled:

- `main_loop` continues to the next prompt after a pipeline, so pipeline input is not executed again as a normal command.
- Built-ins are rejected in pipelines before forking.
- Parent process drops pipe fds before waiting for children.
- Child process exits on `dup2_stdin` / `dup2_stdout` errors instead of returning to shell logic.
- Child process closes inherited pipe fds after `dup2_stdin` / `dup2_stdout` and before `execvp`.
- `exit` inside a pipeline does not exit the parent shell because built-ins are rejected.

Current limitations:

- No quote-aware parsing, so `echo "a|b"` is split incorrectly.
- Built-ins in pipelines are not supported.
- Pipeline command lifecycle messages are not printed per command.
- Pipeline exit status is not propagated as shell status.

## Pipeline Algorithm

For `cmd0 | cmd1 | cmd2`:

- Create `n - 1` pipes.
- For each command `i`:
  - `fork`.
  - Child:
    - If `i > 0`, redirect stdin from `pipes[i - 1].0` with `dup2_stdin`.
    - If `i < n - 1`, redirect stdout to `pipes[i].1` with `dup2_stdout`.
    - Close all pipe fds.
    - `exec_external_or_exit(command)`.
  - Parent:
    - Store child pid.
- Parent after forking all:
  - Drop/close all pipe fds.
  - `waitpid` all child pids.

Useful tests after fixes:

```text
echo hello | grep h
ls | grep rs
printf "a\nb\n" | grep b
yes | head -n 1
```

`yes | head -n 1` is especially useful for exposing fd closing problems.

## Redirection Plan

Redirection has not been completed yet.

Suggested order:

1. Finish standard pipe `|`.
2. Then add output redirection `>`.
3. Then add input redirection `<`.
4. Later add append redirection `>>`.

For redirection, reuse the same conceptual child setup:

- Open target file in child or before fork.
- Use `dup2_stdin` / `dup2_stdout`.
- Then call `exec_external_or_exit`.

Built-ins with redirection need separate design:

- For external commands, redirection is naturally done in the child.
- For parent-affecting built-ins like `cd`, `export`, `unset`, redirection is awkward because they run in the shell process.
- First version can reject redirection for built-ins, or only support output redirection for read-only built-ins by temporarily saving/restoring stdout.

## Rust Concepts Discussed

- `Option` for empty command parsing.
- `Result<Option<T>, E>` when distinguishing "not applicable" from "syntax error".
- `collect::<Option<Vec<T>>>()`: turns iterator of `Option<T>` into `Option<Vec<T>>`.
- `collect::<Result<Vec<_>, _>>()?`: turns iterator of `Result<T, E>` into `Result<Vec<T>, E>`, propagating the first error.
- `let Some(x) = expr else { ... };` is `let-else`, useful for early return on pattern mismatch.
- `execvp` success never returns; child must exit on exec failure.
- Rust 2024 makes environment mutation APIs such as `set_var` and `remove_var` unsafe.
- `OwnedFd` closes on drop.

## User Preferences

- The user often asks for conceptual guidance and may explicitly say not to write code.
- When the user asks to implement or commit, it is OK to edit files and run tools.
- Keep explanations direct, concrete, and in Chinese.
- Prefer explaining OS/Rust concepts around each implementation step.
