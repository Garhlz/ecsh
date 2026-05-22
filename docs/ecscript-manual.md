# ecscript 当前实现手册（stage 3）

本文描述 ecscript **当前已经实现** 的语法与语义。  
这一版已经从 stage 2 的“语句、块、作用域”推进到 **stage 3 的复合数据、访问语法和内置函数调用**，但仍然不是完整脚本语言。

---

## 1. 当前范围

当前已实现：

- 表达式 lexer / Pratt parser / evaluator
- script / stmt parser
- `let` 声明、赋值语句、表达式语句、block
- 词法作用域环境与父环境查找
- 数组 / 对象字面量
- 字段访问、索引访问
- 字段赋值、索引赋值
- 内置函数调用：`len` / `push` / `pop` / `insert` / `remove` / `keys` / `values` / `to_json`
- 基于字节偏移的 parse/runtime 错误定位

当前未实现：

- `if` / `while` / `for`
- 用户自定义函数、`return`
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

`(` `)` `{` `}` `[` `]` `,` `.` `;` `:` `=`

当前会进入 parser / evaluator 的主要是：

- `(` `)`：分组 / 调用
- `{` `}`：block / 对象字面量
- `[` `]`：数组字面量 / 索引访问
- `.`：字段访问
- `,`
- `;`
- `:`
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
assign_stmt  = assign_target "=" expr ";"
expr_stmt    = expr ";"
block        = "{" stmt* "}"

assign_target = identifier
              | postfix "." identifier
              | postfix "[" expr "]"

expr         = logical_or
logical_or   = logical_and ("||" logical_and)*
logical_and  = comparison ("&&" comparison)*
comparison   = term (("==" | "!=" | "<" | ">" | "<=" | ">=") term)*
term         = sum (("+" | "-") sum)*
sum          = prefix (("*" | "/" | "%") prefix)*
prefix       = ("!" | "-") prefix | postfix
postfix      = primary (("." identifier) | ("[" expr "]") | ("(" arg_list? ")"))*
arg_list     = expr ("," expr)*
primary      = "nil"
             | "true"
             | "false"
             | number
             | string
             | identifier
             | array_literal
             | object_literal
             | "(" expr ")"

array_literal  = "[" (expr ("," expr)* ","?)? "]"
object_literal = "{" (object_entry ("," object_entry)* ","?)? "}"
object_entry   = (identifier | string) ":" expr
```

### 3.1 关于 `{ ... }` 的歧义

当前在 **statement 位置** 遇到 `{ ... }` 会解析成 block。  
对象字面量只在 **expression 位置** 解析，例如：

```ecs
let x = {name: 1};
```

而：

```ecs
{name: 1}
```

在顶层 statement 位置会按 block 路径处理，不会被当成对象字面量。

### 3.2 关于调用

当前已经支持通用的 postfix 调用语法：

```ecs
len(arr)
obj.f(x)
foo()[0]
```

但**当前真正可调用的值只有 builtin**。  
用户自定义函数还未实现。

---

## 4. 分号与块规则

### 4.1 普通语句必须以分号结尾

下面这些都必须带 `;`：

- `let x = 1;`
- `x = 2;`
- `obj.name = 3;`
- `arr[i] = 4;`
- `1 + 2;`
- `len(arr);`

### 4.2 block 本身不带分号

```ecs
{ let x = 1; }
```

当前 block 自己作为语句时不需要写成：

```ecs
{ let x = 1; };
```

### 4.3 块内最后一条普通语句也必须带分号

当前 **不支持尾表达式省分号**。  
block 仍然是 **statement block**，不是 **expression block**。

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

例如：

- `obj.arr[0]` 解析为 `(obj.arr)[0]`
- `foo.bar(x)` 解析为 `(foo.bar)(x)`
- `1 + arr[0] * 2` 中 `[]` 比算术绑定更紧

---

## 6. AST

### 6.1 语句节点

```rust
pub enum Stmt {
    Let { name: String, expr: Expr, span: usize },
    Assign { target: AssignTarget, expr: Expr, span: usize },
    ExprStmt { expr: Expr, span: usize },
    Block { stmts: Vec<Stmt>, span: usize },
}

pub enum AssignTarget {
    Name(String),
    Field { object: Expr, field: String },
    Index { object: Expr, index: Expr },
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

