# ecsh / ecscript 设计备忘与演进路线

> 注意：这份文档保留设计推演、阶段拆解和历史思路，内容会比当前实现更长，也可能包含尚未落地的方案。
> 如果你只想看“项目现在做到哪一步”，请先看 [status.md](/home/elaine/work/projects/ecsh/docs/status.md)；
> 如果你想看 `ecscript` 当前已经实现的语法与语义，请看 [ecscript-manual.md](/home/elaine/work/projects/ecsh/docs/ecscript-manual.md)。

## 一、语言设计总览

### 设计哲学：显式优于隐式

一切语言特性的设计遵循一个原则：**看代码的人一眼就知道发生了什么。**

- 声明变量用 `let`，而不是裸赋值（避免"x=1 是命令还是赋值？"的歧义）
- 所有操作数据的函数是**全局内置函数**（`push(arr, val)`、`run(cmd)`、`to_json(obj)`），不加在数据结构上作为方法
- Array 和 Object 分离，不混淆（不搞 Lua table 那种"同一结构同时是数组和字典"）
- 数据结构操作分两层：结构访问用短语法标记（`arr[0]` / `obj.name`），容器操作用全局内置函数（`push(arr, val)` / `to_json(obj)`）
- 四种嵌入语法各用不同定界符：`$VAR` / `${VAR}` / `$(cmd)` / `$[expr]`，互不混用
- 脚本解析和 shell 命令解析由**关键字前缀**切换，不依赖隐式类型推断

> 多写几个字符不是问题。阅读时不需要猜语义才是。

### 命名约定

- **项目名**：`ecsh`
- **语言名**：`ecscript`
- **文件扩展名**：`.ecs`
- **文档与报错措辞**：优先使用 `ecscript parser`、`ecscript runtime`、`ecscript parse error`、`ecscript runtime error`
- **实现模块路径**：MVP 阶段可以继续用 `src/script/`，避免一次性改太多；若后续模块稳定，再统一重命名为 `src/ecscript/`

### 语法策略：关键字前缀切换解析模式

每行输入先 peek 第一个 token，命中关键字则进入 **ecscript** 解析路径，否则回退到现有 shell 命令解析器。这保证了交互式使用和脚本编程共享同一套运行时，但 parser 各走各路、互不污染。

### 关键字清单（9 个）

| 关键字 | 语义 | 示例 |
|--------|------|------|
| `let` | 声明新变量（当前作用域新建） | `let x = 10` |
| `func` | 函数定义 | `func add(a, b) { ... }` |
| `if` / `else` | 条件分支 | `if x > 0 { ... } else { ... }` |
| `while` | 条件循环 | `while i <= n { ... }` |
| `for` | 数字/迭代循环 | `for i in 1..10 { ... }` / `for v in arr { ... }` |
| `return` | 函数返回 | `return x + y` |
| `break` | 退出当前循环 | `break` |
| `continue` | 跳过当前迭代 | `continue` |

### 非关键字语句：靠标识符位置区分 + 延后符号表检查

```
x = 1           → 标识符 + =         → 产出 Assign AST
x()             → 标识符 + (         → 产出 Call AST
x.y()           → 标识符 + . + ident + ( → 产出 Call AST（callee = FieldAccess(x, "y")，不引入隐式 self/this）
obj.name = val  → 标识符 + . + ident + = → 产出 FieldAssign AST
arr[0] = val    → 标识符 + [ expr ] + =  → 产出 IndexAssign AST
ls -la          → 关键字未命中 → shell 命令
```

**MVP 现在已经支持普通赋值 `=` 与复合赋值 `+= -= *= /= %=`。** 复合赋值仍然只是语法糖，但在实现上保留为独立语句节点，这样像 `arr[next_idx()] += 2` 这类左值不会被重复求值。

**Parser 不做符号表查询。** 顶层和函数体内部的 `x = 1` / `greet()` 都只做语法判断，产出 `Assign` / `Call` / `ExprStmt` 等 AST 节点。"变量是否已声明"、"标识符是不是函数"这些检查延后到 evaluator 运行阶段。好处：前向引用（`func a() { b() }` 定义在 `func b()` 之前）不产生 parse-time 错误，仅运行时报"b is not callable"。

**例外**：顶层"关键字未命中 → shell 命令"的分派仍然在 parse 阶段做，因为需要区分"这行走脚本 AST 还是走 shell 执行器"，这不是符号表问题，是语法模式决策。

### 四种嵌入语法

| 语法 | 含义 | 示例 |
|------|------|------|
| `$VAR` | 脚本作用域优先 → fallback `std::env::var` | `echo $HOME` |
| `${VAR}` | **仅** `std::env::var`，不查脚本作用域 | `echo ${HOME}/work` |
| `$(cmd)` | shell 命令替换（bash 兼容） | `echo $(date)` |
| `$[expr]` | 脚本表达式求值 | `echo $[x + 1]` |

**语义：** `$VAR` 先查脚本作用域，不存在才 fallback 环境变量。`${VAR}` 只查环境变量——花括号是显式的"我要环境变量"信号。三者互不混用：

```sh
let HOME = "/custom"
echo $HOME       # → /custom（脚本遮蔽了环境变量）
echo ${HOME}     # → /home/elaine（强取环境变量）
echo $[HOME]     # → /custom（脚本表达式，与 $HOME 一致但用于表达式场景）
```

**`$VAR` 扫描**：`.`、`/`、`-`、空格等非标识符字符自然终止变量名。`$name.c` = 变量 `name` + 字面量 `.c`。

**单引号内不做任何展开。** `echo '$[x + 1]'` 输出字面字符串 `$[x + 1]`，不需要求值。遵循 POSIX 惯例：单引号完全字面，双引号内做 `$` 展开。

### 块语法：`{}`

所有控制流和函数体使用大括号定界。单字符匹配，lexer 简单，不与外部命令名冲突。

### 函数体内默认 shell 命令模式

```sh
func build(name) {
    echo "building $name..."        # shell 命令，$name 查脚本作用域拿到参数
    gcc -O2 $name.c -o $name        # . 和 - 自然终止 $name 的扫描
}
```

规则与顶层一致：关键字启动脚本模式，其余都是 shell 命令。函数参数在脚本作用域中，`$name` 优先查到参数（`${name}` 查的是环境变量，注意区分）。

