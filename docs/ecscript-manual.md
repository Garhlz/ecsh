# ecscript 当前实现手册（stage 9 / 10 过渡期）

本文描述 ecscript **当前已经实现** 的语法与语义。
按当前提交历史，`ecscript` 内核主体已经完成，`ecsh` 顶层、命令桥、值流原语和模块 MVP 都已经接通。本文优先描述**当前语法事实**，方便直接作为 Tree-sitter grammar 的参考基线。

---

## 1. 当前范围

当前已实现：

- expression lexer / Pratt parser / evaluator
- script / stmt parser
- `let`、赋值、复合赋值、表达式语句、block
- 注释：`// ...` 与 `/* ... */`
- 词法作用域与父环境查找（Slot/Binding 模型）
- 数组 / 对象字面量
- 字段访问、索引访问
- 字段赋值、索引赋值
- 全局 builtin：`env` / `cwd` / `join_path` / `stdin` / `read_lines` / `write_lines` / `range` / `len` / `print` / `println` / `push` / `pop` / `insert` / `remove` / `slice` / `keys` / `values` / `to_json` / `from_json`
- 命令桥：`cmd{ ... }` / `command(...)` / `run` / `capture` / `text` / `lines` / `with_env` / `with_cwd`
- 值流原语：`map` / `filter` / `reduce` / `each` / `any` / `all` / `find` / `join`
- `if / else if / else`
- `while`
- `for in`：遍历数组 / 对象 key / 区间
- `break` / `continue`
- `func name(args) { ... }` — 命名函数声明
- `pub let` / `pub func`
- `use ./foo.ecs as foo`
- `(args) => expr` / `(args) => { stmts }` — 匿名函数（lambda/func literal）
- 普通函数调用：`f(x, y)`、`obj.method()`
- `return expr;` / `return;`
- 原始字符串：`r"..."`
- 输出 builtin：`print(...)` / `println(...)`
- **强闭包**：自由变量自动提升为 heap slot，闭包共享可变绑定
- 基于字节偏移的 parse/runtime 错误定位
- `ParseError::format_with_source(src)` / `RuntimeError::format_with_source(src)` 的源码定位格式化
- 独立 `ecscript` 解释器入口：REPL / 文件执行 / `-e` / stdin
- 模块缓存：同一路径模块只初始化一次
- 循环导入检测：`a -> b -> a` 会报错

当前未实现：

- block value / 尾表达式返回值
- 搜索路径、命名导入、`pub use`
- 字符串插值 / 多行字符串等更完整的字符串系统

### 与 ecsh 的当前关系

当前 `ecscript` 与 `ecsh` 的关系如下：

- `ecscript` 已经可以独立运行：REPL、文件执行、`-e`、stdin 都可用
- `ecsh` 已经会在运行时调用 `ecscript` 表达式求值，支撑 `${expr}` 和 `${...arr}`
- `ecsh` 顶层输入已经接入“shell 模式 / script 模式”分派，`.ecs` 文件、`source` / `.`、`.ecshrc` 都复用同一套 ecscript 文件执行入口
- `ecscript` 现在也可以通过 `cmd{ ... }` 结构化命令字面量进入 shell 命令桥，再由 `run` / `capture` / `text` / `lines` 消费执行
- `ecscript` 现在支持 `|>` 值流语法糖：`x |> f(a, b)` 等价于 `f(x, a, b)`
- 命令桥当前也支持单命令纯输出 shell builtin；pipeline 内 builtin 仍未接通
- 文件级模块 MVP 已接通：`.ecs` 文件里可以 `use ./foo.ecs as foo`
- 同一路径模块当前会复用同一个缓存对象，并检测循环导入

### 已知边界（非 bug）

1. **`if` 是语句，不是表达式。** `let f = if x > 0 { ... } else { ... };` 不能解析。用 `if` 语句在块内赋值替代。

2. **闭包捕获只传一层。** `func a() { let x=1; return () => () => x; }` 的最内层 lambda 找不到 `x`——中间层必须显式引用 `x` 才能向下传递捕获。

3. **自由变量 vs builtin 同名。** 如果 lambda 内部要调用 builtin `push`，而外层有同名的局部变量（如 `let push = ...`），则 builtin 可能被遮蔽。避免给闭包变量起 builtin 同名。

4. **带源码行的错误展示目前还是显式 API。** 错误对象内部仍只保存 byte offset；如果调用方想拿到 `line:column + 源码行 + ^` 的格式，需要显式调用 `format_with_source(src)`。

5. **`use` 当前只在文件执行上下文可用。** 模块路径需要相对当前脚本文件目录解析，所以交互 REPL 中暂时不能直接 `use ./foo.ecs as foo`。

6. **命令桥当前只在 shell-backed 执行上下文可用。** `run/capture/text/lines/with_env/with_cwd` 需要宿主提供 `ShellState`；独立 `ecscript` 解释器以及 `ecsh file.ecs` 这类纯文件脚本路径下，目前会报 `... is not available in this context`。

