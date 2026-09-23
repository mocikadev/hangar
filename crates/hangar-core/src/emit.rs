//! 输出总线：业务层统一走 emit/emit_err 打印，线程级静默开关供 TUI 使用。
//!
//! - 经典模式 / 挂起登录流程：默认开启输出
//! - TUI 主循环与后台任务线程：各线程 set_quiet(true)，避免打花全屏
//! - ANSI 颜色包装在 cli 前端（crate ui.rs），本层只发纯文本 + 前端预格式化的行

use std::cell::Cell;

thread_local! {
    static QUIET: Cell<bool> = const { Cell::new(false) };
}

pub fn set_quiet(v: bool) {
    QUIET.with(|q| q.set(v));
}

fn is_quiet() -> bool {
    QUIET.with(|q| q.get())
}

/// 可静默的普通输出（替代各业务模块的 println!）
pub fn emit(s: String) {
    if !is_quiet() {
        println!("{}", s);
    }
}

/// 可静默的告警输出（替代各业务模块的 eprintln!）
pub fn emit_err(s: String) {
    if !is_quiet() {
        eprintln!("{}", s);
    }
}
