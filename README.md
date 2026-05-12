# ecsh

`ecsh` 是一个使用 Rust 实现的简单类 Unix shell，主要用于操作系统实验练习。

这个项目关注 shell 的基本执行模型：读取命令行、解析命令、处理内置命令、
`fork` 子进程、在子进程中通过 `execvp` 替换为目标程序，并在父进程中
使用 `waitpid` 等待子进程结束。

## 当前功能

- 交互式命令提示符
- 基于空白字符的简单命令解析
- 内置命令：
  - `help`：显示支持的内置命令
  - `exit`：退出 shell
  - `cd`：修改 shell 进程的当前工作目录
  - `pwd`：打印 shell 进程的当前工作目录
  - `env`：打印当前环境变量
  - `export KEY=value`：在 shell 进程中设置环境变量
  - `unset KEY`：从 shell 进程中删除环境变量
  - `clear`：清空终端屏幕
- 通过 `fork`、`execvp` 和 `waitpid` 执行外部命令
- 外部命令无效时报告错误，并保持 shell 继续运行
- shell 修改后的环境变量会被后续外部命令继承
- `clear` 会跳过命令生命周期提示，避免清屏后立刻输出 `starting/ending`
- 支持标准 Unix 管道 `|`，例如 `echo hello | grep h`
- 管道会为每个外部命令创建子进程，并使用匿名 pipe 连接相邻命令
- 实验要求的命令生命周期提示：
  - `<command> starting...`
  - `<command> ending.`

## 使用方法

构建并运行：

```bash
cargo run
```

示例命令：

```bash
help
pwd
cd /tmp
pwd
echo hello
export ECSH_NAME=elaine
printenv ECSH_NAME
unset ECSH_NAME
clear
ls
echo hello | grep h
printf "a\nb\n" | grep b
exit
```

## 实现说明

当前 shell 使用下面的数据结构表示一条命令：

```rust
struct Command {
    program: String,
    args: Vec<String>,
}
```

`program` 保存命令名称，`args` 保存命令的剩余参数。执行外部命令时，
`ecsh` 会重新构造 Unix 风格的 `argv`，并把 `program` 放到 `argv[0]`。

空输入不会被视为错误。用户直接按下 Enter 时，shell 会直接进入下一轮提示符。

环境变量相关内置命令必须在 shell 进程自身执行，因为它们的效果需要在命令返回后
继续保留。`export` 和 `unset` 在调用 Rust 的环境变量修改 API 之前，会使用
`[A-Za-z_][A-Za-z0-9_]*` 规则校验变量名。

`clear` 会直接向终端写入 ANSI 转义序列，并跳过命令生命周期提示，使它更接近
真实交互式 shell 中的清屏命令。scrollback 历史能否被清除取决于终端支持。

管道使用标准 shell 语义中的 `|`。`ecsh` 会先创建 `n - 1` 个匿名 pipe，
再为 pipeline 中的每条外部命令 `fork` 一个子进程，并在子进程中使用
`dup2_stdin` / `dup2_stdout` 绑定标准输入输出。父进程在创建完所有子进程后
关闭自己的 pipe 文件描述符，并等待所有子进程结束。

当前版本的管道仍然是简化实现：pipeline 中暂不支持内置命令，也暂不处理引号，
所以 `echo "a|b"` 会被错误地按 `|` 切分。

## 开发计划

### 阶段 1：基础命令执行

- [x] 从标准输入读取命令
- [x] 解析简单的空白分隔参数
- [x] 实现 `help` 和 `exit`
- [x] 执行外部命令
- [x] 报告无效命令
- [x] 实现 `cd` 和 `pwd`
- [x] 添加环境变量相关内置命令：
  - `env`
  - `export`
  - `unset`
- [x] 将 `clear` 实现为安静的交互式内置命令

### 阶段 2：Unix 连接机制

- [x] 支持标准管道 `|`
- [ ] 支持标准输入和标准输出重定向

### 阶段 3：交互式 shell 行为

- [ ] 改进提示符显示
- [ ] 添加命令历史
- [ ] 处理常见交互式信号
- [ ] 探索前台进程组和作业控制

### 阶段 4：脚本化特性

- [ ] 变量
- [ ] 基础展开
- [ ] 条件执行
- [ ] 循环
- [ ] 函数

## 实验要求

原始实验要求实现一个简单 shell，能够：

- 打印命令提示符并循环读取命令
- 区分内置命令、外部命令和无效命令
- 打印命令开始和结束提示
- 支持类似 `dir | more` 的管道执行

本实现使用 Rust 和 `nix` crate 直接练习 Unix 进程相关 API。
