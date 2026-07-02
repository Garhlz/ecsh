# ecscript Smoke Scripts

这些脚本用于覆盖当前 `ecscript` / `ecsh file.ecs` 的主要成功路径和边界失败路径。

语法、builtin 和扩展 API 的当前事实源见 [../../docs/ecscript-reference.md](../../docs/ecscript-reference.md)；项目进度见 [../../docs/status.md](../../docs/status.md)。

运行方式：

```bash
cargo run --bin ecscript -- examples/ecscript/loop_and_accumulate.ecs
cargo run --bin ecsh -- examples/ecscript/loop_and_accumulate.ecs
```

当前成功脚本如下。这些脚本现在也有对应的自动化文件级测试。

| 推荐顺序 | 示例 | 覆盖能力 | 外部依赖 |
|----------|------|----------|----------|
| 1 | `loop_and_accumulate.ecs` | `for` 区间、`while`、数组、复合赋值 | 无 |
| 2 | `closures_and_state.ecs` | 闭包、捕获状态、函数值、数组 | 无 |
| 3 | `objects_and_collections.ecs` | 嵌套对象、字段/索引修改、`keys` / `values` | 无 |
| 4 | `env_and_json.ecs` | `env()`、`range()`、`insert` / `remove`、`to_json` | 无 |
| 5 | `std_iter_draft.ecs` | `\|>`、`map` / `filter` / `reduce` / `any` / `join`、`slice` | 无 |
| 6 | `git_complete.ecs` | completion adapter | `git` 用于实际补全目标 |
| 7 | `bind_insert_template.ecs` | `bind` + `insert` action | 无 |
| 8 | `bind_history_search.ecs` | `bind` + 历史前缀搜索 action | 无 |
| 9 | `fzf_history.ecs` | `bind` + `set_line` action + `ctx.history` | `fzf` |
| 10 | `zoxide.ecs` | shell command adapter、`register_command`、`set_cwd` | `zoxide` |
| 11 | `starship_prompt.ecs` | prompt adapter | `starship` |
| 12 | `starship.toml` | Starship 配置，展示 shell、SHLVL、jobs、命令耗时和退出状态 | `starship` |

在 `~/.ecshrc` 中启用配套 Starship 配置：

```ecs
set_env("STARSHIP_CONFIG", "/path/to/ecsh/examples/ecscript/starship.toml")
use /path/to/ecsh/examples/ecscript/starship_prompt.ecs as starship
starship.init()
```

修改完 `~/.ecshrc` 后，使用 `reload_rc` 重新加载；不要再用 `source ~/.ecshrc`。

在 `~/.ecshrc` 中启用 zoxide：

```ecs
use /path/to/ecsh/examples/ecscript/zoxide.ecs as zoxide
zoxide.init()
```

在 `~/.ecshrc` 中启用按键绑定示例：

```ecs
use /path/to/ecsh/examples/ecscript/bind_insert_template.ecs as bind_insert_template
bind_insert_template.init()

use /path/to/ecsh/examples/ecscript/bind_history_search.ecs as bind_history_search
bind_history_search.init()

use /path/to/ecsh/examples/ecscript/fzf_history.ecs as fzf_history
fzf_history.init()
```

说明：

- `bind_insert_template.ecs` 用于演示 `insert` 动作，按下 `Ctrl-G` 会在光标处插入 `git status`。
- `bind_history_search.ecs` 会把 `Up` / `Down` 改成基于当前前缀的历史搜索，而不是普通上一条/下一条历史。
- `fzf_history.ecs` 使用 `set_line` action + `ctx.history` 实现 Ctrl-R fzf 历史选择器。`ctx.history` 会包含 `~/.ecsh_history` 中已加载的历史和当前会话新输入的命令。需要本机安装 `fzf`。

预期失败脚本：
- 这些脚本用于固定当前语言边界，而不是回归成功行为。
- `boundary_shell_in_block.ecs`：ecscript block 内仍然不能直接写 shell 命令
- `boundary_range_value.ecs`：裸 `1..10` 不再是普通值表达式，应改用 `range(1, 10)`