    Array(Vec<Expr>),
    Object(Vec<(String, Expr)>),
    Index(Box<Expr>, Box<Expr>),
    Field(Box<Expr>, String),
    Call(Box<Expr>, Vec<Expr>),
}
```

`Stmt` 和 `Expr` 上的 `span` 都是**源码字节偏移**，用于错误定位。  
当前约定是：**statement 的 span 一律指向该语句起始 token 的结束偏移**。

对象字面量的 key 已经在 parser 阶段收敛成 `String`：

- `{name: 1}` 的 key 是 `"name"`
- `{"name": 1}` 的 key 也是 `"name"`

当前**不支持动态 key**（例如 `{[expr]: value}`）。

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
    Builtin(Builtin),
}
```

说明：

- `Array` / `Object` 是**共享、可变**容器
- 数组元素类型**不要求统一**
- builtin 也是运行时值的一种，因此可以被变量读取、传递、遮蔽

---

## 8. 环境、作用域与 builtin 查找

当前 `Environment` 支持父链：

- `new()`：创建顶层环境
- `new_child(parent)`：创建子环境
- `insert(name, value, span)`：在**当前层**定义变量
- `get(name, span)`：先查当前层，不存在再沿父链向上查
- `set(target, value, span)`：修改最近一层变量，或修改容器内部元素/字段

### 8.1 builtin fallback

`get(name, span)` 的查找顺序是：

1. 当前作用域
2. 父作用域链
3. builtin 名字表

这意味着：

```ecs
let len = 1;
```

会遮蔽内置的 `len`。

### 8.2 作用域语义

- `let` 只在当前作用域定义变量
- block 会创建新的子环境
- block 内可以读取外层变量
- block 内给外层已有变量赋值时，会沿父链更新外层
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
| `[1, 2]` | `Value::Array(...)` |
| `{name: 1}` | `Value::Object(...)` |

### 9.2 变量

变量表达式会从当前环境开始沿父链查找。  
到根后仍不存在时，再尝试按 builtin 名字查找。  
仍找不到才报 `UndefinedVariable`。

### 9.3 前缀运算符

**`-expr`**

- 接受 `Int` 或 `Float`
- 返回同类型结果
- 其他类型报 `TypeMismatch`

**`!expr`**

- 只接受 `Bool`
- 没有 truthy / falsy 自动转换

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

### 9.5 比较与相等性

**`==` / `!=`** 支持：

- `Int` 与 `Int` / `Float`
- `Float` 与 `Int` / `Float`
- `Nil` 与 `Nil`
- `Bool` 与 `Bool`
- `String` 与 `String`

其他跨类型比较报 `TypeMismatch`。

**`<` `>` `<=` `>=`** 只支持数值类型（Int/Float 四种组合）。

### 9.6 逻辑运算

`&&` 和 `||` 只接受 `Bool` 操作数，无 truthy / falsy 自动转换。

并且**短路求值**：

- `false && rhs` 不会求值 `rhs`
- `true || rhs` 不会求值 `rhs`

### 9.7 容器访问

#### 数组索引

```ecs
arr[0]
```

- 基值必须是 `Array`
- 索引必须是 `Int`
- 当前不支持负索引
- 越界报 `IndexOutOfBounds`

#### 对象索引

```ecs
obj["name"]
```

- 基值必须是 `Object`
- 索引必须求值为 `String`
- 字段不存在时报 `NonExistentField`

#### 对象字段访问

```ecs
obj.name
```

- 基值必须是 `Object`
- 字段名来自源码标识符，不做动态求值
- 字段不存在时报 `NonExistentField`

### 9.8 调用

当前调用表达式的流程是：

1. 先求值 callee
2. 再从左到右求值参数
3. callee 必须是 `Value::Builtin`

其他值被调用时会报 `NotCallable`。

当前真正可调用的值主要是全局 builtin；用户函数尚未实现。

### 9.9 当前 builtin

| 名字 | 语义 | 备注 |
|------|------|------|
| `len(x)` | 返回长度 | 支持 `Array` / `Object` / `String` |
| `push(arr, v...)` | 向数组尾部追加一个或多个值 | 返回 `nil` |
| `pop(arr)` | 弹出尾元素 | 空数组返回 `nil` |
| `insert(arr, i, v)` | 在位置 `i` 插入 | `i` 允许等于长度 |
| `remove(arr, i)` | 删除并返回位置 `i` 的元素 | 越界报错 |
| `keys(obj)` | 返回对象 key 数组 | 按 key 排序 |
| `values(obj)` | 返回对象 value 数组 | 顺序与排序后的 key 一致 |
| `to_json(x)` | 转成 JSON 字符串 | 对象 key 排序；检测循环引用 |