**重要约束**：在 shell 命令模式里，Object 字段访问、数组索引和任意脚本表达式都必须通过 `$[...]` 嵌入；否则就只是普通字面参数。

```sh
echo result.stderr      # 字面量 "result.stderr"
echo $[result.stderr]   # 读取对象字段值
echo $[arr[0]]          # 读取数组元素
```

---

## 二、Shell Word 模型变更

当前 shell lexer 使用 `Token::Word(String)`，`$` 展开在 lex 阶段就写死为普通字符串。新设计下 `$VAR` 需要运行时查脚本作用域，`$[expr]` 需要运行时求值，甚至 `$[...arr]` 会把一个词展开成多个 argv——这些都不能在 lex 时做死。

### 新 Word 模型：片段 AST（每个 argv 词一个 ShellWord）

```rust
struct ShellWord {
    fragments: Vec<WordFragment>,
}

enum WordFragment {
    Lit(String),                        // 纯文本字面量
    Var(String),                        // $VAR  → 执行时查作用域→fallback env
    EnvVar(String),                     // ${VAR} → 执行时只查 env
    Cmd(String),                        // $(cmd) → 执行时通过 /bin/sh -c 做命令替换
    Expr { src: String, spread: bool }, // $[expr] / $[...arr]
}
```

一个命令最终不是 `Vec<String>`，而是先得到 `Vec<ShellWord>`；每个 shell 参数各自保存自己的片段，真正的字符串拼接/展开延后到执行阶段：

```
echo Hello $HOME $[x + 1]
  → [
      ShellWord { fragments: [Lit("echo")] },
      ShellWord { fragments: [Lit("Hello")] },
      ShellWord { fragments: [Var("HOME")] },
      ShellWord { fragments: [Expr{src:"x+1", spread:false}] }
    ]
  → 执行时展开: "echo" "Hello" "/home/elaine" "11"
  → argv: ["echo", "Hello", "/home/elaine", "11"]
```

`spread: true`（即 `$[...arr]`）时，一个 fragment 展开成多个 argv。

---

## 三、解析架构

### 双层解析器

```
输入行 → tokenize
  │
  ├─ peek 第一个 token
  │     ├─ 关键字(let/if/while/for/func/return/break/continue)
  │     │     → 语句解析器（Statement Parser）
  │     │        ├─ let 语句 → parse_let()
  │     │        ├─ if 语句 → parse_if()（含 else/else if 链）
  │     │        ├─ while/for → parse_loop()
  │     │        ├─ func → parse_func()
  │     │        ├─ return/break/continue
  │     │        └─ 内部遇表达式时 → Pratt Parser
  │     │
  │     ├─ 标识符 + =             → 产出 Assign AST
  │     ├─ 标识符 + (             → 产出 Call AST
  │     ├─ 标识符 + . + 标识符 + ( → 产出 Call AST，callee = FieldAccess(...)
  │     ├─ 标识符 + . + 标识符 + = → 产出 FieldAssign AST (obj.name = val)
  │     ├─ 标识符 [ expr ] =      → 产出 IndexAssign AST (arr[0] = val)
  │     │
  │     └─ 其他 → 现有 shell 命令解析器（不动）
```

### 表达式解析：Pratt Parser（自顶向下算符优先）

用前缀解析（prefix）、中缀解析（infix）和绑定力（binding power）三维驱动，处理以下运算符：

- 前缀：`-`（负号）、`!`（逻辑非）
- 中缀：`+` `-` `*` `/` `%`（算术）、`==` `!=` `<` `>` `<=` `>=`（比较）、`&&` `||`（逻辑）、`.`（属性访问）、`(` `)`（函数调用）
- 绑定力：`*`/`/` > `+`/`-` > 比较 > 逻辑

---

## 四、运行时基座

### 动态类型系统

```rust
enum Value {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Array(Rc<RefCell<Vec<Value>>>),        // 有序列表
    Object(Rc<RefCell<HashMap<String, Value>>>), // 键值对 / 结构体
    Func(Rc<Func>),
}
```

### 分离式数据结构：Array 与 Object

不采用 Lua 的 unified Table，而是**显式分离**两种容器。理由：
- 类型错误信息更友好（"expected Array, got Object"）
- `len()` 语义天然正确（Array 就是连续存储）
- `to_json()` 序列化不需要猜类型
- 符合整体"显式优于隐式"的设计风格

| 类型 | 字面量 | 内部存储 | 用途 |
|------|--------|----------|------|
| `Array` | `[1, 2, 3]` | `Vec<Value>` | 有序列表、数字索引 `arr[0]` |
| `Object` | `{a: 1, b: 2}` | `HashMap<String, Value>` | 键值对、字段访问 `obj.name` |

不支持混合字面量（如 `{1, 2, name: "hi"}`）。Array 和 Object 各司其职，互不转换。

### Array 操作：全局内置函数

Array 是纯粹的 `Vec<Value>`，不绑任何方法。所有操作通过**全局内置函数**完成，与 `run()`、`to_json()` 风格一致：

```sh
push(arr, val)       # 末尾追加
pop(arr)             # 弹出末尾，返回弹出的值
len(arr)             # 返回长度
insert(arr, i, val)  # 指定位置插入
remove(arr, i)       # 指定位置删除
```

`push(arr, 42)` 很清晰——一眼看出这是对数组做容器操作，而不是变量赋值。

### Object 方法：函数作为字段值存入

Object 也不需要单独设计"方法"概念。方法就是存储在 Object 字段里的函数值：

```sh
let obj = {count: 0}
obj.inc = func() {
    obj.count = obj.count + 1    # 闭包捕获了 obj
}
obj.inc()
```

`obj.inc` 查 HashMap 拿到 `Value::Func`，`obj.inc()` 就是函数调用。零额外实现。

**已知问题：闭包循环引用。** `obj.method = func() { obj.x = 1 }` 会形成 `obj → func → env → obj` 的 Rc 强引用环。由于 shell 是长寿命进程，频繁使用此模式会导致内存持续增长，不是"退出即回收"能解决的。MVP 阶段的对策：
- **不鼓励**自捕获对象方法。推荐用全局函数传参：`func inc(o) { o.count = o.count + 1 }` 再 `inc(obj)`
- 若确实需要 `obj.method = func() { ... }`，接受 MVP 不回收的代价
- 演进阶段引入 Arena + 标记-清除（Mark-Sweep）彻底解决

