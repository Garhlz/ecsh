#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TREE_SITTER_CLI="$ROOT/packages/tree-sitter-ecscript/node_modules/.bin/tree-sitter"

mkdir -p "$ROOT/packages/vscode-ecscript/assets/queries"

cp "$ROOT/packages/tree-sitter-ecscript/queries/"*.scm \
   "$ROOT/packages/vscode-ecscript/assets/queries/"

if [[ ! -x "$TREE_SITTER_CLI" ]]; then
  echo "missing tree-sitter CLI at $TREE_SITTER_CLI; run npm install in packages/tree-sitter-ecscript first" >&2
  exit 1
fi

WASM="$ROOT/packages/vscode-ecscript/assets/tree-sitter-ecscript.wasm"
GRAMMAR="$ROOT/packages/tree-sitter-ecscript/grammar.js"
SCANNER="$ROOT/packages/tree-sitter-ecscript/src/scanner.c"

if [[ ! -f "$WASM" ]] || [[ "$GRAMMAR" -nt "$WASM" ]] || [[ "$SCANNER" -nt "$WASM" ]]; then
  "$TREE_SITTER_CLI" build --wasm \
    "$ROOT/packages/tree-sitter-ecscript" \
    -o "$WASM"
else
  echo "tree-sitter wasm is up to date; skipping rebuild" >&2
fi
