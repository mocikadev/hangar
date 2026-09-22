use hangar_core::account::{Account, AccountsFile};
use hangar_core::quota::{reset_at_ts, Quota};
use serde_json::{json, Value};

fn safe_account(account: &Account, current: bool) -> Value {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let health = hangar_core::token_health::assess(
        &account.access_token,
        &account.refresh_token,
        account.expires_at,
        account.stale,
        now,
    );
    json!({
        "id": account.id,
        "email": account.email,
        "current": current,
        "stale": account.stale,
        "account_id": account.account_id,
        "organization_id": account.organization_id,
        "expires_at": health.expires_at,
        "token_health": {
            "status": health.label(),
            "refresh_available": health.has_refresh_token,
            "expiry_source": health.expiry_source.map(|source| match source {
                hangar_core::token_health::ExpirySource::Jwt => "jwt",
                hangar_core::token_health::ExpirySource::Ledger => "ledger",
            }),
        },
    })
}

pub fn accounts(file: &AccountsFile, json_output: bool) {
    if json_output {
        let rows: Vec<Value> = file
            .accounts
            .iter()
            .map(|a| safe_account(a, file.current_account_id.as_deref() == Some(a.id.as_str())))
            .collect();
        println!("{}", json!({ "schema_version": 1, "accounts": rows }));
        return;
    }
    if file.accounts.is_empty() {
        println!("没有账号");
        return;
    }
    for account in &file.accounts {
        let current = file.current_account_id.as_deref() == Some(account.id.as_str());
        println!(
            "{}\t{}\t{}{}",
            account.id,
            account.email,
            if current { "current" } else { "inactive" },
            if account.stale { "\tstale" } else { "" }
        );
    }
}

pub fn current(account: Option<&Account>, json_output: bool) {
    if json_output {
        println!(
            "{}",
            json!({ "schema_version": 1, "current": account.map(|a| safe_account(a, true)) })
        );
    } else if let Some(account) = account {
        println!("{}\t{}", account.id, account.email);
    } else {
        println!("当前没有激活账号");
    }
}

pub fn success(action: &str, message: &str, json_output: bool) {
    if json_output {
        println!(
            "{}",
            json!({ "ok": true, "action": action, "message": message })
        );
    } else {
        println!("{message}");
    }
}

fn quota_value(account: &Account, quota: &Quota, now: i64) -> Value {
    let windows: Vec<Value> = quota
        .windows
        .iter()
        .map(|w| {
            json!({
                "label": w.label,
                "remaining_percent": w.window.remaining,
                "limit_seconds": w.window.limit_secs,
                "reset_at": reset_at_ts(&w.window, now),
            })
        })
        .collect();
    let (reset_available, reset_next_expiry) = match &quota.reset_detail {
        Some(detail) => (Some(detail.available), detail.next_expiry()),
        None => (quota.reset_available, None),
    };
    json!({
        "account": safe_account(account, false),
        "plan": quota.plan,
        "windows": windows,
        "reset_credits": {
            "available": reset_available,
            "next_expiry": reset_next_expiry,
        }
    })
}

pub fn quotas(rows: &[(Account, Result<Quota, String>)], json_output: bool, now: i64) {
    if json_output {
        let values: Vec<Value> = rows
            .iter()
            .map(|(account, result)| match result {
                Ok(quota) => json!({ "ok": true, "data": quota_value(account, quota, now) }),
                Err(error) => json!({
                    "ok": false,
                    "account": safe_account(account, false),
                    "error": error,
                }),
            })
            .collect();
        println!("{}", json!({ "quotas": values }));
        return;
    }

    for (account, result) in rows {
        match result {
            Ok(quota) => {
                println!(
                    "{} [{}]",
                    account.email,
                    quota.plan.as_deref().unwrap_or("-")
                );
                if quota.windows.is_empty() {
                    println!("  无可用窗口");
                }
                for window in &quota.windows {
                    let remaining = window
                        .window
                        .remaining
                        .map(|v| format!("{v}%"))
                        .unwrap_or_else(|| "未知".to_string());
                    let reset = reset_at_ts(&window.window, now)
                        .map(hangar_core::quota::fmt_ts_local)
                        .unwrap_or_else(|| "未知".to_string());
                    println!("  {} 剩余 {} 重置于 {}", window.label, remaining, reset);
                }
            }
            Err(error) => println!("{}\t失败：{}", account.email, error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_account_never_serializes_credentials() {
        let account = Account {
            id: "id".into(),
            email: "a@example.com".into(),
            access_token: "access-secret".into(),
            refresh_token: "refresh-secret".into(),
            id_token: "id-secret".into(),
            expires_at: 123,
            stale: false,
            account_id: Some("acct".into()),
            organization_id: None,
        };
        let text = safe_account(&account, true).to_string();
        assert!(!text.contains("access-secret"));
        assert!(!text.contains("refresh-secret"));
        assert!(!text.contains("id-secret"));
        assert!(!text.contains("refresh_token"));
        assert!(!text.contains("id_token"));
    }
}
