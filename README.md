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
- lexer + parser：支持单引号、双引号、变量展开和操作符 token
- 变量展开：`$?`、`$NAME`、`${NAME}`，以及 `prefix-$HOME`、`$HOME/file` 这类词内拼接
- 管道：支持标准 Unix 管道 `|`
- 重定向：支持 `<`、`>`、`>>`，操作符可以不依赖空白分隔
- 条件执行：支持 `&&` 和 `||`
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

## 项目结构

```text
src/
  lib.rs           # 库 crate 入口，供集成测试复用核心模块
  main.rs          # 交互式主循环
  types.rs         # 命令、管道、解析结果和执行状态类型
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