### 字段赋值语义

`obj.name = val` 统一走 `HashMap::insert`：字段存在就覆盖，不存在就创建。不需要声明——`.` 操作符本身就是显式的字段操作标记，和 `let` 在变量层级的角色一致。不存在"这是一个命令还是赋值？"的歧义。

### 数据结构操作的两层模型

Array 和 Object 的操作分为两个正交的抽象层次，一致性体现在两者平等享有：

| 层 | 职责 | Array | Object |
|----|------|-------|--------|
| 结构访问（语法） | 读写单个元素/字段 | `arr[0]` = 索引读写 | `obj.name` = 字段读写 |
| 容器操作（内置函数） | 修改集合形状、元信息 | `push` `pop` `len` `insert` `remove` | `json` `keys` `values` 等全局内置函数 |

结构访问用专用语法（`[]` / `.`）因为这是高频操作，标记越短越好。容器操作用内置函数因为语义更重（改变形状、序列化等），显式命名更清晰。

### 原型链（预留，MVP 不做）

若未来需要对象间共享方法（类似 class / 继承），在 Object 上追加一个字段即可：

```rust
// Object 内部结构（当前）
HashMap<String, Value>

// 演进时变为：
struct Object {
    fields: HashMap<String, Value>,
    proto: Option<Rc<RefCell<Object>>>,  // 查找失败时的 fallback
}
```

字段查找 `obj.name` 的逻辑只需多一行递归：
```
1. fields 里有 name → 返回
2. fields 里没有，且 proto 存在 → proto.fields 里找
3. 递归到头 → Nil
```

原型链（或 class/继承系统）在 MVP 阶段预埋设计但不实现。先把函数、闭包、字段访问跑通再说。

### 内存管理

- MVP 阶段：`Rc<RefCell<Vec<Value>>>` / `Rc<RefCell<HashMap<String, Value>>>` 引用计数。已知闭包自捕获会形成循环引用（见上文"Object 方法"节），不鼓励此模式
- 演进阶段：引入 Arena + 标记-清除（Mark-Sweep）彻底解决循环引用

### 作用域：环境链

每个 `{}` 块进入时压入一个新 `Environment`（`HashMap<String, Value>`），链接到外层环境。变量查找沿链向上。

```sh
let x = 1          # 全局
if true {
    let y = 2       # if 块作用域
    echo $[x + y]   # y 查当前作用域，x 沿链向外查
}
# y 在此不可见
```

### 错误处理

#### 错误分层

脚本相关错误至少分三层，不混在一起：

| 层 | 类型 | 例子 | MVP 处理策略 |
|----|------|------|--------------|
| 词法/语法错误 | `ParseError` | 未闭合引号、非法 token、坏语法 | 直接报错，拒绝执行 |
| 语言运行时错误 | `RuntimeError` | 未定义变量、类型不匹配、不可调用、索引越界 | 立即停止当前脚本/函数执行 |
| shell 命令失败 | `run()` 返回的状态值 | `grep` 找不到、程序退出码非 0、被信号终止 | **不是语言错误**，作为普通返回值交给脚本判断 |

#### 错误传播接口

引入统一的结果类型，脚本内部错误只通过它们传播，绝不 panic ecsh 主进程：

```rust
type ParseResult<T> = Result<T, ParseError>;
type EvalResult<T> = Result<T, RuntimeError>;
```

控制流与错误的推荐分层：

```rust
enum ExecFlow {
    Normal,
    Break,
    Continue,
    Return(Value),
}
```

- 表达式求值：`EvalResult<Value>`
- 语句执行：`EvalResult<ExecFlow>`
- `break` / `continue` / `return` **不是错误**，单独走 `ExecFlow`
- 真正的语言错误（未定义变量、类型不匹配等）才走 `RuntimeError`

#### RuntimeError 结构

不要只用裸字符串，至少保留错误种类和可选位置信息：

```rust
struct RuntimeError {
    kind: RuntimeErrorKind,
    message: String,
    span: Option<Span>,
}
```

`RuntimeErrorKind` MVP 可先覆盖这些：

- `UndefinedVariable`
- `TypeMismatch`
- `NotCallable`
- `ArityMismatch`
- `ExpectedBool`
- `MissingField`
- `NotIndexable`
- `IndexOutOfBounds`

#### 语言级错误策略（MVP）

- **MVP 不做 `try/catch` / `throw` / 异常系统。**
- 语言运行时错误一旦发生，立即停止当前脚本或当前函数执行，向上冒泡到顶层。
- shell 命令失败不算语言错误：

```sh
let x = y          # y 未定义 → RuntimeError
if 123 { ... }     # 条件不是 Bool → RuntimeError

let r = run("grep", "x", "no_such_file")
# 这不是 RuntimeError；由脚本自己检查 r.code / r.signal
```

这个边界必须明确：**语言错误负责“解释器语义错了”，`run()` 失败负责“外部命令没成功”。**

### Truthiness 与类型强制

**不做隐式 truthy/falsy。** `if` / `while` 的条件必须求值为 `Bool`，非 Bool 报错（不学 JS/Lua 的 `0`/`""`/`nil` 隐式判定）。数值比较规则：

- `1 == 1.0` → 合法。Int 自动提升为 Float 后比较
- `"1" == 1` → 报错。不跨类型比较
- `nil == nil` → true。`nil != nil` → false

---

## 五、跨界互操作（Shell ↔ ecscript）

### Script → Shell

| 方式 | 语法 | 说明 |
|------|------|------|
| 脚本变量读取 | `$VAR` | 脚本作用域优先，不存在则 fallback `std::env::var` |
| 环境变量读取 | `${VAR}` | **仅** `std::env::var`，不查脚本作用域 |
| 表达式嵌入 | `$[expr]` | 脚本表达式求值；仅标量值可隐式转字符串，`Array/Object` 运行时报错 |
| 命令替换 | `$(cmd)` | 执行 shell 命令，输出替换到原位置 |
| 显式 JSON 转换 | `$[to_json(expr)]` | 将 `Array/Object` 显式序列化为 JSON 字符串后嵌入单个参数 |
| 参数展开（显式） | `$[...arr]` | 将数组显式拆散为多个独立的 argv 参数（降维） |

