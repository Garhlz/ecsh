# ecscript 实现手册

本文是 `ecscript` 的实现级手册，主要面向维护 parser、evaluator、runtime、错误格式化和 tree-sitter grammar 的读者。

如果只是想写 `.ecs` 脚本，先看 [ecscript-reference.md](ecscript-reference.md)。如果想确认项目当前进度和边界，先看 [status.md](status.md)。后续路线见 [roadmap.md](roadmap.md)。

本文只解释实现结构、语法模型、运行时语义和维护约定。用户可见 API 的完整清单以 reference 为准，当前完成/未完成状态以 status 为准。

---

## 维护入口

代码入口：

- 词法：`src/ecscript/lexer.rs`
- Pratt 表达式 parser：`src/ecscript/pratt.rs`
- 语句 parser：`src/ecscript/parser.rs`
- AST：`src/ecscript/ast.rs`
- evaluator：`src/ecscript/eval.rs`
- 环境和闭包 slot：`src/ecscript/env.rs`
- 运行时值：`src/ecscript/value.rs`
- builtin 分发：`src/ecscript/builtin/mod.rs`
- 模块加载：`src/ecscript/module.rs`
- 顶层分派：`src/ecscript/top_level.rs`
- CLI / REPL：`src/bin/ecscript.rs`
- 命令桥：`src/executor/command_value.rs`
- shell 扩展点：`src/extensions.rs`
- callable 元信息：`src/specs.rs`

测试入口：

- parser / lexer 行为：`tests/lexer.rs`、`tests/parser.rs`
- 解释器 CLI 和示例脚本：`tests/ecscript_cli.rs`
- shell 集成和命令桥：`tests/smoke.rs`
- 纯 evaluator / builtin 单元测试：`src/ecscript/**`

### 已知边界（非 bug）

1. **`if` 是语句，不是表达式。** `let f = if x > 0 { ... } else { ... };` 不能解析。用 `if` 语句在块内赋值替代。

2. **闭包捕获只传一层。** `func a() { let x=1; return () => () => x; }` 的最内层 lambda 找不到 `x`——中间层必须显式引用 `x` 才能向下传递捕获。

3. **自由变量 vs builtin 同名。** 如果 lambda 内部要调用 builtin `push`，而外层有同名的局部变量（如 `let push = ...`），则 builtin 可能被遮蔽。避免给闭包变量起 builtin 同名。

4. **带源码行的错误展示目前还是显式 API。** 错误对象内部仍只保存 byte offset；如果调用方想拿到 `line:column + 源码行 + ^` 的格式，需要显式调用 `format_with_source(src)`。

5. **`use` 当前支持交互式 `ecsh` 顶层。** 在 `.ecs` 文件里，模块路径相对当前脚本目录解析；在交互式 `ecsh` 顶层里，模块路径相对当前 `cwd` 解析。

6. **命令桥当前只在 shell-backed 执行上下文可用。** `run/capture/text/lines/with_env/with_cwd` 需要宿主提供 `ShellState`；独立 `ecscript` 解释器以及 `ecsh file.ecs` 这类纯文件脚本路径下，目前会报 `... is not available in this context`。

7. **reference 才是 builtin/API 清单。** 本文会解释 builtin 的实现分组和运行时语义，但完整名称、签名和用户可见说明以 [ecscript-reference.md](ecscript-reference.md) 为准。

---

## 运行入口

当前已经可以直接运行独立解释器：

```bash
cargo run --bin ecscript --
cargo run --bin ecscript -- script.ecs
cargo run --bin ecscript -- -e 'println(1 + 2);'
echo 'println("hi");' | cargo run --bin ecscript --
```

行为约定：

- 无参数且 stdin 是终端：进入 REPL
- 无参数但 stdin 被 pipe：读取整段 stdin 作为脚本执行
- `ecscript <file.ecs>`：执行文件
- `ecscript -e 'code'`：执行一段源码字符串

REPL 中：

- 主提示符是 `>>> `，续行提示符是 `... `
- 直接输入单个表达式（不带分号）会求值并打印结果；若结果是 `nil` 则不回显
- 未闭合的 block / 括号 / 数组 / 对象 / 字符串 / 注释会自动续行
- 语句仍然沿用原本脚本语法；例如 `let x = 1;`
- `:quit` / `:q` 退出，`:help` / `:h` 显示帮助，`:clear` 清屏
- 历史命令持久化到 `~/.local/share/ecscript/history`
- Ctrl-C 清空当前输入缓冲区，Ctrl-D 退出

---

## 词法

### 空白与注释

