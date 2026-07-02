# ecsh Shell 参考手册

这份文档面向使用 `ecsh` 命令行的人，描述当前 shell 语法、展开规则、builtin 和边界。项目进度见 [status.md](status.md)，`ecscript` 语言参考见 [ecscript-reference.md](ecscript-reference.md)。

## 运行方式

启动交互式 shell：

```bash
cargo run
```

执行 `.ecs` 文件：

```bash
cargo run -- script.ecs
```

交互式启动时，`ecsh` 会尝试加载 `~/.ecshrc`。`reload_rc` 会用新的脚本环境、扩展注册表和模块缓存重新加载该文件。

## 命令语法

外部命令按 program + argv 执行：

```bash
echo hello
printf "%s\n" value
```

支持的控制操作符：

| 语法 | 当前行为 |
|------|----------|
| `cmd1 \| cmd2` | pipeline |
| `cmd > file` | 覆盖写 stdout |
| `cmd >> file` | 追加写 stdout |
| `cmd < file` | 从文件读取 stdin |
| `cmd1 && cmd2` | 前一条成功时执行后一条 |
| `cmd1 \|\| cmd2` | 前一条失败时执行后一条 |
| `cmd1 ; cmd2` | 顺序执行 |
| `cmd &` | 后台执行 |

当前未实现 here-doc、glob 展开、subshell、`|&` 和 `!` 执行语义。

## Word 与引用

普通 word 会按 shell 规则切分为 argv。单引号内不做展开，双引号内支持 `$` 展开。

```bash
echo '$HOME'
echo "$HOME"
echo prefix-$HOME
```

反斜杠可用于保留特殊字符。`~` 会按当前用户 home 做基础展开；当前没有完整的 `~user` 解析。

## 展开

`ecsh` 当前支持以下运行时展开：

| 语法 | 当前行为 |
|------|----------|
| `$VAR` | 先查当前脚本作用域，再回退环境变量 |
| `${expr}` | 执行 `ecscript` 表达式，结果转成单个 shell word |
| `${...arr}` | 把数组展开为多个 argv |
| `$(cmd)` | 通过 `/bin/sh -c` 执行命令替换 |

示例：

```bash
echo $HOME
echo ${env("HOME")}
echo ${1 + 2}
echo ${...["a", "b", "c"]}
echo $(printf cmdsub)
```

## 顶层 ecscript

交互式 `ecsh` 顶层会在 shell 命令和 `ecscript` 输入之间分派。`source` / `.` 会在当前 shell 的脚本环境中执行 `.ecs` 文件。

```bash
source config.ecs
. config.ecs
reload_rc
```

关系：

- 交互式顶层、`source` 和 `.` 共享当前 shell 的脚本环境。
- `ecsh file.ecs` 使用新的脚本根环境。
- `reload_rc` 会重置交互扩展注册表和模块缓存。

## 内置命令

| Builtin | 当前行为 |
|---------|----------|
| `help` | 打印 shell builtin 帮助 |
| `exit` | 退出 shell |
| `cd` | 修改当前工作目录 |
| `pwd` | 打印当前工作目录 |
| `env` | 打印环境变量 |
| `export` | 设置环境变量，形式为 `KEY=value` |
| `unset` | 删除环境变量 |
| `clear` | 清屏 |
| `status` | 打印上一条命令状态 |
| `jobs` | 列出后台和停止的作业 |
| `fg` | 将作业切到前台 |
| `bg` | 在后台恢复作业 |
| `alias` | 定义或展示 alias |
| `unalias` | 删除 alias |
| `trap` | 注册 `EXIT` / `INT` trap |
| `type` | 说明命令名如何解析 |
| `which` | 打印解析到的命令路径或 shell 来源 |
| `history` | 显示命令历史 |
| `source` | 在当前 shell 脚本环境中执行 `.ecs` 文件 |
| `.` | `source` 的别名 |
| `reload_rc` | 用新的脚本环境重新加载 `~/.ecshrc` |

只有纯输出型 builtin 可进入 pipeline：`help`、`pwd`、`env`、`status`。

## Alias 与命令解析

`alias` 定义 shell 命令别名，`unalias` 删除别名。

```bash
alias ll='ls -la'
unalias ll
```

`type NAME` 用于查看名字来源，例如 alias、shell builtin、脚本注册命令或外部命令。`which NAME` 打印可执行路径或 shell 解析来源。

## 作业控制

后台作业使用 `&` 启动：

```bash
sleep 10 &
jobs
fg %1
bg %1
```

当前 job control 是最小实现：支持前台进程组、终端控制权切换和 `jobs` / `fg` / `bg` 主路径；job spec、异步完成通知和完整 POSIX 语义仍有限。

## ecscript 扩展集成

`.ecshrc` 或被 `source` 的 `.ecs` 文件可以注册交互扩展：

- `prompt(func)`：自定义 prompt。
- `complete(name, func)`：注册补全 handler。
- `bind(key, func)`：注册按键绑定。
- `hook(name, func)`：注册生命周期 hook。
- `register_command(name, func)`：注册顶层脚本命令。
- `set_cwd(path)`：修改当前 shell 工作目录。

这些 API 的参数和返回协议见 [ecscript-reference.md](ecscript-reference.md#扩展-api)。

当前脚本注册命令只支持顶层前台执行，不支持后台、pipeline 和重定向，也不能覆盖 shell builtin。

## 当前边界

- 不是完整 POSIX shell。
- 未实现 here-doc `<<`。
- 未实现 glob 展开。
- 未实现 subshell `()`。
- 未实现 `|&`、`!` 等执行语义增强。
- job spec 和异步完成通知仍然有限。
- 命令替换之外的完整 shell 展开规则尚未实现。
