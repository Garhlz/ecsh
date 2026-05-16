/*
 * ecsh-c — Elaine & Cornelia's shell (C version)
 *
 * 一个满足教学实验要求的最小 Unix shell，支持：
 *   - 内部命令 (help, exit, cd, pwd)
 *   - 外部命令 (fork + execvp)
 *   - 无效命令检测 (execvp 失败 → command not found)
 *   - 管道 (cmd1 | cmd2)
 *   - 输出重定向 (> / >>) 和输入重定向 (<)
 *   - 命令执行前后输出 starting/ending 信息
 *   - 管道传输前后输出 transferring/finish 信息
 *
 * 编译: make
 * 运行: ./ecsh
 */

/* _POSIX_C_SOURCE: 启用 POSIX.1-2008 扩展声明（PATH_MAX, gethostname 等） */
#define _POSIX_C_SOURCE 200809L

#include <fcntl.h>
#include <limits.h>
#include <netdb.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h>

#define MAX_LINE 1024 /* 单行输入最大长度 */
#define MAX_ARGS 64   /* 单条命令最大参数数 */
#define MAX_PIPES 16  /* 最大管道段数 (cmd1 | cmd2 | ... | cmd16) */

/* 上条命令退出码，全局供 prompt 显示 [exit_code] 使用 */
static int g_last_status = 0;

/* ========== ANSI 颜色常量 ========== */

#define ANSI_RESET "\x1b[0m"
#define ANSI_BOLD_MAGENTA "\x1b[1;35m"
#define ANSI_BOLD_GREEN "\x1b[1;32m"
#define ANSI_BOLD_CYAN "\x1b[1;36m"
#define ANSI_BOLD_BLUE "\x1b[1;34m"
#define ANSI_BOLD_RED "\x1b[1;31m"
#define ANSI_BOLD_YELLOW "\x1b[1;33m"
/* 256-color 调色板，用于 help 标题的彩色输出 */
#define ANSI_HOT_PINK "\x1b[1;38;5;201m"
#define ANSI_NEON_GREEN "\x1b[1;38;5;120m"
#define ANSI_SUN_YELLOW "\x1b[1;38;5;226m"
#define ANSI_ELECTRIC_CYAN "\x1b[1;38;5;51m"
#define ANSI_WARM_ORANGE "\x1b[1;38;5;214m"

/* ========== 内部命令实现 ========== */

/*
 * help — 打印帮助信息。
 * 标题每个词用不同颜色，与 Rust 版 ecsh 风格一致。
 * 当 stdout 不是终端（如重定向到文件）时自动去掉颜色。
 */
int builtin_help(void) {
    int use_color = isatty(STDOUT_FILENO);

    printf("%s%secsh-c%s - %sElaine%s %s&%s %sCornelia's%s %sshell%s\n",
           use_color ? ANSI_HOT_PINK : "", use_color ? ANSI_HOT_PINK : "",
           use_color ? ANSI_RESET : "", use_color ? ANSI_NEON_GREEN : "",
           use_color ? ANSI_RESET : "", use_color ? ANSI_SUN_YELLOW : "",
           use_color ? ANSI_RESET : "", use_color ? ANSI_ELECTRIC_CYAN : "",
           use_color ? ANSI_RESET : "", use_color ? ANSI_WARM_ORANGE : "",
           use_color ? ANSI_RESET : "");
    printf("ecsh-c builtins:\n");
    printf("  help - show this help message\n");
    printf("  cd - change current working directory\n");
    printf("  pwd - print working directory\n");
    printf("  exit - exit the shell\n");
    return 0;
}

/*
 * exit — 退出 shell。
 * 直接调用 exit(0) 终止 shell 进程。
 */
int builtin_exit(void) {
    exit(0);
    return 0; /* 不可达，消除编译器警告 */
}

/*
 * cd [dir] — 切换工作目录。
 * 不传参数时默认回到 $HOME。
 * 内部命令必须在 shell 进程内执行（不能 fork），因为 chdir 修改的是调用进程的 cwd，
 * 子进程中 chdir 不会影响父进程 shell。
 */
int builtin_cd(char** args) {
    const char* dir = args[1];
    if (dir == NULL || strcmp(dir, "~") == 0) {
        dir = getenv("HOME");
        if (dir == NULL) {
            fprintf(stderr, "cd: HOME not set\n");
            return 1;
        }
    }
    if (chdir(dir) != 0) {
        perror("cd");
        return 1;
    }
    return 0;
}

