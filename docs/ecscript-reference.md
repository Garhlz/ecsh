# ecscript 参考手册

这份文档面向写 `.ecs` 脚本的人，描述当前已经实现的语法、builtin 和 shell 集成 API。实现细节、AST 和 evaluator 语义见 [ecscript-manual.md](ecscript-manual.md)；当前进度和边界见 [status.md](status.md)。

## 概览

`ecscript` 是 `ecsh` 使用的小型脚本语言。它可以独立运行，也可以在交互式 `ecsh` 中用于配置、扩展 prompt/completion/key binding、构造命令值和处理结构化数据。

当前语言特征：

- 显式变量声明：`let`
- 动态值：`nil`、Bool、Int、Float、String、Array、Object、Function、Command
- 控制流：`if`、`while`、`for in`
- 函数和闭包：`func`、lambda
- 容器：数组和对象分离
- 模块 MVP：`pub let` / `pub func`、`use ./foo.ecs as foo`
- shell 集成：`${expr}`、`${...arr}`、`cmd{ ... }`、`run` / `capture` / `text` / `lines`

## 运行脚本

独立解释器：

```bash
cargo run --bin ecscript --
cargo run --bin ecscript -- script.ecs
cargo run --bin ecscript -- -e 'println(1 + 2);'
echo 'println("hi");' | cargo run --bin ecscript --
```

通过 `ecsh`：

```bash
cargo run
cargo run -- script.ecs
```

交互式 `ecsh` 中执行当前 session 脚本：

```bash
source config.ecs
. config.ecs
reload_rc
```

当前关系：

- 交互式 `ecsh` 顶层和 `source` / `.` 共享当前 shell 的脚本环境。
- `ecsh file.ecs` 使用新的脚本根环境。
- `~/.ecshrc` 只在交互模式启动时自动加载。
- `reload_rc` 会用新的脚本环境、扩展注册表和模块缓存重新加载 `~/.ecshrc`。

## 语法速查

### 值

```ecs
let n = 1;
let f = 3.14;
let ok = true;
let none = nil;
let text = "hello";
let raw = r"c:\tmp\file.txt";
let xs = [1, 2, 3];
let user = { name: "elaine", score: 7 };
```

字符串支持普通字符串 `"..."` 和原始字符串 `r"..."`。普通字符串支持 `\\`、`\"`、`\n`、`\t`。

### 变量与赋值

```ecs
let x = 1;
x = x + 1;
x += 2;
x -= 1;
x *= 3;
x /= 2;
x %= 2;
```

字段和索引也可以赋值：

```ecs
let obj = { count: 0 };
obj.count += 1;

let xs = [1, 2, 3];
xs[0] = 10;
```

### 数组与对象

```ecs
let xs = [1, 2, 3];
push(xs, 4);
println(xs[0]);

let user = { name: "elaine", stats: { commits: 7 } };
println(user.name);
println(user.stats.commits);
```

对象 key 可以是标识符或字符串：

```ecs
let obj = { name: "ecs", "long-key": 1 };
```

Object 是字符串键的可变 record / map，不是 class，也不是任意 key 的 HashMap。规则：

- Object key 类型只有 String。
- `{ name: 1 }` 中的 `name` 是字段名语法糖，不会读取变量 `name`。
- `{ name: 1 }` 等价于 `{ "name": 1 }`。
- `obj.name` 是 `obj["name"]` 的短写，只能用于标识符形式的字段名。
- `obj[key]` 会先求值 `key`，求值结果必须是 String。
- 当前没有 `this`、method binding、class、prototype、inheritance。

```ecs
let key = "name";
let obj = { name: "ecs", "long-key": 1 };

println(obj.name);
println(obj["name"]);
println(obj[key]);
println(obj["long-key"]);
```

### 控制流

```ecs
if x > 0 {
    println("positive");
} else if x == 0 {
    println("zero");
} else {
    println("negative");
}

while x < 3 {
    x += 1;
}
```

`if` 是语句，不是表达式。

### For 循环

遍历数组：

```ecs
for x in [1, 2, 3] {
    println(x);
}
```

遍历对象 key：

```ecs
for key in { a: 1, b: 2 } {
    println(key);
}
```

遍历区间：

```ecs
for i in 1..5 {
    println(i);      // 1, 2, 3, 4
}

for i in 1..=5 {
    println(i);      // 1, 2, 3, 4, 5
}
```

普通值世界需要使用 `range(start, end)`，不要写裸 `1..5`：

```ecs
let xs = range(1, 5);    // [1, 2, 3, 4, 5]
```

### 函数与闭包

命名函数：

```ecs
func add(a, b) {
    return a + b;
}

println(add(1, 2));
```

