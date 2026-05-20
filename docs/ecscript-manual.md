# ecscript 第一阶段：表达式核心

本文描述 ecscript stage 1 已实现的功能。当前仅支持单表达式求值，尚未包含语句、控制流、函数等。

---

## 1. 词法

### 1.1 空白

空白字符被忽略。

### 1.2 标识符

```
[A-Za-z_][A-Za-z0-9_]*
```

保留字：`nil` `true` `false`。其他合法标识符按普通变量名处理。

### 1.3 数字

| 格式 | 示例 | 说明 |
|------|------|------|
| 十进制整数 | `42` | 无前缀 |
| 十进制浮点 | `3.14` `.5` | `.5` 在运行时等价于 `0.5` |

不支持科学计数法、十六进制、八进制。`-1` 由 lexer 拆分出 `-` 和 `1`，parser 组装为前缀表达式。

`1..2` 和 `1..=2` 被正确识别为 range 分隔符（token 已预留，语义尚未实现），不会被误认为浮点数。

### 1.4 字符串

仅支持双引号字符串 `"..."`，不区分单引号。

转义序列：

| 输入 | 输出 |
|------|------|
| `\\` | `\` |
| `\"` | `"` |
| `\n` | 换行 |
| `\t` | 制表 |

### 1.5 运算符

| 类别 | 符号 |
|------|------|
| 算术 | `+` `-` `*` `/` `%` |
| 比较 | `==` `!=` `<` `>` `<=` `>=` |
| 逻辑 | `&&` `\|\|` `!` |

单独的 `&` 或 `|` 会报错并提示使用 `&&` 或 `||`。

### 1.6 分隔符

`(` `)` `{` `}` `[` `]` `,` `.` `;` `=` `..` `..=`

当前仅 `(` `)` 在 parser 中有实际语义，其余为后续阶段预留。

---

## 2. 语法

```ebnf
expr         = logical_or

logical_or   = logical_and ("||" logical_and)*
logical_and  = comparison ("&&" comparison)*
comparison   = term (("==" | "!=" | "<" | ">" | "<=" | ">=") term)*
term         = sum (("+" | "-") sum)*
sum          = prefix (("*" | "/" | "%") prefix)*
prefix       = ("!" | "-") prefix | primary
primary      = "nil" | "true" | "false" | number | string | identifier | "(" expr ")"
```

单表达式输入。表达式后出现额外 token 时报解析错误。

---

## 3. 优先级与结合性

从高到低：

| 优先级 | 运算符 | 结合性 |
|--------|--------|--------|
| 前缀 | `!` `-` | — |
| 乘除 | `*` `/` `%` | 左结合 |
| 加减 | `+` `-` | 左结合 |
| 比较 | `==` `!=` `<` `>` `<=` `>=` | 左结合 |
| 逻辑与 | `&&` | 左结合 |
| 逻辑或 | `\|\|` | 左结合 |

相等运算和大小比较处于同一优先级。`1 == 2 < 3` 按左结合解析为 `(1 == 2) < 3`，运行时通常触发类型错误。

---

## 4. AST

```rust
pub struct Expr {
    pub kind: ExprKind,
    pub span: usize,   // 源码字节偏移，用于错误定位
}