空白字符被忽略，不产生 token。  
注释也在 lexer 阶段直接跳过，不产生 token：

- `// ...`：单行注释，跳过直到换行
- `/* ... */`：多行注释，跳过直到 `*/`

### 标识符

```text
[A-Za-z_][A-Za-z0-9_]*
```

保留字：

- `let`
- `pub`
- `nil`
- `true`
- `false`
- `if`
- `else`
- `while`
- `for`
- `in`
- `break`
- `continue`
- `func`
- `return`
- `use`
- `as`

特殊说明：

- `cmd` 在词法上是保留字；当后面紧跟 `{` 时，lexer 会直接把整个 `cmd{ ... }` 识别成单个 `CommandLiteral` token。
- `true` / `false` / `nil` 在词法上是专门的 literal token，不是普通 `Identifier`。

### 数字

| 格式 | 示例 | 说明 |
|------|------|------|
| 十进制整数 | `42` | 无前缀 |
| 十进制浮点 | `3.14` `.5` | `.5` 在运行时等价于 `0.5` |

不支持科学计数法、十六进制、八进制。

数字字面量后面不能直接跟标识符字符；例如：

- `123ab`
- `1.23ms`
- `.5foo`

这些都会在 lexer 阶段直接报 `invalid numeric literal`，而不是拆成“数字 + 标识符”。

如果数字后面跟着的是**另一个表达式起始 token**，例如：

- `42 true`
- `42"hi"`

这类问题不会归到“非法数字后缀”，而会在 parser 阶段报更直接的错误，例如：

- `expected operator or ';' after expression, found keyword 'true'`
- `expected operator or ';' after expression, found string literal`

### 字符串

支持两种字符串：

| 语法 | 含义 |
|------|------|
| `"..."` | 普通字符串，支持转义 |
| `r"..."` | 原始字符串，不处理转义 |

转义序列：

| 输入 | 输出 |
|------|------|
| `\\` | `\` |
| `\"` | `"` |
| `\n` | 换行 |
| `\t` | 制表 |

原始字符串示例：

```ecs
let path = r"c:\tmp\ecs\test.txt";
let pattern = r"\d+\.\d+";
```

`r"..."` 中的反斜杠按字面量保留，不会把 `\n` / `\t` 当成转义。  
当前版本的 raw string 仍然以 `"` 结束，因此不能直接包含双引号本身。

### 运算符

| 类别 | 符号 |
|------|------|
| 算术 | `+` `-` `*` `/` `%` |
| 比较 | `==` `!=` `<` `>` `<=` `>=` |
| 逻辑 | `&&` `\|\|` `!` |
| 值流 | `\|>` |

单独的 `&` 或 `|` 会报错并提示使用 `&&` 或 `||`。

### 分隔符与换行

`(` `)` `{` `}` `[` `]` `,` `.` `;` `:` `=` `+=` `-=` `*=` `/=` `%=` `..` `..=` `=>`

其中：

- `.`：字段访问
- `[]`：索引访问 / 数组字面量
- `()`：分组 / 调用
- `{}`：block / 对象字面量
- `..` / `..=`：保留给 `for` 语句的区间语法

另外，换行在词法上会产生 `Newline` token。当前 parser 会把：

- 一个或多个 `;`
- 一个或多个换行
- `EOF`
- `}`

都视为“简单语句的合法终止位置”。

---

## 语法

当前 parser 接受的是 **script**，也就是一串语句。对 Tree-sitter 来说，最重要的是区分：

- 声明/控制流语句
- 简单语句（`let` / 赋值 / 表达式语句 / `break` / `continue` / `return`）
- `pub` 修饰的声明
- `use ... as ...`