lambda：

```ecs
let inc = (x) => x + 1;
let add = (a, b) => {
    return a + b;
};
```

闭包会捕获自由变量：

```ecs
func make_counter() {
    let n = 0;
    return () => {
        n += 1;
        return n;
    };
}

let next = make_counter();
println(next());
println(next());
```

当前闭包捕获只自动传一层；多跳闭包需要中间层显式引用外层变量。

### 语句结束规则

简单语句可以用分号、换行、EOF 或 `}` 结束：

```ecs
let x = 1
let y = 2;
println(x + y)
```

控制流和函数声明不需要尾部分号：

```ecs
if true {
    println("ok")
}
```

## 模块

模块默认私有，只有 `pub let` 和 `pub func` 会导出。

```ecs
// math.ecs
let hidden = 1
pub let two = hidden + 1

pub func add(a, b) {
    return a + b
}
```

导入模块：

```ecs
use ./math.ecs as math

println(math.two)
println(math.add(1, 2))
```

当前规则：

- `use ./foo.ecs as foo` 会把模块导出对象绑定为普通对象。
- `.ecs` 文件中，路径相对当前脚本文件解析。
- 交互式 `ecsh` 顶层中，路径相对当前 `cwd` 解析。
- 同一规范化绝对路径模块只初始化一次。
- 循环导入会报错。

当前未实现：

- 搜索路径
- 命名导入
- `pub use`

## Shell 集成

### Shell Word 展开

这些展开发生在 `ecsh` shell 命令行中：

```bash
echo $HOME
echo ${env("HOME")}
echo ${1 + 2}
echo $(printf cmdsub)
echo ${...["a", "b", "c"]}
```

规则：

- `$VAR`：先查脚本作用域，再回退环境变量。
- `${expr}`：执行 ecscript 表达式，结果转成单个 shell word。
- `${...arr}`：数组展开为多个 argv。
- `$(cmd)`：通过 `/bin/sh -c` 做命令替换。
- 单引号内不展开；双引号内支持 `$` 展开。

### 命令值

`cmd{ ... }` 创建结构化命令值：

```ecs
let c = cmd{ printf "hello" };
println(text(c));
```

`command(...)` 是 argv-first builder，不解析 shell 语法：

```ecs
let c = command("/bin/echo", "hello", 7, true);
println(text(c));
```

执行命令：

```ecs
run(cmd{ echo hi });

let result = capture(cmd{ sh -c "printf out; printf err 1>&2; exit 3" });
println(result.code);
println(result.stdout);
println(result.stderr);
println(result.ok);

println(text(cmd{ printf "hello" }));
println(lines(cmd{ printf "a\nb\n" }));
```

命令派生：

```ecs
let with_env_cmd = with_env(
    cmd{ sh -c 'printf %s "$NAME"' },
    { NAME: "ecsh" }
);

let in_tmp = with_cwd(cmd{ /bin/pwd }, "/tmp");
```

当前边界：

- `cmd{ ... }` 支持单命令和 pipeline 子集。
- `cmd{ ... }` 不支持 `&&` / `||` / `;` / `&`。
- `run` / `capture` / `text` / `lines` / `with_env` / `with_cwd` 需要 shell-backed 执行上下文。
- 独立 `ecscript` 解释器和 `ecsh file.ecs` 文件脚本路径下，这些执行函数目前不可用。

## 内置函数

### 已检查内置函数索引

下列名称由 `just docs-check` 对照 `src/ecscript/builtin/mod.rs::lookup_builtin` 检查，确保本页没有漏掉当前已注册的 `ecscript` builtin。

<!-- BEGIN CHECKED ECSCRIPT BUILTIN INDEX -->
`all`, `any`, `builtins`, `capture`, `clone`, `command`, `commands`, `complete`, `cwd`, `each`, `env`, `extensions`, `filter`, `find`, `from_json`, `help`, `hook`, `insert`, `join`, `join_path`, `keys`, `len`, `lines`, `map`, `pop`, `print`, `println`, `prompt`, `push`, `range`, `read_lines`, `reduce`, `register_command`, `remove`, `run`, `set_cwd`, `set_env`, `slice`, `stdin`, `text`, `to_json`, `trim`, `unset_env`, `values`, `with_cwd`, `with_env`, `write_lines`
<!-- END CHECKED ECSCRIPT BUILTIN INDEX -->

### 参数检查和错误格式

简单 builtin 的参数数量和参数类型由运行时签名统一检查。错误信息包含 builtin 名、参数名、期望类型和实际类型：

```text
range argument 'start' expects Int, got Bool
len argument 'value' expects Array, Object, or String, got Function
push argument 'array' expects Array, got Int
```