/*
 * pwd — 打印当前工作目录。
 * 使用 getcwd() 系统调用获取进程的 cwd。
 */
int builtin_pwd(void) {
    char cwd[PATH_MAX];
    if (getcwd(cwd, sizeof(cwd)))
        printf("%s\n", cwd);
    else {
        perror("pwd");
        return 1;
    }
    return 0;
}

/* ========== 命令分发 ========== */

/*
 * try_builtin — 判断并执行内部命令。
 * 返回 1 表示已处理（是内部命令），返回 0 表示未匹配（需要走外部命令路径）。
 * 同时记录退出码到 g_last_status，供 prompt 显示。
 */
int try_builtin(char** args) {
    if (args[0] == NULL) return 0;

    if (strcmp(args[0], "help") == 0) {
        printf("%s starting...\n", args[0]);
        builtin_help();
        printf("%s ending.\n", args[0]);
        return 1;
    }
    if (strcmp(args[0], "exit") == 0) {
        printf("%s starting...\n", args[0]);
        builtin_exit();
        return 1;
    }
    if (strcmp(args[0], "cd") == 0) {
        printf("%s starting...\n", args[0]);
        g_last_status = builtin_cd(args);
        printf("%s ending.\n", args[0]);
        return 1;
    }
    if (strcmp(args[0], "pwd") == 0) {
        printf("%s starting...\n", args[0]);
        g_last_status = builtin_pwd();
        printf("%s ending.\n", args[0]);
        return 1;
    }
    return 0;
}

/* ========== 重定向 ========== */

/*
 * parse_redirection — 从参数数组中提取重定向信息。
 *
 * 遍历 args，遇到 > / >> / < 时取出下一个参数作为文件名，
 * 并将这些重定向符号和文件名从 args 中移除。
 * 返回移除后的剩余参数个数，使 args 只包含命令本身和它的参数。
 *
 * 例: ["cat", "<", "in.txt", ">", "out.txt"] → args=["cat"], infile="in.txt", outfile="out.txt"
 */
int parse_redirection(char** args, int argc, char** infile, char** outfile, int* append) {
    *infile = NULL;
    *outfile = NULL;
    *append = 0;

    int new_argc = 0;
    for (int i = 0; i < argc; i++) {
        if (strcmp(args[i], ">") == 0) {
            if (i + 1 < argc) {
                *outfile = args[i + 1];
                *append = 0;
                i++; /* 跳过文件名参数，下一轮循环跳过它 */
            }
        } else if (strcmp(args[i], ">>") == 0) {
            if (i + 1 < argc) {
                *outfile = args[i + 1];
                *append = 1;
                i++;
            }
        } else if (strcmp(args[i], "<") == 0) {
            if (i + 1 < argc) {
                *infile = args[i + 1];
                i++;
            }
        } else {
            /* 非重定向符号，保留在 args 中 */
            args[new_argc++] = args[i];
        }
    }
    args[new_argc] = NULL;
    return new_argc;
}

/*
 * apply_redirection — 在子进程中应用重定向。
 *
 * 使用 dup2() 将文件描述符替换到 stdin/stdout：
 *   - < : open(infile) → dup2(fd, STDIN)  — 输入重定向
 *   - > : open(outfile, O_TRUNC) → dup2(fd, STDOUT) — 覆盖写
 *   - >>: open(outfile, O_APPEND) → dup2(fd, STDOUT) — 追加写
 *
 * 此函数只在子进程中调用，因为 dup2 会永久替换当前进程的 fd。
 * 调用后 execvp 执行的程序就继承了重定向后的 fd。
 */
void apply_redirection(const char* infile, const char* outfile, int append) {
    if (infile) {
        int fd = open(infile, O_RDONLY);
        if (fd < 0) {
            perror(infile);
            exit(1);
        }
        dup2(fd, STDIN_FILENO);
        close(fd); /* dup2 后原 fd 不再需要，关闭避免泄漏 */
    }
    if (outfile) {
        int flags = O_WRONLY | O_CREAT | (append ? O_APPEND : O_TRUNC);
        int fd = open(outfile, flags, 0644);
        if (fd < 0) {
            perror(outfile);
            exit(1);
        }
        dup2(fd, STDOUT_FILENO);
        close(fd);
    }
}

/* ========== 输入解析 ========== */