```ebnf
script          = stmt* EOF

stmt            = pub_stmt
                | use_stmt
                | let_stmt
                | assign_stmt
                | expr_stmt
                | block
                | if_stmt
                | while_stmt
                | for_stmt
                | func_stmt
                | break_stmt
                | continue_stmt
                | return_stmt

pub_stmt        = "pub" (let_stmt | func_stmt)
use_stmt        = "use" module_path "as" identifier stmt_end

let_stmt        = "let" identifier "=" expr stmt_end
assign_stmt     = assign_target ( "=" | "+=" | "-=" | "*=" | "/=" | "%=" ) expr stmt_end
expr_stmt       = expr stmt_end
block           = "{" stmt* "}"

if_stmt         = "if" expr block ("else" (block | if_stmt))?
while_stmt      = "while" expr block
for_stmt        = "for" identifier "in" expr block
func_stmt       = "func" identifier "(" param_list? ")" block
break_stmt      = "break" stmt_end
continue_stmt   = "continue" stmt_end
return_stmt     = "return" expr? stmt_end

stmt_end        = (";" | newline)+ | EOF | "}"

param_list      = identifier ("," identifier)*

assign_target   = identifier
                | postfix "." identifier
                | postfix "[" expr "]"

expr            = range_expr
range_expr      = pipe_expr ((".." | "..=") pipe_expr)?
pipe_expr       = logical_or ("|>" call_expr)*
logical_or      = logical_and ("||" logical_and)*
logical_and     = comparison ("&&" comparison)*
comparison      = term (("==" | "!=" | "<" | ">" | "<=" | ">=") term)*
term            = sum (("+" | "-") sum)*
sum             = prefix (("*" | "/" | "%") prefix)*
prefix          = ("!" | "-") prefix | postfix
postfix         = primary (("." identifier) | ("[" expr "]") | ("(" arg_list? ")"))*
arg_list        = expr ("," expr)*
call_expr       = postfix "(" arg_list? ")"
primary         = "nil"
                | "true"
                | "false"
                | number
                | string
                | identifier
                | command_literal
                | array_literal
                | object_literal
                | lambda_expr
                | "(" expr ")"

lambda_expr     = "(" param_list? ")" "=>" (expr | block)
command_literal = "cmd" "{" shell_source "}"
module_path     = token_sequence_that_builds_a_path

array_literal   = "[" (expr ("," expr)* ","?)? "]"
object_literal  = "{" (object_entry ("," object_entry)* ","?)? "}"
object_entry    = (identifier | string) ":" expr
```

补充说明：

- `|>` 是语法糖。运行时没有独立的 `PipeForward` 节点；parser 会把 `x |> f(a, b)` 直接改写成 `f(x, a, b)`。
- `|>` 右侧当前必须是调用表达式。
- `|>` 后允许换行，而且可以连续出现多个空行；这和运行时 parser、tree-sitter grammar、VS Code 高亮当前保持一致。
- `module_path` 当前不是单个 token；parser 会把 `Identifier` / `String` / `.` / `..` / `/` / `-` 这几类 token 拼起来，直到读到 `as`。
- `command_literal` 当前在 lexer 阶段就是单 token；Tree-sitter 第一版更适合把它当一整块特殊语法岛，而不是完整复刻内部 shell parser。

### `{ ... }` 的歧义

在 **statement 位置** 遇到 `{ ... }` 会解析成 block。  
对象字面量只在 **expression 位置** 解析，例如：

```ecs
let x = {name: 1};
```

### `for in` 的三种来源

当前 `for x in expr { ... }` 支持：

```ecs
for x in [1, 2, 3] { ... }
for k in {a: 1, b: 2} { ... }   // 遍历 key
for i in 0..10 { ... }
for i in 0..=10 { ... }
```

其中 `0..10` / `0..=10` 会在 parser 阶段直接产出 `Range` AST。

### Lambda 语法

`(params) => expr` 或 `(params) => { stmts }`：

```ecs
let add = (a, b) => a + b;
let inc = (x) => { return x + 1; };
let no_args = () => 42;
```

括号内参数可选。`=>` 后可跟单表达式（不需要 `return`）或 block。

### 模块语法（MVP）

当前最小模块语法包括：

```ecs
pub let name = "ecs"

pub func add(a, b) {
    return a + b
}

use ./foo.ecs as foo
println(foo.name)
```

规则：

- 模块内部默认私有
- 只有 `pub let` / `pub func` 进入导出对象
- `use ./foo.ecs as foo` 会把导入结果绑定为普通对象 `foo`
- `foo.bar` 只是普通字段访问，不是特殊命名空间语法
- 当前不支持 `pub use`、命名导入或搜索路径
- 同一路径模块当前会复用同一个缓存对象
- `a -> b -> a` 这类循环导入当前会报错
- 模块导出函数可以捕获模块私有顶层变量；这些私有绑定不会出现在导出对象上

---

## 分号与块规则

### 简单语句的终止规则

下列语句都走“简单语句终止”规则：

- `let x = 1;`
- `x = 2;`
- `x += 1;`
- `1 + 2;`
- `len(arr);`
- `break;`
- `continue;`
- `return;`
- `return expr;`

它们当前可以通过以下任一方式结束：

- 一个或多个 `;`
- 一个或多个换行
- `EOF`
- `}`

也就是说，下面这些现在都合法：

```ecs
let x = 1
let y = 2;
let z = 3
```

但同一行里如果两个表达式/语句直接相邻，没有 `;` 或换行分隔，仍然会报错。