---

## 1.1 运行入口

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

## 2. 词法

### 2.1 空白与注释

空白字符被忽略，不产生 token。  
注释也在 lexer 阶段直接跳过，不产生 token：

- `// ...`：单行注释，跳过直到换行
- `/* ... */`：多行注释，跳过直到 `*/`

### 2.2 标识符

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

### 2.3 数字

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

### 2.4 字符串

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

### 2.5 运算符

| 类别 | 符号 |
|------|------|
| 算术 | `+` `-` `*` `/` `%` |
| 比较 | `==` `!=` `<` `>` `<=` `>=` |
| 逻辑 | `&&` `\|\|` `!` |
| 值流 | `\|>` |

单独的 `&` 或 `|` 会报错并提示使用 `&&` 或 `||`。

### 2.6 分隔符与换行

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

## 3. 语法

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

### 3.1 `{ ... }` 的歧义

在 **statement 位置** 遇到 `{ ... }` 会解析成 block。  
对象字面量只在 **expression 位置** 解析，例如：

```ecs
let x = {name: 1};
```

### 3.2 `for in` 的三种来源

当前 `for x in expr { ... }` 支持：

```ecs
for x in [1, 2, 3] { ... }
for k in {a: 1, b: 2} { ... }   // 遍历 key
for i in 0..10 { ... }
for i in 0..=10 { ... }
```

其中 `0..10` / `0..=10` 会在 parser 阶段直接产出 `Range` AST。

### 3.3 Lambda 语法

`(params) => expr` 或 `(params) => { stmts }`：

```ecs
let add = (a, b) => a + b;
let inc = (x) => { return x + 1; };
let no_args = () => 42;
```

括号内参数可选。`=>` 后可跟单表达式（不需要 `return`）或 block。

### 3.4 模块语法（MVP）

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

## 4. 分号与块规则

### 4.1 简单语句的终止规则

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

### 4.2 不依赖分号的语句

- `if ... { ... }`
- `while ... { ... }`
- `for ... in ... { ... }`
- `func name(args) { ... }`
- block 本身

### 4.3 block 与尾表达式

当前 **不支持 block value / 尾表达式返回值**。
但 block 内最后一条普通语句已经**不必**强制写分号，只要它后面跟着 `}` 即可。

---

## 5. 优先级与结合性

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

## 6. AST

### 6.1 语句节点

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

### 6.2 表达式节点

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

## 7. 运行时值

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

- `Array` / `Object` 是共享、可变容器
- `Function` 支持命名函数和匿名 lambda，闭包捕获自由变量
- builtin 也是普通运行时值，可被遮蔽

### 7.1 闭包模型：Slot / Binding / 自由变量提升

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

## 8. 环境与名字查找

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

## 9. 表达式求值语义

### 9.1 字面量

| 输入 | 输出 |
|------|------|
| `nil` | `Value::Nil` |
| `true` / `false` | `Value::Bool` |
| `42` | `Value::Int(42)` |
| `3.14` | `Value::Float(3.14)` |
| `"hi"` | `Value::String("hi")` |
| `[1, 2]` | `Value::Array(...)` |
| `{name: 1}` | `Value::Object(...)` |

### 9.2 调用

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

### 9.3 容器访问

- `arr[i]`：数组索引，`i` 必须是 `Int`
- `obj["name"]`：对象索引，索引必须是 `String`
- `obj.name`：对象字段访问

### 9.4 区间表达式

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

### 9.5 builtin