/*
 * split_line — 将一行输入按空白字符分割为参数数组。
 * 使用 strtok() 逐个切分 token，存入 args[]。
 * 返回参数个数 argc，args[argc] = NULL 方便直接传给 execvp。
 */
int split_line(char* line, char** args, int max_args) {
    int argc = 0;
    char* token = strtok(line, " \t\n");
    while (token != NULL && argc < max_args - 1) {
        args[argc++] = token;
        token = strtok(NULL, " \t\n");
    }
    args[argc] = NULL;
    return argc;
}

/*
 * split_pipes — 按管道符 | 将参数数组切分为多段。
 *
 * 将 "|" 处替换为 NULL，使每段成为独立的 argv 数组。
 * pipe_args[i] 指向第 i 段的起始位置。
 *
 * 例: ["ls", "-l", "|", "grep", "txt", "|", "wc"]
 *   → pipe_args[0]=["ls", "-l"]  pipe_args[1]=["grep", "txt"]  pipe_args[2]=["wc"]
 *   → 返回 3
 */
int split_pipes(char** args, int argc, char** pipe_args[], int max_pipes) {
    int pipe_count = 0;
    pipe_args[0] = &args[0];

    for (int i = 0; i < argc; i++) {
        if (strcmp(args[i], "|") == 0) {
            args[i] = NULL; /* 切断，使前一段成为独立的 NULL 结尾数组 */
            pipe_count++;
            if (pipe_count >= max_pipes) break;
            pipe_args[pipe_count] = &args[i + 1]; /* 下一段从这里开始 */
        }
    }

    return pipe_count + 1; /* 管道段数 = 管道符个数 + 1 */
}

/* ========== 提示符 ========== */

/*
 * print_prompt — 显示两行彩色提示符。
 *
 * 第 1 行: [ecsh-c] user@host:~/path [exit_code]
 * 第 2 行: $
 *
 * 与 Rust 版 ecsh 保持一致：
 *   - HOME 路径前缀替换为 ~（如 /home/elaine/work → ~/work）
 *   - 退出码为 0 时不显示 [exit_code]，避免 prompt 冗余
 *   - 非 0 退出码用红色标注
 *   - stdout 不是终端时自动去掉颜色（重定向到文件时输出纯文本）
 */
void print_prompt(void) {
    char cwd_buf[PATH_MAX];
    char hostname[128];
    const char* user = getenv("USER");
    if (!user) user = "unknown";

    /* 优先取 $HOSTNAME 环境变量，未设置则 fallback 到 gethostname() 系统调用 */
    const char* host = getenv("HOSTNAME");
    if (!host || host[0] == '\0') {
        gethostname(hostname, sizeof(hostname));
        hostname[sizeof(hostname) - 1] = '\0';
        host = hostname;
    }

    if (!getcwd(cwd_buf, sizeof(cwd_buf))) strcpy(cwd_buf, "?");

    /* 将 HOME 前缀替换为 ~，使路径更简洁 */
    const char* home = getenv("HOME");
    const char* display_cwd;
    char tilde_cwd[PATH_MAX];
    if (home && strcmp(cwd_buf, home) == 0) {
        /* 恰好是 HOME 目录 → 显示 ~ */
        strcpy(tilde_cwd, "~");
        display_cwd = tilde_cwd;
    } else if (home && strncmp(cwd_buf, home, strlen(home)) == 0 && cwd_buf[strlen(home)] == '/') {
        /* HOME 的子目录 → 显示 ~/path */
        snprintf(tilde_cwd, sizeof(tilde_cwd), "~%s", cwd_buf + strlen(home));
        display_cwd = tilde_cwd;
    } else {
        display_cwd = cwd_buf;
    }

    int use_color = isatty(STDOUT_FILENO);

    /* 第一行: [ecsh-c] user@host:~/path [exit_code] */
    printf("%s[ecsh-c]%s ", use_color ? ANSI_BOLD_MAGENTA : "", use_color ? ANSI_RESET : "");
    printf("%s%s%s@%s%s%s", use_color ? ANSI_BOLD_GREEN : "", user, use_color ? ANSI_RESET : "",
           use_color ? ANSI_BOLD_CYAN : "", host, use_color ? ANSI_RESET : "");
    printf(":%s%s%s", use_color ? ANSI_BOLD_BLUE : "", display_cwd, use_color ? ANSI_RESET : "");

    if (g_last_status != 0)
        printf(" %s[%d]%s", use_color ? ANSI_BOLD_RED : "", g_last_status,
               use_color ? ANSI_RESET : "");

    /* 第二行: $ */
    printf("\n%s$ %s", use_color ? ANSI_BOLD_YELLOW : "", use_color ? ANSI_RESET : "");
    fflush(stdout);
}