### 不依赖分号的语句

- `if ... { ... }`
- `while ... { ... }`
- `for ... in ... { ... }`
- `func name(args) { ... }`
- block 本身

### block 与尾表达式

当前 **不支持 block value / 尾表达式返回值**。
但 block 内最后一条普通语句已经**不必**强制写分号，只要它后面跟着 `}` 即可。

---

## 优先级与结合性

从高到低：

| 优先级 | 运算符 | 结合性 |
|--------|--------|--------|
| 后缀 | `.` `[]` `()` | 左结合 |
| 前缀 | `!` `-` | — |
| 乘除 | `*` `/` `%` | 左结合 |
| 加减 | `+` `-` | 左结合 |
| 比较 | `==` `!=` `<` `>` `<=` `>=` | 左结合 |
| 逻辑与 | `&&` | 左结合 |
| 逻辑或 | `\|\|` | 左结合 |
| 值流 / 区间 | `\|>` `..` `..=` | 左结合 |

示例：

- `obj.arr[0]`
- `foo.bar(x)`
- `1 + arr[0] * 2`
- `0..10`
- `range(1, 6) |> filter((x) => x > 2) |> join(",")`

---

## AST

### 语句节点

```rust
pub enum StmtKind {
    Let { name: String, expr: Expr, public: bool },
    Assign { target: AssignTarget, expr: Expr },
    CompoundAssign { target: AssignTarget, op: CompoundAssignOp, expr: Expr },
    ExprStmt { expr: Expr },
    Block { stmts: Vec<Stmt> },
    If { cond: Expr, then_body: Vec<Stmt>, else_body: Vec<Stmt> },
    While { cond: Expr, body: Vec<Stmt> },
    ForIn { var: String, iterable: Expr, body: Vec<Stmt> },
    ForRange { var: String, range: RangeExpr, body: Vec<Stmt> },
    FuncDeclare { name: String, params: Vec<String>, body: Vec<Stmt>, public: bool },
    Use { path: String, alias: String },
    Break,
    Continue,
    Return { value: Option<Expr> },
}

pub struct Stmt {
    pub kind: StmtKind,
    pub span: usize,
}

pub enum CompoundAssignOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
}

pub enum AssignTarget {
    Name(String),
    Field { object: Expr, field: String },
    Index { object: Expr, index: Expr },
}
```

### 表达式节点

```rust
pub enum ExprKind {
    Literal(Literal),
    Variable(String),
    Prefix(PrefixOper, Box<Expr>),
    Infix(Box<Expr>, InfixOper, Box<Expr>),

    Array(Vec<Expr>),
    Object(Vec<(String, Expr)>),
    Index(Box<Expr>, Box<Expr>),
    Field(Box<Expr>, String),
    Call(Box<Expr>, Vec<Expr>),
    Range(RangeExpr),
    FuncLiteral { params: Vec<String>, body: Vec<Stmt> },
    CommandLiteral(CommandValue),
}

pub struct RangeExpr {
    pub start: Box<Expr>,
    pub end: Box<Expr>,
    pub inclusive: bool,
}
```

`Stmt` 和 `Expr` 上的 `span` 都是源码字节偏移。  
当前约定是：**statement / expression 的 `span` 都是错误定位元数据，不追求完整范围信息。**

---

## 运行时值

```rust
pub enum Value {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Array(Rc<RefCell<Vec<Value>>>),
    Object(Rc<RefCell<HashMap<String, Value>>>),
    Function(Rc<Function>),
    Builtin(Builtin),
    Command(CommandInvocation),
}
```

说明：

- `Array` / `Object` 是共享、可变容器；`let b = a` 会复制 `Rc` 句柄，不会深拷贝内容
- `Function` 支持命名函数和匿名 lambda，闭包捕获自由变量
- `Function` 本身是 `Rc<Function>`，函数值赋值和传参会共享同一个函数对象
- builtin 也是普通运行时值，可被遮蔽

需要复制容器内容时，使用 `clone(value)` builtin。它会递归复制 Array / Object；Function、Builtin、Command 不可拷贝，循环引用会报 runtime error。

### 闭包模型：Slot / Binding / 自由变量提升

```rust
pub type Slot = Rc<RefCell<Value>>;

pub enum Binding {
    Direct(Value),    // 普通局部变量
    Shared(Slot),     // 被闭包捕获后提升到堆上
}

pub struct Function {
    pub name: Option<String>,
    pub params: Vec<String>,
    pub stmts: Vec<Stmt>,
    pub captures: HashMap<String, Slot>,  // 只持有被捕获的变量
}
```

