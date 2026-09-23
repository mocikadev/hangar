//! 面向 CLI 与原生桥接的脱敏业务快照。
//!
//! 这些类型只表达账号、令牌健康和周配额事实，不包含 token 或 UI 状态。

use crate::account::Account;
use crate::quota::{reset_at_ts, Quota};
use crate::token_health::{self, AccessTokenState, ExpirySource};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessTokenSummaryState {
    Stale,
    Missing,
    Fresh,
    Expiring,
    Expired,
    Unknown,
}

impl From<AccessTokenState> for AccessTokenSummaryState {
    fn from(value: AccessTokenState) -> Self {
        match value {
            AccessTokenState::Missing => Self::Missing,
            AccessTokenState::Fresh => Self::Fresh,
            AccessTokenState::Expiring => Self::Expiring,
            AccessTokenState::Expired => Self::Expired,
            AccessTokenState::Unknown => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpirySourceSummary {
    Jwt,
    Ledger,
}

impl From<ExpirySource> for ExpirySourceSummary {
    fn from(value: ExpirySource) -> Self {
        match value {
            ExpirySource::Jwt => Self::Jwt,
            ExpirySource::Ledger => Self::Ledger,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TokenHealthSummary {
    pub status: AccessTokenSummaryState,
    pub expiry_source: Option<ExpirySourceSummary>,
    pub refresh_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AccountSummary {
    pub id: String,
    pub email: String,
    pub current: bool,
    pub stale: bool,
    pub account_id: Option<String>,
    pub organization_id: Option<String>,
    pub expires_at: Option<i64>,
    pub token_health: TokenHealthSummary,
}

impl AccountSummary {
    pub fn from_account(account: &Account, current: bool, now: u64) -> Self {
        let health = token_health::assess(
            &account.access_token,
            &account.refresh_token,
            account.expires_at,
            account.stale,
            now,
        );
        Self {
            id: account.id.clone(),
            email: account.email.clone(),
            current,
            stale: account.stale,
            account_id: account.account_id.clone(),
            organization_id: account.organization_id.clone(),
            expires_at: health.expires_at,
            token_health: TokenHealthSummary {
                status: if health.stale {
                    AccessTokenSummaryState::Stale
                } else {
                    health.access.into()
                },
                expiry_source: health.expiry_source.map(Into::into),
                refresh_available: health.has_refresh_token,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QuotaSummary {
    pub plan: Option<String>,
    pub weekly_remaining: Option<i32>,
    pub weekly_reset_at: Option<i64>,
    pub reset_available: Option<i64>,
    pub reset_next_expiry: Option<i64>,
}

impl QuotaSummary {
    pub fn from_quota(quota: &Quota, now: i64) -> Self {
        let weekly = quota
            .windows
            .iter()
            .find(|window| window.label.starts_with('周'));
        let reset_available = quota
            .reset_detail
            .as_ref()
            .map(|detail| detail.available)
            .or(quota.reset_available);
        Self {
            plan: quota.plan.clone(),
            weekly_remaining: weekly.and_then(|window| window.window.remaining),
            weekly_reset_at: weekly.and_then(|window| reset_at_ts(&window.window, now)),
            reset_available,
            reset_next_expiry: quota
                .reset_detail
                .as_ref()
                .and_then(|detail| detail.next_expiry()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quota::{NamedWindow, QuotaWindow, ResetCredit, ResetCredits};

    fn account() -> Account {
        Account {
            id: "id".into(),
            email: "a@example.com".into(),
            access_token: "access-secret".into(),
            refresh_token: "refresh-secret".into(),
            id_token: "id-secret".into(),
            expires_at: 2_000,
            stale: false,
            account_id: Some("acct".into()),
            organization_id: None,
        }
    }

    #[test]
    fn account_summary_never_contains_credentials() {
        let text =
            serde_json::to_string(&AccountSummary::from_account(&account(), true, 1_000)).unwrap();
        for secret in ["access-secret", "refresh-secret", "id-secret"] {
            assert!(!text.contains(secret));
        }
        assert!(!text.contains("access_token"));
        assert!(!text.contains("refresh_token"));
        assert!(!text.contains("id_token"));
    }

    #[test]
    fn quota_summary_only_uses_weekly_window() {
        let quota = Quota {
            plan: Some("Pro 20x".into()),
            windows: vec![
                NamedWindow {
                    label: "会话5h".into(),
                    window: QuotaWindow {
                        remaining: Some(99),
                        limit_secs: Some(18_000),
                        reset_in_secs: None,
                        reset_at: Some(2_000),
                    },
                },
                NamedWindow {
                    label: "周".into(),
                    window: QuotaWindow {
                        remaining: Some(42),
                        limit_secs: Some(604_800),
                        reset_in_secs: None,
                        reset_at: Some(3_000),
                    },
                },
            ],
            reset_available: Some(1),
            reset_detail: Some(ResetCredits {
                available: 2,
                total_earned: Some(3),
                credits: vec![
                    ResetCredit {
                        title: None,
                        available: false,
                        expires_at: Some(1_000),
                    },
                    ResetCredit {
                        title: None,
                        available: true,
                        expires_at: Some(4_000),
                    },
                ],
            }),
        };
        let summary = QuotaSummary::from_quota(&quota, 1_000);
        assert_eq!(summary.weekly_remaining, Some(42));
        assert_eq!(summary.weekly_reset_at, Some(3_000));
        assert_eq!(summary.reset_available, Some(2));
        assert_eq!(summary.reset_next_expiry, Some(4_000));
    }
}
