# ecsh — Elaine & Cornelia's Shell

A learning-oriented Unix-like shell written in Rust.

## Architecture

```
          input (rustyline or stdin)
              │
              ▼
          parse_line()
          ┌─────────────────────────────┐
          │  lexer.rs  → tokenize()      │
          │  parser.rs → parse_tokens()  │
          └─────────────────────────────┘
              │  ParsedLine AST
              ▼
          run_parsed_line()  (main.rs)
          ┌──────────────┐
          │ &, &&, ||, ;  │── control flow dispatch
          └──────────────┘
              │
              ▼
          executor/
          ├── mod.rs      — route: builtin vs external vs pipeline
          ├── launch.rs   — fork, pipe, dup2, execvp
          ├── jobs.rs     — job state machine, terminal control, waitpid
          └── builtins.rs — jobs/fg/bg commands, redirection wrapper
              │
              ▼
          POSIX syscalls (via nix crate)
```

## Key modules

| Module | Responsibility |
|--------|---------------|
| `lexer.rs` | Tokenize input: words, operators (`|`, `&&`, `||`, `;`, `&`, `<`, `>`, `>>`), quotes, `$` expansion, backslash escapes |
| `parser.rs` | Recursive descent parser: operator precedence `;` < `&&`/`||` < `|` < command; left-associative via `rposition` |
| `types.rs` | All core types: `Command`, `Pipeline`, `ParsedLine`, `ParsedJob`, `Job`, `JobProcess`, `ShellState`, `CommandFlow`, etc. |
| `builtin.rs` | Built-in commands: cd, pwd, env, export, unset, clear, status, help, exit |
| `executor/mod.rs` | Entry point: decides builtin vs external vs pipeline path |
| `executor/launch.rs` | Process launch: `fork` → `pipe` → `dup2` → `execvp` |
| `executor/jobs.rs` | Job control state machine: process groups, terminal control (`tcsetpgrp`), foreground wait (`waitpid`), background reaping |
| `executor/builtins.rs` | Special builtins that need job table access: `jobs`, `fg %N`, `bg %N` |
| `signals.rs` | Signal ignore for shell (SIGINT/SIGQUIT/SIGTSTP/SIGTTIN/SIGTTOU), restore defaults for child processes |
| `redirection.rs` | `<` input, `>`/`>>` output redirection. Two paths: builtin (save/restore) and child (dup2 directly) |
| `prompt.rs` | Two-line colored prompt: `[ecsh] user@host:dir [exit_code]\n$ ` |
| `input.rs` | Interactive (rustyline with history) vs non-interactive (plain stdin) input |
| `diagnostics.rs` | Print error to stderr with immediate flush |

## Job control (the hardest part)

Modeled after POSIX shell job control:
- **Process groups**: `setpgid()` creates a group; all processes in a pipeline share one pgid
- **Terminal control**: `tcsetpgrp()` sets which process group owns the foreground terminal
- **Signals**: Shell ignores interactive signals; children restore defaults
- **State tracking**: `waitpid()` polling updates `Job → JobProcess → ProcessState`
- **fg/bg**: `killpg(SIGCONT)` + terminal handover for fg

## Tests

```
tests/lexer.rs  — 11 tests (tokenization, quotes, escapes, expansion, errors)
tests/parser.rs — 10 tests (commands, pipelines, operators, precedence, errors)
tests/smoke.rs  —  5 tests (end-to-end, background jobs, redirection)
Unit tests in:
  src/executor/jobs.rs     — 20 tests (state machine, recompute, update, wait_status)
  src/executor/builtins.rs —  5 tests (parse_job_spec)
```

Run: `cargo test` (51 tests total)

## Dependencies

- `nix` — POSIX syscalls (fork, execvp, pipe, dup2, setpgid, tcsetpgrp, waitpid, killpg, sigaction, etc.)
- `rustyline` — readline with history and line editing

## Future roadmap

See `TODO.md` for prioritized feature list.