创建闭包时：

1. 遍历函数体 AST 收集自由变量（不在 params 和局部 `let` 中的标识符）
2. 通过 `env.capture_upvalue(name)` 沿环境链查找该变量
3. 如果是普通值（Direct），将其提升为 heap slot（Shared）并返回
4. 如果已被其他闭包提升过（Shared），直接 clone `Slot` 的 Rc
5. 捕获集合只存 Slot，不持有整个环境

调用时环境链：

```
local env (params + locals)
  → captures env (自由变量 slot)
    → root env (全局变量 + builtin)
```

---

## 环境与名字查找

`Environment` 支持父链：

- `new()`：顶层环境
- `new_child(parent)`：子环境
- `find_root()`：沿父链找到最外层全局环境
- `insert()`：在当前层定义变量
- `get()`：当前层 → 父链 → builtin fallback
- `set()`：更新已有变量或修改容器内部内容

这意味着：

```ecs
let len = 1;
```

会遮蔽内置 `len`。

当前函数调用采用词法作用域下的闭包环境链：

- 调用函数时会创建新的 call frame
- call frame 里放参数、函数内局部变量、以及函数自己的名字
- call frame 的父环境是 **captures env**
- captures env 的父环境是 **global/root**
- **不是调用点环境**

也就是说：

```ecs
let x = 1;
func f() { return x; }

{
    let x = 2;
    f();   // 当前读到 1，不是 2
}
```

这样可以避免“动态作用域”行为，同时让闭包继续访问创建时捕获到的局部变量，以及 root 中的全局/builtin。

---

## 表达式求值语义

### 字面量

| 输入 | 输出 |
|------|------|
| `nil` | `Value::Nil` |
| `true` / `false` | `Value::Bool` |
| `42` | `Value::Int(42)` |
| `3.14` | `Value::Float(3.14)` |
| `"hi"` | `Value::String("hi")` |
| `[1, 2]` | `Value::Array(...)` |
| `{name: 1}` | `Value::Object(...)` |

### 调用

当前可调用值有两类：

- builtin
- 用户定义函数值（命名函数声明或 lambda）

当前用户函数能力范围：

- 支持 `func add(a, b) { return a + b; }`
- 支持 `let add = (a, b) => a + b;`
- 支持 `let make_counter = () => { ... };`
- 支持普通调用 `add(1, 2)`
- 支持把函数值放进变量、对象字段，再通过调用表达式执行
- 支持闭包捕获外层局部变量
- 参数个数必须严格匹配
- `return;` 等价于返回 `nil`

当前仍未支持：

- `func(...) { ... }` 这种关键字形式的函数字面量
- 普通 block / 函数体的尾表达式隐式返回

### 容器访问

- `arr[i]`：数组索引，`i` 必须是 `Int`
- `obj["name"]`：对象索引，索引必须是 `String`
- `obj.name`：对象字段访问

Object 的运行时模型是 `HashMap<String, Value>`。对象字面量中的裸 key 是 parser 语法糖：

```ecs
let obj = { name: 1 }
```

会在 AST 中保存为字符串 key `"name"`，不会读取变量 `name`。点访问 `obj.name` 同样是字符串字段访问的短写。索引访问 `obj[key]` 会正常求值 `key`，并要求结果是 String。

这意味着 Object 是 string-key record / map，不是 Lua table、Python dict 或 class 系统。当前没有任意类型 key、`this`、method binding、prototype 或 inheritance。

### 区间表达式

当前支持：

```ecs
for i in 0..3 { ... }
for i in 0..=3 { ... }
```

区间语法当前**语法上是表达式**，但**语义上只允许用于 `for`**。
在 `for i in 0..3 { ... }` 这种语法里，parser 会先得到 `ExprKind::Range`，再由 `parse_for` 特判成 `ForRange` 语句节点。

普通值世界里不再把 `0..3` 当成可求值表达式；需要一个整数数组时，使用：

```ecs
range(0, 3)   // => [0, 1, 2, 3]
slice(range(0, 5), 1, 4) // => [1, 2, 3]
```

### builtin 分发

