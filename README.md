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
  - `status`：打印上一条命令的退出状态码
- 通过 `fork`、`execvp` 和 `waitpid` 执行外部命令
- 外部命令无效时报告错误，并保持 shell 继续运行
- shell 修改后的环境变量会被后续外部命令继承
- `clear` 会跳过命令生命周期提示，避免清屏后立刻输出 `starting/ending`
- 支持标准 Unix 管道 `|`，例如 `echo hello | grep h`
- 管道会为每个外部命令创建子进程，并使用匿名 pipe 连接相邻命令
- 管道中支持 `help`、`pwd`、`env` 这类纯输出型内置命令
- 支持标准输入重定向 `<`
- 支持标准输出重定向 `>` 和追加重定向 `>>`
- 普通内置命令支持临时重定向，执行完成后会恢复 shell 的标准输入输出
- 管道中支持边界重定向：首条命令可使用 `<`，末条命令可使用 `>` 或 `>>`
- 管道执行时会打印 `pipeline starting...` 和 `pipeline ending.`
- 命令执行会转换为内部状态码，为后续 `$?`、`&&`、`||` 等功能预留基础
- 支持最小变量展开：
  - `$?`
  - `$NAME`
  - `${NAME}`
  - 词内前后缀拼接，例如 `prefix-$HOME`、`$HOME/file`
- 错误输出统一通过 diagnostics 模块打印并刷新 `stderr`
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
status
clear
echo $?
echo prefix-$HOME
echo $HOME/file
echo ${HOME}
ls
echo hello | grep h
printf "a\nb\n" | grep b
pwd | cat
env | grep PATH
pwd > pwd.txt
cat < pwd.txt
echo done >> pwd.txt
cat < pwd.txt | grep done > result.txt
exit
```

## 实现说明

当前代码按职责拆分为几个模块：

- `main.rs`：交互式主循环
- `types.rs`：命令、管道、解析结果和执行状态类型
- `parser.rs`：普通命令和管道解析
- `builtin.rs`：内置命令识别和执行
- `executor.rs`：外部命令、管道、fork/exec/wait 逻辑
- `redirection.rs`：重定向文件打开、fd 保存恢复和子进程重定向处理
- `diagnostics.rs`：统一错误输出

shell 使用下面的数据结构表示一条命令：

```rust
struct Command {
    program: String,
    args: Vec<String>,
    redirection: Redirection,
}
```

`program` 保存命令名称，`args` 保存命令的剩余参数，`redirection` 保存标准输入
和标准输出重定向设置。执行外部命令时，`ecsh` 会重新构造 Unix 风格的 `argv`，
并把 `program` 放到 `argv[0]`。

shell 还维护一个最小运行时状态 `ShellState`，当前其中保存上一条命令的退出状态
`last_status`。内置命令 `status` 会直接打印这个状态码，后续 `$?`、`&&`、`||`
等功能也会复用这一状态。当前 `$?` 已经会在解析阶段展开为这一状态码。

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

重定向解析目前要求操作符和路径之间使用空白分隔，例如 `echo hello > out.txt`。
外部命令的重定向在子进程中完成；普通内置命令运行在 shell 进程自身，因此会先
保存原始标准输入输出 fd，应用临时重定向，执行完成并刷新缓冲区后再恢复 fd。
相关资源管理逻辑集中在 `redirection.rs` 中，避免 `executor.rs` 同时承担过多
文件描述符细节。管道中的重定向当前只支持边界位置：第一条命令可以使用 `<`，
最后一条命令可以使用 `>` 或 `>>`。

当前版本的管道仍然是简化实现：pipeline 中只支持 `help`、`pwd`、`env` 这类
纯输出型内置命令；`cd`、`export`、`unset`、`exit`、`clear` 这类会改变 shell
状态或强交互行为的内置命令暂不支持出现在管道中。解析器也暂不处理引号，所以
`echo "a|b"` 会被错误地按 `|` 切分。

变量展开当前仍然是最小实现：先按空白切分 token，再对每个 token 做词内扫描。
因此 `$?`、`$HOME`、`${HOME}`、`prefix-$HOME` 这类形式已经可用；但解析器仍
未处理引号、`${...}` 的严格语法错误，以及更完整的 shell 词法规则。

执行层使用 `CommandStatus` 表示命令退出状态，用 `CommandFlow` 区分“继续运行”
和 “exit 请求退出 shell”。当前状态码已经会从 `waitpid` 的 `WaitStatus` 转换出来，
并保存在 `ShellState.last_status` 中，供 `status` 和 `$?` 展开复用。

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
- [x] 支持部分纯输出型内置命令进入管道
- [x] 拆分 parser、builtin、executor、types 和 diagnostics 模块
- [x] 引入基础命令状态模型
- [x] 支持标准输入和标准输出重定向
- [x] 支持普通内置命令的临时重定向
- [x] 支持管道边界重定向
- [x] 将重定向资源管理拆分到独立模块
- [x] 保存上一条命令的退出状态，并提供 `status` 内置命令
- [x] 支持最小变量展开：`$?`、`$NAME`、`${NAME}` 及词内前后缀拼接

### 阶段 3：交互式 shell 行为

- [ ] 改进提示符显示
- [ ] 添加命令历史
- [ ] 处理常见交互式信号
- [ ] 探索前台进程组和作业控制

### 阶段 4：脚本化特性

- [ ] 变量
- [ ] 更完整的展开与引用规则
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
