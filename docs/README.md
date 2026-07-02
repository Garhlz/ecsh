# ecsh 文档地图

这份文档说明仓库内各文档的职责。遇到冲突时，以这里列出的事实源优先级为准。

## 先读哪份

| 目标 | 文档 |
|------|------|
| 第一次打开仓库，想知道项目是什么、怎么运行 | [../README.md](../README.md) |
| 想确认当前完成了什么、还缺什么 | [status.md](status.md) |
| 想使用 `ecsh` 命令行、查 shell 语法和 builtin | [shell-reference.md](shell-reference.md) |
| 想写 `.ecs` 脚本、查语法和 builtin | [ecscript-reference.md](ecscript-reference.md) |
| 想维护 parser / evaluator / runtime / tree-sitter | [ecscript-manual.md](ecscript-manual.md) |
| 想看当前可执行待办 | [TODO.md](TODO.md) |
| 想看后续演进路线 | [roadmap.md](roadmap.md) |
| 想看历史设计长文和已废弃推演 | [design-archive.md](design-archive.md) |
| 想看可运行示例 | [../examples/ecscript/README.md](../examples/ecscript/README.md) |

## 事实源约定

- 当前进度和边界：以 [status.md](status.md) 为准。
- `ecsh` shell 语法、展开和 builtin：以 [shell-reference.md](shell-reference.md) 为准。
- `ecscript` 语法和用户可见 API：以 [ecscript-reference.md](ecscript-reference.md) 为准。
- 实现细节和运行时语义：以 [ecscript-manual.md](ecscript-manual.md) 为准。
- 当前待办：保留在 [TODO.md](TODO.md)，但它不是当前行为的事实源。
- 后续路线：保留在 [roadmap.md](roadmap.md)，但它不是当前行为的事实源。
- 历史设计归档：保留在 [design-archive.md](design-archive.md)，不得覆盖当前 status/reference。

## 文档维护规则

- README 只保留快速入口，不复制长篇阶段状态。
- 新功能落地后，先更新 `status.md`，再按影响面更新 `shell-reference.md` 或 `ecscript-reference.md`，最后按需补充 manual。
- 如果功能还没有代码或测试支撑，不要在 reference 中写成已支持。
- TODO 只记录当前待办，历史阶段和废弃方案进入 `design-archive.md`。
- 提交前可运行 `just docs-check` 检查文档链接、旧进度措辞和 builtin 名称覆盖。
