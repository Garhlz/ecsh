# Agent Instructions

## Scope

This repository now contains two closely related parts:

- `ecsh`: a learning-oriented Unix-like shell written in Rust
- `ecscript`: a small scripting language and interpreter evolving alongside `ecsh`

Instructions in this file apply to both code edits and document edits unless a
section says otherwise.

## Writing Style

Project documents should use a stable engineering-document tone rather than a
proposal tone or a conversational tone.

Preferred style:

- Use objective statements instead of first-person phrasing.
- State current behavior, boundaries, and status directly.
- Keep wording concise and explicit.
- Prefer “what is implemented / not implemented / planned next” over persuasion.
- Keep the distinction clear between:
  - status documents
  - implementation manuals
  - long-form design notes

Avoid wording such as:

- “I think”
- “I suggest”
- “naturally the next step”
- “highest ROI”
- other subjective or sales-like phrasing

Good document patterns:

- “Current status”
- “Implemented”
- “Not implemented”
- “Known boundaries”
- “Next entry points”

For this repository:

- `README.md` should stay short and act as the project entry page.
- `docs/status.md` should describe current progress only.
- `docs/ecscript-manual.md` should describe implemented `ecscript` behavior.
- `docs/TODO.md` may keep longer design discussion and staged planning.

## Comment Style

This is a learning-oriented Rust shell project. Comments may be more detailed
than typical production comments when they help explain Rust syntax, Unix
process concepts, file descriptor ownership, or shell behavior.

Prefer comments that explain why the code is structured this way, not only what
the next line does. It is acceptable to explain Rust patterns such as `Result`,
`Option`, `?`, `while let`, `OwnedFd`, `fork`, `dup2`, and pipe closing when
they are important to understanding the implementation.

When editing comments:

- Keep useful learning notes instead of deleting them just to make the code look terse.
- Improve inaccurate or overly casual wording into stable, clear Chinese.
- Preserve detailed explanations around Unix/Rust boundary code, especially fd
  saving/restoring, pipe ownership, child-process exit paths, and builtin command behavior.
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
the execution flow easier to read, prefer keeping the logic inline near the
call site. This is especially true for short pipeline steps where the local
order matters, such as binding pipe fd, applying redirection, closing inherited
fd, and then running builtin or `execvp`.

## Testing Style

Prefer keeping behavior tests in the `tests/` directory when they can use the
public crate API. This keeps lexer/parser tests close to how downstream code
would call them, and it also explains why `src/lib.rs` exists alongside the
interactive `src/main.rs` binary entry.

Use focused tests for pure logic such as tokenization and parsing, and use
small smoke tests for end-to-end shell behavior that must launch the compiled
binary.

Before preparing a commit, run:

```text
cargo fmt --check
cargo check
cargo test
```

## Commit Format

Use Conventional Commits. Add a short scope when it helps identify the affected
area.

Preferred subject format:

```text
type(scope): concise summary
```

Recommended types:

- `feat`
- `fix`
- `refactor`
- `docs`
- `test`
- `chore`

Recommended scopes in this repository:

- `shell`
- `ecscript`
- `lexer`
- `parser`
- `executor`
- `docs`
- `tests`

Recommended commit body format:

```text
type(scope): concise summary

- State the main code or document changes
- Mention newly supported behavior when relevant
- Mention notable compatibility, diagnostics, or test updates
```

Example:

```text
feat(ecscript): complete stage 6 shellword runtime expansion

- Add runtime ShellWord expansion for $VAR, ${VAR}, $(cmd), and $[expr]
- Support $[...arr] spreading into argv
- Expand redirection targets at execution time
- Add unit and smoke coverage for runtime expansion behavior
```

When a commit corresponds to a planned stage, mention the stage explicitly in
the subject or first bullet so that `git log` remains usable as a progress
record.
