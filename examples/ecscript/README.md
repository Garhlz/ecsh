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
- `starship_prompt.ecs`：阶段 10 prompt adapter 示例，需要本机安装 `starship`
- `starship.toml`：配套 Starship 配置，展示 ecsh 传入的 shell、SHLVL、jobs、命令耗时和退出状态
- `git_complete.ecs`：阶段 10 completion adapter 示例
- `zoxide.ecs`：阶段 10 shell command adapter 示例，需要本机安装 `zoxide`
- `bind_accept_hint.ecs`：阶段 10 bind 示例，把 `Ctrl-E` 绑定为“接受当前补全提示”
- `bind_insert_template.ecs`：阶段 10 bind 示例，把 `Ctrl-G` 绑定为插入 `git status`
- `bind_history_search.ecs`：阶段 10 bind 示例，把 `Up` / `Down` 绑定为历史前缀搜索

在 `~/.ecshrc` 中启用配套 Starship 配置：

```ecs
set_env("STARSHIP_CONFIG", "/path/to/ecsh/examples/ecscript/starship.toml")
use /path/to/ecsh/examples/ecscript/starship_prompt.ecs as starship
starship.init()
```

在 `~/.ecshrc` 中启用 zoxide：

```ecs
use /path/to/ecsh/examples/ecscript/zoxide.ecs as zoxide
zoxide.init()
```

在 `~/.ecshrc` 中启用按键绑定示例：

```ecs
use /path/to/ecsh/examples/ecscript/bind_accept_hint.ecs as bind_accept_hint
bind_accept_hint.init()

use /path/to/ecsh/examples/ecscript/bind_insert_template.ecs as bind_insert_template
bind_insert_template.init()

use /path/to/ecsh/examples/ecscript/bind_history_search.ecs as bind_history_search
bind_history_search.init()
```

说明：
- `bind_accept_hint.ecs` 适合配合当前 completion 体验，`Ctrl-E` 会尝试接受提示文本。
- `bind_insert_template.ecs` 用于演示 `insert` 动作，按下 `Ctrl-G` 会在光标处插入 `git status`。
- `bind_history_search.ecs` 会把 `Up` / `Down` 改成基于当前前缀的历史搜索，而不是普通上一条/下一条历史。

预期失败脚本：
- 这些脚本用于固定当前语言边界，而不是回归成功行为。
- `boundary_shell_in_block.ecs`：ecscript block 内仍然不能直接写 shell 命令
- `boundary_range_value.ecs`：裸 `1..10` 不再是普通值表达式，应改用 `range(1, 10)`
