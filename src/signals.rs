//! 交互式 shell 的信号策略：
//! - shell 自己忽略终端交互信号；
//! - fork 出来的子进程在 exec 前恢复默认行为。

use nix::sys::signal::{SaFlags, SigAction, SigHandler, SigSet, Signal, sigaction};

const INTERACTIVE_SIGNALS: [Signal; 5] = [
    Signal::SIGINT,
    Signal::SIGQUIT,
    Signal::SIGTSTP,
    Signal::SIGTTIN,
    Signal::SIGTTOU,
];

/// 让 shell 自己忽略终端发来的交互信号。
pub fn init_interactive_shell_signals() -> nix::Result<()> {
    set_interactive_signal_handler(SigHandler::SigIgn)
}

/// 在子进程 exec 前恢复交互信号的默认行为。
pub fn restore_child_interactive_signals() -> nix::Result<()> {
    set_interactive_signal_handler(SigHandler::SigDfl)
}

/// 对交互信号集批量安装同一种处理方式。
fn set_interactive_signal_handler(handler: SigHandler) -> nix::Result<()> {
    let action = SigAction::new(handler, SaFlags::empty(), SigSet::empty());
    unsafe {
        for signal in INTERACTIVE_SIGNALS {
            sigaction(signal, &action)?;
        }
    }

    Ok(())
}
