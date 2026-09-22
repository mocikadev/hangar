use hangar_core::account::{normalize_email, Account, AccountsFile};

pub fn resolve<'a>(file: &'a AccountsFile, selector: &str) -> Result<&'a Account, String> {
    let selector = selector.trim();
    if selector.is_empty() {
        return Err("账号选择器不能为空".to_string());
    }

    if let Some(account) = file.accounts.iter().find(|a| a.id == selector) {
        return Ok(account);
    }

    let email = normalize_email(selector);
    let matches: Vec<&Account> = file
        .accounts
        .iter()
        .filter(|a| normalize_email(&a.email) == email)
        .collect();
    match matches.as_slice() {
        [] => Err(format!("未找到账号: {selector}")),
        [account] => Ok(account),
        many => Err(format!(
            "邮箱 {selector} 对应 {} 个账号，请改用完整 ID：{}",
            many.len(),
            many.iter()
                .map(|a| a.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

pub fn current(file: &AccountsFile) -> Result<&Account, String> {
    let id = file
        .current_account_id
        .as_deref()
        .ok_or_else(|| "当前没有激活账号".to_string())?;
    file.accounts
        .iter()
        .find(|a| a.id == id)
        .ok_or_else(|| format!("当前账号指向不存在的 ID: {id}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(id: &str, email: &str) -> Account {
        Account {
            id: id.into(),
            email: email.into(),
            access_token: "secret-at".into(),
            refresh_token: "secret-rt".into(),
            id_token: "secret-id".into(),
            expires_at: 0,
            stale: false,
            account_id: None,
            organization_id: None,
        }
    }

    #[test]
    fn resolves_full_id_or_unique_normalized_email() {
        let file = AccountsFile {
            accounts: vec![account("a", "Alice@Example.com")],
            current_account_id: Some("a".into()),
        };
        assert_eq!(resolve(&file, "a").unwrap().id, "a");
        assert_eq!(resolve(&file, " alice@example.com ").unwrap().id, "a");
        assert_eq!(current(&file).unwrap().id, "a");
    }

    #[test]
    fn rejects_missing_and_ambiguous_email() {
        let file = AccountsFile {
            accounts: vec![
                account("a", "same@example.com"),
                account("b", "same@example.com"),
            ],
            current_account_id: None,
        };
        assert!(resolve(&file, "missing@example.com").is_err());
        let err = resolve(&file, "same@example.com").unwrap_err();
        assert!(err.contains("完整 ID"));
        assert!(current(&file).is_err());
    }
}