/* ========== 命令执行 ========== */

/*
 * execute_single — 执行单条命令（无管道）。
 *
 * 流程:
 *   1. parse_redirection() 从参数中剥离重定向
 *   2. try_builtin()       尝试匹配内部命令
 *   3. fork()              创建子进程
 *   4. 子进程: apply_redirection() → execvp()
 *   5. 父进程: waitpid() 等待子进程结束，记录退出码
 *
 * fflush(stdout) 在 fork 前调用，防止 stdout 缓冲区中的内容
 * 被复制到子进程中导致重复输出。
 */
void execute_single(char** args, int argc) {
    char *infile = NULL, *outfile = NULL;
    int append = 0;

    argc = parse_redirection(args, argc, &infile, &outfile, &append);
    if (argc == 0) return;

    /* 内部命令: 在 shell 进程内直接执行，不 fork */
    if (try_builtin(args)) return;

    /* 外部命令: fork 子进程执行 */
    printf("%s starting...\n", args[0]);
    fflush(stdout); /* fork 前必须刷新，否则缓冲区内容被子进程复制导致重复输出 */

    pid_t pid = fork();
    if (pid < 0) {
        perror("fork");
        return;
    }

    if (pid == 0) {
        /* 子进程: 应用重定向，然后替换为外部程序 */
        apply_redirection(infile, outfile, append);
        execvp(args[0], args);
        /* execvp 只有在失败时才会返回，说明命令不存在 */
        fprintf(stderr, "%s: command not found\n", args[0]);
        exit(127); /* 127 是 shell 约定: 命令未找到的退出码 */
    }

    /* 父进程: 等待子进程结束 */
    int status;
    waitpid(pid, &status, 0);
    /* WIFEXITED: 子进程正常退出 (调用 exit/return) */
    /* WEXITSTATUS: 取出退出码 (0-255) */
    if (WIFEXITED(status))
        g_last_status = WEXITSTATUS(status);
    else
        g_last_status = 1; /* 被信号终止等异常情况 */
    printf("%s ending.\n", args[0]);
}

/*
 * execute_pipeline — 执行管道命令 (cmd1 | cmd2 | ... | cmdN)。
 *
 * 思路: 创建 n-1 个 pipe，fork n 个子进程，
 * 每个子进程用 dup2 把 stdin/stdout 接到对应的管道端。
 *
 * 关键细节:
 *   - 所有 pipe 在 fork 前创建好，保证子进程能继承所有 fd
 *   - 每个子进程必须关闭所有不用的 pipe fd，否则读端不会收到 EOF，下游进程会永远阻塞在 read
 *   - 父进程也要关闭所有 pipe fd，理由同上
 *   - 文件重定向在 dup2(pipe) 之后应用，可以覆盖管道（如 cmd | cat > out.txt）
 */