已接入统一签名检查的 builtin：

- 环境 / IO / JSON / 字符串：`env`、`set_env`、`unset_env`、`cwd`、`stdin`、`read_lines`、`write_lines`、`to_json`、`from_json`、`trim`
- 集合和值流：`range`、`len`、`clone`、`keys`、`values`、`push`、`pop`、`insert`、`remove`、`slice`、`map`、`filter`、`reduce`、`each`、`any`、`all`、`find`、`join`、`join_path`
- 命令值：`command`、`run`、`capture`、`text`、`lines`、`with_cwd`
- 输出：`print`、`println`
- shell 扩展注册和目录 API：`hook`、`prompt`、`complete`、`bind`、`register_command`、`set_cwd`
- 环境派生命令：`with_env` 的基础参数类型

下列附加协议仍使用专门检查：`with_env` 的 `Object<String>` 字段约束、`hook` 的 hook name、`bind` 的 key string、`register_command` 的命令名合法性和 shell builtin 覆盖检查，以及 `complete` / `bind` 等回调返回对象协议。

### 环境

| Builtin | 说明 |
|---------|------|
| `env(name)` | 读取环境变量，缺失时返回 `nil` |
| `set_env(name, value)` | 设置当前进程环境变量 |
| `unset_env(name)` | 删除当前进程环境变量 |
| `cwd()` | 返回当前工作目录 |
| `join_path(left, right)` | 按平台规则拼接两个路径片段 |

```ecs
println(env("HOME"));
set_env("ECSH_DEMO", "ok");
println(join_path("/tmp", "ecsh"));
```

### IO

| Builtin | 说明 |
|---------|------|
| `stdin()` | 返回入口提供的 stdin 文本快照 |
| `read_lines()` | 返回 stdin 的行数组 |
| `write_lines(xs)` | 每个元素按行写到 stdout |
| `print(...)` | 输出参数，不自动换行 |
| `println(...)` | 输出参数并换行 |

```ecs
let lines = read_lines();
write_lines(lines);
println("done");
```

### 集合

| Builtin | 说明 |
|---------|------|
| `range(start, end)` | 返回闭区间整数数组，`start > end` 时为空 |
| `len(value)` | 返回 Array / Object / String 长度 |
| `clone(value)` | 显式深拷贝 Array / Object；Function / Builtin / Command 不可拷贝 |
| `push(array, value...)` | 追加元素 |
| `pop(array)` | 弹出并返回最后一个元素，空数组返回 `nil` |
| `insert(array, index, value)` | 插入元素 |
| `remove(array, index)` | 删除并返回元素 |
| `slice(array, start, end)` | 返回半开区间 `[start, end)` |
| `keys(object)` | 返回排序后的 key 数组 |
| `values(object)` | 按排序 key 返回 value 数组 |
| `map(array, func)` | 映射数组 |
| `filter(array, func)` | 过滤数组 |
| `reduce(array, initial, func)` | 归约数组 |
| `each(array, func)` | 遍历数组，用于副作用 |
| `any(array, func)` | 任一元素匹配即返回 true |
| `all(array, func)` | 所有元素匹配才返回 true |
| `find(array, func)` | 返回第一个匹配元素，否则 `nil` |
| `join(array, separator)` | 拼接数组元素为字符串 |

```ecs
let xs = range(1, 5);
println(xs |> filter((x) => x > 2) |> map((x) => x * 10) |> join(","));
```

Array 和 Object 是引用语义：

```ecs
let a = [1, 2, 3];
let b = a;
b[0] = 9;
println(a);      // [9, 2, 3]

let c = clone(a);
c[0] = 1;
println(a);      // [9, 2, 3]
println(c);      // [1, 2, 3]
```

`clone(value)` 会递归复制 Array / Object。Function、Builtin、Command 值不可拷贝；循环引用的容器会报 runtime error。标量值可以传给 `clone`，结果仍是同值。

### JSON

| Builtin | 说明 |
|---------|------|
| `to_json(value)` | 把 ecscript 值序列化为 JSON 字符串 |
| `from_json(text)` | 把 JSON 字符串解析为 ecscript 值 |

```ecs
let obj = from_json("{\"ok\":true}");
println(obj.ok);
println(to_json(obj));
```

常见命令输出组合：

```ecs
let payload = from_json(text(cmd{ printf "{\"ok\":true}" }));
println(payload.ok);
```

### 命令

