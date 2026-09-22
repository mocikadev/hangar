//! 令牌健康纯规则：统一 JWT/账本有效期优先级与投影守卫，不执行网络或文件 I/O。

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

pub const REFRESH_SKEW_SECS: u64 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpirySource {
    Jwt,
    Ledger,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessTokenState {
    Missing,
    Fresh,
    Expiring,
    Expired,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenHealth {
    pub access: AccessTokenState,
    pub expires_at: Option<i64>,
    pub expiry_source: Option<ExpirySource>,
    pub has_refresh_token: bool,
    pub stale: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshFailureAction {
    MarkStale,
    ContinueWithCurrentAccess,
    AbortWithoutStale,
}

impl TokenHealth {
    pub fn needs_refresh(self) -> bool {
        matches!(
            self.access,
            AccessTokenState::Missing | AccessTokenState::Expiring | AccessTokenState::Expired
        )
    }

    /// `Unknown` 保持对旧库/非 JWT AT 的兼容；明确缺失、临期或过期必须先刷新。
    pub fn can_project_without_refresh(self) -> bool {
        !self.stale
            && matches!(
                self.access,
                AccessTokenState::Fresh | AccessTokenState::Unknown
            )
    }

    pub fn label(self) -> &'static str {
        if self.stale {
            return "stale";
        }
        match self.access {
            AccessTokenState::Missing => "missing",
            AccessTokenState::Fresh => "fresh",
            AccessTokenState::Expiring => "expiring",
            AccessTokenState::Expired => "expired",
            AccessTokenState::Unknown => "unknown",
        }
    }
}

pub fn classify_refresh_failure(
    health_before_refresh: TokenHealth,
    auth_rejected: bool,
) -> RefreshFailureAction {
    if auth_rejected {
        RefreshFailureAction::MarkStale
    } else if health_before_refresh.can_project_without_refresh() {
        RefreshFailureAction::ContinueWithCurrentAccess
    } else {
        RefreshFailureAction::AbortWithoutStale
    }
}

fn jwt_payload(token: &str) -> Option<serde_json::Value> {
    let payload = token.trim().split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn jwt_exp(token: &str) -> Option<i64> {
    jwt_payload(token)?.get("exp")?.as_i64()
}

pub fn effective_expiry(access_token: &str, ledger_expires_at: u64) -> Option<(i64, ExpirySource)> {
    jwt_exp(access_token)
        .map(|exp| (exp, ExpirySource::Jwt))
        .or_else(|| {
            i64::try_from(ledger_expires_at)
                .ok()
                .filter(|exp| *exp > 0)
                .map(|exp| (exp, ExpirySource::Ledger))
        })
}

pub fn assess(
    access_token: &str,
    refresh_token: &str,
    ledger_expires_at: u64,
    stale: bool,
    now: u64,
) -> TokenHealth {
    let expiry = effective_expiry(access_token, ledger_expires_at);
    let access = if access_token.trim().is_empty() {
        AccessTokenState::Missing
    } else {
        match expiry {
            Some((exp, _)) if exp <= now as i64 => AccessTokenState::Expired,
            Some((exp, _)) if exp <= now.saturating_add(REFRESH_SKEW_SECS) as i64 => {
                AccessTokenState::Expiring
            }
            Some(_) => AccessTokenState::Fresh,
            None => AccessTokenState::Unknown,
        }
    };
    TokenHealth {
        access,
        expires_at: expiry.map(|(exp, _)| exp),
        expiry_source: expiry.map(|(_, source)| source),
        has_refresh_token: !refresh_token.trim().is_empty(),
        stale,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jwt(exp: i64) -> String {
        let payload = URL_SAFE_NO_PAD.encode(format!(r#"{{"exp":{exp}}}"#));
        format!("x.{payload}.y")
    }

    #[test]
    fn jwt_expiry_overrides_conflicting_ledger() {
        let health = assess(&jwt(900), "rt", 9_000, false, 1_000);
        assert_eq!(health.access, AccessTokenState::Expired);
        assert_eq!(health.expiry_source, Some(ExpirySource::Jwt));
    }

    #[test]
    fn ledger_is_used_only_when_jwt_expiry_is_unavailable() {
        let health = assess("opaque", "rt", 2_000, false, 1_000);
        assert_eq!(health.access, AccessTokenState::Fresh);
        assert_eq!(health.expiry_source, Some(ExpirySource::Ledger));
    }

    #[test]
    fn missing_and_expiring_tokens_cannot_be_projected() {
        assert!(!assess("", "rt", 0, false, 1_000).can_project_without_refresh());
        assert!(!assess(&jwt(1_200), "rt", 0, false, 1_000).can_project_without_refresh());
        assert!(assess("opaque", "", 0, false, 1_000).can_project_without_refresh());
    }

    #[test]
    fn refresh_failure_policy_separates_auth_and_transient_errors() {
        let fresh = assess("opaque", "rt", 2_000, false, 1_000);
        let expired = assess("opaque", "rt", 900, false, 1_000);
        assert_eq!(
            classify_refresh_failure(fresh, false),
            RefreshFailureAction::ContinueWithCurrentAccess
        );
        assert_eq!(
            classify_refresh_failure(expired, false),
            RefreshFailureAction::AbortWithoutStale
        );
        assert_eq!(
            classify_refresh_failure(fresh, true),
            RefreshFailureAction::MarkStale
        );
    }
}