void execute_pipeline(char** pipe_args[], int n) {
    int pipefd[MAX_PIPES][2];
    pid_t pids[MAX_PIPES];

    /* 预先创建所有管道: pipefd[i][0]=读端, pipefd[i][1]=写端 */
    for (int i = 0; i < n - 1; i++) {
        if (pipe(pipefd[i]) < 0) {
            perror("pipe");
            return;
        }
    }

    for (int i = 0; i < n; i++) {
        char *infile = NULL, *outfile = NULL;
        int append = 0;
        char** args = pipe_args[i];

        /* 计算该段参数个数 (NULL 结尾) */
        int seg_argc = 0;
        while (args[seg_argc] != NULL) seg_argc++;

        seg_argc = parse_redirection(args, seg_argc, &infile, &outfile, &append);
        if (seg_argc == 0) continue;

        printf("%s starting...\n", args[0]);
        fflush(stdout);

        pids[i] = fork();
        if (pids[i] < 0) {
            perror("fork");
            return;
        }

        if (pids[i] == 0) {
            /* --- 子进程: 设置管道连接 + 重定向 --- */

            /*
             * dup2(old_fd, new_fd): 将 new_fd 指向 old_fd 所指的文件。
             * 效果: 写入 new_fd 实际写入 old_fd 指向的地方。
             * 这里用 dup2 把 stdin/stdout 重连到管道的读/写端。
             */

            /* 不是最后一段 → stdout 接到当前管道的写端 */
            if (i < n - 1) {
                dup2(pipefd[i][1], STDOUT_FILENO);
            }

            /* 不是第一段 → stdin 接到前一个管道的读端 */
            if (i > 0) {
                dup2(pipefd[i - 1][0], STDIN_FILENO);
            }

            /*
             * 关闭所有 pipe fd: 子进程已经通过 dup2 拿到了需要的端，
             * 剩余的 fd 必须关闭，否则:
             *   - 写端未关闭 → 读端的 read() 不会返回 0 (EOF)，
             *     下游进程会永远阻塞在等待输入
             *   - 读端未关闭 → 写端的 write() 不会收到 SIGPIPE，
             *     上游进程不知道下游已退出
             */
            for (int j = 0; j < n - 1; j++) {
                close(pipefd[j][0]);
                close(pipefd[j][1]);
            }

            /* 文件重定向优先级高于管道: 如 cmd1 | cmd2 > out.txt 会被重定向到文件 */
            apply_redirection(infile, outfile, append);

            execvp(args[0], args);
            fprintf(stderr, "%s: command not found\n", args[0]);
            exit(127);
        }
    }

    /* --- 父进程 --- */

    /* 父进程关闭所有 pipe fd，理由同上: 避免子进程读不到 EOF */
    for (int i = 0; i < n - 1; i++) {
        close(pipefd[i][0]);
        close(pipefd[i][1]);
    }

    /* 管道传输开始: 所有子进程已启动，数据开始流过管道 */
    for (int i = 0; i < n - 1; i++) {
        printf("pipe(%s|%s) transferring data...\n", pipe_args[i][0], pipe_args[i + 1][0]);
    }
    fflush(stdout);

    /* 等待所有子进程结束 */
    for (int i = 0; i < n; i++) {
        int status;
        waitpid(pids[i], &status, 0);
        /* 用管道最后一条命令的退出码作为整个管道的退出码 */
        if (i == n - 1) {
            if (WIFEXITED(status))
                g_last_status = WEXITSTATUS(status);
            else
                g_last_status = 1;
        }
        printf("%s ending.\n", pipe_args[i][0]);
    }

    /* 管道传输完毕: 所有子进程已退出 */
    for (int i = 0; i < n - 1; i++) {
        printf("pipe(%s|%s) finish data.\n", pipe_args[i][0], pipe_args[i + 1][0]);
    }
}

/* ========== 主循环 ========== */

/*
 * main — shell 主循环: 提示符 → 读输入 → 解析 → 执行 → 重复
 *
 * 流程:
 *   1. print_prompt()     显示提示符
 *   2. fgets()            读取一行输入
 *   3. split_line()       按空白分割为参数数组
 *   4. 检测参数中是否有 | 判断走管道还是单命令路径
 *   5. split_pipes() + execute_pipeline()  或  execute_single()
 */
int main(void) {
    char line[MAX_LINE];
    char* args[MAX_ARGS];
    char** pipe_args[MAX_PIPES];

    printf("ecsh-c - Elaine & Cornelia's shell (C version)\n");
    printf("Type 'help' for built-in commands\n\n");

    while (1) {
        print_prompt();

        /* fgets 返回 NULL 表示 EOF (Ctrl-D)，退出 shell */
        if (fgets(line, sizeof(line), stdin) == NULL) {
            printf("\n");
            break;
        }

        /* 空行直接跳过 */
        if (line[0] == '\n') continue;

        /* 将输入行分割为参数数组 */
        int argc = split_line(line, args, MAX_ARGS);
        if (argc == 0) continue;

        /* 检测是否包含管道符 |，决定走哪条执行路径 */
        int has_pipe = 0;
        for (int i = 0; i < argc; i++) {
            if (strcmp(args[i], "|") == 0) {
                has_pipe = 1;
                break;
            }
        }

        if (has_pipe) {
            /* 管道路径: 切分段 → 创建管道 → fork 多个子进程 */
            int n = split_pipes(args, argc, pipe_args, MAX_PIPES);
            execute_pipeline(pipe_args, n);
        } else {
            /* 单命令路径: 内部命令直接执行 / 外部命令 fork+exec */
            execute_single(args, argc);
        }
    }

    return 0;
}
