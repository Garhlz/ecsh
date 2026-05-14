# ecsh

`ecsh` 是 **Elaine & Cornelia's shell**，一个使用 Rust 实现的教学型类 Unix
shell，主要用于操作系统实验练习。

项目目标是用尽量直接的代码练习 shell 的核心执行模型：读取输入、解析命令、
处理内置命令、`fork` 子进程、在子进程中 `execvp` 目标程序，并由父进程
`waitpid` 等待结束。

## 功能概览

- 外部命令执行：`fork`、`execvp`、`waitpid`
- 内置命令：`help`、`exit`、`cd`、`pwd`、`env`、`export`、`unset`、`clear`、`status`
- 两行 prompt：显示 `[ecsh] user@host:cwd` 和上一条命令的非 0 状态码
- 启动欢迎页：交互式启动时打印欢迎词并展示内置命令帮助
- 交互式输入：使用 `rustyline` 支持命令历史、方向键导航和基础行编辑
- lexer + parser：支持单引号、双引号、变量展开和操作符 token
- 变量展开：`$?`、`$NAME`、`${NAME}`，以及 `prefix-$HOME`、`$HOME/file` 这类词内拼接
- 管道：支持标准 Unix 管道 `|`
- 重定向：支持 `<`、`>`、`>>`，操作符可以不依赖空白分隔
- 条件执行：支持 `&&` 和 `||`
- 命令序列：支持 `;` 顺序执行多条命令
- 普通内置命令支持临时重定向，执行结束后恢复 shell 的标准输入输出
- 管道中支持 `help`、`pwd`、`env` 这类纯输出型内置命令
- 统一错误输出，并保留实验要求的命令生命周期提示

## 快速开始

构建并运行：

```bash
cargo run
```

可以尝试：

```bash
help
pwd
cd /tmp
echo "$HOME"
echo prefix-$HOME
echo hello | grep h
pwd > pwd.txt
cat < pwd.txt
echo done >> pwd.txt
cat < pwd.txt | grep done > result.txt
true && echo ok
false || echo fallback
status
exit
```

## 演示命令

下面这组命令适合演示，基本覆盖当前版本的主要功能。

### 基础命令和内置命令

```bash
help
pwd
cd /tmp
pwd
status
```

### 外部命令和错误处理

```bash
echo hello
ls
not-exist-command
status
echo $?
```

### 环境变量和变量展开

```bash
export ECSH_DEMO=hello
echo $ECSH_DEMO
echo prefix-$ECSH_DEMO
echo ${ECSH_DEMO}/path
env | grep ECSH_DEMO
unset ECSH_DEMO
echo $ECSH_DEMO
```

### 引号和操作符字面量

```bash
echo "hello world"
echo '$HOME'
echo "$HOME"
echo "a|b && c > d"
echo 'a;b'
```

### 管道

```bash
echo hello | grep h
printf "a\nb\nc\n" | grep b | wc -l
pwd | cat
env | grep PATH
```

### 重定向

```bash
pwd > ecsh_demo.txt
cat < ecsh_demo.txt
echo done >> ecsh_demo.txt
cat < ecsh_demo.txt | grep done > ecsh_result.txt
cat ecsh_result.txt
```

### 条件执行和命令序列

```bash
true && echo ok
false && echo should-not-print
false || echo fallback
true || echo should-not-print
echo first; echo second; pwd
false && echo no; echo yes
```

### 交互体验

```text
按 ↑ / ↓ 浏览历史命令
按 ← / → 移动光标
按 Ctrl-A / Ctrl-E 跳到行首/行尾
按 Ctrl-C 取消当前输入行
按 Ctrl-D 结束输入并退出 shell
```

## 当前边界

`ecsh` 不是完整 POSIX shell。当前暂不支持：

- 反斜杠转义
- 命令替换
- here-doc `<<`
- 单个 `&` 后台执行
- glob 展开
- 完整的 `${...}` 参数展开语法
- 完整作业控制和前台进程组切换

管道中的内置命令也仍是简化实现：目前只支持纯输出型内置命令进入管道；
`cd`、`export`、`unset`、`exit`、`clear` 这类会改变 shell 状态或强交互行为的
内置命令暂不支持出现在管道中。

交互式历史记录保存在 `~/.ecsh_history`。当 `ecsh` 被管道或测试程序驱动时，
会退回普通按行读取模式，因此 `printf 'echo hi\nexit\n' | cargo run` 这类用法
仍然可用。

## 项目结构

```text
src/
  lib.rs           # 库 crate 入口，供集成测试复用核心模块
  main.rs          # 交互式主循环
  types.rs         # 命令、管道、解析结果和执行状态类型
  input.rs         # rustyline 和普通 read_line 输入层
  lexer.rs         # 输入行到 token 流
  parser.rs        # token 流到 ParsedLine
  prompt.rs        # prompt 构造与着色
  builtin.rs       # 内置命令
  executor.rs      # 外部命令、管道、fork/exec/wait
  redirection.rs   # 重定向与 fd 保存恢复
  diagnostics.rs   # 统一错误输出
tests/
  lexer.rs         # lexer 行为测试
  parser.rs        # parser 行为测试
  smoke.rs         # 启动真实二进制的端到端烟测
```

更详细的实现说明见 [docs/design.md](docs/design.md)，后续计划见
[docs/roadmap.md](docs/roadmap.md)。

## 测试

常用验证命令：

```bash
cargo fmt --check
cargo check
cargo test
```

## 实验要求

原始实验要求实现一个简单 shell，能够：

- 打印命令提示符并循环读取命令
- 区分内置命令、外部命令和无效命令
- 打印命令开始和结束提示
- 支持类似 `dir | more` 的管道执行

本实现使用 Rust 和 `nix` crate 直接练习 Unix 进程相关 API。
