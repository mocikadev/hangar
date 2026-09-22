//! 经典菜单模式（非 TTY / --classic 回退）：stdin 编号交互。

use crate::ui;
use hangar_core as core;
use std::collections::HashMap;
use std::io::{self, Write};

pub fn show_menu_and_handle() -> Result<(), String> {
    let file = core::account::load_accounts()?;
    let current = file.current_account_id.as_deref();

    if file.accounts.is_empty() {
        ui::empty_state();
    } else {
        ui::section("账号");
        for (i, acc) in file.accounts.iter().enumerate() {
            ui::account_line(
                i + 1,
                &acc.email,
                Some(acc.id.as_str()) == current,
                acc.stale,
            );
        }
    }

    ui::help_bar();

    let mut input = String::new();
    // EOF（管道输入耗尽/重定向读完）时退出，避免空输入无限循环空转
    let n = io::stdin()
        .read_line(&mut input)
        .map_err(|e| format!("读取输入失败: {}", e))?;
    if n == 0 {
        std::process::exit(0);
    }
    let input = input.trim();

    match input {
        "q" | "quit" | "exit" => std::process::exit(0),
        "a" | "add" => {
            println!("  {}", ui::action("正在启动 OAuth 登录（将打开浏览器）..."));
            let email = core::do_login()?;
            println!(
                "  {}",
                ui::success(&format!("已添加账号: {}（未激活，输入编号切换）", email))
            );
            warn_if_codex_running();
        }
        "r" | "reauth" => {
            reauth_interactive()?;
        }
        "u" | "usage" => {
            quota_interactive()?;
        }
        "doctor" | "check" => {
            doctor()?;
        }
        "update" => {
            update_interactive()?;
        }
        "d" | "delete" => {
            delete_interactive()?;
        }
        n if n.chars().all(|c| c.is_ascii_digit()) && !n.is_empty() => {
            let idx: usize = n.parse().map_err(|_| "无效编号".to_string())?;
            let file = core::account::load_accounts()?;
            let account = file
                .accounts
                .get(idx.wrapping_sub(1))
                .ok_or_else(|| format!("编号 {} 不存在", idx))?;
            if account.stale {
                println!(
                    "  {}",
                    ui::warn(&format!(
                        "账号 {} 的凭据已失效，按 r 可定向重新登录复活（不会建重复项）",
                        account.email
                    ))
                );
                return Ok(());
            }
            if Some(account.id.as_str()) == file.current_account_id.as_deref() {
                // 已是使用中账号：重写 auth.json 纯多余且可能覆盖 Codex 刚轮换的 token
                println!(
                    "  {}",
                    ui::info(&format!(
                        "账号 {} 已是使用中的账号，无需切换",
                        account.email
                    ))
                );
                return Ok(());
            }
            let email = account.email.clone();
            let id = account.id.clone();
            core::account::switch_account(&id)?;
            println!("  {}", ui::success(&format!("已切换到: {}", email)));
            warn_if_codex_running();
        }
        "" => {}
        other => println!("  {} {}", ui::warn("未知命令"), other),
    }
    Ok(())
}

fn quota_interactive() -> Result<(), String> {
    let file = core::account::load_accounts()?;
    if file.accounts.is_empty() {
        println!("  {}", ui::info("没有账号可查"));
        return Ok(());
    }
    ui::section("配额");
    let mut quotas = HashMap::new();
    for acc in &file.accounts {
        if acc.stale {
            println!(
                "  {} {}",
                ui::dim(&acc.email),
                ui::yellow("⚠ 已失效，跳过（按 r 复活）")
            );
            continue;
        }
        print!("  {} 查询中...", ui::dim(&acc.email));
        use std::io::Write as _;
        let _ = io::stdout().flush();
        match core::quota::fetch_quota_for_account(&acc.id) {
            Ok((_, q)) => {
                use std::time::{SystemTime, UNIX_EPOCH};
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;
                let plan = q.plan.as_deref().unwrap_or("-");
                println!("\r  {} [{}]", acc.email, plan);
                if let Some(w) = q.windows.iter().find(|w| w.label.starts_with('周')) {
                    println!(
                        "    周剩余 {} {:>4} · 重置于 {}",
                        core::quota::quota_bar(w.window.remaining),
                        w.window
                            .remaining
                            .map(|p| format!("{}%", p))
                            .unwrap_or_else(|| "未知".to_string()),
                        core::quota::reset_at_ts(&w.window, now)
                            .map(core::quota::fmt_ts_local)
                            .unwrap_or_else(|| "未知".to_string()),
                    );
                } else {
                    println!("    周剩余 未知");
                }
                quotas.insert(acc.id.clone(), q);
            }
            Err(e) => println!("\r  {} {}", acc.email, ui::warn(&e)),
        }
    }
    println!(
        "  {}",
        ui::dim("剩余%=100-used；接口为非公开契约，失败属正常波动")
    );
    match crate::recommendation::recommend(
        &file.accounts,
        file.current_account_id.as_deref(),
        &quotas,
    ) {
        Some(recommendation)
            if recommendation.reason == crate::recommendation::Reason::KeepCurrent =>
        {
            println!(
                "  {}",
                ui::success(&format!(
                    "建议继续使用 {}（周剩余 {}%）",
                    recommendation.email, recommendation.weekly_remaining
                ))
            );
        }
        Some(recommendation) => println!(
            "  {}",
            ui::info(&format!(
                "建议使用 {}（周剩余 {}%）",
                recommendation.email, recommendation.weekly_remaining
            ))
        ),
        None => println!("  {}", ui::dim("暂无可用建议")),
    }
    Ok(())
}

