# 当前进度

这份文档用于同步当前实现状态。

- 目标：说明项目已经完成的能力、当前正在收口的部分，以及后续工作入口
- 范围：只描述现状与边界，不展开长期设计推演

更长的设计路线保留在 [TODO.md](TODO.md)。

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
- `${expr}`：调用 `ecscript` 表达式求值
- `${env("VAR")}`：通过 ecscript 内置函数显式读取环境变量
- `set_env("VAR", value)` / `unset_env("VAR")`：从 `.ecshrc` 或 source 脚本修改当前 shell 环境
- `$(cmd)`：通过 `/bin/sh -c` 做命令替换
- `${...arr}`：把数组展开成多个 argv

这一部分对应 [TODO.md](TODO.md) 中的 stage 6。当前 `HEAD` 对应的主提交也是这一阶段。

### ecscript stage 1-6

`ecscript` 当前已经完成：

- expression lexer / Pratt parser / evaluator
- `let`、赋值、复合赋值、block
- 数组 / 对象字面量与访问
- `if` / `while` / `for in`
- `break` / `continue` / `return`
- 命名函数、lambda、闭包、自由变量捕获
- builtin：`env`、`cwd`、`join_path`、`stdin`、`read_lines`、`write_lines`、`range`、`len`、`push`、`pop`、`insert`、`remove`、`keys`、`values`、`to_json`、`from_json`、`print`、`println`
- 命令桥 builtin：`run`、`capture`、`text`、`lines`、`with_env`、`with_cwd`
- 独立解释器入口：REPL / 文件执行 / `-e` / stdin
- parse/runtime 错误的偏移定位和源码格式化

当前还已经收紧了两条语言边界：

- `1..10` / `1..=10` 只在 `for` 语句中保留；普通值世界改用 `range(start, end)`，默认闭区间
- `ecsh` 顶层已支持脚本模式分派，但脚本 block 内仍然只接受 ecscript 语句；shell 命令需要通过 `cmd{ ... }` 命令桥进入

## 阶段状态

### 阶段 7：顶层集成与文件执行（已完成）

阶段 7 已完成的内容包括：

- 交互顶层按首 token 在 shell 模式 / ecscript 模式之间分派
- `ecsh file.ecs` 走 ecscript 文件执行路径
- `source` / `.` 在当前 shell 的 `script_env` 中执行 `.ecs` 文件
- 交互启动时自动加载 `~/.ecshrc`
- `reload_rc` 可在当前 session 中用全新脚本环境和模块缓存重载 `~/.ecshrc`
- 顶层 shell parse error 与 ecscript parse/runtime error 已分别走源码定位输出

当前固定的环境边界是：

- 交互顶层与 `source` / `.` 共享当前 shell 的 `script_env`
- `reload_rc` / 交互启动加载 `.ecshrc` 会重建脚本环境、扩展注册表和模块缓存，然后再切换到新结果
- `ecsh file.ecs` 使用新的脚本根环境
- ecscript block 内仍是纯 ecscript 语句，不直接执行 shell 命令；命令需要显式写成 `cmd{ ... }`

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

### 阶段 8：命令值与结构化执行桥（进行中）

阶段 8 当前已经落地的内容包括：

- `cmd{ ... }` 结构化命令字面量
- `run(cmd)`：继承当前终端执行，非零退出码报语言错误
- `capture(cmd)`：返回包含 `code`、`signal`、`stdout`、`stderr`、`duration_ms`、`ok` 的普通对象
- `text(cmd)` / `lines(cmd)`：基于 `capture(cmd)` 的高频消费接口
- `command(...)`：argv-first 的程序化命令值 builder
- `from_json(text(cmd{ ... }))`：命令输出到 JSON 值的推荐组合方式
- `stdin()` / `read_lines()` / `write_lines(...)`：text/value bridge 已补齐，格式转换继续通过显式组合完成
  - 典型组合：`from_json(stdin())`、`write_lines(read_lines())`
- `with_env(cmd, obj)` / `with_cwd(cmd, path)`：以不可变派生方式调整命令值
- `cmd{ a | b }` pipeline 子集
- 单命令纯输出 shell builtin：如 `pwd` / `env` / `status` / `help`
- `run/capture/text/lines` 当前要求 shell-backed 执行上下文；独立 `ecscript` 解释器和 `ecsh file.ecs` 路径下暂未完全接通

