//! 终端样式模块：ANSI 颜色 / 图标 / 分隔线
//!
//! 降级规则：
//! - 设置了 `NO_COLOR` 环境变量 → 所有样式输出纯文本
//! - stdout 不是 TTY（管道/重定向）→ 同上，避免转义码污染日志文件
//! - 其余场景启用 ANSI 颜色（16 色基础码，几乎所有终端都支持）

use std::io::IsTerminal;
use std::sync::OnceLock;

static COLOR_ENABLED: OnceLock<bool> = OnceLock::new();

// 静默开关与输出总线在 core::emit（业务层共用）；本模块提供 ANSI 着色包装
pub use hangar_core::emit::set_quiet;

fn color_enabled() -> bool {
    *COLOR_ENABLED
        .get_or_init(|| std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal())
}

/// 包裹 ANSI SGR 代码；未启用颜色时原样返回
fn paint(code: &str, text: &str) -> String {
    if color_enabled() {
        format!("\x1b[{}m{}\x1b[0m", code, text)
    } else {
        text.to_string()
    }
}

// ---- 语义色 ----

pub fn bold(t: &str) -> String {
    paint("1", t)
}
pub fn dim(t: &str) -> String {
    paint("2", t)
}
pub fn green(t: &str) -> String {
    paint("32", t)
}
pub fn yellow(t: &str) -> String {
    paint("33", t)
}
pub fn red(t: &str) -> String {
    paint("31", t)
}
pub fn cyan(t: &str) -> String {
    paint("36", t)
}
pub fn cyan_bold(t: &str) -> String {
    paint("1;36", t)
}

// ---- 语义标记 ----

/// 成功消息 ✅ 前缀绿色
pub fn success(msg: &str) -> String {
    format!("{} {}", green("✅"), msg)
}
/// 警告消息 ⚠ 前缀黄色
pub fn warn(msg: &str) -> String {
    format!("{} {}", yellow("⚠"), msg)
}
/// 错误消息 ✗ 前缀红色
pub fn error(msg: &str) -> String {
    format!("{} {}", red("✗"), msg)
}
/// 提示消息 ℹ 前缀青色
pub fn info(msg: &str) -> String {
    format!("{} {}", cyan("ℹ"), msg)
}
/// 进行中消息（刷新/收编等）——青色
pub fn action(msg: &str) -> String {
    cyan(msg)
}

// ---- 布局 ----

/// 标题横幅：╭─ 品牌名 ─╮ 风格
pub fn banner() {
    println!();
    println!("{}", cyan_bold("  ◆ Hangar · Codex 多账号管理"));
    println!("{}", dim("  ─────────────────────────────"));
}

/// 账号列表分区标题
pub fn section(title: &str) {
    println!();
    println!("  {}", bold(title));
}

/// 打印一行账号条目：序号、email、当前/stale 徽标
pub fn account_line(idx: usize, email: &str, is_current: bool, stale: bool) {
    let num = dim(&format!("{:>2}.", idx));
    let mail = if stale {
        yellow(email)
    } else {
        email.to_string()
    };
    let badges = {
        let mut s = String::new();
        if is_current {
            s.push_str(&format!(" {}", green("● 使用中")));
        }
        if stale {
            s.push_str(&format!(" {}", yellow("⚠ 需重新登录")));
        }
        s
    };
    println!("  {} {}{}", num, mail, badges);
}

/// 空状态提示
pub fn empty_state() {
    println!();
    println!("  {} 还没有账号，按 {} 添加", dim("·"), bold("a"));
}

/// 命令提示行（帮助条）
pub fn help_bar() {
    println!();
    println!(
        "  {}  {} 切换   {} 添加   {} 复活   {} 配额   {} 自检   {} 删除   {} 更新   {} 退出",
        dim("命令:"),
        bold("[编号]"),
        bold("a"),
        bold("r"),
        bold("u"),
        bold("doctor"),
        bold("d"),
        bold("update"),
        bold("q")
    );
    print!("  {} ", cyan_bold("›"));
    use std::io::Write;
    let _ = std::io::stdout().flush();
}
