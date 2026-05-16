# 实现说明

这份文档记录 `ecsh` 当前实现的主要设计。`ecsh` 是
**Elaine & Cornelia's shell**。README 只保留项目入口信息；这里保留更适合
学习和维护的实现细节。

## 模块职责

当前代码按职责拆分为几个模块：

- `lib.rs`：库 crate 入口，导出 shell 核心模块，便于集成测试复用
- `main.rs`：交互式主循环
- `types.rs`：命令、管道、解析结果和执行状态类型
- `input.rs`：交互式 `rustyline` 和非交互式 `read_line` 输入层
- `lexer.rs`：将输入行扫描为 token 流，并处理引号和最小变量展开
- `parser.rs`：将 token 流转换为命令、管道、条件执行和后台执行语法结构
- `prompt.rs`：交互式 prompt 构造与着色
- `builtin.rs`：内置命令识别和执行
- `signals.rs`：交互式 shell 信号初始化，以及 child 进程恢复默认信号行为
- `executor/mod.rs`：执行入口分发
- `executor/launch.rs`：单命令 / pipeline job 的 fork、pipe、exec 启动逻辑
- `executor/jobs.rs`：前台等待、后台回收、终端前台权切换和 job 状态更新
- `executor/builtins.rs`：`jobs`、`fg`、`bg` 等需要访问 job 表的特殊内置命令
- `redirection.rs`：重定向文件打开、fd 保存恢复和子进程重定向处理
- `diagnostics.rs`：统一错误输出

## 命令模型

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

shell 还维护运行时状态 `ShellState`。其中既包含上一条命令的退出状态
`last_status`，也包含交互式作业控制所需的状态，例如：

- `interactive`：当前是否在真实 tty 中运行
- `shell_pgid`：shell 自己所在的进程组
- `shell_terminal_fd`：当前控制终端 fd
- `jobs`：后台或已停止作业表
- `next_job_id`：下一个 shell 分配的 job 编号
- `current_fg_pgid`：当前占用前台终端的进程组

内置命令 `status` 会直接打印 `last_status`；`$?`、`&&`、`||` 和 prompt 也会
读取这一状态。当用户直接按回车输入空行时，主循环会把状态恢复为成功，避免旧错误码
一直挂在提示符上。

为了解耦“语法控制流”和“前后台执行语义”，parser 当前返回 `ParsedJob`：

- `line: ParsedLine`：命令、管道、`&&`、`||`、`;` 组成的语法结构
- `background: bool`：是否带有行尾 `&`
- `command_line: String`：用户输入的原始命令文本，供 `jobs` 展示

空输入不会被视为错误。用户直接按下 Enter 时，shell 会直接进入下一轮提示符。

## 内置命令和环境

环境变量相关内置命令必须在 shell 进程自身执行，因为它们的效果需要在命令返回后
继续保留。`export` 和 `unset` 在调用 Rust 的环境变量修改 API 之前，会使用
`[A-Za-z_][A-Za-z0-9_]*` 规则校验变量名。

`jobs`、`fg`、`bg` 也必须在 shell 进程自身执行，因为它们需要直接访问和修改
`ShellState.jobs`，并在 `fg` 场景下切换终端前台权、重新等待前台作业。

`clear` 会直接向终端写入 ANSI 转义序列，并跳过命令生命周期提示，使它更接近
真实交互式 shell 中的清屏命令。scrollback 历史能否被清除取决于终端支持。

## Prompt 和交互式输入

prompt 由 `prompt.rs` 统一构造。第一行显示 shell 标识、用户名、主机名、当前目录
和可选状态码；第二行显示真正的输入提示符 `$ `。

主机名会优先读取 `HOSTNAME` 环境变量，不存在时再通过
`nix::unistd::gethostname()` 获取。为了避免把 ANSI 控制序列写进重定向目标，
prompt 只有在 `stdout` 连接到 tty 时才启用颜色。

交互式输入由 `rustyline` 负责，因此真实终端中可以使用上下方向键浏览历史命令，
使用左右方向键移动光标，并进行基础行内编辑。`Ctrl-D` 会结束输入并退出 shell；
`Ctrl-C` 会取消当前输入行，shell 本身继续运行，并把上一条状态设置为 130。

历史记录保存到 `~/.ecsh_history`。读取历史文件失败不会阻止 shell 启动，
保存历史文件失败也不会影响 shell 的退出状态，因为这些错误通常来自首次运行、
`HOME` 不存在或目录权限限制。

当 `stdin` 或 `stdout` 不是 tty 时，`ecsh` 会退回普通 `read_line` 路径，
保证管道输入、重定向输入和自动化测试仍然可以按行驱动 shell。

欢迎页也只在交互式终端中显示。启动时会打印简短欢迎词，并复用 builtin `help`
的输出展示可用内置命令；非 tty 模式不会打印欢迎页，避免脚本和测试输出被额外
内容污染。

## Lexer 和 Parser

解析分为两层：`lexer.rs` 先把输入行扫描为 token 流，`parser.rs` 再把 token 流
转换为 `ParsedJob`。当前 lexer 支持普通词、单引号、双引号、反斜杠转义、
`|`、`&&`、`||`、`;`、`&`、`<`、`>`、`>>`。

引号只影响当前词内部的解释方式，不会自动结束当前词，因此 `a"b"c` 会被解析为
一个词 `abc`。当前暂不支持命令替换、here-doc `<<` 和完整 POSIX shell 词法规则。

