# vscode-ecscript

VS Code extension for `ecscript`, backed by `tree-sitter-ecscript`.

## Current scope

The extension currently provides:

- registers `.ecs` as the `ecscript` language
- installs line/block comments and bracket pairs
- exposes a small status command for smoke-checking activation
- exposes a syntax-tree command for inspecting the parsed CST
- registers a tree-sitter-backed semantic tokens provider

## Planned architecture

The long-term goal is to keep the extension thin and reuse the grammar from
[`tree-sitter-ecscript`](../tree-sitter-ecscript):

1. generate a parser artifact suitable for VS Code loading
2. load the parser from the extension host
3. reuse `queries/highlights.scm`
4. map tree-sitter captures to VS Code highlighting
5. optionally reuse `queries/injections.scm` for `cmd{ ... }`

## Why this is separate

This folder is intentionally structured so it can become its own repository
later without heavy refactoring.

The intended split is:

- `tree-sitter-ecscript/`: language grammar, queries, corpus tests
- `vscode-ecscript/`: VS Code packaging and editor integration

## Local development

Install dependencies and build the extension:

```bash
cd vscode-ecscript
npm install
npm run build
```

Refresh the copied query files and wasm from the grammar package:

```bash
just sync-vscode-assets
```

Then open this folder in VS Code and launch the extension host with the normal
extension development workflow.

The current minimum useful checks are:

- `Ecscript: Show Extension Status`
  - verifies that the extension can load the bundled parser wasm and highlight query
- `Ecscript: Show Syntax Tree`
  - parses the active `.ecs` document and opens its syntax tree as an S-expression

The extension currently registers a semantic tokens provider for `.ecs` files.
The first version is intentionally conservative and reuses the synced
`assets/queries/highlights.scm` query plus the generated wasm parser.

## Next steps

- improve token conflict resolution for overlapping captures
- consume `queries/injections.scm` for `cmd{ ... }`
- add incremental parsing instead of reparsing the entire document
