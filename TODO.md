# ecsh 未来功能开发清单

按投入产出比（ROI）排序，越靠前越值得先做。

## 高优先（体验提升大、实现成本低）

### 1. Tab 补全
- **描述**：按 Tab 自动补全文件名和命令名
- **涉及文件**：新增 `src/completion.rs`，修改 `src/input.rs`
- **关键点**：rustyline 有 `Completer` trait，实现 `complete()` 方法，返回候选列表
- **工作量**：小

### 2. alias / unalias 命令
- **描述**：`alias ll='ls -l'` / `unalias ll`
- **涉及文件**：新增两个 builtin，修改 `src/builtin.rs`
- **关键点**：alias 展开发生在 parser 阶段——tokenize 后检查 word 是否是已有 alias，是则替换
- **工作量**：小

### 3. 配置文件 ~/.ecshrc
- **描述**：shell 启动时自动执行 `~/.ecshrc`，用于设置 alias、环境变量
- **涉及文件**：`src/main.rs`（启动时加载）
- **关键点**：在 `init_shell_job_control` 之后、主循环之前，逐行 source 执行
- **工作量**：小

## 中优先（有用但实现成本中等）

### 4. here-doc (<<)
- **描述**：`cat << END` 将多行文本作为命令的 stdin
- **涉及文件**：`src/lexer.rs`（新增 here-doc 状态），`src/parser.rs`（生成 here-doc AST 节点），`src/redirection.rs`
- **关键点**：lexer 遇到 `<<` 时读入之后的行直到遇到结束标记；执行时把多行文本写入管道或用临时文件
- **工作量**：中

### 5. 通配符展开 (globbing)
- **描述**：`ls *.rs` 将 `*.rs` 展开为当前目录匹配的文件列表
- **涉及文件**：新增 `src/expansion.rs`，放在 parser 之后、executor 之前
- **关键点**：用 `glob` crate 或手写 `fnmatch`；需要处理 `*` `?` `[...]` 三种模式
- **工作量**：中

### 6. 信号处理增强
- **描述**：`trap` 命令、更好的信号传播（Ctrl-C 只杀前台 job，不杀管道中间进程）
- **涉及文件**：`src/signals.rs`、`src/builtin.rs`
- **关键点**：当前信号模型是正确的，增强主要是边界情况
- **工作量**：中

## 低优先（完整 shell 体验但非核心）

### 7. 命令替换 $(cmd)
- **描述**：`echo "today is $(date)"` → 先执行 date，输出替换到原位置
- **涉及文件**：`src/lexer.rs`（识别 `$(` 语法），parser 或 executor 需要递归调用 shell
- **关键点**：需要一个 pipe 捕获子进程 stdout；可能 fork 一个子 shell
- **工作量**：大（需要递归执行 + 输出捕获）

### 8. 管道增强
- **描述**：`|&` 同时重定向 stderr、`!` 取反退出码
- **涉及文件**：`src/lexer.rs`、`src/parser.rs`、`src/executor/launch.rs`
- **关键点**：`|&` 是 bash 的 `2>&1 |` 简写；`!` 是 POSIX 管道取反
- **工作量**：小～中

### 9. 更多内置命令
- **描述**：`type`、`which`、`read`、`shift`、`source`/`.`、`history`、`set`
- **涉及文件**：`src/builtin.rs`
- **关键点**：`source` 是读文件逐行执行；`type` 区分 builtin/alias/external
- **工作量**：每个都很小

### 10. subshell ($())
- **描述**：`(cd /tmp && ls)` 在子进程中执行
- **涉及文件**：`src/parser.rs`、`src/executor/`
- **关键点**：fork 一个子 shell 执行括号内的命令
- **工作量**：中

---

## 技术债（代码质量改进）

- [ ] 去掉 `run_command` 里的 "starting... / ending." 调试输出
- [ ] `main.rs` 里 `run_parsed_line` 的 clone 开销（递归构造 ParsedJob 时可以传引用而不是 clone）
- [ ] parser 的 `ParsedLine` 从 Box 改成 Rc 或 arena，减少 clone
- [ ] 给 executor 的 launch 部分加更多集成测试（fork/exec 路径本来就难单测，但 smoke 测试可以更细）
