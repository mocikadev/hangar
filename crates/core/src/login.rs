//! 登录 + 入库（三元组去重）+ 切换的组合动作，两个前端共用。
/// OAuth 登录并入库 + 切换（经典循环与 TUI 共用；调用方负责浏览器交互的屏幕形态）
/// 返回登录邮箱
pub fn do_login() -> Result<String, String> {
    do_login_with(&crate::oauth::StdinHooks)
}

/// 同上，交互经 hooks 注入（GUI 用弹窗版）；默认行为与 `do_login` 一致
pub fn do_login_with(hooks: &dyn crate::oauth::LoginHooks) -> Result<String, String> {
    let account = crate::oauth::login_codex_with(hooks)?;
    let email = account.email.clone();
    let current_id = crate::account::with_accounts_lock(|| {
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
            file.current_account_id = Some(existing.id.clone());
        } else {
            file.current_account_id = Some(account.id.clone());
            file.accounts.push(account.clone());
        }
        let id = file
            .current_account_id
            .clone()
            .ok_or_else(|| "登录入库后缺失 current_account_id（内部不一致）".to_string())?;
        crate::account::save_accounts_unlocked(&file)?;
        Ok(id)
    })?;
    // 登录即切换到新账号
    crate::account::switch_account(&current_id)?;
    Ok(email)
}
