# TODO

这份文档只记录当前可执行待办，不描述已经完成的历史阶段，也不作为当前实现行为的事实源。

事实源：

- 当前进度和边界：[status.md](status.md)
- `ecsh` shell 用户参考：[shell-reference.md](shell-reference.md)
- `ecscript` 用户参考：[ecscript-reference.md](ecscript-reference.md)
- `ecscript` 实现手册：[ecscript-manual.md](ecscript-manual.md)
- 历史设计长文：[design-archive.md](design-archive.md)
- 后续路线：[roadmap.md](roadmap.md)

## 当前优先级

### P0：阶段 11 收口

- 明确 `help(...)` 与 `type` / `which` 的职责边界。
- 明确 ecscript builtin 与 shell builtin 同名时的展示规则。
- 整理推荐 `.ecshrc` 组合片段，把 prompt、completion、bind、zoxide、starship 示例串成可读配置。
- 继续收口外部命令 adapter / help / completion provider 的最小协议。

### P1：文档和示例

- 为 `docs/ecscript-reference.md` 中的 builtin 表补齐签名和错误边界。
- 为 [shell-reference.md](shell-reference.md) 补充更多真实交互例子。
- 将 `examples/ecscript/` 按“语言基础 / 命令桥 / 交互扩展”分组。
- 评估是否为 `ecscript-reference.md` 生成 GitHub Pages 页面。

### P2：语言体验

- 设计函数 / lambda block body 的尾表达式返回值。
- 评估更完整字符串系统：字符串插值、多行字符串、更强 raw string 定界。
- 评估多层闭包自动透传捕获，消除 `() => () => x` 需要中间层显式引用的限制。

### P3：Shell 语义补完

- here-doc `<<`。
- glob 展开。
- subshell `()`。
- 更完整 job spec 和异步完成通知。
- `|&`、`!` 等执行语义增强。

## 维护规则

- 已完成事项移入 [status.md](status.md) 或对应 reference，不在本文保留长篇历史。
- 尚未设计清楚的想法移入 [roadmap.md](roadmap.md)，不要写成承诺。
- 历史推演和废弃方案移入 [design-archive.md](design-archive.md)。
- 新增用户可见行为时，同步更新 `status.md` 和对应 reference。
