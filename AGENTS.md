# Agent 协作指南

## 适用范围

本仓库包含两条紧密相关的实现线：

- `ecsh`：用 Rust 编写的教学型类 Unix shell
- `ecscript`：随 `ecsh` 演进的小型脚本语言和解释器

除非某节另有说明，本文件适用于代码、测试、注释和文档修改。

## 事实源优先级

修改文档前必须先确认当前实现状态，不要从历史 TODO 或旧文档反推行为。

当前事实源按职责划分：

- `README.md`：项目入口，只保留快速启动、最短示例、文档导航和开发命令。
- `docs/README.md`：文档地图，说明每份文档的职责。
- `docs/status.md`：当前进度和边界的唯一事实源。
- `docs/shell-reference.md`：使用 `ecsh` 命令行时查 shell 语法、展开和 builtin 的事实源。
- `docs/ecscript-reference.md`：写 `.ecs` 时查语法、builtin、命令桥和扩展 API 的事实源。
- `docs/ecscript-manual.md`：实现级手册，面向 parser、evaluator、runtime、错误模型和 tree-sitter 维护。
- `docs/TODO.md`：当前可执行待办，不是当前行为事实源。
- `docs/roadmap.md`：后续演进路线，不是当前行为事实源。
- `docs/design-archive.md`：历史设计归档，不是当前行为事实源。
- `examples/ecscript/README.md`：示例脚本说明。

写当前行为时，应优先对照代码：

- shell builtin：`src/builtin.rs::BUILTIN_NAMES`
- ecscript builtin：`src/ecscript/builtin/mod.rs::lookup_builtin`
- builtin 名称和值模型：`src/ecscript/value.rs::Builtin`
- shell 扩展点：`src/extensions.rs` 和 `src/ecscript/builtin/mod.rs`
- callable 元信息：`src/specs.rs`
- 顶层分派：`src/main.rs` 和 `src/ecscript/top_level.rs`
- 命令桥：`src/executor/command_value.rs`
- 语法覆盖：parser / lexer 测试、`tests/smoke.rs`、`tests/ecscript_cli.rs`

没有代码或测试支撑的能力，不要在 reference 或 status 中写成已支持。

## 文档写作风格

项目文档应使用稳定的工程文档语气，不使用提案式或聊天式语气。

推荐写法：

- 使用客观陈述，避免第一人称。
- 直接说明当前行为、边界和状态。
- 保持措辞简洁、明确、可检索。
- 优先写“已实现 / 未实现 / 当前边界 / 下一步入口”。
- 明确区分进度文档、用户参考、实现手册和历史设计备忘。

避免写法：

- “我认为”
- “建议”
- “自然下一步”
- “收益最高”
- 其他主观、营销或劝说式表述

文档职责约束：

- README 不承载长篇阶段状态。
- `docs/status.md` 不展开历史设计推演。
- `docs/ecscript-reference.md` 不写 AST、parser 内部和 evaluator 细节。
- `docs/ecscript-manual.md` 可以详细，但不要替代 status/reference 的事实源职责。
- `docs/TODO.md` 只保留当前可执行待办。
- `docs/roadmap.md` 保留后续候选方向，但不能写成已实现。
- `docs/design-archive.md` 保留历史推演和废弃方案，但不能覆盖 status/reference。

## 注释风格

这是教学型 Rust shell 项目。涉及 Rust 语法、Unix 进程模型、文件描述符所有权、终端控制、pipe 关闭、`fork` / `execvp` / `dup2` 等边界时，注释可以比生产项目更详细。

修改注释时：

- 保留有学习价值的说明，不要为了简洁删除有效解释。
- 优先解释“为什么这样组织”，不要只复述下一行代码。
- 可以解释 `Result`、`Option`、`?`、`while let`、`OwnedFd` 等 Rust 模式，只要它们对理解实现有帮助。
- 保留 fd 保存/恢复、pipe 所有权、子进程退出路径、builtin 执行等资源敏感代码附近的详细说明。
- 改掉不准确、过度口语化或过时的注释。
- 避免只重复显而易见代码的空注释。

## 重构风格

偏好简单、局部、可读的控制流。只有满足以下条件之一时才提取 helper：

- 明显降低调用方复杂度。
- 封装真实重复逻辑。
- 命名一个重要 shell / Unix 概念。
- 隔离资源敏感行为，例如 fd 保存/恢复、pipe 所有权、子进程退出路径或 `execvp` 设置。

如果 helper 只使用一次，而且没有保护资源边界或提升执行流可读性，优先让逻辑留在调用点附近。短 pipeline 步骤尤其如此，因为绑定 pipe fd、应用重定向、关闭继承 fd、运行 builtin 或 `execvp` 的局部顺序很重要。

## 测试风格

优先把行为测试放在 `tests/` 目录中，只要它们可以通过 public crate API 表达。这让 lexer/parser 测试更接近下游调用方式，也解释了为什么项目同时有 `src/lib.rs` 和交互式 `src/main.rs`。

测试组织：

- 纯逻辑使用聚焦单测，例如 tokenization、parser、值求值。
- 需要真实二进制行为的 shell 流程使用小型 smoke tests。
- 会修改进程级状态的测试需要格外小心，例如 cwd、env、umask、fd、terminal state。
- 跨平台路径断言不要写死 `/tmp`；优先使用 `std::env::temp_dir().canonicalize()` 后的结果。

提交代码前优先运行：

```text
cargo fmt --check
cargo check
cargo test
```

纯文档修改至少检查：

```text
just docs-check
```

修改 `ecscript` reference 时，还要对照：

- `src/ecscript/builtin/mod.rs::lookup_builtin`
- `src/builtin.rs::BUILTIN_NAMES`
- `src/specs.rs`

## 提交格式

使用 Conventional Commits。必要时添加 scope。

推荐格式：

```text
type(scope): concise summary
```

推荐类型：

- `feat`
- `fix`
- `refactor`
- `docs`
- `test`
- `chore`

推荐 scope：

- `shell`
- `ecscript`
- `lexer`
- `parser`
- `executor`
- `docs`
- `tests`

推荐提交正文：

```text
type(scope): concise summary

- State the main code or document changes
- Mention newly supported behavior when relevant
- Mention notable compatibility, diagnostics, or test updates
```

如果提交对应某个计划阶段，应在 subject 或正文第一条中明确阶段名称，方便 `git log` 作为进度记录使用。

提交正文的 bullet 列表应连续书写，bullet 行之间不要插入空行。

提交信息末尾不要添加 agent 签名、生成器署名或类似 `Co-Authored-By` 的自动协作签名。
