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
- `parser.rs`：将 token 流转换为命令、管道和条件执行语法结构
- `prompt.rs`：交互式 prompt 构造与着色
- `builtin.rs`：内置命令识别和执行
- `executor.rs`：外部命令、管道、fork/exec/wait 逻辑
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

shell 还维护一个最小运行时状态 `ShellState`，当前其中保存上一条命令的退出状态
`last_status`。内置命令 `status` 会直接打印这个状态码，`$?`、`&&`、`||` 和
prompt 也会读取这一状态。当用户直接按回车输入空行时，主循环会把状态恢复为
成功，避免旧错误码一直挂在提示符上。

空输入不会被视为错误。用户直接按下 Enter 时，shell 会直接进入下一轮提示符。

## 内置命令和环境

环境变量相关内置命令必须在 shell 进程自身执行，因为它们的效果需要在命令返回后
继续保留。`export` 和 `unset` 在调用 Rust 的环境变量修改 API 之前，会使用
`[A-Za-z_][A-Za-z0-9_]*` 规则校验变量名。

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
转换为 `ParsedLine`。当前 lexer 支持普通词、单引号、双引号、`|`、`&&`、`||`、
`;`、`<`、`>`、`>>`。

引号只影响当前词内部的解释方式，不会自动结束当前词，因此 `a"b"c` 会被解析为
一个词 `abc`。当前暂不支持反斜杠转义、命令替换、here-doc `<<`、单个 `&`
后台执行和完整 POSIX shell 词法规则。

变量展开当前是最小实现：支持 `$?`、`$NAME`、`${NAME}` 和词内前后缀拼接。
单引号内不展开变量，双引号内会展开变量。`${...}` 内当前只支持环境变量名形式
`[A-Za-z_][A-Za-z0-9_]*`，不支持位置参数、默认值语法或更完整的参数展开。

## 管道和重定向

管道使用标准 shell 语义中的 `|`。`ecsh` 会先创建 `n - 1` 个匿名 pipe，再为
pipeline 中的每条外部命令 `fork` 一个子进程，并在子进程中使用
`dup2_stdin` / `dup2_stdout` 绑定标准输入输出。父进程在创建完所有子进程后
关闭自己的 pipe 文件描述符，并等待所有子进程结束。

重定向解析基于 token 流完成，因此操作符和普通词之间可以没有空白，例如
`echo hello>out.txt` 和 `cat<in.txt`。外部命令的重定向在子进程中完成；
普通内置命令运行在 shell 进程自身，因此会先保存原始标准输入输出 fd，应用临时
重定向，执行完成并刷新缓冲区后再恢复 fd。

相关资源管理逻辑集中在 `redirection.rs` 中，避免 `executor.rs` 同时承担过多
文件描述符细节。管道中的重定向当前只支持边界位置：第一条命令可以使用 `<`，
最后一条命令可以使用 `>` 或 `>>`。

当前版本的管道仍然是简化实现：pipeline 中只支持 `help`、`pwd`、`env` 这类
纯输出型内置命令；`cd`、`export`、`unset`、`exit`、`clear` 这类会改变 shell
状态或强交互行为的内置命令暂不支持出现在管道中。

## 执行状态

执行层使用 `CommandStatus` 表示命令退出状态，用 `CommandFlow` 区分“继续运行”和
“exit 请求退出 shell”。当前状态码会从 `waitpid` 的 `WaitStatus` 转换出来，
并保存在 `ShellState.last_status` 中，供 `status` 和 `$?` 展开复用。

`&&` 和 `||` 通过递归执行 `ParsedLine` 实现：`&&` 只在左侧成功时执行右侧，
`||` 只在左侧失败时执行右侧。
`;` 的优先级低于 `&&` 和 `||`，用于无条件顺序执行多条命令；如果序列左侧请求
`exit`，右侧不会继续执行。
