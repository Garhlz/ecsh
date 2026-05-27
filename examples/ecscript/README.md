# ecscript Smoke Scripts

这些脚本用于覆盖当前 `ecscript` / `ecsh file.ecs` 的主要成功路径和边界失败路径。

运行方式：

```bash
cargo run --bin ecscript -- examples/ecscript/loop_and_accumulate.ecs
cargo run --bin ecsh -- examples/ecscript/loop_and_accumulate.ecs
```

当前分类如下：

成功脚本：
- 这些脚本现在也有对应的自动化文件级测试。
- `loop_and_accumulate.ecs`：`for` 区间、`while`、数组、复合赋值
- `closures_and_state.ecs`：闭包、捕获状态、函数值、数组
- `objects_and_collections.ecs`：嵌套对象、字段/索引修改、`keys` / `values`
- `env_and_json.ecs`：`env()`、`range()`、`insert` / `remove`、`to_json`
- `std_iter_draft.ecs`：`|>`、`map/filter/reduce/any/join`、`slice`，作为阶段 9 的标准库草案

预期失败脚本：
- 这些脚本用于固定当前语言边界，而不是回归成功行为。
- `boundary_shell_in_block.ecs`：ecscript block 内仍然不能直接写 shell 命令
- `boundary_range_value.ecs`：裸 `1..10` 不再是普通值表达式，应改用 `range(1, 10)`
