# ecsh

`ecsh` 是 **Elaine & Cornelia's shell**，一个用 Rust 编写的教学型类 Unix shell。

仓库当前包含两部分：

- `ecsh`：交互式 shell，已具备命令执行、管道、重定向、条件执行、后台作业和最小 job control
- `ecscript`：与 `ecsh` 一起演进的小型脚本语言，已具备独立解释器、REPL、控制流、函数、闭包和容器类型

## 入口

构建：

```bash
cargo build
```

运行 `ecsh`：

```bash
cargo run
```

运行 `ecscript`：

```bash
cargo run --bin ecscript --
cargo run --bin ecscript -- script.ecs
cargo run --bin ecscript -- -e 'println(1 + 2);'
```

运行测试：

```bash
cargo test
```

## 当前状态

当前已经完成：

- shell 主体能力：外部命令、内置命令、管道、`<` / `>` / `>>`、`&&` / `||` / `;`、行尾 `&`、`jobs` / `fg` / `bg`
- shell 交互与体验：Tab 补全、alias / unalias、`trap EXIT|INT`、`type` / `which` / `history`
- shell 运行时展开：`$VAR`、`${expr}`、`${env("VAR")}`、`$(cmd)`、`${...arr}`
- `ecscript` stage 1-6：表达式、语句、数组/对象、控制流、函数/闭包、独立解释器入口、源码定位错误格式化、`env()` / `range()`
- 阶段 7：`ecsh` 顶层脚本模式、`ecsh file.ecs`、`source` / `.`、`.ecshrc` 已接通
- 阶段 8：`cmd{ ... }` 命令字面量、`command(...)` builder、`run` / `capture` / `text` / `lines`、`from_json`、`with_env` / `with_cwd` 已接通；单命令纯输出 builtin 也可走命令桥
- 阶段 9（进行中）：`|>` 值流语法糖、`map` / `filter` / `reduce` / `each` / `any` / `all` / `find` / `join`、`slice`
- 阶段 7.5：shell 诊断与交互收口已完成，当前已具备结构化 `ParseError`、续行读取和 shell parse 错误定位输出

当前仍未完成：

- 阶段 12：here-doc、glob、subshell、更完整的作业控制与执行语义
- `ecscript` 的 block value、模块系统、字符串插值、多行字符串

更完整的进度说明见 [docs/status.md](/home/elaine/work/projects/ecsh/docs/status.md)。

## 快速示例

### ecsh

```bash
echo hello
echo prefix-$HOME
echo ${env("HOME")}
echo ${1 + 2}
echo $(printf cmdsub)
echo hello | grep h
pwd > out.txt
cat < out.txt
true && echo ok
false || echo fallback
sleep 5 &
jobs
```

### ecscript

```ecs
let xs = [1, 2];
push(xs, 3);
let ys = range(1, 5);

func fact(n) {
    if n <= 1 {
        return 1;
    }
    return n * fact(n - 1);
}

println(fact(5));
println(text(cmd{ printf "hello" }));
println(text(command("/bin/echo", "builder", 7, true)));
println(from_json(text(cmd{ printf "{\"ok\":true}" })).ok);
println(range(1, 5) |> filter((x) => x > 2) |> map((x) => x * 10) |> join(","));
```

## 文档

- [docs/status.md](/home/elaine/work/projects/ecsh/docs/status.md)：当前进度与后续入口
- [docs/ecscript-manual.md](/home/elaine/work/projects/ecsh/docs/ecscript-manual.md)：`ecscript` 当前实现手册
- [docs/TODO.md](/home/elaine/work/projects/ecsh/docs/TODO.md)：设计备忘与分阶段路线
- [examples/ecscript/README.md](/home/elaine/work/projects/ecsh/examples/ecscript/README.md)：完整 `.ecs` 冒烟脚本与当前边界示例

## 仓库结构

```text
src/
  main.rs            # ecsh 主循环
  lexer.rs           # shell lexer
  parser.rs          # shell parser
  executor/          # 执行、fork/exec、作业控制、运行时展开
  ecscript/          # ecscript 词法、解析、求值、环境、值模型
  bin/ecscript.rs    # ecscript CLI / REPL
tests/
  lexer.rs
  parser.rs
  smoke.rs
  ecscript_cli.rs
docs/
  status.md
  ecscript-manual.md
  TODO.md
```

## 当前边界

`ecsh` 当前不是完整 POSIX shell。明确未实现的能力包括：

- here-doc `<<`
- 命令替换之外的更完整 shell 展开规则
- glob 展开
- subshell `()`
- 更完整的 job spec 和异步完成通知

`ecscript` 当前的主要边界包括：

- `if` 仍是语句，不是表达式
- `1..10` / `1..=10` 只在 `for` 语句里合法；需要值时使用 `range(start, end)`
- 没有模块系统
- 没有字符串插值和多行字符串
- block 还没有值语义
