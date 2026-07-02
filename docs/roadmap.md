# Roadmap

这份文档记录后续演进方向。它不是当前行为事实源；当前已经实现的能力以 [status.md](status.md) 为准。

## 阶段 11：调用元信息与外部命令适配层

当前阶段已经完成统一 `Spec` / `CallableSpec`、`help()`、`help("name")`、`builtins()`、`commands()`、`extensions()`、shell builtin spec 和 VS Code hover 接线。

剩余目标：

- 明确 `help(...)`、`type`、`which` 的分工。
- 明确 ecscript builtin 与 shell builtin 同名时的展示和文档规则。
- 把示例目录整理成推荐 `.ecshrc` 组合片段。
- 设计外部命令 adapter / help / completion provider 的最小协议。

## 阶段 12：Shell 语义补完

目标是补齐传统 shell 使用中最明显的语义缺口，但仍不以完整 POSIX 兼容为硬目标。

候选能力：

- here-doc `<<`
- glob 展开
- subshell `()`
- `|&`
- `!`
- 更完整 job spec
- 异步完成通知

## ecscript 语言体验

候选能力：

- 函数 / lambda block body 的尾表达式返回值。
- 字符串插值。
- 多行字符串。
- 更强 raw string 定界。
- 多层闭包自动透传捕获。
- 模块搜索路径。
- 命名导入。
- `pub use`。

## 标准库和示例

候选方向：

- `std.iter`：`take`、`drop`、`contains` 等集合工具。
- `std.str`：字符串处理工具。
- `std.path`：跨平台路径工具。
- `std.json`：JSON 辅助函数。
- `std.proc`：命令桥辅助函数。
- 示例分组：语言基础、命令桥、交互扩展、外部工具 adapter。

## 文档和发布

候选方向：

- 使用 `ecscript-reference.md` 生成 GitHub Pages。
- 为 builtin / shell builtin / extension spec 生成可检索索引。
- 将 `docs-check` 纳入提交前检查或 CI。
- 为 VS Code 插件补充用户安装和调试文档。
