# ecscript 当前实现手册（stage 5：基础函数）

本文描述 ecscript **当前已经实现** 的语法与语义。  
这一版已经从 stage 4 的“控制流”继续推进到 **stage 5 的第一步**：在 `if / else`、`while`、`for in`、`break`、`continue` 之外，已经打通了**命名函数、普通函数调用和 `return`**。  
不过**闭包、匿名函数、函数字面量**还没有开始实现。

---

## 1. 当前范围

当前已实现：

- expression lexer / Pratt parser / evaluator
- script / stmt parser
- `let`、赋值、表达式语句、block
- 词法作用域与父环境查找
- 数组 / 对象字面量
- 字段访问、索引访问
- 字段赋值、索引赋值
- 全局 builtin：`len` / `push` / `pop` / `insert` / `remove` / `keys` / `values` / `to_json`
- `if / else if / else`
- `while`
- `for in`：
  - 遍历数组
  - 遍历对象 key
  - 遍历区间 `a..b` / `a..=b`
- `break` / `continue`
- 命名函数声明：`func name(args) { ... }`
- 普通函数调用：`f(x, y)`
- `return expr;` / `return;`
- 基于字节偏移的 parse/runtime 错误定位

当前未实现：

- 闭包捕获
- 匿名函数 / 函数字面量
- 注释
- shell 集成后的正式脚本入口
- block value / 尾表达式返回值
- 模块系统

---

## 2. 词法

### 2.1 空白

空白字符被忽略，不产生 token。

### 2.2 标识符

```text
[A-Za-z_][A-Za-z0-9_]*
```

保留字：

- `let`
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

### 2.3 数字

| 格式 | 示例 | 说明 |
|------|------|------|
| 十进制整数 | `42` | 无前缀 |
| 十进制浮点 | `3.14` `.5` | `.5` 在运行时等价于 `0.5` |

不支持科学计数法、十六进制、八进制。

### 2.4 字符串

仅支持双引号字符串 `"..."`。

转义序列：

| 输入 | 输出 |
|------|------|
| `\\` | `\` |
| `\"` | `"` |
| `\n` | 换行 |
| `\t` | 制表 |

### 2.5 运算符

| 类别 | 符号 |
|------|------|
| 算术 | `+` `-` `*` `/` `%` |
| 比较 | `==` `!=` `<` `>` `<=` `>=` |
| 逻辑 | `&&` `\|\|` `!` |

单独的 `&` 或 `|` 会报错并提示使用 `&&` 或 `||`。

### 2.6 分隔符

`(` `)` `{` `}` `[` `]` `,` `.` `;` `:` `=` `..` `..=`

其中：

- `.`：字段访问
- `[]`：索引访问 / 数组字面量
- `()`：分组 / 调用
- `{}`：block / 对象字面量
- `..` / `..=`：区间表达式

---

## 3. 语法

当前 parser 接受的是 **script**，也就是一串语句。

```ebnf
script          = stmt* EOF

stmt            = let_stmt
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

let_stmt        = "let" identifier "=" expr ";"
assign_stmt     = assign_target "=" expr ";"
expr_stmt       = expr ";"
block           = "{" stmt* "}"

if_stmt         = "if" expr block ("else" (block | if_stmt))?
while_stmt      = "while" expr block
for_stmt        = "for" identifier "in" expr block
func_stmt       = "func" identifier "(" param_list? ")" block
break_stmt      = "break" ";"
continue_stmt   = "continue" ";"
return_stmt     = "return" expr? ";"

param_list      = identifier ("," identifier)*

assign_target   = identifier
                | postfix "." identifier
                | postfix "[" expr "]"

expr            = range
range           = logical_or ((".." | "..=") logical_or)?
logical_or      = logical_and ("||" logical_and)*
logical_and     = comparison ("&&" comparison)*
comparison      = term (("==" | "!=" | "<" | ">" | "<=" | ">=") term)*
term            = sum (("+" | "-") sum)*
sum             = prefix (("*" | "/" | "%") prefix)*
prefix          = ("!" | "-") prefix | postfix
postfix         = primary (("." identifier) | ("[" expr "]") | ("(" arg_list? ")"))*
arg_list        = expr ("," expr)*
primary         = "nil"
                | "true"
                | "false"
                | number
                | string
                | identifier
                | array_literal
                | object_literal
                | "(" expr ")"

array_literal   = "[" (expr ("," expr)* ","?)? "]"
object_literal  = "{" (object_entry ("," object_entry)* ","?)? "}"
object_entry    = (identifier | string) ":" expr
```

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

---

## 4. 分号与块规则

### 4.1 必须带分号的语句

- `let x = 1;`
- `x = 2;`
- `1 + 2;`
- `len(arr);`
- `break;`
- `continue;`
- `return;`
- `return expr;`

### 4.2 不带分号的语句

- `if ... { ... }`
- `while ... { ... }`
- `for ... in ... { ... }`
- `func name(args) { ... }`
- block 本身

### 4.3 block 内最后一条普通语句也必须带分号

当前 **不支持尾表达式省分号**。  
block 仍然是 statement block，不是 expression block。

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
| 区间 | `..` `..=` | 左结合 |

示例：

- `obj.arr[0]`
- `foo.bar(x)`
- `1 + arr[0] * 2`
- `0..10`

---

## 6. AST

### 6.1 语句节点

```rust
pub enum Stmt {
    Let { name: String, expr: Expr, span: usize },
    Assign { target: AssignTarget, expr: Expr, span: usize },
    ExprStmt { expr: Expr, span: usize },
    Block { stmts: Vec<Stmt>, span: usize },

