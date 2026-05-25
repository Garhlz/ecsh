# 当前进度

这份文档用于同步当前实现状态。

- 目标：说明项目已经完成的能力、当前正在收口的部分，以及后续工作入口
- 范围：只描述现状与边界，不展开长期设计推演

更长的设计路线保留在 [TODO.md](/home/elaine/work/projects/ecsh/docs/TODO.md)。

## 项目结构

仓库当前包含两条实现线：

- `ecsh`：教学型交互 shell
- `ecscript`：为 `ecsh` 演进出来的小型脚本语言与解释器

当前阶段已经不再是“仅实现 shell 基础命令”，而是“shell + 脚本语言”联合演进。

## 已完成

### shell 主体

已完成的 shell 能力包括：

- 外部命令执行：`fork`、`execvp`、`waitpid`
- 内置命令：`help`、`exit`、`cd`、`pwd`、`env`、`export`、`unset`、`clear`、`status`、`jobs`、`fg`、`bg`
- 交互输入：`rustyline`、历史记录、基础行编辑、欢迎页、两行 prompt
- 词法与解析：引号、反斜杠转义、`|`、`&&`、`||`、`;`、行尾 `&`
- 执行能力：管道、`<` / `>` / `>>`、纯输出型 builtin 进管道、普通 builtin 临时重定向
- 状态与控制：`$?`、`status`、前台进程组、最小 job control、终端控制权切换、常见交互信号处理

### shell 运行时展开

shell 当前采用 `ShellWord` 运行时展开模型，而不是在词法阶段把所有 `$` 展开写死。

已支持：

- `$VAR`：脚本作用域优先，再回退环境变量
- `${VAR}`：只查环境变量
- `$(cmd)`：通过 `/bin/sh -c` 做命令替换
- `$[expr]`：调用 `ecscript` 表达式求值
- `$[...arr]`：把数组展开成多个 argv

这一部分对应 [TODO.md](/home/elaine/work/projects/ecsh/docs/TODO.md) 中的 stage 6。当前 `HEAD` 对应的主提交也是这一阶段。

### ecscript stage 1-6

`ecscript` 当前已经完成：

- expression lexer / Pratt parser / evaluator
- `let`、赋值、复合赋值、block
- 数组 / 对象字面量与访问
- `if` / `while` / `for in`
- `break` / `continue` / `return`
- 命名函数、lambda、闭包、自由变量捕获
- builtin：`len`、`push`、`pop`、`insert`、`remove`、`keys`、`values`、`to_json`、`print`、`println`
- 独立解释器入口：REPL / 文件执行 / `-e` / stdin
- parse/runtime 错误的偏移定位和源码格式化

## 阶段状态

### 阶段 7：顶层集成与文件执行

当前仍未完成的阶段 7 事项包括：

- 顶层输入按首 token 在 shell 模式 / ecscript 模式之间分派
- `ecsh` 的正式脚本文件执行入口
- `~/.ecshrc`
- `source` / `.`
- shell REPL 与 `ecscript` REPL 在 incomplete 检测和错误输出上的进一步统一

### 阶段 7.5：Shell 诊断与交互收口（已完成）

阶段 7.5 的核心收口项已经完成：

- shell `lexer.rs` / `parser.rs` 已经切换到结构化 `ParseError`
- 未闭合引号、`${}`、`$()`、`$[]` 已统一标记为 `incomplete`
- shell 主循环已经根据 `incomplete` 做续行读取
- shell parse 错误已经提供 `line:column + 源码行 + caret`
- 历史记录已经按完整命令写入
- 续行中的 `${...}`、`$(...)`、双引号、`$[...]` 和 Ctrl-C 边界都已覆盖到测试

这部分对应 `TODO.md` 中的阶段 7.5，不引入新的语言能力，只收口诊断与交互行为。

阶段 7.5 中已经落地的体验层能力还包括：

- Tab 补全（命令名与文件路径）
- alias / unalias
- `trap EXIT|INT`
- `type` / `which` / `history`

### 阶段 8：Shell 语义补完

阶段 8 负责传统 shell 语义缺口，当前尚未开始的主要内容包括：

- here-doc `<<`
- glob 展开
- subshell `()`
- 更完整的 job control 语义
- `|&` 与 `!` 这类执行语义增强

## 未完成

### ecscript 后续能力

以下 `ecscript` 能力仍未实现：

- block value / 尾表达式返回值
- 模块系统
- 字符串插值
- 多行字符串

## 后续入口

如果按当前路线继续推进，后续工作可以归为四类：

### 1. 顶层脚本集成

对应 `TODO.md` 中的阶段 7：

- 顶层输入分派
- `ecsh` 文件执行入口
- `~/.ecshrc` 与 `source` / `.`
- shell / script 双入口的错误处理与续行逻辑统一

### 2. shell 诊断与交互收口

对应 `TODO.md` 中的阶段 7.5：

- Tab 补全、alias / unalias、交互层体验增强
- shell 侧后续诊断接口的增量整理

### 3. shell 语义补完

对应 `TODO.md` 中的阶段 8：

- here-doc
- glob
- subshell
- 更完整的作业控制与执行语义

### 4. 工程收口

- 统一 shell / `ecscript` 的错误类型与格式化接口
- 清理阶段性文档与命名残留
- 继续补测试覆盖和说明

## 目录结构说明

当前目录结构可以继续支撑后续开发，暂时不需要做大规模重排。

现有结构的边界如下：

- `src/ecscript/`：脚本语言内核
- `src/executor/`：shell 执行、启动、作业控制、运行时展开
- `src/bin/ecscript.rs`：独立解释器入口
- `src/main.rs`：`ecsh` 主循环与顶层调度

如果后续继续推进顶层双模式分派，可以再把 `src/main.rs` 中的调度逻辑拆薄一层。  
如果 shell 侧全面转向结构化错误，也可以把 shell 错误相关接口进一步收束到专门模块中。