builtin 名称到实现的入口是 `src/ecscript/builtin/mod.rs::lookup_builtin`。完整用户可见清单、签名和示例以 [ecscript-reference.md](ecscript-reference.md#内置函数) 为准，本文只记录维护时需要注意的实现分组。

主要分组：

- 环境和路径：读取/修改进程环境、当前目录和路径拼接。
- IO：`stdin` 快照、按行读写、`print` / `println`。
- 容器：数组长度、增删、切片、对象 key/value。
- JSON：语言值和 JSON 文本互转，序列化时检测循环引用。
- 命令桥：构造、执行、捕获和派生命令值。
- 集合高阶函数：eager Array 上的 map/filter/reduce/each/any/all/find/join。
- shell 扩展点：prompt、completion、bind、hook、脚本命令注册和受控目录切换。
- introspection：`help`、`builtins`、`commands`、`extensions`，数据来源是统一 `Spec` / `CallableSpec`。

维护规则：

- 新增 builtin 时必须同时更新 `lookup_builtin`、`Value::Builtin`、`src/specs.rs` 和 reference。
- 简单参数协议应优先定义 `Signature` 并调用 `check_signature`，不要在 builtin 主逻辑中重复拼接参数数量和类型错误文案。
- `ParamType::OneOf` 用于值可以接受多个类型的入口，例如 `len(Array|Object|String)`。
- 通过签名检查后，后续 `match` / `let else` 只作为内部解包断言；用户可见错误应由签名层产生。
- `with_env`、`hook`、`prompt`、`complete`、`bind`、`register_command` 这类结构协议仍保留专门检查，直到有通用对象字段协议抽象。
- 删除或重命名 builtin 时必须同步更新 `docs-check` 覆盖结果、测试和 examples。
- 需要 shell 状态的 builtin 必须通过上下文显式检查，不应在独立解释器中隐式创建 shell 状态。
- `help(...)` 和 VS Code hover 应复用 `src/specs.rs`，不要另建一份文档表。

当前签名层位于 `src/ecscript/builtin/support.rs`：

- `ParamType` 描述运行时值类型。
- `ParamSpec` 记录参数名和类型。
- `Arity` 描述 exact / at-least / range 参数数量。
- `Signature` 组合 builtin 名、固定参数、可变参数和 arity。
- `check_signature(sig, args, span)` 统一产生 `ArityMismatch` / `TypeMismatch`。

统一类型错误格式：

```text
{builtin} argument '{param}' expects {Expected}, got {Actual}
```

统一数量错误格式：

```text
range expects 2 arguments, got 1
push expects at least 2 arguments, got 1
```

### 脚本命令

`register_command(name, func)` 用于从 `.ecshrc` 或显式初始化的模块注册真正的 ecsh 命令，而不是文本 alias：

```ecs
register_command("hello", (ctx) => {
    println(join(ctx.args, ","));
    return 0;
});
```

命令解析顺序为 shell builtin、脚本命令、PATH 外部命令。脚本命令回调返回 `nil` 时退出码为 `0`，也可以返回非负 `Int` 作为退出码。当前 MVP 只支持顶层前台执行，不支持管道、后台执行和重定向。

### bridge 常见组合

text/value bridge 的最小用法：

```ecs
let text = stdin();
let lines = read_lines();

println(text);
println(lines);
write_lines(lines);
```

目录/路径相关的最小用法：

```ecs
println(cwd());
println(join_path("/tmp", "ecsh"));
```

说明：

- `stdin()` 返回整份输入文本
- `read_lines()` 返回 `Array<String>`
- `write_lines(xs)` 适合把一组文本行重新送回 stdout

JSON bridge 推荐继续显式组合：

```ecs
let data = from_json(stdin());
println(to_json(data));

let payload = from_json(text(cmd{ printf "{\"ok\":true}" }));
println(payload.ok);
```

这里的设计原则是：

- 文本桥只负责文本和数组行视图
- JSON 桥只负责文本和结构化值互转
- 先通过显式组合完成格式转换，不额外引入 `json(cmd)` 或更重的格式协议

命令桥的当前语法入口：

```ecs
let c1 = cmd{ printf "hello" }
let c2 = command("printf", "hello")
```

其中：

- `cmd{ ... }` 由 lexer 直接识别成单个 `CommandLiteral` token
- `command(...)` 是普通 builtin 调用
- `cmd{ ... }` 当前支持单命令、重定向和 pipeline，不支持 `&&` / `||` / `;`

---

## 语句执行语义

### `let`

```ecs
let x = expr;
```

- 先求右值
- 再写入当前作用域
- 当前作用域重复定义报 `DuplicateVariable`

### 赋值

当前允许三种赋值目标：

- 变量
- 字段访问
- 索引访问

其他形式（如 `1 + 2 = 3`）会在 parse 阶段报错。

同时支持复合赋值：

- `x += 1`
- `obj.count -= 1`
- `arr[i] *= 2`

当前实现不会把 `x += y` 直接粗暴降级成 `x = x + y`；对于字段/索引左值，会先解析一次目标，再完成读-改-写，避免 `arr[next_idx()] += 2` 这类情况把左值副作用执行两次。

### `if / else if / else`

```ecs
if cond { ... }
if cond { ... } else { ... }
if cond { ... } else if cond2 { ... } else { ... }
```

- `cond` 必须求值为 `Bool`
- `then_body` / `else_body` 通过 block 语义执行

### `while`

```ecs
while cond { ... }
```

- 每轮开始都会重新求值 `cond`
- `cond` 必须是 `Bool`
- 支持 `break` / `continue`

### `for in`

```ecs
for v in arr { ... }
for k in obj { ... }
for i in 0..10 { ... }
```

#### 遍历数组

- 当前会先拍一个元素快照，再执行循环体
- 因此循环体里即使修改原数组，也不会影响本轮已经决定好的迭代序列
- 这样可以避免 `RefCell` 借用冲突

#### 遍历对象

- 当前遍历的是对象 key
- key 会先排序，因此顺序稳定

#### 遍历区间

- `a..b`：不包含 `b`
- `a..=b`：包含 `b`
- 起点和终点都必须是 `Int`

### `break` / `continue`

- 只能在循环中使用
- 顶层或普通 block 中使用会在运行时报错

### `func`

```ecs
func add(a, b) {
    return a + b;
}
```

- 当前只支持**命名函数声明语句**
- 声明后函数值会绑定到当前作用域
- 调用时创建独立函数调用帧
- 当前函数体可以读取：
  - 参数
  - 函数内 `let` 局部变量
  - 函数自己的名字
  - global/root 中的全局变量
  - builtin
- 当前**不会**透传调用者局部变量

### `return`

- `return expr;`：返回表达式结果
- `return;`：返回 `nil`
- `return` 可以穿过 `if` / `while` / `for` / block 向上传播，直到函数调用边界
- 顶层使用 `return` 会报运行时错误

---

## 错误模型

### ParseError

典型场景：

- 非法字符、非法转义、未闭合字符串
- 非法数字字面量后缀（如 `123ab`、`1.23ms`）
- 相邻表达式之间缺少运算符或语句分隔（如 `42 true`、`42"hi"`）
- 缺失 `)`、`]`、`}`
- 缺失 `,`、`:`、`;`
- `let` / `for` 后缺标识符
- 非法赋值左值
- `if/while/for/func` 后缺 block

典型报错：

- `expected '{' after while, found integer literal`
- `invalid assignment target; expected variable, field access, or index access`
- `expected operator or ';' after expression, found string literal`
- `unexpected '}' at top level`

### RuntimeError

当前运行时错误种类：

| kind | 触发条件 |
|------|---------|
| `UndefinedVariable` | 变量未定义 |
| `TypeMismatch` | 类型不匹配 |
| `DivisionByZero` | 除零或模零 |
| `DuplicateVariable` | 同一作用域内重复定义 |
| `IndexOutOfBounds` | 数组索引越界 |
| `NonExistentField` | 对象字段不存在 |
| `NotCallable` | 调用了不可调用值 |
| `ArityMismatch` | builtin 或用户函数参数个数不对 |
| `CircularReference` | `to_json` 检测到循环引用，或模块导入形成环 |
| `IoError` | builtin 在 stdout/stderr 等 IO 上失败 |
| `BreakOutsideLoop` | 循环外使用 `break` |
| `ContinueOutsideLoop` | 循环外使用 `continue` |
| `ReturnOutsideFunction` | 函数外使用 `return` |

典型报错：

- `if condition must be Bool, got Int`
- `while condition must be Bool, got String`
- `for-in iterable must be Array or Object, got Int`
- `for range start must be Int, got Bool`
- `break outside loop`
- `continue outside loop`
- `return outside function`
- `object has no field 'name'`

---

## 偏移定位与源码格式化

`ParseError.offset` 和 `RuntimeError.offset` 都是**字节偏移**。

当前大致约定：

| 场景 | offset 指向 |
|------|-------------|
| 字面量 / 变量表达式 | 对应 token 的结束偏移 |
| 前缀表达式 | 前缀运算符的结束偏移 |
| 中缀表达式 | 中缀运算符的结束偏移 |
| 普通语句 | 语句起始 token 的结束偏移 |
| 顶层 `break` / `continue` | `break` / `continue` 关键字的结束偏移 |

如果调用方同时持有源码字符串，可以额外调用：

- `ParseError::format_with_source(src)`
- `RuntimeError::format_with_source(src)`

格式会变成：

```text
ecscript parse error at 3:17: expected ')'
 3 | let x = add(1, 2;
   |                 ^
```

当前这是解释器层提供的格式化 API；是否默认这样打印，取决于更外层入口代码有没有接入它。

---

## Shell 扩展系统实现注意

以下 builtin 仅在 `ecsh` 交互环境下可用；独立 `ecscript` 解释器中调用会报 `... is not available in this context`。

用户可见 API、参数和返回协议见 [ecscript-reference.md](ecscript-reference.md#扩展-api)。这里仅记录实现层的生命周期和错误处理规则。

### `hook(name, func)`

注册一个钩子回调。支持的钩子名称：

| 名称 | 触发时机 |
|------|---------|
| `"before_prompt"` | 读取用户输入之前 |
| `"after_cd"` | 工作目录变更后（含 `cd` 命令和 `set_cwd()`） |
| `"preexec"` | 命令解析成功、即将执行之前 |
| `"postexec"` | 命令执行完成（含解析错误、执行失败、`exit`）之后 |

钩子生命周期细节：

- **`preexec`**：仅在 shell 成功解析并即将执行一条命令时触发。解析错误（如语法错误、未闭合引号）不会触发 `preexec`。
- **`postexec`**：在提交的命令达到最终结果后触发，包括解析错误和执行失败的情况。`exit` 也会触发 `postexec`（在 shell 退出之前）。
- **`after_cd`**：当 `set_cwd()` 或 `cd` 命令改变当前目录时触发。如果在 `after_cd` 钩子内部再次调用 `set_cwd()`，目录和环境变量（`PWD`/`OLDPWD`）仍会更新，但不会递归触发 `after_cd` 钩子。
- **错误处理**：钩子回调中的运行时错误按 best-effort 处理：打印错误信息，跳过该 handler，继续执行后续 handler。

### `prompt(func)`

注册一个提示符生成函数。

- 入参 ctx 包含 `cwd`、`shell`、`status`、`jobs`、`shlvl`、`terminal_width`、`duration_ms`
- 必须返回 String，否则打印错误并回退到默认 prompt
- 回调中发生运行时错误：打印错误（同类错误同会话只打印一次），回退到默认 prompt
- 只能注册一个 prompt handler；多次调用会覆盖

### `complete(name, func)`

为命令 `name` 注册一个补全 handler。

- 入参 ctx 包含 `line`、`word`、`argv`、`arg_index`、`cwd`
- 必须返回 `Array<Object>`，每个 Object 必须有 String 字段 `value`，可选 `display`、`desc`、`kind`
- 返回非 Array 类型：打印错误（同命令同错误同会话只打印一次），回退到无脚本候选
- 数组中单个 item 格式错误（缺失 `value`、类型不对、非 Object）：打印错误并跳过该 item，继续处理其余 item
- 数组中所有 item 都被跳过时，回退到无脚本候选（不会 panic）

### `register_command(name, func)`

将一个 ecscript shell 命令注册为交互 shell 命令。

- 入参 ctx 包含 `name`、`args`、`cwd`
- 返回 `nil` 表示成功（退出码 0），返回非负 `Int` 表示退出码，返回其他类型打印错误并返回失败状态
- 脚本命令不支持后台执行（`&`）、管道（`|`）、重定向（`<`/`>`/`>>`）
- 可以在脚本命令内部调用 `set_cwd(path)` 来修改当前工作目录

### `set_cwd(path)`

修改当前 shell 工作目录。

- 内部调用 `std::env::set_current_dir`，并同步 `PWD` 和 `OLDPWD` 环境变量
- 如果存在 `after_cd` 钩子且当前不在 `after_cd` 分派中，会触发钩子
- 如果已经在 `after_cd` 钩子内部，不会递归触发钩子（reentry guard）

### `bind(key, func)`

为按键序列注册回调。

- 入参 ctx 包含 `key`、`line`、`cursor`、`cwd`、`history`
  - `history` 是 `Array<String>`，包含启动时从 `~/.ecsh_history` 加载的历史以及当前会话新输入的命令（按时间从旧到新排列）
- 返回 `nil` 表示不执行动作，返回 Object 包含 `action` 字段指定 rustyline 动作
- 支持的动作：
  - 无参数动作：`"accept"`、`"newline"`、`"complete"`、`"complete_hint"`、`"clear_screen"`、`"interrupt"`
  - 历史动作：`"history_search_backward"`、`"history_search_forward"`、`"previous_history"`、`"next_history"`
  - `"insert"`（需要 `text` 字段）：在光标处插入文本
  - `"set_line"`（需要 `text` 字段）：用给定文本替换整个当前输入行