    If {
        cond: Expr,
        then_body: Vec<Stmt>,
        else_body: Vec<Stmt>,
        span: usize,
    },
    While {
        cond: Expr,
        body: Vec<Stmt>,
        span: usize,
    },
    ForIn {
        var: String,
        iterable: Expr,
        body: Vec<Stmt>,
        span: usize,
    },
    ForRange {
        var: String,
        range: RangeExpr,
        body: Vec<Stmt>,
        span: usize,
    },
    FuncDeclare {
        name: String,
        params: Vec<String>,
        body: Vec<Stmt>,
        span: usize,
    },
    Break { span: usize },
    Continue { span: usize },
    Return { value: Option<Expr>, span: usize },
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
}

pub struct RangeExpr {
    pub start: Box<Expr>,
    pub end: Box<Expr>,
    pub inclusive: bool,
}
```

`Stmt` 和 `Expr` 上的 `span` 都是源码字节偏移。  
当前约定是：**statement 的 span 指向该语句起始 token 的结束偏移**。

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
}
```

说明：

- `Array` / `Object` 是共享、可变容器
- 数组元素类型不要求统一
- `Function` 当前是**命名函数值**
- builtin 也是普通运行时值，因此可以被遮蔽

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

当前函数调用采用一个**无闭包阶段的过渡语义**：

- 调用函数时会创建新的 call frame
- call frame 里放参数、函数内局部变量、以及函数自己的名字
- 这个 call frame 的父环境是 **global/root**
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

这样可以先避免“动态作用域”行为；等闭包实现后，再把 root-only 模型升级成 captures + global/root。

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
- 用户定义的命名函数

当前用户函数能力范围：

- 支持 `func add(a, b) { return a + b; }`
- 支持普通调用 `add(1, 2)`
- 参数个数必须严格匹配
- `return;` 等价于返回 `nil`

当前仍未支持：

- 闭包捕获
- 匿名函数
- `func(...) { ... }` 这种函数字面量

### 9.3 容器访问

- `arr[i]`：数组索引，`i` 必须是 `Int`
- `obj["name"]`：对象索引，索引必须是 `String`
- `obj.name`：对象字段访问

### 9.4 区间表达式

当前支持：

```ecs
0..3
0..=3
```

在运行时，区间表达式当前会被求值成 `Array<Int>`。  
在 `for i in 0..3 { ... }` 这种语法里，parser 会直接产出 `ForRange` 语句节点。

### 9.5 builtin

| 名字 | 语义 | 备注 |
|------|------|------|
| `len(x)` | 返回长度 | 支持 `Array` / `Object` / `String` |
| `push(arr, v...)` | 向数组尾部追加一个或多个值 | 返回 `nil` |
| `pop(arr)` | 弹出尾元素 | 空数组返回 `nil` |
| `insert(arr, i, v)` | 在位置 `i` 插入 | `i == len` 合法 |
| `remove(arr, i)` | 删除并返回位置 `i` 的元素 | 越界报错 |
| `keys(obj)` | 返回对象 key 数组 | 按 key 排序 |
| `values(obj)` | 返回对象 value 数组 | 顺序与排序后的 key 一致 |
| `to_json(x)` | 转成 JSON 字符串 | 对象 key 排序；检测循环引用 |

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
- 缺失 `)`、`]`、`}`
- 缺失 `,`、`:`、`;`
- `let` / `for` 后缺标识符
- 非法赋值左值
- `if/while/for/func` 后缺 block

典型报错：

- `expected '{' after while, found integer literal`
- `invalid assignment target; expected variable, field access, or index access`
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
| `CircularReference` | `to_json` 检测到循环引用 |
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

## 12. 偏移定位

`ParseError.offset` 和 `RuntimeError.offset` 都是**字节偏移**。

当前大致约定：

| 场景 | offset 指向 |
|------|-------------|
| 字面量 / 变量表达式 | 对应 token 的结束偏移 |
| 前缀表达式 | 前缀运算符的结束偏移 |
| 中缀表达式 | 中缀运算符的结束偏移 |
| 普通语句 | 语句起始 token 的结束偏移 |
| 顶层 `break` / `continue` | `break` / `continue` 关键字的结束偏移 |

---

## 13. 当前阶段速记

- 已经支持 block、作用域、复合数据和 builtin
- 已经支持 `if / else if / else`
- 已经支持 `while`
- 已经支持 `for in` 遍历数组、对象 key 和区间
- 已经支持 `break` / `continue`
- 已经支持命名函数声明、普通函数调用和 `return`
- `for in obj` 当前遍历的是 **排序后的 key**
- `for in array` 当前使用 **迭代快照**，循环体修改原数组不会影响本轮迭代序列
- 当前函数调用帧只继承 **global/root**，不继承调用者局部变量
- 闭包、匿名函数、函数字面量还没开始

---

## 14. 后续自然延伸方向

从当前实现继续往下做，比较自然的顺序通常是：

- 闭包与 slot/cell 捕获
- 匿名函数 / `func(...) { ... }`
- 对象字段里的函数值
- block value / 尾表达式
- shell 集成
- 更完整的 span / diagnostics 系统
