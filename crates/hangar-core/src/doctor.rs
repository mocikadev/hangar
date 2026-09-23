//! 离线自检：账号库/权限/逐账号凭据/官方 auth.json 一致性/config 路由/锁。
/// doctor 自检：纯离线检查，不做任何网络请求与写入
/// 返回 (展示行, 问题数)，经典循环直接打印，TUI 作覆盖层展示
/// `binary_version` 必须由 cli 层传入自身的 `env!("CARGO_PKG_VERSION")`
///（本模块在 core 求值会是 core 版本，见 updater 同款陷阱）
pub fn doctor_lines(binary_version: &str) -> Result<(Vec<String>, usize), String> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut out = vec![];
    let mut bad = 0;
    let ok = |out: &mut Vec<String>, m: String| out.push(format!("✅ {}", m));
    let warn = |out: &mut Vec<String>, m: String| out.push(format!("⚠ {}", m));
    let err = |out: &mut Vec<String>, m: String| out.push(format!("✗ {}", m));

    // 1. 账号库
    let acc_path = crate::account::accounts_file_path()?;
    let file = match crate::account::load_accounts() {
        Ok(f) => {
            ok(
                &mut out,
                format!(
                    "账号库可解析：{} 个账号（{}）",
                    f.accounts.len(),
                    acc_path.display()
                ),
            );
            if f.accounts.is_empty() {
                warn(&mut out, "账号库为空，按 a 添加".to_string());
            }
            if let Some(cur) = &f.current_account_id {
                if f.accounts.iter().any(|a| &a.id == cur) {
                    ok(&mut out, "当前账号指向有效".to_string());
                } else {
                    err(&mut out, "当前账号指向不存在的 id".to_string());
                    bad += 1;
                }
            }
            f
        }
        Err(e) => {
            err(&mut out, format!("账号库损坏且 .bak 无法恢复：{}", e));
            return Ok((out, 1));
        }
    };
    if acc_path.with_file_name("accounts.json.bak").exists() {
        ok(&mut out, "存在 .bak 备份".to_string());
    } else {
        warn(&mut out, "无 .bak 备份（首次写入后自动生成）".to_string());
    }

    // 2. 权限（unix）
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode =
            |p: &std::path::Path| std::fs::metadata(p).map(|m| m.permissions().mode() & 0o777);
        match mode(&acc_path) {
            Ok(0o600) => ok(&mut out, "accounts.json 权限 600".to_string()),
            Ok(m) => {
                warn(&mut out, format!("accounts.json 权限 {:o}（建议 600）", m));
            }
            Err(_) => warn(&mut out, "accounts.json 不存在（尚未创建）".to_string()),
        }
        if let Some(hangar_dir) = acc_path.parent() {
            match mode(hangar_dir) {
                Ok(0o700) => ok(&mut out, "账号库目录权限 700".to_string()),
                Ok(m) => warn(&mut out, format!("账号库目录权限 {:o}（建议 700）", m)),
                Err(_) => {}
            }
        }
    }

    // 3. 逐账号
    for acc in &file.accounts {
        let mut issues = vec![];
        if acc.access_token.trim().is_empty() {
            issues.push("缺 access_token");
        }
        if acc.refresh_token.trim().is_empty() {
            issues.push("缺 refresh_token（无法静默刷新）");
        }
        if acc
            .account_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .is_none()
        {
            issues.push("缺 account_id（配额查询可能不准）");
        }
        let health = crate::token_health::assess(
            &acc.access_token,
            &acc.refresh_token,
            acc.expires_at,
            acc.stale,
            now,
        );
        let exp = match health.access {
            crate::token_health::AccessTokenState::Missing => "AT 缺失".to_string(),
            crate::token_health::AccessTokenState::Expired => {
                "AT 已过期（切换前必须刷新）".to_string()
            }
            crate::token_health::AccessTokenState::Expiring => {
                "AT 即将过期（切换前必须刷新）".to_string()
            }
            crate::token_health::AccessTokenState::Fresh => format!(
                "AT 至 {}",
                crate::quota::fmt_ts_local(health.expires_at.unwrap_or_default())
            ),
            crate::token_health::AccessTokenState::Unknown => "AT 有效期未知".to_string(),
        };
        if acc.stale {
            err(
                &mut out,
                format!("{}：已失效，用「复活」重新登录", acc.email),
            );
            bad += 1;
        } else if issues.is_empty() {
            ok(&mut out, format!("{}：{}，{}", acc.email, exp, "凭据完整"));
        } else {
            warn(
                &mut out,
                format!("{}：{}，{}", acc.email, exp, issues.join("；")),
            );
        }
    }

    // 4. 官方 auth.json
    let home = crate::account::codex_home_path()?;
    let auth_path = home.join("auth.json");
    match std::fs::read_to_string(&auth_path) {
        Ok(s) => match serde_json::from_str::<serde_json::Value>(&s) {
            Ok(v) => {
                let has_at = v
                    .get("tokens")
                    .and_then(|t| t.get("access_token"))
                    .and_then(|x| x.as_str())
                    .map(|x| !x.trim().is_empty())
                    .unwrap_or(false);
                if !has_at {
                    err(&mut out, "官方 auth.json 无有效 access_token".to_string());
                    bad += 1;
                } else {
                    let email = v
                        .get("tokens")
                        .and_then(|t| t.get("id_token"))
                        .and_then(|x| x.as_str())
                        .and_then(crate::account::email_from_id_token)
                        .unwrap_or_else(|| "-".to_string());
                    let cur_email = file
                        .current_account_id
                        .as_deref()
                        .and_then(|id| file.accounts.iter().find(|a| a.id == id))
                        .map(|a| a.email.clone())
                        .unwrap_or_else(|| "-".to_string());
                    if email != "-" && crate::account::normalize_email(&email) == cur_email {
                        ok(
                            &mut out,
                            format!("官方 auth.json 与当前账号一致（{}）", email),
                        );
                    } else {
                        warn(
                            &mut out,
                            format!(
                                "官方 auth.json 归属 {}，库中当前 {}（下次操作将自动收敛）",
                                email, cur_email
                            ),
                        );
                    }
                }
            }
            Err(e) => {
                err(&mut out, format!("官方 auth.json 解析失败：{}", e));
                bad += 1;
            }
        },
        Err(_) => warn(
            &mut out,
            "官方 auth.json 不存在（尚未登录/切换）".to_string(),
        ),
    }

    // 5. config.toml 自定义路由
    let cfg = home.join("config.toml");
    if let Ok(s) = std::fs::read_to_string(&cfg) {
        let custom = s
            .lines()
            .map(str::trim_start)
            .filter(|l| !l.starts_with('#'))
            .any(|l| {
                (l.starts_with("model_provider") || l.starts_with("openai_base_url"))
                    && l.contains('=')
            });
        if custom {
            warn(
                &mut out,
                "config.toml 含自定义 provider 路由，OAuth 切换时将被清理".to_string(),
            );
        } else {
            ok(&mut out, "config.toml 无冲突路由".to_string());
        }
    }

    // 6. 残留锁
    if let Ok(p) = crate::account::accounts_lock_path() {
        if let Ok(m) = std::fs::metadata(&p).and_then(|m| m.modified()) {
            let age = SystemTime::now()
                .duration_since(m)
                .unwrap_or_default()
                .as_secs();
            if age > 120 {
                warn(&mut out, "发现超期残留锁（下次写库时自动清理）".to_string());
            } else {
                warn(&mut out, "锁文件存在（可能有另一实例在运行）".to_string());
            }
        }
    }

    if bad == 0 {
        ok(&mut out, "自检通过".to_string());
    } else {
        warn(&mut out, format!("发现 {} 项需处理", bad));
    }
    // 7. 自身版本与更新检查状态（只读，不联网）
    ok(&mut out, format!("版本 hangar {}", binary_version.trim()));
    match crate::updater::last_check_secs() {
        Some(ts) => ok(
            &mut out,
            format!("上次检查更新 {}", crate::quota::fmt_ts_local(ts as i64)),
        ),
        None => warn(
            &mut out,
            "尚未检查更新（启动满 24h 或手动 update 后记录）".to_string(),
        ),
    }
    Ok((out, bad))
}
