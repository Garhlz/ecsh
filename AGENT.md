# Agent Instructions

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