`$[arr]`、`$[to_json(arr)]` 和 `$[...arr]` 的区别：
```sh
let a = [1, 2, 3]
echo $[a]       # → RuntimeError（Array 不能隐式字符串化）
echo $[to_json(a)] # → echo "[1,2,3]"（一个 JSON 字符串参数）
echo $[...a]    # → echo 1 2 3（三个独立参数）
```

这条规则体现“显式优于隐式”：`$[expr]` 适合标量值（`String/Int/Float/Bool/Nil`）嵌入；复合值必须显式说明你想要的是 **JSON 字符串**（`to_json(obj)`）还是 **argv 展开**（`$[...arr]`）。若后续需要其他文本化策略，再单独加内置函数（如 `join(arr, ",")`），而不是让 `Array/Object` 自动变字符串。

### Shell → Script（状态捕获）

`run(cmd, arg1, arg2, ...)` 是脚本内置函数：底层 fork + pipe + execvp 执行单条命令，捕获 stdout/stderr，返回 Object。

```sh
let result = run("gcc", "-O2", "main.c", "-o", "main")
# result = { code: 0, signal: 0, stdout: "", stderr: "" }
# signal = 0 表示正常退出；signal > 0 表示被信号终止（如 SIGTERM = 15）
if result.code != 0 || result.signal != 0 {
    echo $[result.stderr]
}
```

**注意**：`run()` 的非零退出码 / 信号终止不是脚本语言错误，不走 `EvalResult::Err`；它只是返回了一个“命令失败”的普通值，是否中止流程由脚本自行决定。

**MVP 边界：`run()` 只支持单条命令 + 参数列表，不支持 shell 操作符（`|`、`&&`、`;` 等）。** 需要管道时拆成多次 `run()` 调用，或回到 shell 命令行使用。

**与 `$[...arr]` 共享展开规则：** `run("gcc", ...args)` 和 `echo $[...args]` 的 `...` 是同一套展开语义——将数组拆散为独立参数。两个入口，同一套逻辑。这里的 `...args` 是脚本函数调用参数 grammar 的一部分，等价于“把数组元素逐个追加到参数列表”。

### 数据序列化

```sh
let data = {name: "elaine", age: 25}
echo $[to_json(data)] > /tmp/data.json
echo $[to_json(data)] | jq .name
```

`to_json(table)` 内置函数将 Object/Array 序列化为 JSON 字符串。若要把脚本表达式结果送入 shell 的重定向或管道，先用 `$[...]` 把它嵌入为 shell 参数；`Array/Object` 本身不能直接通过 `$[expr]` 隐式字符串化，必须显式写成 `to_json(...)`（或未来其他专用转换函数）。`|` 管道仅在 shell 命令行模式下可用，脚本表达式内不要混用。

---

## 六、开发路线（优化版：7 阶段）

### 总体实施策略

- **先做独立 ecscript 内核，再接 shell。** 不要一开始就改 `main.rs` 和现有 shell parser；先把 `src/script/` 跑通，最后一阶段再接线。
- **每阶段都要有最小可运行产物。** 优先得到“可在单元测试或小 REPL 中运行”的子系统，而不是一次性把所有模块写完。
- **优先让 AST 干净，再让语法糖降级。** 例如 `x.y()` 直接在 parser 中降成 `Call(FieldAccess(x, "y"), ...)`，不要在 evaluator 里额外分支。
- **shell 命令模式与 script 表达式模式严格分离。** shell 里只认 `$[...]` 作为脚本表达式入口，不做隐式字段读取。

### 阶段 1：ecscript 表达式内核（已完成）

**目标**：先得到一个与 shell 完全解耦的 `expr -> Value` 子系统。  
**当前状态**：已完成，形成了 lexer → Pratt parser → evaluator 的独立表达式内核。

**已落地模块**
- [x] `src/ecscript/ast.rs`：`Expr` / `Literal` / 一元和二元运算
- [x] `src/ecscript/error.rs`：`ParseError` / `RuntimeError` / `EvalResult<T>`
- [x] `src/ecscript/value.rs`：`Value`
- [x] `src/ecscript/lexer.rs`：数字、字符串、标识符、运算符与分隔符 token
- [x] `src/ecscript/pratt.rs`：Pratt parser
- [x] `src/ecscript/eval.rs`：表达式求值入口

**本阶段已实现语法**
- [x] 字面量：`Bool` / `Int` / `Float` / `String` / `Nil`
- [x] 前缀：`-`、`!`
- [x] 中缀：`+ - * / % == != < > <= >= && ||`
- [x] 分组：`(...)`
- [x] 变量引用：`x`

**当前接口**
- [x] `parse_expr(tokens: &[Token]) -> Result<Expr, ParseError>`
- [x] `parse_expr_in(state: &mut TokenStream<'_>) -> Result<Expr, ParseError>`
- [x] `eval_expr(expr: &Expr, env: &Environment) -> EvalResult<Value>`

**本阶段已确认语义**
- [x] 结构化错误类型已建立，不再只返回字符串
- [x] 表达式 evaluator 通过 `EvalResult<T>` 传播用户可见失败
- [x] `&&` / `||` 已实现短路
- [x] byte offset 已进入 parse/runtime 错误

**测试重点（已覆盖）**
- [x] 运算符优先级与结合性
- [x] 括号覆盖优先级
- [x] 变量读取
- [x] 类型错误、未定义变量错误

**完成标准**
- [x] `1 + 2 * 3`、`!(1 < 2)`、`a + b` 等表达式能稳定求值
- [x] parse/eval 错误通过统一错误类型返回，不 panic

### 阶段 2：变量、语句与块（已完成）

**目标**：让脚本拥有最基本的“执行多条语句”的能力。  
**当前状态**：已完成，脚本已经支持多语句执行、词法作用域和 block。

**已落地模块**
- [x] `src/ecscript/env.rs`：环境链 `Environment`
- [x] `src/ecscript/ast.rs`：`Stmt::{Let, Assign, ExprStmt, Block}` 与 `AssignTarget`
- [x] `src/ecscript/parser.rs`：语句 parser
- [x] `src/ecscript/eval.rs`：`eval_script` / `eval_stmt` / `eval_block`

**本阶段已实现语法**
- [x] `let x = expr;`
- [x] `x = expr;`
- [x] 块 `{ ... }`
- [x] 表达式语句 `foo + bar;`