pub enum ExprKind {
    Literal(Literal),
    Variable(String),
    Prefix(PrefixOper, Box<Expr>),
    Infix(Box<Expr>, InfixOper, Box<Expr>),
}
```

---

## 5. 运行时值

```rust
pub enum Value {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
}
```

无数组、对象、函数值。

---

## 6. 变量环境

`Environment` 维护 `HashMap<String, Value>`。操作：

- `new()` — 创建空环境
- `insert(name, value)` — 写入变量
- `get(name, span)` — 读取变量，不存在则报 `UndefinedVariable`

当前无作用域链、无赋值语法。环境仅供上层入口（测试、未来 REPL）注入变量。

---

## 7. 求值语义

### 7.1 字面量

| 输入 | 输出 |
|------|------|
| `nil` | `Value::Nil` |
| `true` / `false` | `Value::Bool` |
| `42` | `Value::Int(42)` |
| `3.14` | `Value::Float(3.14)` |
| `"hi"` | `Value::String("hi")` |

### 7.2 前缀运算符

**`-expr`**：接受 `Int` 或 `Float`，返回同类型。其他类型抛出 `TypeMismatch`。

**`!expr`**：接受 `Bool`，返回 `Bool`。没有 truthy/falsy 自动转换——`!0`、`!nil`、`!"x"` 均报错。

### 7.3 算术运算

有 `Int` 和 `Float` 混合时，`Int` 提升为 `Float`，结果为 `Float`。

**`+`** 额外支持 `String + String`（拼接）。不支持数字与字符串混加。

| 运算 | 类型组合 | 结果 |
|------|---------|------|
| `+` | Int/Int, Int/Float, Float/Int, Float/Float | 数值 |
| `+` | String/String | 拼接 |
| `-` | 同上（不含 String） | 数值 |
| `*` | 同上 | 数值 |
| `/` | 同上 | 数值 |
| `%` | Int/Int | 整数 |

边界行为：

- `Int / Int` 采用 Rust 的向零截断除法（`10 / 3 = 3`，`-10 / 3 = -3`），非地板除
- `右操作数为 0` 或 `0.0` 抛出 `DivisionByZero`
- `%` 仅支持整数；`% 0` 抛出 `DivisionByZero`

### 7.4 比较与相等性

**`==` / `!=`** 允许以下类型组合：

| 左 | 右 |
|----|-----|
| Int | Int, Float |
| Float | Int, Float |
| Nil | Nil |
| Bool | Bool |
| String | String |

跨类型比较（如 `1 == "1"`、`true == 1`）抛出 `TypeMismatch`，不返回 `false`。

**`<` `>` `<=` `>=`** 仅支持数值类型（Int/Float 四种组合）。其他类型抛出 `TypeMismatch`。

### 7.5 逻辑运算

`&&` 和 `||` 仅接受 `Bool` 操作数，无 truthy/falsy 自动转换。

**当前不短路**：左右操作数均求值后再执行逻辑运算。即使左侧已能决定结果，右侧仍会求值；右侧有错误仍会抛出。

---

## 8. 错误模型

### ParseError

词法/语法阶段的错误：

- 非法字符、非法转义、未闭合字符串
- 缺失 `)`、表达式后多余 token
- 以非前缀运算符开头的表达式

携带 `offset: usize`（字节偏移）和 `message: String`。

### RuntimeError

求值阶段的错误：

| kind | 触发条件 |
|------|---------|
| `UndefinedVariable` | 变量未定义 |
| `TypeMismatch` | 类型不匹配 |
| `DivisionByZero` | 除零或模零 |

携带 `offset`、`kind`、`message`。

---

## 9. 偏移定位

AST 节点上的 `span` 和错误中的 `offset` 均为单字节偏移（非区间）：

| 表达式 | span 指向 |
|--------|----------|
| 字面量 / 标识符 | token 结束偏移 |
| 前缀表达式 | 前缀运算符 |
| 中缀表达式 | 中缀运算符 |
| 括号表达式 | 内部表达式（不产生额外节点） |

示例：`1 / 0` 的除零错误定位在 `/` 位置，`!1` 的类型错误定位在 `!` 位置。

---

## 10. 已识别但未启用的 token

以下 token 已被 lexer 识别，parser 尚未使用，为后续阶段预留：

```
{ } [ ] , . ; = .. ..=
```

---

## 11. 当前阶段速记

- 表达式语言，非完整脚本语言
- 五种值：`Nil` `Bool` `Int` `Float` `String`
- 严格类型：逻辑只接受 `Bool`，比较不接受跨类型
- `Int / Int` 的结果仍是 `Int`（向零截断）
- `&&` `||` 不短路
- 错误带字节偏移但非完整 span