#### `len(String)`

当前 `len("你好")` 返回的是 **Unicode 标量值数量**，不是 UTF-8 字节数。

#### `to_json`

`to_json`：

- 返回 `Value::String`
- 对 `Object` 的输出按 key 排序，结果稳定
- `NaN` / `Infinity` 不可序列化，会报错
- 若数组/对象形成循环引用，会报 `CircularReference`

---

## 10. 语句执行语义

### 10.1 `let`

```ecs
let x = expr;
```

执行顺序：

1. 先计算右侧表达式
2. 再把结果写入当前作用域
3. 如果当前作用域已存在同名变量，报 `DuplicateVariable`

### 10.2 赋值

```ecs
x = 1;
obj.name = 2;
arr[i] = 3;
obj["name"] = 4;
```

赋值目标当前只允许三种：

- 变量
- 字段访问
- 索引访问

其他表达式（如 `1 + 2 = 3`）会在 parse 阶段报错。

#### 变量赋值

- 从当前作用域向上查找最近绑定
- 找到就更新
- 整条作用域链都找不到则报 `UndefinedVariable`

#### 字段赋值

- 基值必须是 `Object`
- 当前语义是 `insert` / 覆盖，不要求字段预先存在

#### 索引赋值

- `Array` + `Int`：更新数组元素
- `Object` + `String`：按 key 写入 / 覆盖

### 10.3 表达式语句

表达式会被求值，但结果被丢弃。

### 10.4 block

执行 block 时会创建新的子环境。  
block 内定义的新变量不会泄露到外层；但对外层已有变量的赋值会生效。

---

## 11. 错误模型

### 11.1 ParseError

词法 / 语法阶段错误，包括：

- 非法字符、非法转义、未闭合字符串
- 缺失 `)`、缺失 `]`、缺失 `}`
- 缺失 `,` / `:` / `;`
- `let` 后缺变量名
- 非法赋值左值
- 在错误位置遇到 `}`

当前错误文案尽量写成：

```text
expected X, found Y
```

或语义更明确的短句，例如：

- `invalid assignment target; expected variable, field access, or index access`
- `unexpected '}' at top level`

### 11.2 RuntimeError

当前运行时错误种类：

| kind | 触发条件 |
|------|---------|
| `UndefinedVariable` | 变量未定义 |
| `TypeMismatch` | 类型不匹配 |
| `DivisionByZero` | 除零或模零 |
| `DuplicateVariable` | 同一作用域内重复定义变量 |
| `IndexOutOfBounds` | 数组索引越界 |
| `NonExistentField` | 对象字段不存在 |
| `NotCallable` | 调用了不可调用值 |
| `ArityMismatch` | builtin 参数个数不对 |
| `CircularReference` | `to_json` 检测到循环引用 |

当前错误文案会尽量带上类型名、字段名、下标或 builtin 名，例如：

- `undefined variable 'x'`
- `array index must be Int, got String`
- `object has no field 'name'`
- `cannot access field 'name' on Int`
- `Int is not callable`
- `insert index 3 out of bounds for length 2`
- `to_json cannot serialize cyclic Array/Object values`

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

---

## 13. 当前阶段速记

- 已经支持基本语句、block 和词法作用域
- 已经支持数组 / 对象字面量
- 已经支持 `obj.name`、`obj["name"]`、`arr[i]`
- 已经支持 `obj.name = expr`、`obj["name"] = expr`、`arr[i] = expr`
- object literal 的 key 当前只能是标识符或字符串
- 普通语句必须以 `;` 结束
- `&&` / `||` 已实现短路
- 已经有最小 builtin 调用能力，但**还没有用户函数**
- `to_json` 会稳定排序对象 key，并检测循环引用

---

## 14. 后续自然延伸方向

从当前实现继续往下做，比较自然的顺序通常是：

- `if` / `while`
- 更完整的 `ExecFlow`（`break` / `continue` / `return`）
- 用户函数与闭包
- 让 `Call` 同时支持 builtin 和用户函数
- block value / 尾表达式
- shell 集成
- 更完整的 span / 诊断系统