**本阶段明确不做**
- [x] 复合赋值（如 `+=` / `-=`）在当时阶段范围内暂不支持；已于阶段 5.5 落地

**语义要求（已对齐）**
- [x] parser 不查符号表，只产出 AST
- [x] 赋值时由 evaluator 检查变量是否存在
- [x] 进入 `{}` 创建新作用域，退出后局部绑定失效
- [x] 错误通过 `EvalResult<T>` 传播
- [x] 语句执行统一走 `ExecFlow`，为后续控制流扩展留出接口

**输入模型**
- [x] 已有整块解析能力：`parse_script(tokens) -> Vec<Stmt>`
- [ ] 多行交互续行规则仍属于后续顶层 shell 集成阶段

**测试重点（已覆盖）**
- [x] 变量遮蔽
- [x] 父作用域读取
- [x] 块退出后的可见性
- [x] 未声明赋值报错
- [x] 运行时错误会正确中止后续语句

**完成标准**
- [x] 可以执行一个由多条 `let/assign/block` 组成的小脚本
- [x] `parse_script` 已能处理 block 结构，不再局限单表达式解析

### 阶段 3：复合数据与访问语法（已完成）

**目标**：先把 Array/Object 跑通，再做依赖它们的循环和函数例子。  
**当前状态**：已完成，数组/对象、访问语法、赋值和 builtin 都已接入当前 evaluator。

**已落地模块**
- [x] `src/ecscript/ast.rs`：数组/对象字面量、索引、字段访问、调用、`Range`
- [x] `src/ecscript/builtin.rs`：`len/push/pop/insert/remove/to_json/keys/values`
- [x] `src/ecscript/eval.rs`：容器读写与 builtin 分发
- [x] `src/ecscript/value.rs`：`Array` / `Object` / `Builtin`

**本阶段已实现语法**
- [x] Array 字面量：`[1, 2, 3]`
- [x] Object 字面量：`{name: "elaine"}`
- [x] 字段访问：`obj.name`
- [x] 对象索引：`obj["name"]`
- [x] 数组索引：`arr[0]`
- [x] 字段赋值：`obj.name = expr`
- [x] 索引赋值：`arr[0] = expr` / `obj["name"] = expr`

**实现注意（已落地设计）**
- [x] `{k: v}` 与 block `{ ... }` 在 parser 中明确区分
- [x] `obj.x = ...` / `arr[i] = ...` 通过 `AssignTarget::{Field, Index}` 表达，而不是单独语句种类
- [x] object literal 的裸标识符 key 在 parser 阶段直接降成字符串
- [x] 容器操作统一走全局内置函数，不加隐式 `self/this`
- [x] builtin 通过 `Environment::get()` fallback 注入，允许用户变量自然遮蔽内置名

**测试重点（已覆盖）**
- [x] Object/Array 字面量解析
- [x] 字段/索引读写
- [x] `values(obj)` / `keys(obj)` / `to_json(obj)`
- [x] 越界索引、类型不匹配、循环引用检测

**完成标准**
- [x] `obj.name`、`arr[0]`、`to_json(data)` 都能在脚本 evaluator 中工作

### 阶段 4：控制流（已完成）

**目标**：让脚本能写出非平凡流程，但仍然不接 shell。  
**当前状态**：已完成，控制流已经打通并带有较完整的错误处理和测试。

**已落地模块**
- [x] `Stmt` 已扩展出 `If` / `While` / `ForIn` / `ForRange` / `Break` / `Continue`
- [x] `ExecFlow` 已承担 `Normal` / `Break` / `Continue` 的传播职责
- [x] parser / evaluator / manual 已同步到控制流语义

**本阶段已实现语法**
- [x] `if / else if / else`
- [x] `while`
- [x] `for i in 1..10`
- [x] `for i in 1..=10`
- [x] `for v in arr`
- [x] `for k in obj`
- [x] `for v in values(obj)`（通过普通表达式求值为数组后工作）
- [x] `break` / `continue`

**语义要求（已对齐）**
- [x] `if` / `while` 条件必须是 `Bool`
- [x] `for k in obj` 当前遍历排序后的键名
- [x] `for v in values(obj)` 通过 stage3 builtin 自然成立，不需要额外语法特判
- [x] `break/continue` 只允许在循环内部；顶层使用会报运行时错误
- [x] `break/continue` 通过控制流枚举传播，不混入普通表达式错误
- [x] `for v in arr` 当前采用**迭代快照**语义，避免循环体再次借用同一 `RefCell` 时发生冲突

**测试重点（已覆盖）**
- [x] 条件判断的 Bool 限制
- [x] range 左闭右开 / 左闭右闭
- [x] `break/continue` 对循环流程的影响
- [x] 顶层 `break/continue` 的错误路径
- [x] 数组快照遍历和对象 key 稳定遍历

**完成标准**
- [x] 可以用纯脚本实现计数循环、分支和遍历示例

### 阶段 5：函数与闭包（已完成）

**目标**：补上脚本的抽象能力，并为对象函数字段、返回函数值和状态闭包做好铺垫。
**当前状态**：已完成。命名函数、匿名函数、`return`、强闭包捕获、对象字段中的函数值调用都已经落地；当前保留的已知边界是“闭包捕获只自动传一层”，多跳透传仍是后续增强项。

**新增/扩展项**
- [x] `Value::Function(Rc<Function>)`
- [x] `Stmt::FuncDeclare`
- [x] `Stmt::Return`
- [x] `Expr::FuncLiteral`（lambda / func literal）
- [x] `ExecFlow::Return(...)`
- [x] 将环境绑定升级为 `HashMap<String, Binding>`，其中 `Binding = Direct(Value) | Shared(Slot)`

**本阶段语法**
- [x] `func name(args) { ... }`
- [x] `return expr;`
- [x] `return;`
- [x] `(args) => expr` / `(args) => { ... }` 作为表达式
- [x] 普通函数调用 `f(x, y)`
- [x] `obj.inc()` 仍然只是 `Call(FieldAccess(obj, "inc"), [])`

**当前语义（已落地）**

- [x] 调用函数时新建 call frame
- [x] call frame 中放参数、局部变量和函数自己的名字
- [x] 当前查找顺序为 call frame → captures → global/root → builtin fallback
- [x] 当前**不**透传调用者局部变量，避免动态作用域
- [x] 命名函数递归依靠 call frame 重新绑定函数名
- [ ] 多跳闭包捕获自动透传仍未完成（`return () => () => x` 仍需中间层显式引用）

