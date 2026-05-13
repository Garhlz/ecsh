# Agent Instructions

## Comment Style

This is a learning-oriented Rust shell project. Comments may be more detailed
than typical production comments when they help explain Rust syntax, Unix
process concepts, file descriptor ownership, or shell behavior.

Prefer comments that explain why the code is structured this way, not only what
the next line does. It is acceptable to explain Rust patterns such as `Result`,
`Option`, `?`, `while let`, `OwnedFd`, `fork`, `dup2`, and pipe closing when they
are important to understanding the implementation.

When editing comments:

- Keep useful learning notes instead of deleting them just to make the code look
  terse.
- Improve inaccurate or overly casual wording into stable, clear Chinese.
- Preserve detailed explanations around Unix/Rust boundary code, especially fd
  saving/restoring, pipe ownership, child-process exit paths, and builtin command
  behavior.
- Avoid comments that merely restate obvious code unless they teach an important
  Rust or Unix concept.

## Refactoring Style

Prefer simple local control flow over unnecessary helper functions. A helper is
worth extracting when it does at least one of the following:

- Reduces meaningful complexity in the caller.
- Encapsulates duplicated logic.
- Names an important shell or Unix concept.
- Isolates resource-sensitive behavior such as fd saving/restoring, pipe
  ownership, child-process exit paths, or `execvp` setup.

If a helper is only used once and does not protect a resource boundary or make
the execution flow easier to read, prefer keeping the logic inline near the call
site. This is especially true for short pipeline steps where the local order
matters, such as binding pipe fd, applying redirection, closing inherited fd, and
then running builtin or `execvp`.

## Testing Style

Prefer keeping behavior tests in the `tests/` directory when they can use the
public crate API. This keeps lexer/parser tests close to how downstream code
would call them, and it also explains why `src/lib.rs` exists alongside the
interactive `src/main.rs` binary entry.

Use focused tests for pure logic such as tokenization and parsing, and use small
smoke tests for end-to-end shell behavior that must launch the compiled binary.
Before preparing a commit, run:

```text
cargo fmt --check
cargo check
cargo test
```

## Commit Message Format

Use Conventional Commits with a short scope when it helps clarify the affected area.

Preferred format:

```text
type(scope): concise summary

- List the main changes in bullet points
- Mention newly supported behavior or user-visible functionality
- Mention notable project, dependency, or configuration changes
```

Example:

```text
feat(shell): implement basic command execution

- Add initial Cargo project metadata and dependency lockfile
- Implement prompt, input reading, and argument parsing
- Support help and exit built-ins
- Run external commands through fork, execvp, and waitpid
- Keep the shell alive after command execution failures
- Ignore target and .vscode
```