当前阶段 8 仍然保留的边界：

- `cmd{ ... }` 仍不支持 `&&` / `||` / `;` / `&`
- pipeline 中的 shell builtin 目前还不作为命令值执行桥的一部分

### 阶段 9：值流与脚本标准库核心（进行中）

阶段 9 当前已经落地的内容包括：

- `|>` 值流语法糖：`x |> f(a, b)` 等价于 `f(x, a, b)`
- eager Array 版高阶函数：
  - `map`
  - `filter`
  - `reduce`
  - `each`
  - `any`
  - `all`
  - `find`
  - `join`
- 数组工具原语：`slice(arr, start, end)`，采用半开区间 `[start, end)`
- 一份脚本级标准库草案：
  - `examples/ecscript/std_iter_draft.ecs`

当前阶段 9 仍然保留的边界：

- 高阶函数当前只接受 `Array`
- 还没有 lazy iterator / stream
- 还没有正式模块化的 `std.iter`
- `|>` 右侧当前必须是调用表达式

### 阶段 12：Shell 语义补完

阶段 12 负责传统 shell 语义缺口，当前尚未开始的主要内容包括：

- here-doc `<<`
- glob 展开
- subshell `()`
- 更完整的 job control 语义
- `|&` 与 `!` 这类执行语义增强

## 未完成

### ecscript 后续能力

以下 `ecscript` 能力仍未实现：

- block value / 尾表达式返回值
- 搜索路径 / 命名导入
- 字符串插值
- 多行字符串

## 后续入口

如果按当前路线继续推进，后续工作可以归为四类：

### 1. 模块、扩展点与交互脚本化

对应 `TODO.md` 中的阶段 10：

- `use ... as ...`
- `pub let` / `pub func`
- 模块缓存
- hook / completion / prompt / bind

阶段 10 当前已经落地：

- 文件级模块导入：`use ./foo.ecs as foo`
- 导出可见性：`pub let` / `pub func`
- 模块求值结果会映射成普通对象命名空间，成员访问继续复用 `foo.bar`
- 最小 shell 扩展点：
  - `hook("before_prompt" | "after_cd" | "preexec" | "postexec", func)`
  - `prompt((ctx) => { ... })`
  - `complete(name, (ctx) => { ... })`
  - `bind(key, (ctx) => { ... })`
  - `register_command(name, (ctx) => { ... })`
- completion 当前接受结构化候选对象：
  `{ value, display, desc, kind }`
- 脚本命令当前接受 `{ name, args, cwd }`，可以通过 `set_cwd(path)` 修改当前目录
- bind 回调当前接受 `{ key, line, cursor, cwd, history }`（`history` 为 `Array<String>`）
- bind 回调当前支持：
  - `{ action: "accept" | "newline" | "complete" | "complete_hint" | "clear_screen" | "interrupt" }`
  - `{ action: "history_search_backward" | "history_search_forward" | "previous_history" | "next_history" }`
  - `{ action: "insert", text: "..." }`
  - `{ action: "set_line", text: "..." }`（替换整行）

阶段 10 收口已完成（错误语义与生命周期加固）：

- `hook` 错误按 best-effort 处理：打印错误，跳过该 handler，继续执行后续 handler
- `preexec`：仅在 shell 成功解析后、执行前触发；解析错误不触发 `preexec`
- `postexec`：命令到达最终结果后触发，包含解析错误和执行失败场景
- `prompt` 错误或非 String 返回：打印错误（同类错误同会话只打印一次），回退到默认 prompt
- `complete` 错误或无效候选：打印错误（同命令同错误同会话只打印一次），回退到无脚本候选；非法 item 跳过不 panic
- `after_cd` 钩子内调用 `set_cwd()`：目录和 `PWD`/`OLDPWD` 正常更新，但不递归触发 `after_cd`（reentry guard + Drop 守卫确保 panic 时也复位）
- `bind` 回调的 cooked tty 切换：通过 Drop 守卫确保回调 panic 时终端属性仍然恢复
- `reload_rc` 现在以 rc 文件为唯一真相源启动 staged 状态（别名/陷阱/作业/历史均清除后重载，防止已删除配置残留）
- `register_command` 保持原有语义：handler 错误打印并返回失败；组合限制不变（禁止后台/管道/重定向）
- `commands()` / `help()` 现在共享同一份 specs 数据源，shell builtins 优先从 specs 表查询再补充运行时命名表

