//! 登录 + 入库（三元组去重），三个用户界面共用。
/// 仅添加：OAuth 登录并入库，不切换激活账号（current 与官方 auth.json 均保持原样）
/// 返回登录邮箱
pub fn do_login() -> Result<String, String> {
    do_login_with(&crate::oauth::StdinHooks)
}

/// 同上，交互经 hooks 注入（GUI 用弹窗版）；默认行为与 `do_login` 一致
pub fn do_login_with(hooks: &dyn crate::oauth::LoginHooks) -> Result<String, String> {
    let account = crate::oauth::login_codex_with(hooks)?;
    let email = account.email.clone();
    store_logged_in_account(account)?;
    Ok(email)
}

/// Persist a completed OAuth account without changing the active account.
/// Native frontends use this after their explicit OAuth session succeeds.
pub fn store_logged_in_account(account: crate::account::Account) -> Result<(), String> {
    crate::account::with_accounts_lock(|| {
        let mut file = crate::account::load_accounts()?;
        upsert_logged_in_account(&mut file, account.clone());
        // 仅添加不切换：current_account_id 与官方 auth.json 均保持不变，
        // 激活由用户显式切换（选中回车/点击切换）完成
        crate::account::save_accounts_unlocked(&file)?;
        Ok(())
    })
}

/// 把 OAuth 返回的账号收进库；只负责三元组匹配与凭据更新，不改变激活账号。
fn upsert_logged_in_account(
    file: &mut crate::account::AccountsFile,
    account: crate::account::Account,
) {
    // 去重：email + account/org 三元组（同邮箱多 org 不合并，老库无三元组时退化为 email）
    if let Some(existing) = file.accounts.iter_mut().find(|a| {
        if crate::account::normalize_email(&a.email) != account.email {
            return false;
        }
        if let (Some(x), Some(y)) = (a.account_id.as_deref(), account.account_id.as_deref()) {
            let (x, y) = (x.trim(), y.trim());
            if !x.is_empty() && !y.is_empty() && x != y {
                return false;
            }
        }
        if let (Some(x), Some(y)) = (
            a.organization_id.as_deref(),
            account.organization_id.as_deref(),
        ) {
            let (x, y) = (x.trim(), y.trim());
            if !x.is_empty() && !y.is_empty() && x != y {
                return false;
            }
        }
        true
    }) {
        existing.access_token = account.access_token;
        existing.refresh_token = account.refresh_token;
        existing.id_token = account.id_token;
        existing.expires_at = account.expires_at;
        existing.stale = false;
        if account.account_id.is_some() {
            existing.account_id = account.account_id;
        }
        if account.organization_id.is_some() {
            existing.organization_id = account.organization_id;
        }
    } else {
        file.accounts.push(account);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::{Account, AccountsFile};

    fn account(id: &str, email: &str, token: &str) -> Account {
        Account {
            id: id.into(),
            email: email.into(),
            access_token: format!("at-{token}"),
            refresh_token: format!("rt-{token}"),
            id_token: format!("id-{token}"),
            expires_at: 42,
            stale: false,
            account_id: Some("acct".into()),
            organization_id: Some("org".into()),
        }
    }

    #[test]
    fn adding_account_never_changes_current_account() {
        let mut file = AccountsFile {
            accounts: vec![account("active", "active@example.com", "old")],
            current_account_id: Some("active".into()),
        };

        upsert_logged_in_account(&mut file, account("new", "new@example.com", "new"));

        assert_eq!(file.current_account_id.as_deref(), Some("active"));
        assert_eq!(file.accounts.len(), 2);
    }

    #[test]
    fn adding_existing_identity_updates_in_place_without_switching() {
        let mut old = account("kept-id", "User@Example.com", "old");
        old.stale = true;
        let mut file = AccountsFile {
            accounts: vec![old],
            current_account_id: Some("another-id".into()),
        };
        let fresh = account("oauth-new-id", "user@example.com", "fresh");

        upsert_logged_in_account(&mut file, fresh);

        assert_eq!(file.accounts.len(), 1);
        assert_eq!(file.accounts[0].id, "kept-id");
        assert_eq!(file.accounts[0].access_token, "at-fresh");
        assert!(!file.accounts[0].stale);
        assert_eq!(file.current_account_id.as_deref(), Some("another-id"));
    }

    #[test]
    fn same_email_with_different_account_id_stays_distinct() {
        let mut file = AccountsFile {
            accounts: vec![account("one", "user@example.com", "one")],
            current_account_id: Some("one".into()),
        };
        let mut second = account("two", "user@example.com", "two");
        second.account_id = Some("different-account".into());

        upsert_logged_in_account(&mut file, second);

        assert_eq!(file.accounts.len(), 2);
        assert_eq!(file.current_account_id.as_deref(), Some("one"));
    }
}