| Builtin | 说明 |
|---------|------|
| `command(program, arg...)` | 创建 argv-first 命令值 |
| `run(cmd)` | 执行命令；非零状态报 runtime error |
| `capture(cmd)` | 执行命令并返回 `{ code, signal, stdout, stderr, duration_ms, ok }` |
| `text(cmd)` | 返回 stdout 文本；非零状态报 runtime error |
| `lines(cmd)` | 返回 stdout 行数组；非零状态报 runtime error |
| `with_env(cmd, obj)` | 返回带环境覆盖的新命令值 |
| `with_cwd(cmd, path)` | 返回带 cwd 覆盖的新命令值 |

`command(program, arg...)` 的 `program` 必须是 String；后续 argv 片段接受 String / Int / Float / Bool / Nil，不接受 Array / Object / Function / Builtin / Command。

### 内省

| Builtin | 说明 |
|---------|------|
| `help()` | 返回 callable 总览文本 |
| `help(name)` | 返回指定 callable 的签名、说明和示例 |
| `builtins()` | 返回 ecscript builtin 名称数组 |
| `commands()` | 返回当前 shell 中可见命令及来源 |
| `extensions()` | 返回 shell extension 名称数组 |

```ecs
println(help("map"));
println(builtins());
println(commands());
```

当前职责边界：

- `help(...)` 解释 callable/API 的语义。
- `type` / `which` 解释 shell 命令名如何解析。
- 同名 ecscript builtin 和 shell builtin 的展示边界仍在阶段 11 后半收口中。

### 工具

| Builtin | 说明 |
|---------|------|
| `trim(text)` | 去掉字符串首尾空白 |

```ecs
println(trim("  hi  "));
```

## 扩展 API

这些 API 需要 shell-backed 执行上下文，通常写在 `.ecshrc`、被 `source` 的脚本或交互式 `ecsh` 顶层中。

### `hook(name, func)`

支持的 hook：

| Hook | 触发时机 |
|------|----------|
| `"before_prompt"` | 每次 prompt 前 |
| `"after_cd"` | `cd` 或 `set_cwd()` 改变目录后 |
| `"preexec"` | shell 成功解析后、执行前 |
| `"postexec"` | 命令产生最终结果后，包括 parse error 和执行失败 |

```ecs
hook("after_cd", (ctx) => {
    println("cd:", ctx.cwd);
});
```

`after_cd` hook 内调用 `set_cwd()` 会更新目录和 `PWD` / `OLDPWD`，但不会递归触发 `after_cd`。

### `prompt(func)`

注册 prompt handler。handler 应返回 String；错误或非 String 返回会回退默认 prompt。

```ecs
prompt((ctx) => {
    return ctx.cwd + " $ ";
});
```

### `complete(name, func)`

注册命令补全 handler。handler 返回候选数组。

```ecs
complete("git", (ctx) => {
    return [
        { value: "status", display: "status", desc: "show working tree status" },
        { value: "diff" }
    ];
});
```

候选对象字段：

- `value`：必需，插入值
- `display`：可选，展示文本
- `desc`：可选，说明
- `kind`：可选，分类

### `bind(key, func)`

注册按键绑定。回调接收 `{ key, line, cursor, cwd, history }`。

```ecs
bind("ctrl-g", (ctx) => {
    return { action: "insert", text: "git status" };
});
```

常用 action：

- `{ action: "insert", text: "..." }`
- `{ action: "set_line", text: "..." }`
- `{ action: "accept" }`
- `{ action: "newline" }`
- `{ action: "complete" }`
- `{ action: "complete_hint" }`
- `{ action: "clear_screen" }`
- `{ action: "interrupt" }`
- `{ action: "history_search_backward" }`
- `{ action: "history_search_forward" }`
- `{ action: "previous_history" }`
- `{ action: "next_history" }`

### `register_command(name, func)`

注册一个新的 ecsh 顶层命令。回调接收 `{ name, args, cwd }`。

```ecs
register_command("hello", (ctx) => {
    println("hello", join(ctx.args, ","));
    return 0;
});
```

规则：

- 返回 `nil` 表示成功。
- 返回非负 Int 作为退出码。
- 不能覆盖 shell builtin。
- 不支持后台、管道和重定向。

### `set_cwd(path)`

修改当前 shell 工作目录，并同步 `PWD` / `OLDPWD`。

```ecs
register_command("jump_tmp", (ctx) => {
    set_cwd("/tmp");
});
```

## 当前边界

- `if` 不是表达式。
- block 没有值语义。
- 裸 range 只在 `for` 中合法；普通值使用 `range(start, end)`。
- 闭包捕获只自动传一层。
- 没有字符串插值。
- 没有多行字符串。
- 模块没有搜索路径、命名导入和 `pub use`。
- `cmd{}` 不支持 `&&` / `||` / `;` / `&`。
- 命令桥执行函数需要 shell-backed 上下文。
- 脚本命令不支持后台、管道和重定向。
