# ecsh 脚本语言设计与开发路线

## 一、语言设计总览

### 设计哲学：显式优于隐式

一切语言特性的设计遵循一个原则：**看代码的人一眼就知道发生了什么。**

- 声明变量用 `let`，而不是裸赋值（避免"x=1 是命令还是赋值？"的歧义）
- 所有操作数据的函数是**全局内置函数**（`push(arr, val)`、`run(cmd)`、`json(obj)`），不加在数据结构上作为方法
- Array 和 Object 分离，不混淆（不搞 Lua table 那种"同一结构同时是数组和字典"）
- 数据结构操作分两层：结构访问用短语法标记（`arr[0]` / `obj.name`），容器操作用全局内置函数（`push(arr, val)` / `json(obj)`）
- 四种嵌入语法各用不同定界符：`$VAR` / `${VAR}` / `$(cmd)` / `$[expr]`，互不混用
- 脚本解析和 shell 命令解析由**关键字前缀**切换，不依赖隐式类型推断

> 多写几个字符不是问题。阅读时不需要猜语义才是。

### 语法策略：关键字前缀切换解析模式

每行输入先 peek 第一个 token，命中关键字则进入脚本语言解析路径，否则回退到现有 shell 命令解析器。这保证了交互式使用和脚本编程共享同一套运行时，但 parser 各走各路、互不污染。

### 关键字清单（9 个）

| 关键字 | 语义 | 示例 |
|--------|------|------|
| `let` | 声明新变量（当前作用域新建） | `let x = 10` |
| `fn` | 函数定义 | `fn add(a, b) { ... }` |
| `if` / `else` | 条件分支 | `if x > 0 { ... } else { ... }` |
| `while` | 条件循环 | `while i <= n { ... }` |
| `for` | 数字/迭代循环 | `for i in 1..10 { ... }` / `for v in arr { ... }` |
| `return` | 函数返回 | `return x + y` |
| `break` | 退出当前循环 | `break` |
| `continue` | 跳过当前迭代 | `continue` |

### 非关键字语句：靠标识符位置区分 + 延后符号表检查

```
x = 1           → 标识符 + =         → 产出 Assign AST
x += 1          → 标识符 + +=        → 产出 Assign AST（复合赋值）
x()             → 标识符 + (         → 产出 Call AST
x.y()           → 标识符 + . + ident + ( → 产出 Call AST（callee = FieldAccess(x, "y")，不引入隐式 self/this）
obj.name = val  → 标识符 + . + ident + = → 产出 FieldAssign AST
arr[0] = val    → 标识符 + [ expr ] + =  → 产出 IndexAssign AST
ls -la          → 关键字未命中 → shell 命令
```

