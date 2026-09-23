use crate::args::Command;
use crate::{output, selector};
use hangar_core as core;
use hangar_core::account::{Account, AccountError, AccountErrorKind, AccountsFile};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Internal = 1,
    Usage = 2,
    State = 3,
    Auth = 4,
    External = 5,
}

#[derive(Debug)]
pub struct CommandError {
    pub kind: ErrorKind,
    pub message: String,
}

impl CommandError {
    fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Internal, message)
    }

    fn state(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::State, message)
    }

    fn auth(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Auth, message)
    }

    fn external(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::External, message)
    }

    fn from_account(error: AccountError) -> Self {
        let kind = match error.kind() {
            AccountErrorKind::State => ErrorKind::State,
            AccountErrorKind::Auth => ErrorKind::Auth,
            AccountErrorKind::External => ErrorKind::External,
            AccountErrorKind::Internal => ErrorKind::Internal,
        };
        Self::new(kind, error.to_string())
    }
}

fn load_after_harvest() -> Result<AccountsFile, CommandError> {
    core::account::harvest_checked().map_err(CommandError::internal)?;
    core::account::load_accounts().map_err(CommandError::internal)
}

fn selected(file: &AccountsFile, value: &str) -> Result<Account, CommandError> {
    selector::resolve(file, value)
        .cloned()
        .map_err(CommandError::state)
}

/// 先在当前账号库中解析 selector，再执行可能写库的 harvest。
/// 这样不存在或歧义 selector 会在任何写副作用之前失败。
fn selected_after_harvest(value: &str) -> Result<(AccountsFile, Account), CommandError> {
    let before = core::account::load_accounts().map_err(CommandError::internal)?;
    let target_id = selected(&before, value)?.id;
    core::account::harvest_checked().map_err(CommandError::internal)?;
    let after = core::account::load_accounts().map_err(CommandError::internal)?;
    let target = after
        .accounts
        .iter()
        .find(|account| account.id == target_id)
        .cloned()
        .ok_or_else(|| CommandError::state(format!("账号已不存在: {target_id}")))?;
    Ok((after, target))
}

pub fn execute(command: Command, json: bool) -> Result<(), CommandError> {
    core::emit::set_quiet(true);
    match command {
        Command::List => {
            let file = load_after_harvest()?;
            output::accounts(&file, json);
        }
        Command::Current => {
            let file = load_after_harvest()?;
            let account = selector::current(&file).map_err(CommandError::state)?;
            output::current(Some(account), json);
        }
        Command::Switch { selector: value } => {
            let (file, account) = selected_after_harvest(&value)?;
            if file.current_account_id.as_deref() != Some(account.id.as_str()) {
                core::account::switch_account_checked(&account.id)
                    .map_err(CommandError::from_account)?;
            }
            output::success("switch", &format!("已切换到: {}", account.email), json);
        }
        Command::Login => {
            core::emit::set_quiet(false);
            let email = core::do_login().map_err(CommandError::external)?;
            core::emit::set_quiet(true);
            output::success("login", &format!("已添加账号: {email}（未激活）"), json);
        }
        Command::Reauth { selector: value } => {
            let (_, target) = selected_after_harvest(&value)?;
            if !target.stale {
                return Err(CommandError::state(format!(
                    "账号 {} 未标记为失效，无需复活",
                    target.email
                )));
            }
            core::emit::set_quiet(false);
            let fresh = core::oauth::login_codex().map_err(CommandError::external)?;
            core::emit::set_quiet(true);
            core::account::reauth_account_checked(&target.id, &fresh)
                .map_err(CommandError::from_account)?;
            output::success(
                "reauth",
                &format!("已重新登录: {}（当前账号未改变）", target.email),
                json,
            );
        }
        Command::Remove {
            selector: value,
            yes,
        } => {
            if !yes {
                return Err(CommandError::new(
                    ErrorKind::Usage,
                    "非交互删除必须显式传入 --yes",
                ));
            }
            let (_, account) = selected_after_harvest(&value)?;
            core::account::delete_account_checked(&account.id)
                .map_err(CommandError::from_account)?;
            output::success("remove", &format!("已删除: {}", account.email), json);
        }
        Command::Quota {
            selector: value,
            all,
        } => {
            let (file, selected_account) = if let Some(value) = value.as_deref() {
                let (file, account) = selected_after_harvest(value)?;
                (file, Some(account))
            } else {
                (load_after_harvest()?, None)
            };
            if let Some(account) = selected_account.as_ref().filter(|account| account.stale) {
                return Err(CommandError::auth(format!(
                    "账号 {} 已失效，需要重新登录",
                    account.email
                )));
            }
            let accounts: Vec<Account> = if all {
                file.accounts.iter().filter(|a| !a.stale).cloned().collect()
            } else if let Some(account) = selected_account {
                vec![account]
            } else {
                vec![selector::current(&file)
                    .cloned()
                    .map_err(CommandError::state)?]
            };
            if accounts.is_empty() {
                return Err(CommandError::state("没有可查询配额的正常账号"));
            }
            let rows: Vec<(Account, Result<core::quota::Quota, String>)> = accounts
                .into_iter()
                .map(|account| {
                    let result = if account.stale {
                        Err("账号已失效，需要重新登录".to_string())
                    } else {
                        core::quota::fetch_quota_for_account(&account.id).map(|(_, quota)| quota)
                    };
                    (account, result)
                })
                .collect();
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            let failed = rows.iter().any(|(_, result)| result.is_err());
            output::quotas(&rows, json, now);
            if failed {
                return Err(CommandError::external("一个或多个账号配额查询失败"));
            }
        }
        Command::Doctor => {
            let (lines, issues) = core::doctor::doctor_lines(env!("CARGO_PKG_VERSION"))
                .map_err(CommandError::internal)?;
            if json {
                println!(
                    "{}",
                    serde_json::json!({ "issues": issues, "lines": lines })
                );
            } else {
                for line in lines {
                    println!("{line}");
                }
            }
            if issues > 0 {
                return Err(CommandError::state(format!("自检发现 {issues} 个问题")));
            }
        }
        Command::Harvest => {
            let report = core::account::harvest_checked().map_err(CommandError::internal)?;
            output::success(
                "harvest",
                &format!(
                    "已收敛凭据，共 {} 个账号{}",
                    report.account_count,
                    if report.changed {
                        "（有更新）"
                    } else {
                        ""
                    }
                ),
                json,
            );
        }
        Command::Update => {
            let result = core::updater::check_update(true, env!("CARGO_PKG_VERSION"))
                .map_err(CommandError::external)?;
            match result {
                Some(info) => {
                    let version =
                        core::updater::apply_update(&info).map_err(CommandError::external)?;
                    output::success("update", &format!("已升级到 {version}"), json);
                }
                None => output::success("update", "已是最新版本", json),
            }
        }
        Command::Tui | Command::Classic => {
            return Err(CommandError::new(
                ErrorKind::Usage,
                "交互界面命令应由入口分发",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_error_kinds_map_without_parsing_messages() {
        let map = |kind| match kind {
            AccountErrorKind::State => ErrorKind::State,
            AccountErrorKind::Auth => ErrorKind::Auth,
            AccountErrorKind::External => ErrorKind::External,
            AccountErrorKind::Internal => ErrorKind::Internal,
        };
        assert_eq!(map(AccountErrorKind::Auth), ErrorKind::Auth);
        assert_eq!(map(AccountErrorKind::External), ErrorKind::External);
    }
}
