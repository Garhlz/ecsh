# ecsh

`ecsh` 是 **Elaine & Cornelia's shell**，一个用 Rust 编写的教学型类 Unix shell。

仓库当前包含两条主线：

- `ecsh`：交互式 shell，已具备外部命令、builtin、管道、重定向、条件执行、后台作业和最小 job control
- `ecscript`：与 `ecsh` 一起演进的小型脚本语言，已具备独立解释器、REPL、控制流、函数、闭包、容器、命令桥和交互扩展点

当前实现已经推进到阶段 11 前半：shell 基础、`ecscript` 核心、命令桥、值流、模块 MVP、shell 扩展点、callable spec / help / hover 已接通；阶段 11 后半和阶段 12 仍未完成。权威进度见 [docs/status.md](docs/status.md)。

## Quick Start

构建：

```bash
cargo build
```

运行 `ecsh`：

```bash
cargo run
```

运行 `ecscript`：

```bash
cargo run --bin ecscript --
cargo run --bin ecscript -- script.ecs
cargo run --bin ecscript -- -e 'println(1 + 2);'
```

运行测试：

```bash
cargo test
```

## Examples

`ecsh` 命令：

```bash
echo hello
echo prefix-$HOME
echo ${env("HOME")}
echo ${1 + 2}
echo $(printf cmdsub)
echo hello | grep h
pwd > out.txt
cat < out.txt
true && echo ok
false || echo fallback
sleep 5 &
jobs
```

`ecscript` 脚本：

```ecs
let xs = range(1, 5);

func fact(n) {
    if n <= 1 {
        return 1;
    }
    return n * fact(n - 1);
}

println(fact(5));
println(xs |> filter((x) => x > 2) |> map((x) => x * 10) |> join(","));
println(text(cmd{ printf "hello" }));
println(from_json(text(cmd{ printf "{\"ok\":true}" })).ok);
```

模块 MVP：

```ecs
// foo.ecs
let hidden = 1
pub let visible = hidden + 1

// main.ecs
use ./foo.ecs as foo
println(foo.visible)
```

## Documentation

- [docs/README.md](docs/README.md)：文档地图，说明每份文档的职责
- [docs/status.md](docs/status.md)：当前进度的唯一事实源
- [docs/shell-reference.md](docs/shell-reference.md)：使用 `ecsh` 命令行时查 shell 语法、展开和 builtin
- [docs/ecscript-reference.md](docs/ecscript-reference.md)：写 `.ecs` 时查语法、builtin 和扩展 API
- [docs/ecscript-manual.md](docs/ecscript-manual.md)：`ecscript` 实现级手册
- [docs/TODO.md](docs/TODO.md)：当前可执行待办
- [docs/roadmap.md](docs/roadmap.md)：后续演进路线
- [docs/design-archive.md](docs/design-archive.md)：历史设计归档，不是当前行为事实源
- [examples/ecscript/README.md](examples/ecscript/README.md)：示例脚本说明

## Repository Layout

```text
src/
  main.rs            # ecsh 主循环和顶层分派
  lexer.rs           # shell lexer
  parser.rs          # shell parser
  executor/          # 执行、fork/exec、作业控制、运行时展开、命令桥
  ecscript/          # ecscript 词法、解析、求值、环境、值模型
  bin/ecscript.rs    # ecscript CLI / REPL
tests/
  lexer.rs
  parser.rs
  smoke.rs
  ecscript_cli.rs
docs/
  README.md
  status.md
  shell-reference.md
  ecscript-reference.md
  ecscript-manual.md
  TODO.md
  roadmap.md
  design-archive.md
packages/
  tree-sitter-ecscript/
  vscode-ecscript/
```

## Development Commands

本项目使用 [just](https://github.com/casey/just) 管理多语言 monorepo 开发流程：

```bash
just npm-install       # 首次开发：安装 tree-sitter 和 VS Code 插件的 npm 依赖
just test              # cargo test --workspace + tree-sitter corpus
just docs-check        # 检查文档链接和 reference / 代码名称漂移
just ts-generate       # 重新生成 tree-sitter parser
just sync-vscode       # 同步 tree-sitter wasm/query/spec 到 VS Code 插件 assets/
just vscode            # sync-vscode + tsc 编译 VS Code 插件
just vsix              # vscode + 打包 VSIX
just all               # 一条龙
```

VS Code 插件 assets 由 `scripts/sync-vscode-assets.sh` 从 `packages/tree-sitter-ecscript` 和 `src/specs.rs` 同步，不应手动维护生成产物。
