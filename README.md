# ecsh

`ecsh` is a small Unix-like shell implemented in Rust for OS lab practice.

The project focuses on understanding the basic shell execution model:
read a command line, parse it, handle built-in commands, fork a child process,
replace the child with the target program by `execvp`, and wait for the child
from the parent shell process.

## Current Features

- Interactive command prompt
- Whitespace-based command parsing
- Built-in commands:
  - `help`: show supported built-ins
  - `exit`: exit the shell
  - `cd`: change the shell process current working directory
  - `pwd`: print the shell process current working directory
  - `env`: print current environment variables
  - `export KEY=value`: set an environment variable in the shell process
  - `unset KEY`: remove an environment variable from the shell process
- External command execution through `fork`, `execvp`, and `waitpid`
- Invalid external command reporting without terminating the shell
- Environment variable updates inherited by external commands
- Required command lifecycle messages:
  - `<command> starting...`
  - `<command> ending.`

## Usage

Build and run:

```bash
cargo run
```

Example commands:

```bash
help
pwd
cd /tmp
pwd
echo hello
export ECSH_NAME=elaine
printenv ECSH_NAME
unset ECSH_NAME
ls
exit
```

## Implementation Notes

The shell currently represents a command as:

```rust
struct Command {
    program: String,
    args: Vec<String>,
}
```

`program` stores the command name, while `args` stores the remaining arguments.
When executing an external command, `ecsh` rebuilds the Unix `argv` list by
placing `program` at `argv[0]`.

Empty input is treated as `None` instead of an error, so pressing Enter simply
starts the next prompt.

Environment variable built-ins run in the shell process because their effects
must persist after the command returns. `export` and `unset` validate variable
names with the `[A-Za-z_][A-Za-z0-9_]*` rule before calling Rust's environment
mutation APIs.

## Development Plan

### Stage 1: Basic Command Execution

- [x] Read commands from standard input
- [x] Parse simple whitespace-separated arguments
- [x] Implement `help` and `exit`
- [x] Execute external commands
- [x] Report invalid commands
- [x] Implement `cd` and `pwd`
- [x] Add environment-related built-ins:
  - `env`
  - `export`
  - `unset`

### Stage 2: Unix Connectors

- [ ] Support a simple two-command pipe with `||`
- [ ] Redirect standard input and output

### Stage 3: Interactive Shell Behavior

- [ ] Improve prompt display
- [ ] Add command history
- [ ] Handle common interactive signals
- [ ] Explore foreground process groups and job control

### Stage 4: Script-like Features

- [ ] Variables
- [ ] Basic expansion
- [ ] Conditional execution
- [ ] Loops
- [ ] Functions

## Lab Requirement

The original lab asks for a simple shell that can:

- Print a command prompt and read commands repeatedly
- Distinguish built-in commands, external commands, and invalid commands
- Print command start and end messages
- Support pipeline execution such as `dir || more`

This implementation uses Rust and the `nix` crate to practice the Unix process
APIs directly.
