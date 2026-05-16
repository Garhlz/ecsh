# ecsh-c — Elaine & Cornelia's Shell (C version)

满足操作系统课程实验要求的教学 shell。

## 功能

- 内部命令: `help`, `cd`, `pwd`, `exit`
- 外部命令: `ls`, `cat`, `grep` 等任意磁盘可执行程序
- 无效命令: 自动检测并提示 `command not found`
- 管道: `cmd1 | cmd2`
- 输出重定向: `>` (覆盖), `>>` (追加)
- 输入重定向: `<`
- 命令执行前后输出 `starting...` / `ending.`
- 管道传输前后输出 `transferring data...` / `finish data.`
- 彩色提示符: `[ecsh-c] user@host:~/path [exit_code]`
- `~` 路径缩写

## 编译与运行

```bash
make        # 编译
./ecsh      # 运行
make clean  # 清理
```

## 示例

```
[ecsh-c] elaine@host:~/projects
$ help
help starting...
ecsh-c - Elaine & Cornelia's shell
ecsh-c builtins:
  help - show this help message
  cd - change current working directory
  pwd - print working directory
  exit - exit the shell
help ending.

[ecsh-c] elaine@host:~/projects
$ ls | head -3
ls starting...
head starting...
pipe(ls|head) transferring data...
Cargo.toml
cshell
src
ls ending.
head ending.
pipe(ls|head) finish data.

[ecsh-c] elaine@host:~/projects
$ echo hello > out.txt
echo starting...
echo ending.

[ecsh-c] elaine@host:~/projects
$ cat < out.txt
cat starting...
hello
cat ending.
```