/// S9：切换只覆盖了 auth.json 文件，正在运行的 Codex 进程内存中仍是旧凭据，
/// 需重启才生效。检测到 codex 进程时给出提示。
fn warn_if_codex_running() {
    if core::codex_process_running() {
        println!(
            "  {}",
            ui::info("检测到 Codex 正在运行，请重启 Codex 以使新凭据生效")
        );
    }
}
fn doctor() -> Result<(), String> {
    ui::section("自检");
    let (lines, _) = core::doctor::doctor_lines(env!("CARGO_PKG_VERSION"))?;
    for l in lines {
        println!("  {}", l);
    }
    Ok(())
}

fn update_interactive() -> Result<(), String> {
    println!("  {}", ui::action("正在检查更新..."));
    match core::updater::check_update(true, env!("CARGO_PKG_VERSION"), env!("CARGO_PKG_NAME")) {
        Ok(Some(info)) => match core::updater::apply_update(&info) {
            Ok(v) => println!(
                "  {}",
                ui::success(&format!("已升级到 {}，请重启 hangar 生效", v))
            ),
            Err(e) => println!(
                "  {}",
                ui::warn(&format!("更新失败：{}（旧版继续可用）", e))
            ),
        },
        Ok(None) => println!("  {}", ui::success("已是最新版本")),
        Err(e) => println!("  {}", ui::warn(&e)),
    }
    Ok(())
}

fn delete_interactive() -> Result<(), String> {
    let file = core::account::load_accounts()?;
    if file.accounts.is_empty() {
        println!("  {}", ui::info("没有可删除的账号"));
        return Ok(());
    }
    ui::section("要删除哪个账号？");
    for (i, acc) in file.accounts.iter().enumerate() {
        let is_current = file.current_account_id.as_deref() == Some(acc.id.as_str());
        let mut badge = String::new();
        if is_current {
            badge.push_str(&format!(" {}", ui::green("● 使用中")));
        }
        if acc.stale {
            badge.push_str(&format!(" {}", ui::yellow("⚠ 需重新登录")));
        }
        println!(
            "  {} {}{}",
            ui::dim(&format!("{:>2}.", i + 1)),
            acc.email,
            badge
        );
    }
    print!("  {} ", ui::cyan_bold("›"));
    let _ = io::stdout().flush();
    println!("{}", ui::dim("输入编号，回车取消"));

    let mut input = String::new();
    let n = io::stdin()
        .read_line(&mut input)
        .map_err(|e| format!("读取输入失败: {}", e))?;
    if n == 0 {
        println!("  {}", ui::dim("（输入结束，取消删除）"));
        return Ok(());
    }
    let input = input.trim();
    if input.is_empty() {
        return Ok(());
    }
    let idx: usize = input.parse().map_err(|_| "无效编号".to_string())?;
    let account = file
        .accounts
        .get(idx.wrapping_sub(1))
        .ok_or_else(|| format!("编号 {} 不存在", idx))?;
    // 使用中账号直接拦截，不进确认环节（底层 delete_account 也有同款守卫）
    if Some(account.id.as_str()) == file.current_account_id.as_deref() {
        println!(
            "  {}",
            ui::warn(&format!(
                "账号 {} 正在使用中，无法删除；请先切换到其他账号",
                account.email
            ))
        );
        return Ok(());
    }

    print!("  确认删除 {}? (y/N): ", ui::yellow(&account.email));
    // 管道关闭（EPIPE）时不 panic，与 EOF 处理保持一致直接走取消路径
    let _ = io::stdout().flush();
    let mut confirm = String::new();
    let n = io::stdin()
        .read_line(&mut confirm)
        .map_err(|e| format!("读取输入失败: {}", e))?;
    if n == 0 {
        println!("  {}", ui::dim("（输入结束，取消删除）"));
        return Ok(());
    }
    if !confirm.trim().eq_ignore_ascii_case("y") {
        println!("  {}", ui::dim("已取消"));
        return Ok(());
    }

    let id = account.id.clone();
    core::account::delete_account(&id)?;
    println!("  {}", ui::success("已删除"));
    Ok(())
}

fn reauth_interactive() -> Result<(), String> {
    let file = core::account::load_accounts()?;
    let stale_list: Vec<(usize, String, String)> = file
        .accounts
        .iter()
        .enumerate()
        .filter(|(_, a)| a.stale)
        .map(|(i, a)| (i + 1, a.id.clone(), a.email.clone()))
        .collect();
    if stale_list.is_empty() {
        println!("  {}", ui::info("没有需复活的账号（无 ⚠ 标记）"));
        return Ok(());
    }
    ui::section("复活哪个账号？");
    for (n, _, email) in &stale_list {
        println!("  {} {}", ui::dim(&format!("{:>2}.", n)), email);
    }
    print!("  {} ", ui::cyan_bold("›"));
    let _ = io::stdout().flush();
    println!("{}", ui::dim("输入编号，回车取消"));
    let mut input = String::new();
    if io::stdin().read_line(&mut input).unwrap_or(0) == 0 {
        return Ok(());
    }
    let input = input.trim();
    if input.is_empty() {
        return Ok(());
    }
    let idx: usize = input.parse().map_err(|_| "无效编号".to_string())?;
    let (_, id, email) = stale_list
        .iter()
        .find(|(n, _, _)| *n == idx)
        .ok_or_else(|| format!("编号 {} 不是待复活账号", idx))?;
    println!(
        "  {}",
        ui::action(&format!("正在为 {} 重新登录（将打开浏览器）...", email))
    );
    let fresh = core::oauth::login_codex()?;
    core::account::reauth_account(id, &fresh)?;
    println!(
        "  {}",
        ui::success(&format!("已复活并切换到: {}", fresh.email))
    );
    warn_if_codex_running();
    Ok(())
}
