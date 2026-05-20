//! 信号处理：shell 自身的信号屏蔽 与 子进程的信号恢复。
//!
//! job control 中信号处理的核心原则：
//!   1. shell 忽略交互信号（SIGINT/SIGTSTP 等），把终端产生的信号留给前台进程组
//!   2. fork 出来的子进程必须恢复信号默认行为，否则用户程序也会继承 shell 的忽略策略

use nix::sys::signal::{SaFlags, SigAction, SigHandler, SigSet, Signal, sigaction};

/// 交互式 shell 需要忽略的 5 个信号。
///
/// 为什么是这 5 个？它们都来自终端驱动或内核，默认行为都会"阻止进程继续跑"。
/// shell 忽略它们，是为了保护自己的主循环不被打断，让信号正确地到达前台 job。
const INTERACTIVE_SIGNALS: [Signal; 5] = [
    // Ctrl-C → 内核向终端前台进程组广播 SIGINT，默认行为：终止进程。
    // shell 忽略后，Ctrl-C 只会杀死前台 job，不会杀死 shell 自己。
    Signal::SIGINT,
    // Ctrl-\ → 内核向终端前台进程组广播 SIGQUIT，默认行为：终止进程 + 产生 core dump。
    // shell 忽略后，Ctrl-\ 只会影响前台 job。
    Signal::SIGQUIT,
    // Ctrl-Z → 内核向终端前台进程组广播 SIGTSTP（Terminal STop），默认行为：暂停进程。
    // shell 忽略后，Ctrl-Z 只会暂停前台 job，不会把 shell 自己挂起。
    Signal::SIGTSTP,
    // 当后台进程组试图从终端读取输入时，内核自动发送 SIGTTIN，默认行为：暂停进程。
    // shell 在 tcsetpgrp 切换终端控制权期间可能短暂处于"后台"，此时如果读了终端，
    // 内核会给 shell 发 SIGTTIN。忽略它，防止 shell 自己被意外挂起。
    Signal::SIGTTIN,
    // 当后台进程组试图写终端或执行 tcsetpgrp 时，内核可能发送 SIGTTOU，默认行为：暂停进程。
    // 交互式 shell 需要反复调用 tcsetpgrp 来切换前台，忽略此信号保证切换不会被内核阻止。
    Signal::SIGTTOU,
];

/// shell 启动时调用：让 shell 自身忽略所有交互信号。
///
/// POSIX 调用 `sigaction(signum, &action)`：
///   将信号 `signum` 的处理方式设为 `action`。
///   这里把 5 个交互信号全部设为 `SigIgn`（忽略），
///   意味着内核产生这些信号时，直接丢弃，不做任何处理。
///
/// 注意：`sigaction` 不影响 exec 后的新进程镜像——exec 会把信号处理恢复为默认。
/// 所以这只影响 shell 自己的主循环，不会传染给用户运行的程序。
pub fn init_interactive_shell_signals() -> nix::Result<()> {
    // SigAction 的 3 个参数：
    //   SigHandler::SigIgn  — 忽略信号
    //   SaFlags::empty()    — 不加额外标志（不用 SA_RESTART 等）
    //   SigSet::empty()     — 信号处理函数执行期间不需要屏蔽其他信号
    let ignore = SigAction::new(SigHandler::SigIgn, SaFlags::empty(), SigSet::empty());

    unsafe {
        for signal in INTERACTIVE_SIGNALS {
            sigaction(signal, &ignore)?;
        }
    }

    Ok(())
}

/// 子进程 fork 之后、exec 之前调用：恢复交互信号的默认行为。
///
/// 为什么必须恢复？系统调用 `fork` 会完整复制父进程的信号处理表（sigaction 设置）。
/// 如果子进程在 exec 用户程序之前不做恢复：
///   - 用户的程序也会忽略 SIGINT → Ctrl-C 对用户程序无效
///   - 用户的程序也会忽略 SIGTSTP → Ctrl-Z 对用户程序无效
///
/// sigaction 设为 `SigDfl`（默认）后：
///   SIGINT/SIGQUIT → 终止进程
///   SIGTSTP         → 暂停进程
///   SIGTTIN/SIGTTOU → 暂停进程
///
/// 注意：execvp 成功后，内核会自动把信号恢复为默认行为（见 POSIX exec 语义）。
/// 但这里仍然显式恢复，原因有二：
///   1. execvp 之前，子进程本身也可能需要响应信号（虽然时间窗口很短）
///   2. 显式恢复是防御性编程，文档明确表达"这里信号处理发生了切换"
pub fn restore_child_interactive_signals() -> nix::Result<()> {
    // SigHandler::SigDfl 表示：恢复为内核为该信号定义的默认行为。
    let default = SigAction::new(SigHandler::SigDfl, SaFlags::empty(), SigSet::empty());

    unsafe {
        for signal in INTERACTIVE_SIGNALS {
            sigaction(signal, &default)?;
        }
    }

    Ok(())
}