| 名字 | 语义 | 备注 |
|------|------|------|
| `env(name)` | 读取环境变量 | `name` 必须是 `String`，不存在时返回 `nil` |
| `cwd()` | 返回当前工作目录 | 返回绝对路径字符串 |
| `join_path(a, b)` | 按平台规则拼接两段路径 | 参数都必须是 `String` |
| `range(start, end)` | 生成闭区间整数数组 | `start` / `end` 必须是 `Int` |
| `len(x)` | 返回长度 | 支持 `Array` / `Object` / `String` |
| `print(v...)` | 输出一个或多个值 | 参数之间用空格分隔，不自动换行 |
| `println(v...)` | 输出一个或多个值并换行 | 参数之间用空格分隔 |
| `push(arr, v...)` | 向数组尾部追加一个或多个值 | 返回 `nil` |
| `pop(arr)` | 弹出尾元素 | 空数组返回 `nil` |
| `insert(arr, i, v)` | 在位置 `i` 插入 | `i == len` 合法 |
| `remove(arr, i)` | 删除并返回位置 `i` 的元素 | 越界报错 |
| `slice(arr, start, end)` | 返回半开区间子数组 | 结果是 `arr[start..end)` |
| `command(program, arg...)` | 以 argv-first 方式构造命令值 | 不解析 shell 语法 |
| `keys(obj)` | 返回对象 key 数组 | 按 key 排序 |
| `values(obj)` | 返回对象 value 数组 | 顺序与排序后的 key 一致 |
| `stdin()` | 读取当前脚本输入文本 | 文件执行和管道输入场景返回完整文本；交互 REPL 默认空字符串 |
| `read_lines()` | 按行读取当前脚本输入 | 基于 `stdin()` 的文本桥，返回 `Array<String>` |
| `write_lines(xs)` | 将数组逐项按行写到 stdout | 使用元素的 display 文本，每项自动换行 |
| `to_json(x)` | 转成 JSON 字符串 | 对象 key 排序；检测循环引用 |
| `from_json(text)` | 把 JSON 字符串解析成语言值 | 失败时报 `ParseInExpr` |
| `run(cmd)` | 继承当前终端执行命令值 | 当前要求 shell-backed 执行上下文 |
| `capture(cmd)` | 执行并返回结果对象 | 返回 `{ code, signal, stdout, stderr, duration_ms, ok }` |
| `text(cmd)` | 返回命令 stdout 文本 | 当前要求 shell-backed 执行上下文 |
| `lines(cmd)` | 返回命令 stdout 行数组 | 当前要求 shell-backed 执行上下文 |
| `with_env(cmd, obj)` | 返回附加环境覆盖的新命令值 | 不修改当前 shell 全局环境 |
| `with_cwd(cmd, path)` | 返回附加 cwd 覆盖的新命令值 | 不修改当前 shell 当前目录 |
| `map(arr, func)` | 对数组逐项映射 | 缺失 `return` 视为 `nil` |
| `filter(arr, func)` | 只保留谓词为 `true` 的元素 | 回调必须返回 `Bool` |
| `reduce(arr, init, func)` | 左折叠 | 回调接收 `(acc, x)` |
| `each(arr, func)` | 逐项执行回调 | 返回 `nil` |
| `any(arr, func)` | 存在量词 | 回调必须返回 `Bool` |
| `all(arr, func)` | 全称量词 | 回调必须返回 `Bool` |
| `find(arr, func)` | 返回首个匹配元素 | 没有匹配时返回 `nil` |
| `join(arr, sep)` | 按 display 文本连接数组元素 | `sep` 必须是 `String` |

### 9.x bridge 常见组合

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

## 10. 语句执行语义

### 10.1 `let`

```ecs
let x = expr;
```

- 先求右值
- 再写入当前作用域
- 当前作用域重复定义报 `DuplicateVariable`

### 10.2 赋值

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

### 10.3 `if / else if / else`

```ecs
if cond { ... }
if cond { ... } else { ... }
if cond { ... } else if cond2 { ... } else { ... }
```

- `cond` 必须求值为 `Bool`
- `then_body` / `else_body` 通过 block 语义执行

### 10.4 `while`

```ecs
while cond { ... }
```

- 每轮开始都会重新求值 `cond`
- `cond` 必须是 `Bool`
- 支持 `break` / `continue`

### 10.5 `for in`

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

### 10.6 `break` / `continue`

- 只能在循环中使用
- 顶层或普通 block 中使用会在运行时报错

### 10.7 `func`

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

### 10.8 `return`

- `return expr;`：返回表达式结果
- `return;`：返回 `nil`
- `return` 可以穿过 `if` / `while` / `for` / block 向上传播，直到函数调用边界
- 顶层使用 `return` 会报运行时错误

---

## 11. 错误模型

### 11.1 ParseError

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

### 11.2 RuntimeError

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

## 12. 偏移定位与源码格式化

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

## 13. 当前阶段速记

- 已经支持 block、作用域、复合数据和 builtin
- 已经支持 `if / else if / else`
- 已经支持 `while`
- 已经支持 `for in` 遍历数组、对象 key 和区间
- 已经支持 `break` / `continue`
- 已经支持命名函数、匿名函数、闭包捕获和 `return`
- 已经支持 `//` / `/* */` 注释
- 已经支持原始字符串 `r"..."`
- 已经支持复合赋值 `+= -= *= /= %=`
- 已经支持独立 `ecscript` 入口（REPL / file / `-e` / stdin）
- 已经支持 `print(...)` / `println(...)`
- `for in obj` 当前遍历的是 **排序后的 key**
- `for in array` 当前使用 **迭代快照**，循环体修改原数组不会影响本轮迭代序列
- 当前函数调用链是 **local → captures → global/root**，不继承调用者局部变量
- 闭包只捕获自由变量，不会复制整个外层环境
- 导出函数可以捕获模块私有顶层变量；模块对象只暴露 `pub` 名字，不暴露私有绑定
- parse/runtime error 已经可以格式化成 `line:column + 源码行 + caret`

---

## 14. 后续扩展方向

从当前实现继续向下扩展时，后续工作通常会落在以下几类：

- 多层闭包自动透传捕获（让 `() => () => x` 直接成立）
- block value / 尾表达式
- 搜索路径 / 命名导入 / `pub use`
- shell 集成
- 把源码格式化错误默认接入解释器入口
- 字节码 VM / 后端演化
