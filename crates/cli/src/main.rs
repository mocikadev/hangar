mod classic;
mod tui;
mod ui;

use hangar_core as core;

#[derive(Debug, Default)]
struct Flags {
    classic: bool,
    show_version: bool,
    no_update: bool,
    check_update: bool,
}

fn parse_flags(args: &[String]) -> Flags {
    let mut f = Flags::default();
    for a in args.iter().skip(1) {
        match a.as_str() {
            "--classic" => f.classic = true,
            "--version" | "-V" => f.show_version = true,
            "--no-update" => f.no_update = true,
            "--check-update" => f.check_update = true,
            _ => {}
        }
    }
    if !std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        f.classic = true;
    }
    f
}

fn main() {
    let flags = parse_flags(&std::env::args().collect::<Vec<_>>());
    if flags.show_version {
        println!("hangar {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    // Windows 残留 hangar.old 清理（最佳努力，永不报错）
    core::updater::cleanup_pending_old();
    if flags.check_update {
        match core::updater::check_update(true, env!("CARGO_PKG_VERSION")) {
            Ok(Some(info)) => match core::updater::apply_update(&info) {
                Ok(v) => {
                    println!("已升级到 {}", v);
                    std::process::exit(0);
                }
                Err(e) => {
                    eprintln!("更新失败：{}（旧版继续可用）", e);
                    std::process::exit(1);
                }
            },
            Ok(None) => {
                println!("已是最新版本");
                std::process::exit(0);
            }
            Err(e) => {
                eprintln!("检查更新失败：{}", e);
                std::process::exit(1);
            }
        }
    }
    let skip_update = flags.no_update
        || std::env::var("HANGAR_NO_UPDATE")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
    if !skip_update {
        match core::updater::check_update(false, env!("CARGO_PKG_VERSION")) {
            Ok(Some(info)) => match core::updater::apply_update(&info) {
                Ok(v) => {
                    println!("已升级到 {}，请重新运行 hangar 生效", v);
                    std::process::exit(0);
                }
                Err(e) => eprintln!("自动更新失败：{}（继续使用旧版）", e),
            },
            Ok(None) => {}
            Err(e) => eprintln!("检查更新失败：{}（继续启动）", e),
        }
    }
    if flags.classic {
        classic_loop();
    } else if let Err(e) = tui::run() {
        eprintln!("TUI 启动失败（{}），回退经典模式", e);
        classic_loop();
    }
}

fn classic_loop() {
    ui::banner();
    loop {
        // 每次交互前重新收敛：本进程是常驻菜单循环（非一次性命令），
        // 若只在启动 harvest 一次，长会话期间 Codex 轮换 RT 后再切换会用旧 RT
        // 刷新 → 误标 stale。无变化时 harvest 静默，开销仅两次文件读
        core::account::harvest();
        if let Err(e) = classic::show_menu_and_handle() {
            eprintln!("  {}", ui::error(&e));
        }
        println!();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_parse() {
        assert!(parse_flags(&["hangar".into(), "--version".into()]).show_version);
        assert!(parse_flags(&["hangar".into(), "--no-update".into()]).no_update);
        assert!(parse_flags(&["hangar".into(), "--check-update".into()]).check_update);
        assert!(parse_flags(&["hangar".into(), "--classic".into()]).classic);
    }
}
