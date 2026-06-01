npm-install:
    cd packages/tree-sitter-ecscript && npm install
    cd packages/vscode-ecscript && npm install

test:
    cargo test --workspace
    cd packages/tree-sitter-ecscript && npm test

ts-generate:
    cd packages/tree-sitter-ecscript && npm run generate

sync-vscode: sync-vscode-assets

sync-vscode-assets:
    bash ./scripts/sync-vscode-assets.sh

vscode:
    just sync-vscode-assets
    cd packages/vscode-ecscript && npm run build

vsix:
    just vscode
    cd packages/vscode-ecscript && npx @vscode/vsce package

all:
    just test
    just vscode
