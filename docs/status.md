# 当前进度

这份文档是当前实现状态的唯一事实源。它只描述已经落地的能力、仍然存在的边界和下一步入口；当前待办见 [TODO.md](TODO.md)，后续路线见 [roadmap.md](roadmap.md)，历史设计推演保留在 [design-archive.md](design-archive.md)。

## 状态图例

| 状态 | 含义 |
|------|------|
| Done | 当前能力已接通，并有测试或示例覆盖 |
| Done with limits | 主路径已可用，但保留明确边界 |
| In Progress | 正在收口，部分能力已落地 |
| Not Started | 尚未开始实现 |
| Deferred | 有设计想法，但不在当前阶段推进 |

## 当前摘要

`ecsh` 已经从基础教学 shell 演进为“交互 shell + ecscript 脚本语言 + 命令桥 + 交互扩展点”的联合项目。当前完成到阶段 11 前半：shell 基础、`ecscript` 核心、命令桥、值流、模块 MVP、shell 扩展点、callable spec / help / hover 已接通。阶段 11 后半和阶段 12 仍未完成。

## Feature Matrix

| 领域 | 状态 | 当前事实 |
|------|------|----------|
| Shell 执行 | Done with limits | 外部命令、builtin、管道、重定向、`&&` / `||` / `;`、后台 `&`、`jobs` / `fg` / `bg` 已可用 |
| Shell 解析与诊断 | Done | 结构化 `ParseError`、续行读取、shell parse 错误定位、历史按完整命令写入已落地 |
| Shell 运行时展开 | Done | 支持 `$VAR`、`${expr}`、`${env("VAR")}`、`$(cmd)`、`${...arr}` |
| Shell 交互体验 | Done with limits | rustyline、历史、Tab 补全、alias / unalias、`trap EXIT|INT`、`type` / `which` / `history` 已可用 |
| ecscript 核心语言 | Done with limits | 表达式、语句、数组/对象、控制流、函数、lambda、闭包、模块 MVP、源码定位错误已可用 |
| ecscript CLI / REPL / 文件模式 | Done | `ecscript` REPL、stdin、文件执行、`-e` 已可用；`ecsh file.ecs` 走文件脚本路径 |
| 顶层集成 | Done | 交互式 `ecsh` 顶层可按输入分派到 shell 或 ecscript；`source` / `.` 共享当前 shell 的 script env |
| 命令桥 | Done with limits | `cmd{}`、`command(...)`、`run`、`capture`、`text`、`lines`、`with_env`、`with_cwd` 已可用 |
| text/value / JSON bridge | Done | `stdin`、`read_lines`、`write_lines`、`from_json`、`to_json` 已接通 |
| 值流和集合 builtin | Done with limits | `|>`、`map`、`filter`、`reduce`、`each`、`any`、`all`、`find`、`join`、`slice` 已可用；当前是 eager Array 模型 |
| 模块系统 | Done with limits | `pub let` / `pub func`、`use ./foo.ecs as foo`、模块缓存、循环导入检测已可用 |
| Shell 扩展点 | Done with limits | `hook`、`prompt`、`complete`、`bind`、`register_command`、`set_cwd` 已可用 |
| Callable specs / help / introspection | In Progress | `Spec` / `CallableSpec`、`help()`、`help("name")`、`builtins()`、`commands()`、`extensions()` 已接通；同名展示边界和 adapter 元数据仍待收口 |
| 编辑器工具链 | In Progress | tree-sitter grammar、query、VS Code semantic tokens / folding / symbols / diagnostics / spec-backed hover 已具备基础能力 |
| Stage 12 shell 语义补完 | Not Started | here-doc、glob、subshell、更完整 job semantics 尚未实现 |

## 已完成能力

### Shell

- 外部命令执行：`fork`、`execvp`、`waitpid`
- 内置命令：`help`、`exit`、`cd`、`pwd`、`env`、`export`、`unset`、`clear`、`status`、`jobs`、`fg`、`bg`、`alias`、`unalias`、`trap`、`type`、`which`、`history`、`source` / `.`、`reload_rc`
- 管道和重定向：`|`、`<`、`>`、`>>`
- 控制操作符：`&&`、`||`、`;`、行尾 `&`
- 纯输出型 builtin 可进入管道：`help`、`pwd`、`env`、`status`
- 前台进程组、终端控制权切换、最小 job control
- 交互补全、历史记录、续行读取、parse error 源码定位

### ShellWord 展开

- `$VAR`：脚本作用域优先，再回退环境变量
- `${expr}`：执行 ecscript 表达式
- `${env("VAR")}`：显式读取环境变量
- `$(cmd)`：通过 `/bin/sh -c` 做命令替换
- `${...arr}`：把数组展开成多个 argv

### ecscript

- 词法、Pratt 表达式 parser、语句 parser、evaluator
- `let`、赋值、复合赋值、block
- 数组、对象、字段访问、索引访问
- `if`、`while`、`for in`
- `break`、`continue`、`return`
- 命名函数、lambda、闭包和自由变量捕获
- 原始字符串 `r"..."`
- 模块 MVP：`pub let`、`pub func`、`use ./foo.ecs as foo`
- 模块缓存和循环导入检测
- parse/runtime 错误的源码定位格式化