**已采用运行时模型：强闭包，但不捕获整个环境**

不要让函数对象直接强引用“定义时整块 `Environment`”。  
更推荐脚本语言常见的 **slot / cell / upvalue** 路线：

- [x] 定义 `type Slot = Rc<RefCell<Value>>`
- [x] `Environment` 内部改存 `HashMap<String, Binding>`
- [x] `let` 默认写入 `Binding::Direct(Value)`
- [x] `get(name)` 能同时读取 `Direct` / `Shared`
- [x] `set(name, value)` 能原地写回共享 slot

**闭包捕获策略**

- [x] 函数对象不捕获整块环境，而是只捕获**自由变量对应的 slot**
- [x] `Function` 内部保存 `captures: HashMap<String, Slot>`
- [x] 闭包共享的是“变量绑定”，不是定义时的值快照
- [x] 因此闭包可以正确支持：
  - [x] 读取外层变量的最新值
  - [x] 修改外层变量绑定
  - [x] 多个闭包共享同一个被捕获局部变量

**调用时环境组织**

调用时仍然新建 call frame，但查找顺序不再是“整条旧环境链透传”，而是：

1. [x] 当前调用帧：参数、局部变量、递归函数名
2. [x] captures：定义时捕获的自由变量 slot
3. [x] global/root：全局变量
4. [x] builtin fallback

也就是说：

- [x] 局部变量和参数始终优先于 capture
- [x] capture 优先于全局
- [x] 全局变量不必全部捕获，可以保留单独的 global/root 层

**递归支持**

- [x] 递归函数名不要作为普通自由变量捕获
- [x] 调用函数时，把函数自己的名字重新绑定到 call frame
- [x] 这样函数体里的 `fact(...)` 先在当前调用帧命中自己
- [x] 递归查找能力与闭包捕获职责分离

**为什么采用这条路线**

- [x] 更接近 Python / Lua 这类脚本语言的强闭包语义
- [x] 捕获的是 slot，不是值快照；`make_counter()` 这类例子才能成立
- [x] 避免最经典的“整块环境被函数对象直接强持有”
- [x] 不需要用 `Weak` 去牺牲逃逸闭包的可靠性

**测试重点**
- [x] 普通函数调用与参数绑定
- [x] `return` 的非局部退出
- [x] `return;` 返回 `nil`
- [x] 顶层 `return` 报错
- [x] 当前函数只读取调用时 `call frame -> captures -> global/root`，不读取调用者局部变量
- [x] 闭包读取外层变量
- [x] 闭包修改外层变量
- [x] `make_counter()` 这种共享状态闭包
- [x] 多个闭包共享同一个 captured slot
- [x] 递归（具名函数递归 / 对象中的匿名递归调用）
- [x] 对象字段里的函数值调用

**完成标准**
- [x] `func add(a, b) { return a + b; }`
- [x] `func make_counter() { let x = 0; return () => { x = x + 1; return x; }; }`
- [x] `let c = make_counter(); c(); c();` 能看到共享状态递增
- [x] `obj.inc = () => { ... }; obj.inc()` 可运行

### 阶段 5.5：语言易用性打磨（进行中）

**目标**：在进入 shell 集成之前，先补一批“低风险、高体感收益”的语言特性，让 ecscript 从“语义已经成立”进一步提升到“写起来顺手、报错可读”。

**这一阶段的原则**

- 优先做 **lexer / parser 局部改动就能完成** 的特性
- 优先做 **日常脚本高频出现** 的特性
- 避免在这一阶段提前把语言推向“全面 block value / 模块系统 / shell bridge”这种跨模块大改

**P0：应该优先做**

- [x] 注释：`// ...` 与 `/* ... */`
- [x] 诊断从 byte offset 升级到 `line:column`，并能展示源代码行

**为什么优先**

- [x] 注释几乎是最低成本、最高体感收益的缺口；没有注释，脚本一变长可维护性就明显下降
- [x] 当前错误对象虽然内部仍保存 offset，但解释器层已经能稳定格式化出 `line:column + 源码行 + ^`

**P1：高 ROI，但比 P0 稍晚**

- [x] 原始字符串：`r"..."`
- [x] 复合赋值：`+= -= *= /= %=`
- [ ] 函数 / lambda 的尾表达式返回值

**尾表达式返回值的阶段边界**

- [ ] 若要做，可先只支持函数 / lambda 体的隐式尾返回
- [ ] 暂时**不要**把普通 block 全部升级为有值表达式
- [ ] 例如先支持：
  - [ ] `func add(a, b) { a + b }`
  - [ ] `let inc = (x) => { x + 1 }`
- [ ] 这样能提升函数书写体验，但不会过早引入“block value”整套语义

**P2：可继续后放**

- [ ] 双引号字符串插值（如 `"hello ${name}"` 或等价设计）
- [ ] 更完整的字符串字面量族（多行字符串、更丰富的 raw string 定界等）

**为什么暂缓**

- [ ] 字符串插值会和 shell 侧 `${VAR}` / `$[expr]` 设计形成语义邻接，最好在 shell 集成方向更清晰后再定
- [ ] 这类特性提升体验明显，但实现和语法决策都比注释 / 原始字符串 / 复合赋值更容易牵一发动全身

**明确不放在 5.5 的内容**

- [ ] `run()` builtin：它更接近 shell bridge，应归到阶段 6/7
- [ ] 模块系统：属于工程化扩展，不是当前 MVP 的阻塞项
- [ ] `try/catch`：当前语言仍然适合保持“运行时错误终止执行”的简单模型
- [ ] 模式匹配 / class / 原型链：都不是当前投入产出比最高的方向

**推荐推进顺序**

1. [x] 注释
2. [x] 行列号诊断
3. [x] 原始字符串 `r"..."`
4. [x] 复合赋值
5. [ ] 函数 / lambda 尾表达式返回值

**完成标准**

