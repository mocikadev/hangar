//! 基于本次会话周剩余额度的纯推荐规则。
//!
//! 本模块不联网、不刷新 token、不写文件，也不执行账号切换。

use hangar_core::account::Account;
use hangar_core::quota::Quota;
use std::collections::HashMap;

pub(crate) const KEEP_CURRENT_THRESHOLD: i32 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reason {
    KeepCurrent,
    HigherWeeklyRemaining,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Recommendation {
    pub account_id: String,
    pub email: String,
    pub weekly_remaining: i32,
    pub reason: Reason,
}

pub(crate) fn weekly_remaining(quota: &Quota) -> Option<i32> {
    quota
        .windows
        .iter()
        .find(|window| window.label.starts_with('周'))
        .and_then(|window| window.window.remaining)
}

pub(crate) fn recommend(
    accounts: &[Account],
    current: Option<&str>,
    quotas: &HashMap<String, Quota>,
) -> Option<Recommendation> {
    let mut best: Option<(&Account, i32)> = None;
    let mut current_candidate: Option<(&Account, i32)> = None;

    for account in accounts.iter().filter(|account| !account.stale) {
        let Some(remaining) = quotas.get(&account.id).and_then(weekly_remaining) else {
            continue;
        };
        if current == Some(account.id.as_str()) {
            current_candidate = Some((account, remaining));
        }
        if best
            .as_ref()
            .is_none_or(|(_, best_remaining)| remaining > *best_remaining)
        {
            best = Some((account, remaining));
        }
    }

    let (best_account, best_remaining) = best?;
    if let Some((account, remaining)) = current_candidate {
        if best_remaining.saturating_sub(remaining) <= KEEP_CURRENT_THRESHOLD {
            return Some(Recommendation {
                account_id: account.id.clone(),
                email: account.email.clone(),
                weekly_remaining: remaining,
                reason: Reason::KeepCurrent,
            });
        }
    }

    Some(Recommendation {
        account_id: best_account.id.clone(),
        email: best_account.email.clone(),
        weekly_remaining: best_remaining,
        reason: Reason::HigherWeeklyRemaining,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use hangar_core::quota::{NamedWindow, QuotaWindow};

    fn account(id: &str, stale: bool) -> Account {
        Account {
            id: id.to_string(),
            email: format!("{id}@example.com"),
            access_token: "at".to_string(),
            refresh_token: "rt".to_string(),
            id_token: String::new(),
            expires_at: 0,
            stale,
            account_id: None,
            organization_id: None,
        }
    }

    fn quota(weekly: Option<i32>) -> Quota {
        Quota {
            plan: None,
            windows: vec![NamedWindow {
                label: "周".to_string(),
                window: QuotaWindow {
                    remaining: weekly,
                    limit_secs: Some(604_800),
                    reset_in_secs: None,
                    reset_at: None,
                },
            }],
            reset_available: None,
            reset_detail: None,
        }
    }

    fn quotas(rows: &[(&str, Option<i32>)]) -> HashMap<String, Quota> {
        rows.iter()
            .map(|(id, remaining)| ((*id).to_string(), quota(*remaining)))
            .collect()
    }

    #[test]
    fn recommends_higher_weekly_when_gap_exceeds_threshold() {
        let accounts = vec![account("current", false), account("best", false)];
        let result = recommend(
            &accounts,
            Some("current"),
            &quotas(&[("current", Some(40)), ("best", Some(51))]),
        )
        .unwrap();
        assert_eq!(result.account_id, "best");
        assert_eq!(result.weekly_remaining, 51);
        assert_eq!(result.reason, Reason::HigherWeeklyRemaining);
    }

    #[test]
    fn keeps_current_when_gap_is_at_most_threshold() {
        let accounts = vec![account("current", false), account("best", false)];
        let result = recommend(
            &accounts,
            Some("current"),
            &quotas(&[("current", Some(40)), ("best", Some(50))]),
        )
        .unwrap();
        assert_eq!(result.account_id, "current");
        assert_eq!(result.weekly_remaining, 40);
        assert_eq!(result.reason, Reason::KeepCurrent);
    }

    #[test]
    fn equal_values_prefer_current_account() {
        let accounts = vec![account("first", false), account("current", false)];
        let result = recommend(
            &accounts,
            Some("current"),
            &quotas(&[("first", Some(70)), ("current", Some(70))]),
        )
        .unwrap();
        assert_eq!(result.account_id, "current");
        assert_eq!(result.reason, Reason::KeepCurrent);
    }

    #[test]
    fn excludes_stale_and_unknown_accounts() {
        let accounts = vec![
            account("unknown", false),
            account("stale", true),
            account("usable", false),
        ];
        let result = recommend(
            &accounts,
            None,
            &quotas(&[("unknown", None), ("stale", Some(99)), ("usable", Some(20))]),
        )
        .unwrap();
        assert_eq!(result.account_id, "usable");
        assert_eq!(result.weekly_remaining, 20);
    }

    #[test]
    fn ignores_non_weekly_windows() {
        let accounts = vec![account("session-only", false)];
        let mut session = quota(Some(90));
        session.windows[0].label = "会话5h".to_string();
        let quotas = [("session-only".to_string(), session)]
            .into_iter()
            .collect();
        assert!(recommend(&accounts, None, &quotas).is_none());
    }
}