当前边界：

- `use` 只支持相对路径 / 绝对路径文件导入
- `use` 当前已经支持交互式 `ecsh` 顶层输入，基准目录取当前 `cwd`
- 已支持最小模块缓存：同一路径模块只初始化一次
- 脚本命令当前只支持顶层前台执行，不支持管道、后台执行和重定向
- 已支持循环导入检测
- 还没有搜索路径、命名导入、`pub use`

### 2. 调用元信息与外部命令适配层

对应 `TODO.md` 中的阶段 11：

- builtin / 命令桥 API 的参数 shape 与帮助信息
- 外部命令 adapter / completion 元数据

阶段 11 当前已经落地：

- 统一 callable 元信息表：`Spec` / `CallableSpec`
- spec 当前覆盖三类：
  - ecscript builtin
  - shell extension
  - shell builtin
- 第一版 introspection builtin：
  - `help()`：按 kind / category 返回 overview 文本
  - `help("name")`：返回匹配 callable 的签名、摘要、详细说明与示例
  - `builtins()`：返回去重排序后的 builtin 名称数组
  - `extensions()`：返回去重排序后的 shell extension 名称数组
  - `commands()`：返回当前 shell 中可见命令来源数组，元素形如 `{ name, kind }`
- `commands()` 与 `help()` 当前共享同一份 specs 数据源；shell builtin 先查 specs，再补运行时命名表
- `type` / `which` 已能区分 alias、shell builtin、registered command、external command
- VS Code hover 当前在调用目标位置优先显示统一 spec 文档；未命中 spec 时再回退到 AST/debug 节点类型
- panic 安全加固：
  - `bind` 的 cooked tty 恢复由 Drop 守卫兜底
  - `after_cd` reentry flag 由 Drop 守卫兜底复位
  - `reload_rc` 以 rc 文件为 staged 状态唯一真相源重新装载

阶段 11 当前仍未完成：

- `help("name")` 与 `type` / `which` 对同名 ecscript builtin / shell builtin 的展示边界文档
- 示例插件目录整理与推荐 `.ecshrc` 组合片段
- 更通用的外部命令 adapter / help/completion provider

### 3. shell 语义补完

对应 `TODO.md` 中的阶段 12：

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
- `packages/tree-sitter-ecscript/`：tree-sitter 语法定义、external scanner、corpus 测试、highlights/locals/injections query
- `packages/vscode-ecscript/`：VS Code 扩展（语法高亮、折叠、大纲、悬停类型提示、语法错误诊断）
- `scripts/sync-vscode-assets.sh`：从 `packages/tree-sitter-ecscript` 同步 wasm 和 query 到 VS Code 插件 assets

## 编辑器工具链

`packages/tree-sitter-ecscript` 提供 ecscript 的 tree-sitter grammar，覆盖阶段 9 语法（表达式、语句、控制流、函数/lambda、`use`/`pub`、`|>` 管道、`for` range、`cmd{...}` 语法岛）。

`packages/vscode-ecscript` 基于 tree-sitter wasm 提供：

- **Semantic tokens**——按声明/控制/导入/命令/可见性分类的关键字高亮、函数/变量/参数/属性的区分着色
- **Folding ranges**——`statement_block`、`object`、`array`、`command_literal` 可折叠
- **Document symbols（大纲/面包屑）**——函数、`let` 变量、`use` 模块导入
- **Hover**——光标悬停显示 tree-sitter 节点类型名
- **Diagnostics**——tree-sitter ERROR 和 MISSING 节点转 VS Code 红色波浪线，300ms 防抖
- **多行 token 拆分**——`command_body` 等跨行节点按行拆分 semantic token
- **解析缓存**——同一文档版本的 parse tree 在 semantic tokens/folding/symbols 三个 provider 间共享缓存（版本变化时 fresh parse，暂未启用 tree-sitter 增量解析因为需要 `Tree.edit()`）

`cmd{...}` 作为语法岛处理：external scanner 维护 brace-depth/quote/escape 状态机识别边界，内部 shell 语义仍由 ecsh/ecscript runtime 处理。scanner 额外跟踪 `${...}` 展开深度避免把展开内的 `}` 当成 cmd 闭合。

所有构建通过 `just` 管理（`just test` / `just vscode` / `just vsix`），VS Code 插件 assets 由 sync 脚本同步，不手动维护两份 query 文件。