- [x] 可以在脚本里自然写注释说明算法和数据
- [x] parse/runtime error 至少能稳定报出 `line:column`（解释器层格式化 API 已提供；入口层默认接线仍可继续补）
- [x] 相邻表达式起始 token（如 `42 true` / `42"hi"`）会给出更明确的 parse error，而不是只停在模糊的分号报错
- [x] 常见路径 / 正则可以用原始字符串减少转义噪音
- [x] 计数器和累加逻辑不再大量重复 `x = x + 1`
- [ ] 简短函数和 lambda 不再被 `return` / 分号样板代码淹没

### 阶段 6：ShellWord 与四种嵌入语法（已完成）

**目标**：把脚本值安全地桥接到现有 shell 执行器。

**已完成**
- [x] `src/types.rs`：`ShellWord` / `WordFragment` 数据模型，`Command` 改用 `ShellWord`
- [x] `src/lexer.rs`：`handle_dollar` 产出 `Var`/`EnvVar`/`Cmd`/`Expr` fragment，
      不再在 lex 阶段展开，引号/深度计数/转义联合判定已覆盖
- [x] `src/parser.rs`：`Token::Word(ShellWord)` 通路
- [x] `src/executor/expand.rs`：`expand_argv` / `expand_shell_word` / `expand_cmd`
      （`/bin/sh -c` + stdout capture），四种语法展开逻辑已实现
- [x] `libc` 依赖已添加
- [x] `ShellState.script_env: Environment<'static>` 全局根环境已接入
- [x] 所有现有测试适配为新数据模型，lexer 测试验证 fragment 产出

**后续可继续做**
- [ ] 调用参数展开 `...expr` grammar
- [x] `launch.rs` 前的执行路径已统一接入运行时展开（包含 builtin / external / pipeline / redirection）
- [x] $[...arr] spread 的端到端验证
- [x] 执行时展开的完整测试（含 builtin 参数、动态命令头、redirection、$(cmd)）

### 阶段 7：顶层集成与文件执行

**目标**：把独立 ecscript 内核真正接到 ecsh 上。
**当前状态**：在接回 `ecsh` 主入口之前，已经有独立 `ecscript` binary 可用于 REPL、stdin、文件执行和 `-e`。

**涉及的现有模块**
- [ ] `src/main.rs`：统一入口分派
- [ ] `src/input.rs`：续行读取与 `... ` prompt
- [ ] `src/parser.rs` 或新增 glue 模块：顶层关键字分派
- [ ] `src/lib.rs`：导出 script 模块

**本阶段要做的事**
- [ ] 顶层 parser 集成：关键字开头走 ecscript parser，其他走现有 shell parser
- [ ] 函数体内部沿用同一规则：关键字语句 vs shell 命令
- [ ] `ecsh foo.ecs`：走文件级 parser + evaluator
- [ ] `~/.ecshrc`：走与 `.ecs` 文件相同的文件级入口
- [ ] `source` / `.`：也走同一套文件级 parser + evaluator
- [ ] continuation prompt：`{}` / 引号未闭合时继续读

**集成测试重点**
- [ ] 顶层 `let/func/if` 与普通 shell 命令共存
- [ ] 在函数体内执行 shell 命令并读取脚本变量
- [ ] 文件执行、`source`、`.ecshrc` 共用同一语义

**完成标准**
- [ ] 交互模式、脚本文件模式、`source` 模式三者行为一致
- [ ] 现有 shell 功能（管道、重定向、job control）不被 ecscript 集成破坏

---

### 阶段 7.5：Shell 诊断与交互收口（已完成）

**目标**：在进入顶层脚本集成前，先把 shell 侧的错误模型、续行行为和交互细节收成一套稳定接口。

**已完成**

- [x] Shell `lexer.rs` 的 `tokenize()` 和 `handle_dollar()` 改为返回 `ParseError`
      （携带 offset 和 incomplete 标志），不再用裸 `String`
- [x] Shell `parser.rs` 的 `parse_line()` 及所有子函数改为返回 `ParseError`
- [x] 未闭合引号/`${}`/`$()`/`$[]` 统一标记为 `ParseError::incomplete`
- [x] shell 主循环已根据 `incomplete` 继续读续行输入
- [x] 续行提示符 `... ` 已接入
- [x] 历史记录改为按完整命令写入，而不是按续行片段写入
- [x] lexer/parser 测试已适配为比对 `ParseError.message`
- [x] smoke 已覆盖跨行双引号、跨行 `$[...]` 和 EOF 下的不完整输入报错
- [x] smoke 已覆盖跨行 `$(...)`
- [x] shell parse 错误输出已升级为 `line:column + 源码行 + caret`
- [x] 跨行 `${...}` 的续行 / EOF 边界测试已补齐
- [x] 续行中的 Ctrl-C 单元测试已补齐
- [x] shell parse 错误格式化已收束到独立模块

**可继续演进的整理项**

- [ ] shell 侧后续若出现新的运行时诊断类型，可继续沿同一接口扩展
- [ ] shell 与 `ecscript` 的错误格式若需要完全统一，仍可继续抽公共辅助层

**7.5 可选扩展**

这些能力仍然属于 shell 交互层或使用体验层，可以作为 7.5 的后续扩展继续推进：

### 高优先
- [x] **Tab 补全** — `src/completion.rs` 已接入 rustyline，支持命令名与文件路径补全
- [x] **alias / unalias 命令** — alias 已在 parser 阶段做顶层首词展开，内置命令已接入
- [x] **信号处理增强** — `trap EXIT|INT` 已接入；前台 job 的默认信号恢复逻辑保持原有实现

### 中优先
- [x] **更多内置命令** — `type`、`which`、`history` 已接入
- [ ] **更多内置命令** — `read`、`shift`

### 阶段 8：Shell 语义补完

**目标**：在顶层脚本集成与 7.5 收口完成后，继续补上传统 shell 语义缺口。

**输入与展开**

- [ ] here-doc
- [ ] 通配符展开 (globbing)

**执行模型**

- [ ] subshell `()`

**作业控制与执行语义**

- [ ] 更完整 job control 语义
- [ ] 管道增强：`|&` 同时重定向 stderr、`!` 取反退出码

## 八、技术债

- [ ] 去掉 `run_command` 里的 "starting... / ending." 调试输出
- [ ] `main.rs` 里 `run_parsed_line` 的 clone 开销
- [ ] parser 的 `ParsedLine` 从 Box 改成 Rc 或 arena

---

## 附：展开与解析规则速查表

### 一、顶层入口分派（每行输入的第一个 token）

