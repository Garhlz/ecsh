# ecscript 当前实现手册（stage 2）

本文描述 ecscript **当前已经实现** 的语法与语义。  
这一版已经从 stage 1 的“单表达式求值”推进到 **stage 2 的语句、代码块和作用域**，但还不是完整脚本语言。

---

## 1. 当前范围

当前已实现：

- 表达式 lexer / Pratt parser / evaluator
- 语句 parser
- `let` 声明
- 赋值语句
- 表达式语句
- `{ ... }` 代码块
- 词法作用域环境与父环境查找
- 基于字节偏移的 parse/runtime 错误定位

当前未实现：

- `if` / `while` / `for`
- 函数、调用、返回
- 数组、对象、成员访问、下标
- 注释
- shell 集成后的正式脚本入口
- block value / 尾表达式返回值

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

其他合法标识符按普通变量名处理。

### 2.3 数字

| 格式 | 示例 | 说明 |
|------|------|------|
| 十进制整数 | `42` | 无前缀 |
| 十进制浮点 | `3.14` `.5` | `.5` 在运行时等价于 `0.5` |

不支持科学计数法、十六进制、八进制。`-1` 由 lexer 拆成 `-` 和 `1`，再由 parser 组装成前缀表达式。

`1..2` 和 `1..=2` 会被正确识别为 range 相关 token，不会被误认为浮点数；但 range 语义目前未实现。

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

`(` `)` `{` `}` `[` `]` `,` `.` `;` `=` `..` `..=`

当前真正进入 parser / evaluator 的主要是：

- `(` `)`
- `{` `}`
- `;`
- `=`

---

## 3. 语法

当前 parser 接受的是 **script**，也就是一串语句。

```ebnf
script       = stmt* EOF

stmt         = let_stmt
             | assign_stmt
             | expr_stmt
             | block

let_stmt     = "let" identifier "=" expr ";"
assign_stmt  = identifier "=" expr ";"
expr_stmt    = expr ";"
block        = "{" stmt* "}"

expr         = logical_or
logical_or   = logical_and ("||" logical_and)*
logical_and  = comparison ("&&" comparison)*
comparison   = term (("==" | "!=" | "<" | ">" | "<=" | ">=") term)*
term         = sum (("+" | "-") sum)*
sum          = prefix (("*" | "/" | "%") prefix)*
prefix       = ("!" | "-") prefix | primary
primary      = "nil" | "true" | "false" | number | string | identifier | "(" expr ")"
```

---

## 4. 分号与块规则

这是当前 stage 2 最明确的一条规则：

### 4.1 普通语句必须以分号结尾

下面这些都必须带 `;`：

- `let x = 1;`
- `x = 2;`
- `1 + 2;`
- `foo;`

### 4.2 block 本身不带分号

```ecs
{ let x = 1; }
```

block 自己作为一个语句时，不需要额外写成：

```ecs
{ let x = 1; };
```

### 4.3 块内最后一条普通语句也必须带分号

当前 **不支持尾表达式省分号**。

也就是说：

```ecs
{ 1; }
```

合法，而：

```ecs
{ 1 }
```

当前会报错。

这代表当前的 block 是 **statement block**，不是 **expression block**。

---

## 5. 优先级与结合性

从高到低：

| 优先级 | 运算符 | 结合性 |
|--------|--------|--------|
| 前缀 | `!` `-` | — |
| 乘除 | `*` `/` `%` | 左结合 |
| 加减 | `+` `-` | 左结合 |
| 比较 | `==` `!=` `<` `>` `<=` `>=` | 左结合 |
| 逻辑与 | `&&` | 左结合 |
| 逻辑或 | `\|\|` | 左结合 |

相等运算和大小比较处于同一优先级。  
`1 == 2 < 3` 按左结合解析为 `(1 == 2) < 3`，运行时通常触发类型错误。

---

## 6. AST

### 6.1 语句节点

```rust
pub enum Stmt {
    Let { name: String, expr: Expr, span: usize },
    Assign { name: String, expr: Expr, span: usize },
    ExprStmt { expr: Expr, span: usize },
    Block { stmts: Vec<Stmt>, span: usize },
}
```

### 6.2 表达式节点

```rust
pub struct Expr {
    pub kind: ExprKind,
    pub span: usize,
}

pub enum ExprKind {
    Literal(Literal),
    Variable(String),
    Prefix(PrefixOper, Box<Expr>),
    Infix(Box<Expr>, InfixOper, Box<Expr>),
}
```

`Stmt` 和 `Expr` 上的 `span` 都是**源码字节偏移**，用于错误定位。  
当前约定是：**statement 的 span 一律指向该语句起始 token 的结束偏移**。

---

## 7. 运行时值

```rust
pub enum Value {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
}
```

当前无数组、对象、函数值。

---

## 8. 环境与作用域

当前 `Environment` 支持父链：

- `new()`：创建顶层环境
- `new_child(parent)`：创建子环境
- `insert(name, value, span)`：在**当前层**定义变量
- `get(name, span)`：先查当前层，不存在再沿父链向上查
- `set(name, value, span)`：修改最近一层已存在的变量；如果整条父链都没有，则报错

语义上：

- `let` 只在当前作用域定义变量
- block 会创建新的子环境
- block 内可以读取外层变量
- block 内给外层已存在变量赋值时，会沿父链更新外层
- block 内 `let x = ...;` 可以遮蔽外层同名变量
- 同一作用域内重复 `let` 会报错

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

### 9.2 变量

变量表达式会从当前环境开始沿父链查找。  
找不到时报 `UndefinedVariable`。

### 9.3 前缀运算符

