use hangar_core::operation::{OperationErrorCode, TaskPhase};
use hangar_core::summary::{
    AccessTokenSummaryState, AccountSummary, ExpirySourceSummary, QuotaSummary,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum AccessTokenStatus {
    Stale,
    Missing,
    Fresh,
    Expiring,
    Expired,
    Unknown,
}

impl From<AccessTokenSummaryState> for AccessTokenStatus {
    fn from(value: AccessTokenSummaryState) -> Self {
        match value {
            AccessTokenSummaryState::Stale => Self::Stale,
            AccessTokenSummaryState::Missing => Self::Missing,
            AccessTokenSummaryState::Fresh => Self::Fresh,
            AccessTokenSummaryState::Expiring => Self::Expiring,
            AccessTokenSummaryState::Expired => Self::Expired,
            AccessTokenSummaryState::Unknown => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum TokenExpirySource {
    Jwt,
    Ledger,
}

impl From<ExpirySourceSummary> for TokenExpirySource {
    fn from(value: ExpirySourceSummary) -> Self {
        match value {
            ExpirySourceSummary::Jwt => Self::Jwt,
            ExpirySourceSummary::Ledger => Self::Ledger,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct TokenHealthRecord {
    pub status: AccessTokenStatus,
    pub expires_at: Option<i64>,
    pub expiry_source: Option<TokenExpirySource>,
    pub refresh_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AccountRecord {
    pub id: String,
    pub email: String,
    pub current: bool,
    pub stale: bool,
    pub account_id: Option<String>,
    pub organization_id: Option<String>,
    pub token_health: TokenHealthRecord,
}

impl From<AccountSummary> for AccountRecord {
    fn from(value: AccountSummary) -> Self {
        Self {
            id: value.id,
            email: value.email,
            current: value.current,
            stale: value.stale,
            account_id: value.account_id,
            organization_id: value.organization_id,
            token_health: TokenHealthRecord {
                status: value.token_health.status.into(),
                expires_at: value.expires_at,
                expiry_source: value.token_health.expiry_source.map(Into::into),
                refresh_available: value.token_health.refresh_available,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct QuotaRecord {
    pub plan: Option<String>,
    pub weekly_remaining: Option<i32>,
    pub weekly_reset_at: Option<i64>,
    pub reset_available: Option<i64>,
    pub reset_next_expiry: Option<i64>,
}

impl From<QuotaSummary> for QuotaRecord {
    fn from(value: QuotaSummary) -> Self {
        Self {
            plan: value.plan,
            weekly_remaining: value.weekly_remaining,
            weekly_reset_at: value.weekly_reset_at,
            reset_available: value.reset_available,
            reset_next_expiry: value.reset_next_expiry,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum QuotaLoadState {
    Pending,
    Loading,
    Success,
    Failed,
    Unknown,
    Stale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ErrorCode {
    State,
    Auth,
    External,
    Internal,
}

impl From<OperationErrorCode> for ErrorCode {
    fn from(value: OperationErrorCode) -> Self {
        match value {
            OperationErrorCode::State => Self::State,
            OperationErrorCode::Auth => Self::Auth,
            OperationErrorCode::External => Self::External,
            OperationErrorCode::Internal => Self::Internal,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum RefreshPhase {
    Pending,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl From<TaskPhase> for RefreshPhase {
    fn from(value: TaskPhase) -> Self {
        match value {
            TaskPhase::Pending => Self::Pending,
            TaskPhase::Running => Self::Running,
            TaskPhase::Succeeded => Self::Succeeded,
            TaskPhase::Failed => Self::Failed,
            TaskPhase::Cancelled => Self::Cancelled,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AccountCardRecord {
    pub account: AccountRecord,
    pub quota: Option<QuotaRecord>,
    pub quota_state: QuotaLoadState,
    pub quota_fetched_at: Option<i64>,
    pub quota_is_fresh: bool,
    pub recommended: bool,
    pub error_code: Option<ErrorCode>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RefreshSnapshot {
    pub generation: u64,
    pub phase: RefreshPhase,
    pub accounts: Vec<AccountCardRecord>,
    pub error_code: Option<ErrorCode>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SwitchResult {
    pub account_id: String,
    pub email: String,
    pub codex_running: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SwitchSnapshot {
    pub generation: u64,
    pub phase: RefreshPhase,
    pub result: Option<SwitchResult>,
    pub error_code: Option<ErrorCode>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum LoginPhase {
    Waiting,
    Exchanging,
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LoginStart {
    pub session_id: u64,
    pub authorization_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LoginSnapshot {
    pub session_id: u64,
    pub phase: LoginPhase,
    pub error_code: Option<ErrorCode>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DoctorReport {
    pub lines: Vec<String>,
    pub issue_count: u64,
}

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum BridgeError {
    #[error("状态错误: {message}")]
    State { message: String },
    #[error("认证错误: {message}")]
    Auth { message: String },
    #[error("外部服务错误: {message}")]
    External { message: String },
    #[error("内部错误: {message}")]
    Internal { message: String },
}
