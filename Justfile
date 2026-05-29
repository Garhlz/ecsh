test:
    cargo test --workspace
    cd packages/tree-sitter-ecscript && npm test

ts-generate:
    cd packages/tree-sitter-ecscript && npm run generate

sync-vscode:
    bash ./scripts/sync-vscode-assets.sh

sync-vscode-assets:
    bash ./scripts/sync-vscode-assets.sh

vscode:
    just sync-vscode-assets
    cd packages/vscode-ecscript && npm run build

all:
    just test
    just vscode