**`-expr`**

- 接受 `Int` 或 `Float`
- 返回同类型结果
- 其他类型报 `TypeMismatch`

**`!expr`**

- 只接受 `Bool`
- 没有 truthy / falsy 自动转换
- `!0`、`!nil`、`!"x"` 都会报 `TypeMismatch`

### 9.4 算术运算

有 `Int` 和 `Float` 混合时，`Int` 会提升为 `Float`。

| 运算 | 类型组合 | 结果 |
|------|---------|------|
| `+` | Int/Int, Int/Float, Float/Int, Float/Float | 数值 |
| `+` | String/String | 拼接 |
| `-` | Int/Int, Int/Float, Float/Int, Float/Float | 数值 |
| `*` | Int/Int, Int/Float, Float/Int, Float/Float | 数值 |
| `/` | Int/Int（整除）, Int/Float, Float/Int, Float/Float | 数值 |
| `%` | Int/Int | 整数 |

边界行为：

- `Int / Int` 使用 Rust 的向零截断除法
- 除数为 `0` 或 `0.0` 报 `DivisionByZero`
- `%` 仅支持整数；`% 0` 报 `DivisionByZero`
- 不支持字符串与数字混加

### 9.5 比较与相等性

**`==` / `!=`** 支持：

- `Int` 与 `Int` / `Float`
- `Float` 与 `Int` / `Float`
- `Nil` 与 `Nil`
- `Bool` 与 `Bool`
- `String` 与 `String`

其他跨类型比较（如 `1 == "1"`、`true == 1`）报 `TypeMismatch`。

**`<` `>` `<=` `>=`** 只支持数值类型（Int/Float 四种组合）。  
其他类型报 `TypeMismatch`。

### 9.6 逻辑运算

`&&` 和 `||` 只接受 `Bool` 操作数，无 truthy / falsy 自动转换。

并且**短路求值**：

- `false && rhs` 不会求值 `rhs`
- `true || rhs` 不会求值 `rhs`
- 只有在结果仍依赖右侧时，才会继续求值右操作数

---

## 10. 语句执行语义

### 10.1 `let`

```ecs
let x = 1;
```

执行顺序：

1. 先计算右侧表达式
2. 再把结果写入当前作用域
3. 如果当前作用域里已经有同名变量，报 `DuplicateVariable`

### 10.2 赋值

```ecs
x = 2;
```

执行顺序：

1. 先计算右侧表达式
2. 从当前作用域开始向上查找同名变量
3. 找到就更新最近的一层
4. 整条作用域链都找不到则报 `UndefinedVariable`

### 10.3 表达式语句

```ecs
1 + 2;
```

表达式会被求值，但结果被丢弃。

### 10.4 block

```ecs
{
    let x = 1;
    x = 2;
}
```

执行 block 时会创建新的子环境。  
block 内定义的新变量不会泄露到外层；但对外层已有变量的赋值会生效。

---

## 11. 错误模型

### 11.1 ParseError

词法 / 语法阶段错误，包括：

- 非法字符、非法转义、未闭合字符串
- 缺失 `)`、缺失 `;`
- `let` 后缺变量名
- 赋值缺 `=`
- block 缺失 `}`
- 表达式后仍有多余 token
- 在错误位置遇到 `}`

当前错误文案尽量写成：

```text
expected X, found Y
```

例如：

- `expected variable name after 'let', found '='`
- `expected ';' after statement, found end of input`
- `expected expression, found ';'`

### 11.2 RuntimeError

当前运行时错误种类：

| kind | 触发条件 |
|------|---------|
| `UndefinedVariable` | 变量未定义 |
| `TypeMismatch` | 类型不匹配 |
| `DivisionByZero` | 除零或模零 |
| `DuplicateVariable` | 同一作用域内重复定义变量 |

当前运行时错误文案会尽量带上类型名或变量名，例如：

- `undefined variable 'x'`
- `variable 'x' already defined in this scope`
- `cannot add Int and String`
- `'&&' requires Bool operands, got Int and Bool`

---

## 12. 偏移定位

`ParseError.offset` 和 `RuntimeError.offset` 都是**字节偏移**，不是 `[start, end)` 区间。

当前大致约定：

| 节点 / 场景 | offset 指向 |
|------------|-------------|
| 字面量 / 变量表达式 | 对应 token 的结束偏移 |
| 前缀表达式 | 前缀运算符的结束偏移 |
| 中缀表达式 | 中缀运算符的结束偏移 |
| 任意 statement（`let` / `assign` / expr-stmt / block） | 语句起始 token 的结束偏移 |
| 缺分号 | 当前看到的下一个 token（例如 `}` 或 EOF） |

例如：

- `1 / 0` 的除零错误定位在 `/`
- `!1` 的类型错误定位在 `!`
- `let x = 1` 缺分号时，错误会落在输入末尾

---

## 13. 当前阶段速记

- 已经不是纯表达式语言，支持基本语句和 block
- 普通语句必须以 `;` 结束
- block 本身不带 `;`
- block 内最后一条普通语句也必须带 `;`
- `let` 在当前作用域定义变量
- 赋值会沿作用域链向上查找最近定义
- 允许 block 内遮蔽外层同名变量
- `&&` / `||` 已实现短路
- 错误已尽量改成“expected X, found Y”风格

---

## 14. 后续自然延伸方向

从当前实现继续往下做，比较自然的顺序通常是：

- `if` / `while`
- 更完整的 `ExecFlow`（`break` / `continue` / `return`）
- block value / 尾表达式
- 函数与调用
- 数组 / 对象
- 成员访问与下标
- 更完整的 span / 诊断系统