| 输入 | parser 判定 | 示例 |
|------|-----------|------|
| `let` | 脚本语句（声明） | `let x = 10` |
| `func` | 脚本语句（函数定义） | `func add(a,b) { ... }` |
| `if` | 脚本语句（条件） | `if x > 0 { ... }` |
| `while` | 脚本语句（循环） | `while i <= n { ... }` |
| `for` | 脚本语句（循环） | `for i in 1..10 { ... }` |
| `return` | 脚本语句（返回） | `return x + y` |
| `break` | 脚本语句 | `break` |
| `continue` | 脚本语句 | `continue` |
| `标识符 + =` | 产出 Assign AST（evaluator 阶段才检查是否已声明） | `x = 5` |
| `标识符 + (` | 产出 Call AST（evaluator 阶段才检查是否可调用） | `greet("hi")` |
| `标识符 . 标识符 + (` | 产出 Call AST（callee = FieldAccess(...)） | `obj.inc()` |
| `标识符 . 标识符 + =` | 产出 FieldAssign AST | `obj.name = val` |
| `标识符 [ expr ] =` | 产出 IndexAssign AST | `arr[0] = val` |
| 其他一切 | shell 命令 | `ls -la` / `echo hi` / `gcc main.c` |

### 二、`$` 展开（shell 命令行和双引号内）

| 语法 | 查什么 | 结果 | 示例 |
|------|--------|------|------|
| `$VAR` | 脚本作用域优先 → fallback `std::env::var` | 字符串，单参数 | `echo $HOME` |
| `${VAR}` | **仅** `std::env::var`（不查脚本作用域） | 字符串，单参数 | `echo ${HOME}/work` |
| `$(cmd)` | 执行 shell 命令 | 通过 `/bin/sh -c` 捕捉 stdout，单参数 | `echo $(date)` |
| `$[expr]` | 脚本表达式求值 | 标量值转字符串；`Array/Object` 报错 | `echo $[x + 1]` |
| `$[to_json(expr)]` | 脚本表达式求值并序列化为 JSON | JSON 字符串，单参数 | `echo $[to_json(obj)]` |
| `$[...arr]` | 脚本表达式求值 + 展开 | 数组拆散为多个参数 | `echo $[...a]` |

**优先级**: `$VAR` 先查脚本作用域，不存在才 fallback 到环境变量。`${VAR}` 只查环境变量——花括号是显式的"我要环境变量"的信号。若脚本声明了 `let HOME = "/x"`，`$HOME` 取脚本值 `/x`，`${HOME}` 仍取环境变量值。

**`$VAR` 变量名扫描**：`.`、`/`、`-`、空格等非标识符字符会自然终止变量名。`$name.c` = 变量 `name` + 字面量 `.c`，不需要花括号。

**`$(cmd)` 嵌套规则**：使用深度计数 + 引号状态 + 转义联合判定。`$(` 初始 depth=1；在双引号/单引号内的 `)` 不参与计数；`\` 转义的 `\)` 不参与计数。仅当 depth 归零且不在引号内时才结束。

**单引号内**：全部 `$` 语法按字面量输出，不做任何展开。

**双引号内**：`$VAR`、`${VAR}`、`$(cmd)`、`$[expr]` 都生效。

### 三、脚本模式下的值

| 语法 | 类型 | 示例 |
|------|------|------|
| `true` / `false` | Bool | `let flag = true` |
| `42` | Int | `let n = 42` |
| `3.14` | Float | `let pi = 3.14` |
| `"hello"` | String | `let s = "hello"` |
| `r"raw"` | String（原始字符串，不处理转义） | `let s = r"c:\tmp\$HOME"` |
| `nil` | Nil | `let x = nil` |
| `[1, 2, 3]` | Array | `let a = [1, 2]` |
| `{k: "v"}` | Object | `let o = {name: "hi"}` |

**脚本字符串插值**：脚本模式的双引号内，`$name` 查脚本作用域，`${VAR}` 仍然**只查环境变量**，`$[expr]` 也生效。若需要显式表达"这里是脚本表达式，不是环境变量"，用 `$[name]`：
```sh
let name = "elaine"
let msg = "hello $name"          # → "hello elaine"
let home = "home = ${HOME}"      # → 读取环境变量 HOME
let exact = "[$[name]]"          # → "[elaine]"
let calc = "result: $[1 + 2]"    # → "result: 3"
```

### 四、数据结构操作速查

| 操作 | Array | Object |
|------|-------|--------|
| 字面量 | `[1, 2, 3]` | `{a: 1, b: 2}` |
| 索引/字段 | `arr[0]` | `obj.name` / `obj["key"]` |
| 赋值 | `arr[0] = val` | `obj.name = val` |
| 追加 | `push(arr, val)` | — |
| 弹出 | `pop(arr)` | — |
| 长度 | `len(arr)` | — |
| 插入 | `insert(arr, i, val)` | — |
| 删除 | `remove(arr, i)` | — |
| 序列化 | `to_json(arr)` | `to_json(obj)` |

### 五、`run()` 函数

```sh
let r = run("gcc", "-O2", "main.c")
# r = { code: 0, signal: 0, stdout: "", stderr: "" }
```

- MVP 只支持单条命令 + 参数列表，不支持 shell 操作符（`|`/`&&`/`;`）
- 数组展开：`run("echo", ...args)` 共用 `$[...]` 的展开规则
- 需要管道时拆分多次 `run()` 调用或回到 shell 命令行

### 六、`for` 循环语义

```sh
for i in 1..10 { ... }        # 左闭右开: 1,2,...,9
for i in 1..=10 { ... }       # 左闭右闭: 1,2,...,10
for v in arr { ... }          # 遍历 Array 的元素值
for k in obj { ... }          # 遍历 Object 的键名
for v in values(obj) { ... }  # 遍历 Object 的值
```

### 七、引号规则

| 引号类型 | 上下文 | \$ 展开 | 换行 | 用途 |
|---------|--------|---------|------|------|
| `'...'` | shell + script | 否 | 否 | 纯字面量 |
| `"..."` | shell | `$VAR`/`${VAR}`/`$(cmd)`/`$[expr]` | 否 | 带展开的字符串 |
| `"..."` | script | `$name`（脚本变量）/ `${VAR}`（环境变量）/ `$[expr]` | 否 | 脚本字符串插值 |