反斜杠转义当前是最小实现。普通状态下，`\x` 会把后面的 `x` 作为普通字符放入
当前 word，因此 `hello\ world` 会变成一个参数 `hello world`，`\|` 不会产生
管道 token。单引号内反斜杠保持字面量。双引号内只特殊处理 `\"`、`\$`、`\\`，
其他 `\x` 保留为 `\x`。

变量展开当前是最小实现：支持 `$?`、`$NAME`、`${NAME}` 和词内前后缀拼接。
单引号内不展开变量，双引号内会展开变量。`${...}` 内当前只支持环境变量名形式
`[A-Za-z_][A-Za-z0-9_]*`，不支持位置参数、默认值语法或更完整的参数展开。

后台执行当前只支持教学型最小语义：`&` 必须出现在行尾，并且只作用于单个命令或
整条 pipeline。像 `true && echo ok &` 这类把作业控制和控制流混合的形式当前会被
解析器拒绝，以保持语法和执行模型清晰。

## 管道和重定向

管道使用标准 shell 语义中的 `|`。`ecsh` 会先创建 `n - 1` 个匿名 pipe，再为
pipeline 中的每条命令 `fork` 一个子进程，并在子进程中使用
`dup2_stdin` / `dup2_stdout` 绑定标准输入输出。父进程在创建完所有子进程后
关闭自己的 pipe 文件描述符，并根据前台/后台语义进入等待或立刻返回。

重定向解析基于 token 流完成，因此操作符和普通词之间可以没有空白，例如
`echo hello>out.txt` 和 `cat<in.txt`。外部命令的重定向在子进程中完成；
普通内置命令运行在 shell 进程自身，因此会先保存原始标准输入输出 fd，应用临时
重定向，执行完成并刷新缓冲区后再恢复 fd。

相关资源管理逻辑集中在 `redirection.rs` 中，避免 `executor.rs` 同时承担过多
文件描述符细节。管道中的重定向当前只支持边界位置：第一条命令可以使用 `<`，
最后一条命令可以使用 `>` 或 `>>`。

当前版本的管道仍然是简化实现：pipeline 中只支持 `help`、`pwd`、`env` 这类
纯输出型内置命令，以及 `status` 这类只读取 shell 退出码的命令；`cd`、`export`、
`unset`、`exit`、`clear` 这类会改变 shell 状态或强交互行为的内置命令暂不支持
出现在管道中。

## 信号、前台进程组和作业控制

交互式模式下，shell 启动时会做三件事：

1. 确保自己处于独立进程组中
2. 通过 `tcsetpgrp` 拿回当前终端的前台控制权
3. 忽略 `SIGINT`、`SIGQUIT`、`SIGTSTP`、`SIGTTIN`、`SIGTTOU`

这样 shell 自己不会因为 `Ctrl-C`、`Ctrl-Z` 或终端前后台规则而被直接终止或挂起。

但 `fork` 出来的 child 会继承这些信号处置方式，因此 child 在 `exec` 前必须恢复
默认行为。否则用户运行的程序也会错误地忽略 `Ctrl-C` / `Ctrl-Z`。

单命令和 pipeline 在执行层都统一建模为一个 job。job control 的核心对象是
“进程组”，不是单个 pid：

- 单命令 job：child 自己是进程组组长
- pipeline job：第一条命令的 pid 作为 pgid，后续命令全部加入同一进程组

这样终端发给前台进程组的 `SIGINT` / `SIGTSTP` 才会同时作用于整条管道。

前台 job 的执行流程是：

1. shell 用 `tcsetpgrp` 把终端前台权交给 job 的进程组
2. shell 调用 `waitpid(-pgid, WUNTRACED | WCONTINUED)` 等待这个进程组中任意成员的状态变化
3. 每收到一次 `WaitStatus`，就更新对应 `JobProcess` 的状态，并重新推导整个 job 的状态
4. job 全部完成或整体停止后，shell 再用 `tcsetpgrp` 把终端前台权切回自己

后台 job 的处理则是教学型最小实现：

- 启动后立即返回 prompt，不阻塞等待
- job 会被放入 `ShellState.jobs`
- 主循环每轮开始时通过 `reap_background_jobs()` 非阻塞调用
  `waitpid(-1, WNOHANG | WUNTRACED | WCONTINUED)`，同步回收所有已发生的后台状态变化

`jobs` / `fg` / `bg` 的实现也围绕这套状态模型展开：

- `jobs`：打印当前后台和已停止 job，并清理已完成 job
- `fg %N`：把 job `N` 从表中取出，切前台、必要时发 `SIGCONT`，然后重新进入前台等待
- `bg %N`：只把 job `N` 标记为 Running 并向整个进程组发送 `SIGCONT`，不切前台、不等待

## 执行状态

执行层使用 `CommandStatus` 表示命令退出状态，用 `CommandFlow` 区分“继续运行”和
“exit 请求退出 shell”。当前状态码会从 `waitpid` 的 `WaitStatus` 转换出来，
并保存在 `ShellState.last_status` 中，供 `status` 和 `$?` 展开复用。

`&&` 和 `||` 通过递归执行 `ParsedLine` 实现：`&&` 只在左侧成功时执行右侧，
`||` 只在左侧失败时执行右侧。
`;` 的优先级低于 `&&` 和 `||`，用于无条件顺序执行多条命令；如果序列左侧请求
`exit`，右侧不会继续执行。