**Parser 不做符号表查询。** 顶层和函数体内部的 `x = 1` / `greet()` 都只做语法判断，产出 `Assign` / `Call` / `ExprStmt` 等 AST 节点。"变量是否已声明"、"标识符是不是函数"这些检查延后到 evaluator 运行阶段。好处：前向引用（`fn a() { b() }` 定义在 `fn b()` 之前）不产生 parse-time 错误，仅运行时报"b is not callable"。

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
fn build(name) {
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
    Cmd(String),                        // $(cmd) → 执行时 fork 子 shell
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
  │     ├─ 关键字(let/if/while/for/fn/return/break/continue)
  │     │     → 语句解析器（Statement Parser）
  │     │        ├─ let 语句 → parse_let()
  │     │        ├─ if 语句 → parse_if()（含 else/else if 链）
  │     │        ├─ while/for → parse_loop()
  │     │        ├─ fn → parse_fn()
  │     │        ├─ return/break/continue
  │     │        └─ 内部遇表达式时 → Pratt Parser
  │     │
  │     ├─ 标识符 + (= 或 +=)     → 产出 Assign AST
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
- `json()` 序列化不需要猜类型
- 符合整体"显式优于隐式"的设计风格

| 类型 | 字面量 | 内部存储 | 用途 |
|------|--------|----------|------|
| `Array` | `[1, 2, 3]` | `Vec<Value>` | 有序列表、数字索引 `arr[0]` |
| `Object` | `{a: 1, b: 2}` | `HashMap<String, Value>` | 键值对、字段访问 `obj.name` |

不支持混合字面量（如 `{1, 2, name: "hi"}`）。Array 和 Object 各司其职，互不转换。

### Array 操作：全局内置函数

Array 是纯粹的 `Vec<Value>`，不绑任何方法。所有操作通过**全局内置函数**完成，与 `run()`、`json()` 风格一致：

```sh
push(arr, val)       # 末尾追加
pop(arr)             # 弹出末尾，返回弹出的值
len(arr)             # 返回长度
insert(arr, i, val)  # 指定位置插入
remove(arr, i)       # 指定位置删除
```

`push(arr, 42)` 比 `arr += 42` 清晰——一眼看出这是对数组做操作，不是普通赋值。

### Object 方法：函数作为字段值存入

Object 也不需要单独设计"方法"概念。方法就是存储在 Object 字段里的函数值：

```sh
let obj = {count: 0}
obj.inc = fn() {
    obj.count = obj.count + 1    # 闭包捕获了 obj
}
obj.inc()
```

`obj.inc` 查 HashMap 拿到 `Value::Func`，`obj.inc()` 就是函数调用。零额外实现。

**已知问题：闭包循环引用。** `obj.method = fn() { obj.x = 1 }` 会形成 `obj → fn → env → obj` 的 Rc 强引用环。由于 shell 是长寿命进程，频繁使用此模式会导致内存持续增长，不是"退出即回收"能解决的。MVP 阶段的对策：
- **不鼓励**自捕获对象方法。推荐用全局函数传参：`fn inc(o) { o.count = o.count + 1 }` 再 `inc(obj)`
- 若确实需要 `obj.method = fn() { ... }`，接受 MVP 不回收的代价
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

引入 `EvalResult<T>`——脚本内部错误（未定义变量、类型不匹配）只通过此类型传播，绝不 panic ecsh 主进程。

### Truthiness 与类型强制

**不做隐式 truthy/falsy。** `if` / `while` 的条件必须求值为 `Bool`，非 Bool 报错（不学 JS/Lua 的 `0`/`""`/`nil` 隐式判定）。数值比较规则：

- `1 == 1.0` → 合法。Int 自动提升为 Float 后比较
- `"1" == 1` → 报错。不跨类型比较
- `nil == nil` → true。`nil != nil` → false

---

## 五、跨界互操作（Shell ↔ Script）

### Script → Shell

| 方式 | 语法 | 说明 |
|------|------|------|
| 脚本变量读取 | `$VAR` | 脚本作用域优先，不存在则 fallback `std::env::var` |
| 环境变量读取 | `${VAR}` | **仅** `std::env::var`，不查脚本作用域 |
| 表达式嵌入 | `$[expr]` | 脚本表达式求值，结果转为字符串嵌入命令参数 |
| 命令替换 | `$(cmd)` | 执行 shell 命令，输出替换到原位置 |
| 参数展开（隐式） | `$[arr]` | 将表达式结果转为字符串嵌入单个参数 |
| 参数展开（显式） | `$[...arr]` | 将数组显式拆散为多个独立的 argv 参数（降维） |

`$[arr]` 和 `$[...arr]` 的区别：
```sh
let a = [1, 2, 3]
echo $[a]       # → echo "1 2 3"（一个参数）
echo $[...a]    # → echo 1 2 3（三个独立参数）
```

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

**MVP 边界：`run()` 只支持单条命令 + 参数列表，不支持 shell 操作符（`|`、`&&`、`;` 等）。** 需要管道时拆成多次 `run()` 调用，或回到 shell 命令行使用。

**与 `$[...arr]` 共享展开规则：** `run("gcc", ...args)` 和 `echo $[...args]` 的 `...` 是同一套展开语义——将数组拆散为独立参数。两个入口，同一套逻辑。这里的 `...args` 是脚本函数调用参数 grammar 的一部分，等价于“把数组元素逐个追加到参数列表”。

### 数据序列化

```sh
let data = {name: "elaine", age: 25}
echo $[json(data)] > /tmp/data.json
echo $[json(data)] | jq .name
```

`json(table)` 内置函数将 Object/Array 序列化为 JSON 字符串。若要把脚本表达式结果送入 shell 的重定向或管道，先用 `$[...]` 把它嵌入为 shell 参数；`|` 管道仅在 shell 命令行模式下可用，脚本表达式内不要混用。

---

## 六、开发路线（优化版：7 阶段）

### 总体实施策略

- **先做独立脚本内核，再接 shell。** 不要一开始就改 `main.rs` 和现有 shell parser；先把 `src/script/` 跑通，最后一阶段再接线。
- **每阶段都要有最小可运行产物。** 优先得到“可在单元测试或小 REPL 中运行”的子系统，而不是一次性把所有模块写完。
- **优先让 AST 干净，再让语法糖降级。** 例如 `x.y()` 直接在 parser 中降成 `Call(FieldAccess(x, "y"), ...)`，不要在 evaluator 里额外分支。
- **shell 命令模式与 script 表达式模式严格分离。** shell 里只认 `$[...]` 作为脚本表达式入口，不做隐式字段读取。

### 阶段 1：脚本表达式内核（最小可运行单元）

**目标**：先得到一个与 shell 完全解耦的 `expr -> Value` 子系统。

**建议新增文件**
- [ ] `src/script/mod.rs`
- [ ] `src/script/ast.rs`：`Expr`、一元/二元运算、字面量、变量引用
- [ ] `src/script/error.rs`：`ParseError` / `EvalError`
- [ ] `src/script/value.rs`：`Value` 枚举 + `Display`
- [ ] `src/script/lexer.rs`：数字、字符串、标识符、运算符 token
- [ ] `src/script/pratt.rs`：Pratt parser
- [ ] `src/script/eval.rs`：表达式求值入口

**本阶段只做这些语法**
- [ ] 字面量：`Bool` / `Int` / `Float` / `String` / `Nil`
- [ ] 前缀：`-`、`!`
- [ ] 中缀：`+ - * / % == != < > <= >= && ||`
- [ ] 分组：`(...)`
- [ ] 变量引用：`x`

**推荐暴露的最小 API**
- [ ] `parse_expr(src: &str) -> Result<Expr, ParseError>`
- [ ] `eval_expr(expr: &Expr, env: &Environment) -> EvalResult<Value>`
- [ ] 或一步到位：`eval_expr_src(src: &str, env: &Environment) -> EvalResult<Value>`

**开发辅助**
- [ ] 一个独立 REPL（可以是临时 dev harness），输入表达式后打印值

**测试重点**
- [ ] 运算符优先级与结合性
- [ ] 括号覆盖优先级
- [ ] 变量读取
- [ ] 类型错误、未定义变量错误

**完成标准**
- [ ] `1 + 2 * 3`、`!(1 < 2)`、`a + b` 等表达式能稳定求值
- [ ] parse/eval 错误都通过统一错误类型返回，不 panic

### 阶段 2：变量、语句与块

**目标**：让脚本拥有最基本的“执行多条语句”的能力。

**建议新增/扩展文件**
- [ ] `src/script/env.rs`：环境链 `Environment`
- [ ] `src/script/stmt.rs`：`Stmt` AST（`Let` / `Assign` / `ExprStmt` / `Block`）
- [ ] `src/script/parser.rs`：语句 parser（先不做控制流/函数）
- [ ] 在 `eval.rs` 中新增 `eval_stmt` / `eval_block`

**本阶段语法**
- [ ] `let x = expr`
- [ ] `x = expr`
- [ ] `x += expr`
- [ ] 块 `{ ... }`
- [ ] 表达式语句 `foo + bar`

**语义要求**
- [ ] parser 不查符号表，只产出 AST
- [ ] 赋值时由 evaluator 检查变量是否存在
- [ ] 进入 `{}` 压新作用域，退出时弹出
- [ ] 错误通过 `EvalResult<T>` 传播

**输入模型**
- [ ] 增加“整块解析”能力：`parse_script(src) -> Vec<Stmt>`
- [ ] 定义多行输入规则：`{}`、引号未闭合时继续读续行

**测试重点**
- [ ] 变量遮蔽
- [ ] 父作用域读取
- [ ] 块退出后的可见性
- [ ] 未声明赋值报错

**完成标准**
- [ ] 可以执行一个由多条 `let/assign/block` 组成的小脚本
- [ ] `parse_script` 能处理多行 block，不再局限一行一 parse

### 阶段 3：复合数据与访问语法

**目标**：先把 Array/Object 跑通，再做依赖它们的循环和函数例子。

**建议新增/扩展文件**
- [ ] 在 `ast.rs` 中加入数组/对象字面量、索引、字段访问节点
- [ ] 在 `stmt.rs` 中加入 `FieldAssign` / `IndexAssign`
- [ ] 在 `eval.rs` 中加入容器读写逻辑
- [ ] `src/script/builtins.rs`：`len/push/pop/insert/remove/json/keys/values`

**本阶段语法**
- [ ] Array 字面量：`[1, 2, 3]`
- [ ] Object 字面量：`{name: "elaine"}`
- [ ] 字段访问：`obj.name` / `obj["name"]`
- [ ] 数组索引：`arr[0]`
- [ ] 字段赋值：`obj.name = expr`
- [ ] 索引赋值：`arr[0] = expr`

**实现注意**
- [ ] `{k: v}` 与 block `{ ... }` 的 parser 分支要明确区分
- [ ] `x.y()` 先解析成字段访问，再降成普通 `Call`，不单独引入运行时方法机制
- [ ] 容器操作统一走全局内置函数，不加隐式 `self/this`

**测试重点**
- [ ] Object/Array 字面量解析
- [ ] 字段/索引读写
- [ ] `values(obj)` / `keys(obj)` / `json(obj)`
- [ ] 错误索引、类型不匹配

**完成标准**
- [ ] `obj.name`、`arr[0]`、`json(data)` 都能在脚本 evaluator 中工作

### 阶段 4：控制流

**目标**：让脚本能写出非平凡流程，但仍然不接 shell。

**建议扩展**
- [ ] `Stmt` 新增 `If` / `While` / `ForIn` / `ForRange`
- [ ] 新增控制流枚举：`ExecFlow::{Normal, Break, Continue, Return(Value)}`

**本阶段语法**
- [ ] `if / else if / else`
- [ ] `while`
- [ ] `for i in 1..10`
- [ ] `for i in 1..=10`
- [ ] `for v in arr`
- [ ] `for k in obj`
- [ ] `for v in values(obj)`
- [ ] `break` / `continue`

**语义要求**
- [ ] `if` / `while` 条件必须是 `Bool`
- [ ] `for k in obj` 遍历键名
- [ ] `for v in values(obj)` 遍历对象值
- [ ] `break/continue` 只允许在循环内部

**测试重点**
- [ ] 条件判断的 Bool 限制
- [ ] range 左闭右开 / 左闭右闭
- [ ] `break/continue` 对循环流程的影响

**完成标准**
- [ ] 用纯脚本实现计数循环、分支和遍历示例

### 阶段 5：函数与闭包

**目标**：补上脚本的抽象能力，并为对象函数字段做好铺垫。

**建议新增/扩展**
- [ ] `Value::Func`
- [ ] `Stmt::FnDecl`
- [ ] `Expr::Call`
- [ ] `Expr::FnLiteral`（用于 `obj.inc = fn() { ... }`）

**本阶段语法**
- [ ] `fn name(args) { ... }`
- [ ] `return expr`
- [ ] `fn() { ... }` 作为表达式
- [ ] 普通函数调用 `f(x, y)`
- [ ] 语法糖 `obj.inc()` → `Call(FieldAccess(obj, "inc"), [])`

**实现要求**
- [ ] 函数对象捕获定义时环境
- [ ] 调用时：参数求值 → 新作用域 → 执行函数体 → 处理 `return`
- [ ] 前向引用由 runtime 负责报错，不在 parser 拒绝

**已知限制**
- [ ] `obj.method = fn() { obj.x = 1 }` 会造成循环引用；MVP 允许但不鼓励

**测试重点**
- [ ] 闭包捕获外层变量
- [ ] return 非局部退出
- [ ] 对象字段里的函数值调用

**完成标准**
- [ ] `fn add(a,b) { return a + b }`
- [ ] `obj.inc = fn() { ... }`
- [ ] `obj.inc()` 语法糖都能运行

### 阶段 6：ShellWord 与四种嵌入语法

**目标**：把脚本值安全地桥接到现有 shell 执行器，但暂时还不改顶层分派。

**建议改动的现有模块**
- [ ] `src/types.rs`：`Command.args: Vec<ShellWord>` 或同等结构
- [ ] `src/lexer.rs`：shell word 不再直接产出 `String`
- [ ] `src/parser.rs`：接受新的 `ShellWord`
- [ ] `src/executor/`：新增 `expand_shell_word` / `expand_argv`

**本阶段要做的事**
- [ ] **ShellWord 重构**：将现有 `Token::Word(String)` 替换为 `ShellWord { fragments: Vec<WordFragment> }`
- [ ] `WordFragment::{Lit, Var, EnvVar, Cmd, Expr}`
- [ ] `$VAR`：执行时查脚本作用域，失败再 fallback env
- [ ] `${VAR}`：执行时只查 env
- [ ] `$[expr]`：执行时调用脚本 evaluator
- [ ] `$[...arr]`：执行时展开成多个 argv
- [ ] `$(cmd)`：执行时 fork 子 shell，捕获 stdout

**词法规则**
- [ ] `$VAR` 按最长标识符扫描
- [ ] `${VAR}` 读到匹配 `}`
- [ ] `$[expr]` 使用方括号深度计数，支持 `$[arr[0]]`
- [ ] `$(cmd)` 使用括号深度 + 引号状态 + 转义联合判定

**调用参数展开**
- [ ] 在脚本函数调用 grammar 中加入 `...expr`
- [ ] `run("echo", ...args)` 与 `$[...args]` 共享同一套展开实现

**重要规则**
- [ ] shell 命令模式下，字段访问/索引/任意脚本表达式必须写成 `$[...]`
- [ ] `echo result.stderr` 是字面量；`echo $[result.stderr]` 才是字段读取

**测试重点**
- [ ] `$HOME` / `${HOME}` 差异
- [ ] `$[x + 1]` 与 `$[...arr]`
- [ ] 嵌套 `$[arr[0]]`
- [ ] 嵌套 `$(echo $(date))`
- [ ] 单引号/双引号中的展开差异

**完成标准**
- [ ] 不修改现有 shell 执行模型的前提下，四种嵌入语法都能展开为正确 argv

### 阶段 7：顶层集成与文件执行

**目标**：把独立脚本内核真正接到 ecsh 上。

**建议改动的现有模块**
- [ ] `src/main.rs`：统一入口分派
- [ ] `src/input.rs`：续行读取与 `... ` prompt
- [ ] `src/parser.rs` 或新增 glue 模块：顶层关键字分派
- [ ] `src/lib.rs`：导出 script 模块

**本阶段要做的事**
- [ ] 顶层 parser 集成：关键字开头走 script parser，其他走现有 shell parser
- [ ] 函数体内部沿用同一规则：关键字语句 vs shell 命令
- [ ] `ecsh script.ecs`：走文件级 parser + evaluator
- [ ] `~/.ecshrc`：走与 `script.ecs` 相同的文件级入口
- [ ] `source` / `.`：也走同一套文件级 parser + evaluator
- [ ] continuation prompt：`{}` / 引号未闭合时继续读

**集成测试重点**
- [ ] 顶层 `let/fn/if` 与普通 shell 命令共存
- [ ] 在函数体内执行 shell 命令并读取脚本变量
- [ ] 文件执行、`source`、`.ecshrc` 共用同一语义

**完成标准**
- [ ] 交互模式、脚本文件模式、`source` 模式三者行为一致
- [ ] 现有 shell 功能（管道、重定向、job control）不被 script 集成破坏

---

## 七、既有 Shell 功能待办

### 高优先
- [ ] **Tab 补全** — 新增 `src/completion.rs`，实现 rustyline `Completer` trait
- [ ] **alias / unalias 命令** — alias 展开在 parser 阶段（tokenize 后查表替换）
- [ ] **配置文件 ~/.ecshrc** — 启动时走文件级 parser + 脚本 evaluator 执行（与 `ecsh script.ecs` 共用同一入口，非逐行 shell 模式）

### 中优先
- [ ] **here-doc (`<<`)** — lexer 新增 here-doc 状态，执行时用 pipe 或临时文件
- [ ] **通配符展开 (globbing)** — 新增 `src/expansion.rs`，`*` `?` `[...]` 三种模式
- [ ] **信号处理增强** — `trap` 命令、边界情况（前台管道信号传播）

### 低优先
- [ ] **管道增强** — `|&` 同时重定向 stderr、`!` 取反退出码
- [ ] **更多内置命令** — `type`、`which`、`read`、`shift`、`source`/`.`（走与 `ecsh script.ecs` 相同的文件级 parser + evaluator，非逐行 shell 模式）、`history`
- [ ] **subshell `()`** — fork 子 shell 执行括号内命令

---

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
| `fn` | 脚本语句（函数定义） | `fn add(a,b) { ... }` |
| `if` | 脚本语句（条件） | `if x > 0 { ... }` |
| `while` | 脚本语句（循环） | `while i <= n { ... }` |
| `for` | 脚本语句（循环） | `for i in 1..10 { ... }` |
| `return` | 脚本语句（返回） | `return x + y` |
| `break` | 脚本语句 | `break` |
| `continue` | 脚本语句 | `continue` |
| `标识符 + = / +=` | 产出 Assign AST（evaluator 阶段才检查是否已声明） | `x = 5` / `x += 1` |
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
| `$(cmd)` | 执行 shell 命令 | 捕捉 stdout，单参数 | `echo $(date)` |
| `$[expr]` | 脚本表达式求值 | 结果转字符串，单参数 | `echo $[x + 1]` |
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
| `'raw'` | String（字面量，不展开） | `let s = 'no $expansion'` |
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
| 序列化 | `json(arr)` | `json(obj)` |

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