### 命令桥与交互扩展

- 命令值：`cmd{ ... }`、`command(...)`
- 执行/捕获：`run`、`capture`、`text`、`lines`
- 派生命令值：`with_env`、`with_cwd`
- 文本和 JSON：`stdin`、`read_lines`、`write_lines`、`from_json`、`to_json`
- 交互扩展：`hook`、`prompt`、`complete`、`bind`、`register_command`、`set_cwd`
- introspection：`help`、`builtins`、`commands`、`extensions`

## 进行中

### 阶段 11：调用元信息与外部命令适配层

已落地：

- 统一 callable 元信息表：`Spec` / `CallableSpec`
- spec 覆盖 ecscript builtin、shell extension、shell builtin
- `help()`：按 kind / category 返回 overview 文本
- `help("name")`：返回匹配 callable 的签名、摘要、说明和示例
- `builtins()`：返回 ecscript builtin 名称数组
- `extensions()`：返回 shell extension 名称数组
- `commands()`：返回当前 shell 中可见命令来源数组，元素形如 `{ name, kind }`
- `type` / `which` 已能区分 alias、shell builtin、registered command、external command
- VS Code hover 优先显示统一 spec 文档

仍待收口：

- `help("name")` 与 `type` / `which` 对同名 ecscript builtin / shell builtin 的职责边界文档
- 示例包目录和推荐 `.ecshrc` 组合片段
- 更通用的外部命令 adapter / help / completion provider

## 当前边界

### Shell 边界

- 不是完整 POSIX shell
- 未实现 here-doc `<<`
- 未实现 glob 展开
- 未实现 subshell `()`
- 未实现 `|&`、`!` 等执行语义增强
- job spec 和异步完成通知仍然有限
- 命令替换之外的完整 shell 展开规则尚未实现

### ecscript 边界

- `if` 是语句，不是表达式
- block 没有值语义，暂不支持尾表达式返回值
- `1..10` / `1..=10` 只在 `for` 语句中合法；普通值使用 `range(start, end)`
- 闭包捕获只自动传一层，多跳闭包需要中间层显式引用
- 没有字符串插值
- 没有多行字符串
- 模块没有搜索路径、命名导入和 `pub use`

### 命令桥边界

- `cmd{ ... }` 支持单命令和 pipeline 子集
- `cmd{ ... }` 不支持 `&&` / `||` / `;` / `&`
- pipeline 中的 shell builtin 还不是完整命令桥能力
- `run` / `capture` / `text` / `lines` / `with_env` / `with_cwd` 要求 shell-backed 执行上下文
- 独立 `ecscript` 解释器和 `ecsh file.ecs` 文件脚本路径下，命令桥执行函数目前不可用

### 扩展点边界

- `register_command` 注册的脚本命令只支持顶层前台执行
- 脚本命令不支持后台、管道和重定向
- `register_command` 不能覆盖 shell builtin
- `complete` 候选结构仍是最小对象协议

## 后续入口

1. 收口阶段 11 文档边界：明确 `help(...)` 解释接口语义，`type` / `which` 解释名字解析来源。
2. 整理 examples：把 prompt、completion、bind、zoxide、starship 等示例组织成更清楚的推荐配置片段。
3. 设计外部命令 adapter/provider：补齐 help/completion 元数据，而不改变基础 `execvp` 模型。
4. 进入阶段 12 shell 语义补完：here-doc、glob、subshell、更完整 job semantics。

## 工程和工具链状态

- Rust workspace 测试入口：`cargo test`
- monorepo 总测试入口：`just test`
- 文档漂移检查入口：`just docs-check`
- tree-sitter grammar 位于 `packages/tree-sitter-ecscript`
- VS Code 插件位于 `packages/vscode-ecscript`
- VS Code assets 由 `scripts/sync-vscode-assets.sh` 同步，不手动维护生成产物

## 状态核对入口

更新本页时应优先对照这些代码位置，避免从历史 TODO 或旧阶段说明反推当前行为：

- shell builtin 名称：`src/builtin.rs::BUILTIN_NAMES`
- shell builtin 行为：`src/builtin.rs`
- shell 解析和执行：`src/lexer.rs`、`src/parser.rs`、`src/executor/`
- 顶层 shell / ecscript 分派：`src/main.rs`、`src/ecscript/top_level.rs`
- `ecscript` builtin 名称：`src/ecscript/builtin/mod.rs::lookup_builtin`
- `ecscript` 值模型和 callable 名称：`src/ecscript/value.rs`
- 命令桥：`src/executor/command_value.rs`
- 交互扩展点：`src/extensions.rs`
- callable 元信息和 hover/help 来源：`src/specs.rs`
- 行为测试：`tests/lexer.rs`、`tests/parser.rs`、`tests/smoke.rs`、`tests/ecscript_cli.rs`
