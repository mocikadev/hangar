//! 登录 + 入库（三元组去重），两个前端共用。
/// 仅添加：OAuth 登录并入库，不切换激活账号（current 与官方 auth.json 均保持原样）
/// 返回登录邮箱
pub fn do_login() -> Result<String, String> {
    do_login_with(&crate::oauth::StdinHooks)
}

/// 同上，交互经 hooks 注入（GUI 用弹窗版）；默认行为与 `do_login` 一致
pub fn do_login_with(hooks: &dyn crate::oauth::LoginHooks) -> Result<String, String> {
    let account = crate::oauth::login_codex_with(hooks)?;
    let email = account.email.clone();
    crate::account::with_accounts_lock(|| {
        let mut file = crate::account::load_accounts()?;
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
            existing.access_token = account.access_token.clone();
            existing.refresh_token = account.refresh_token.clone();
            existing.id_token = account.id_token.clone();
            existing.expires_at = account.expires_at;
            existing.stale = false;
            if account.account_id.is_some() {
                existing.account_id = account.account_id.clone();
            }
            if account.organization_id.is_some() {
                existing.organization_id = account.organization_id.clone();
            }
        } else {
            file.accounts.push(account.clone());
        }
        // 仅添加不切换：current_account_id 与官方 auth.json 均保持不变，
        // 激活由用户显式切换（选中回车/点击切换）完成
        crate::account::save_accounts_unlocked(&file)?;
        Ok(())
    })?;
    Ok(email)
}
