use crate::dto::{
    AccountCardRecord, BridgeError, DoctorReport, ErrorCode, LoginPhase, LoginSnapshot, LoginStart,
    QuotaLoadState, RefreshSnapshot, SwitchResult, SwitchSnapshot,
};
use hangar_core::account::AccountErrorKind;
use hangar_core::account::{Account, AccountsFile};
use hangar_core::oauth::{OAuthLoginSession, OAuthSessionPhase};
use hangar_core::operation::{OperationErrorCode, TaskPhase};
use hangar_core::quota::Quota;
use hangar_core::quota_cache::{self, Entry as CachedQuota};
use hangar_core::summary::{AccountSummary, QuotaSummary};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{SystemTime, UNIX_EPOCH};

type LoadAccounts = dyn Fn() -> Result<AccountsFile, String> + Send + Sync;
type FetchQuota = dyn Fn(&str) -> Result<(Account, Quota), String> + Send + Sync;
type Harvest = dyn Fn() + Send + Sync;
type LoadQuotaCache = dyn Fn() -> HashMap<String, CachedQuota> + Send + Sync;

#[derive(Debug, Clone)]
struct RefreshState {
    generation: u64,
    phase: TaskPhase,
    accounts: Vec<AccountCardRecord>,
    error_code: Option<OperationErrorCode>,
}

#[derive(Debug, Clone)]
struct SwitchState {
    generation: u64,
    phase: TaskPhase,
    result: Option<SwitchResult>,
    error_code: Option<OperationErrorCode>,
    message: Option<String>,
}

impl SwitchState {
    fn snapshot(&self) -> SwitchSnapshot {
        SwitchSnapshot {
            generation: self.generation,
            phase: self.phase.into(),
            result: self.result.clone(),
            error_code: self.error_code.map(Into::into),
            message: self.message.clone(),
        }
    }
}

impl RefreshState {
    fn snapshot(&self) -> RefreshSnapshot {
        RefreshSnapshot {
            generation: self.generation,
            phase: self.phase.into(),
            accounts: self.accounts.clone(),
            error_code: self.error_code.map(Into::into),
        }
    }
}

#[derive(uniffi::Object)]
pub struct HangarService {
    next_generation: AtomicU64,
    next_switch_generation: AtomicU64,
    next_login_session: AtomicU64,
    closed: AtomicBool,
    state: Mutex<RefreshState>,
    switch_state: Mutex<SwitchState>,
    login_sessions: Mutex<HashMap<u64, LoginSessionEntry>>,
    load_accounts: Arc<LoadAccounts>,
    fetch_quota: Arc<FetchQuota>,
    harvest: Arc<Harvest>,
    load_quota_cache: Arc<LoadQuotaCache>,
    quota_attempts: Mutex<HashMap<String, u64>>,
}

enum LoginMode {
    Add,
    Reauth(String),
}

struct LoginSessionEntry {
    session: Arc<OAuthLoginSession>,
    mode: LoginMode,
    committed: bool,
    commit_error: Option<BridgeError>,
}

#[uniffi::export]
impl HangarService {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Self::with_runtime_dependencies(
            Arc::new(hangar_core::account::load_accounts),
            Arc::new(hangar_core::quota::fetch_quota_for_account),
            Arc::new(hangar_core::account::harvest),
            Arc::new(quota_cache::load),
        )
    }

    pub fn start_refresh_all(self: Arc<Self>) -> u64 {
        self.start_refresh(true)
    }

    pub fn start_refresh_due(self: Arc<Self>) -> u64 {
        self.start_refresh(false)
    }

    fn start_refresh(self: Arc<Self>, force: bool) -> u64 {
        if self.closed.load(Ordering::Acquire) {
            return 0;
        }
        let generation = self.next_generation.fetch_add(1, Ordering::AcqRel) + 1;
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let previous_accounts = state.accounts.clone();
        *state = RefreshState {
            generation,
            phase: TaskPhase::Pending,
            accounts: previous_accounts,
            error_code: None,
        };
        drop(state);
        let weak = Arc::downgrade(&self);
        std::thread::spawn(move || Self::run_refresh(weak, generation, force));
        generation
    }

    pub fn refresh_snapshot(&self) -> RefreshSnapshot {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .snapshot()
    }

    pub fn cancel_refresh(&self, generation: u64) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.generation != generation
            || matches!(
                state.phase,
                TaskPhase::Succeeded | TaskPhase::Failed | TaskPhase::Cancelled
            )
        {
            return false;
        }
        state.phase = TaskPhase::Cancelled;
        true
    }

    pub fn start_switch(self: Arc<Self>, account_id: String) -> Result<u64, BridgeError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(BridgeError::State {
                message: "Hangar 服务已关闭".to_string(),
            });
        }
        let refresh_phase = self
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .phase;
        if matches!(refresh_phase, TaskPhase::Pending | TaskPhase::Running)
            && self.refresh_snapshot().generation != 0
        {
            return Err(BridgeError::State {
                message: "账号额度仍在刷新，请稍后再切换".to_string(),
            });
        }
        let mut state = self
            .switch_state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if matches!(state.phase, TaskPhase::Pending | TaskPhase::Running) && state.generation != 0 {
            return Err(BridgeError::State {
                message: "已有账号切换正在进行".to_string(),
            });
        }
        let generation = self.next_switch_generation.fetch_add(1, Ordering::AcqRel) + 1;
        *state = SwitchState {
            generation,
            phase: TaskPhase::Pending,
            result: None,
            error_code: None,
            message: None,
        };
        drop(state);
        let weak = Arc::downgrade(&self);
        std::thread::spawn(move || Self::run_switch(weak, generation, account_id));
        Ok(generation)
    }

    pub fn switch_snapshot(&self) -> SwitchSnapshot {
        self.switch_state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .snapshot()
    }

    pub fn start_login(&self) -> Result<LoginStart, BridgeError> {
        self.start_login_mode(LoginMode::Add)
    }

    pub fn start_reauth(&self, account_id: String) -> Result<LoginStart, BridgeError> {
        let file = hangar_core::account::load_accounts().map_err(internal_error)?;
        if !file.accounts.iter().any(|account| account.id == account_id) {
            return Err(BridgeError::State {
                message: format!("账号未找到: {account_id}"),
            });
        }
        self.start_login_mode(LoginMode::Reauth(account_id))
    }

    pub fn submit_callback(
        &self,
        session_id: u64,
        callback_url: String,
    ) -> Result<(), BridgeError> {
        let sessions = self
            .login_sessions
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let entry = sessions
            .get(&session_id)
            .ok_or_else(|| BridgeError::State {
                message: format!("OAuth 会话不存在: {session_id}"),
            })?;
        entry
            .session
            .submit_callback(&callback_url)
            .map_err(|message| BridgeError::Auth { message })
    }

    pub fn cancel_login(&self, session_id: u64) -> bool {
        self.login_sessions
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(&session_id)
            .is_some_and(|entry| entry.session.cancel())
    }

    /// Release a login session after cancellation or any terminal result.
    /// Removing the bridge entry does not expose or return credentials.
    pub fn release_login(&self, session_id: u64) -> bool {
        let entry = self
            .login_sessions
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&session_id);
        if let Some(entry) = entry {
            let _ = entry.session.cancel();
            true
        } else {
            false
        }
    }

    pub fn poll_login(&self, session_id: u64) -> Result<LoginSnapshot, BridgeError> {
        let mut sessions = self
            .login_sessions
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let entry = sessions
            .get_mut(&session_id)
            .ok_or_else(|| BridgeError::State {
                message: format!("OAuth 会话不存在: {session_id}"),
            })?;

        if entry.session.phase() == OAuthSessionPhase::Succeeded && !entry.committed {
            let account = entry
                .session
                .take_account()
                .ok_or_else(|| BridgeError::Internal {
                    message: "OAuth 结果已被消费".to_string(),
                })?;
            let result = match &entry.mode {
                LoginMode::Add => {
                    hangar_core::login::store_logged_in_account(account).map_err(internal_error)
                }
                LoginMode::Reauth(account_id) => {
                    hangar_core::account::reauth_account_checked(account_id, &account)
                        .map_err(account_error)
                }
            };
            match result {
                Ok(()) => entry.committed = true,
                Err(error) => entry.commit_error = Some(error),
            }
        }

        if let Some(error) = &entry.commit_error {
            return Ok(LoginSnapshot {
                session_id,
                phase: LoginPhase::Failed,
                error_code: Some(error_code(error)),
                message: Some(error.to_string()),
            });
        }
        let phase = entry.session.phase();
        Ok(LoginSnapshot {
            session_id,
            phase: phase.into(),
            error_code: (phase == OAuthSessionPhase::Failed).then_some(ErrorCode::Auth),
            message: entry.session.error_message(),
        })
    }

    /// Potentially blocking filesystem operation. Native callers must invoke it
    /// away from their UI actor/thread.
    pub fn delete_account(&self, account_id: String) -> Result<(), BridgeError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(BridgeError::State {
                message: "Hangar 服务已关闭".to_string(),
            });
        }
        hangar_core::account::delete_account_checked(&account_id)
            .map(|_| ())
            .map_err(account_error)
    }

    /// Offline diagnosis. The returned lines are display-only and never contain
    /// raw tokens; callers still run this filesystem work away from the UI actor.
    pub fn doctor(&self, binary_version: String) -> Result<DoctorReport, BridgeError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(BridgeError::State {
                message: "Hangar 服务已关闭".to_string(),
            });
        }
        let (lines, issue_count) =
            hangar_core::doctor::doctor_lines(&binary_version).map_err(internal_error)?;
        Ok(DoctorReport {
            lines,
            issue_count: issue_count as u64,
        })
    }

    pub fn close(&self) {
        self.closed.store(true, Ordering::Release);
        let generation = self
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .generation;
        let _ = self.cancel_refresh(generation);
        for entry in self
            .login_sessions
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .values()
        {
            let _ = entry.session.cancel();
        }
    }
}

impl HangarService {
    #[cfg(test)]
    fn with_dependencies(
        load_accounts: Arc<LoadAccounts>,
        fetch_quota: Arc<FetchQuota>,
    ) -> Arc<Self> {
        Self::with_runtime_dependencies(
            load_accounts,
            fetch_quota,
            Arc::new(|| {}),
            Arc::new(HashMap::new),
        )
    }

    fn with_runtime_dependencies(
        load_accounts: Arc<LoadAccounts>,
        fetch_quota: Arc<FetchQuota>,
        harvest: Arc<Harvest>,
        load_quota_cache: Arc<LoadQuotaCache>,
    ) -> Arc<Self> {
        Arc::new(Self {
            next_generation: AtomicU64::new(0),
            next_switch_generation: AtomicU64::new(0),
            next_login_session: AtomicU64::new(0),
            closed: AtomicBool::new(false),
            state: Mutex::new(RefreshState {
                generation: 0,
                phase: TaskPhase::Succeeded,
                accounts: Vec::new(),
                error_code: None,
            }),
            switch_state: Mutex::new(SwitchState {
                generation: 0,
                phase: TaskPhase::Succeeded,
                result: None,
                error_code: None,
                message: None,
            }),
            login_sessions: Mutex::new(HashMap::new()),
            load_accounts,
            fetch_quota,
            harvest,
            load_quota_cache,
            quota_attempts: Mutex::new(HashMap::new()),
        })
    }

    fn start_login_mode(&self, mode: LoginMode) -> Result<LoginStart, BridgeError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(BridgeError::State {
                message: "Hangar 服务已关闭".to_string(),
            });
        }
        let session = OAuthLoginSession::start().map_err(internal_error)?;
        let session_id = self.next_login_session.fetch_add(1, Ordering::AcqRel) + 1;
        let authorization_url = session.authorization_url().to_string();
        self.login_sessions
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(
                session_id,
                LoginSessionEntry {
                    session,
                    mode,
                    committed: false,
                    commit_error: None,
                },
            );
        Ok(LoginStart {
            session_id,
            authorization_url,
        })
    }

    fn run_switch(weak: Weak<Self>, generation: u64, account_id: String) {
        let Some(service) = weak.upgrade() else {
            return;
        };
        {
            let mut state = service
                .switch_state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if state.generation != generation || state.phase != TaskPhase::Pending {
                return;
            }
            state.phase = TaskPhase::Running;
        }
        let result: Result<SwitchResult, BridgeError> = (|| {
            let file = hangar_core::account::load_accounts().map_err(internal_error)?;
            let account = file
                .accounts
                .iter()
                .find(|account| account.id == account_id)
                .ok_or_else(|| BridgeError::State {
                    message: format!("账号未找到: {account_id}"),
                })?;
            let email = account.email.clone();
            hangar_core::account::switch_account_checked(&account_id).map_err(account_error)?;
            Ok(SwitchResult {
                account_id,
                email,
                codex_running: hangar_core::process::codex_process_running(),
            })
        })();
        let mut state = service
            .switch_state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if state.generation != generation || state.phase != TaskPhase::Running {
            return;
        }
        match result {
            Ok(result) => {
                state.phase = TaskPhase::Succeeded;
                state.result = Some(result);
            }
            Err(error) => {
                state.phase = TaskPhase::Failed;
                state.error_code = Some(match &error {
                    BridgeError::State { .. } => OperationErrorCode::State,
                    BridgeError::Auth { .. } => OperationErrorCode::Auth,
                    BridgeError::External { .. } => OperationErrorCode::External,
                    BridgeError::Internal { .. } => OperationErrorCode::Internal,
                });
                state.message = Some(error.to_string());
            }
        }
    }

    fn now_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    fn run_refresh(weak: Weak<Self>, generation: u64, force: bool) {
        let Some(service) = weak.upgrade() else {
            return;
        };
        (service.harvest)();
        let accounts_file = match (service.load_accounts)() {
            Ok(file) => file,
            Err(_) => {
                service.finish_with_error(generation, OperationErrorCode::Internal);
                return;
            }
        };
        let cache = (service.load_quota_cache)();
        let now = Self::now_secs();
        if !service.install_accounts(generation, &accounts_file, &cache, now) {
            return;
        }
        drop(service);

        let accounts = accounts_file.accounts.clone();
        let current = accounts_file.current_account_id.clone();
        let mut quotas: HashMap<String, Quota> = cache
            .iter()
            .filter(|(id, entry)| {
                entry.is_fresh(now)
                    && accounts
                        .iter()
                        .any(|account| !account.stale && account.id == **id)
            })
            .map(|(id, entry)| (id.clone(), entry.quota.clone()))
            .collect();
        for account in accounts.iter().filter(|account| !account.stale).cloned() {
            let Some(service) = weak.upgrade() else {
                return;
            };
            if !quota_cache::due(cache.get(&account.id), now, force) {
                continue;
            }
            if !force
                && service
                    .quota_attempts
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .get(&account.id)
                    .is_some_and(|last| now >= *last && now - *last < quota_cache::TTL_SECS)
            {
                continue;
            }
            if !service.mark_loading(generation, &account.id) {
                return;
            }
            service
                .quota_attempts
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(account.id.clone(), now);
            let result = (service.fetch_quota)(&account.id);
            if let Ok((_, quota)) = &result {
                quotas.insert(account.id.clone(), quota.clone());
            } else {
                quotas.remove(&account.id);
            }
            if !service.apply_quota_result(generation, account, result) {
                return;
            }
        }

        if let Some(service) = weak.upgrade() {
            let recommended =
                hangar_core::recommendation::recommend(&accounts, current.as_deref(), &quotas)
                    .map(|result| result.account_id);
            service.finish_success(generation, recommended.as_deref());
        }
    }

    fn install_accounts(
        &self,
        generation: u64,
        file: &AccountsFile,
        cache: &HashMap<String, CachedQuota>,
        now: u64,
    ) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.generation != generation || state.phase == TaskPhase::Cancelled {
            return false;
        }
        state.phase = TaskPhase::Running;
        state.accounts = file
            .accounts
            .iter()
            .map(|account| {
                let cached = (!account.stale).then(|| cache.get(&account.id)).flatten();
                let summary = cached
                    .map(|entry| QuotaSummary::from_quota(&entry.quota, entry.fetched_at as i64));
                AccountCardRecord {
                    account: AccountSummary::from_account(
                        account,
                        file.current_account_id.as_deref() == Some(account.id.as_str()),
                        now,
                    )
                    .into(),
                    quota: summary.as_ref().cloned().map(Into::into),
                    quota_state: if account.stale {
                        QuotaLoadState::Stale
                    } else if summary
                        .as_ref()
                        .is_some_and(|summary| summary.weekly_remaining.is_some())
                    {
                        QuotaLoadState::Success
                    } else if cached.is_some() {
                        QuotaLoadState::Unknown
                    } else {
                        QuotaLoadState::Pending
                    },
                    quota_fetched_at: cached.map(|entry| entry.fetched_at as i64),
                    quota_is_fresh: cached.is_some_and(|entry| entry.is_fresh(now)),
                    recommended: false,
                    error_code: None,
                }
            })
            .collect();
        let fresh_quotas: HashMap<String, Quota> = cache
            .iter()
            .filter(|(id, entry)| {
                entry.is_fresh(now)
                    && file
                        .accounts
                        .iter()
                        .any(|account| !account.stale && account.id == **id)
            })
            .map(|(id, entry)| (id.clone(), entry.quota.clone()))
            .collect();
        let recommended = hangar_core::recommendation::recommend(
            &file.accounts,
            file.current_account_id.as_deref(),
            &fresh_quotas,
        );
        for card in &mut state.accounts {
            card.recommended = recommended
                .as_ref()
                .is_some_and(|choice| choice.account_id == card.account.id);
        }
        true
    }

    fn mark_loading(&self, generation: u64, account_id: &str) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.generation != generation || state.phase != TaskPhase::Running {
            return false;
        }
        let Some(card) = state
            .accounts
            .iter_mut()
            .find(|card| card.account.id == account_id)
        else {
            return false;
        };
        card.quota_state = QuotaLoadState::Loading;
        true
    }

    fn apply_quota_result(
        &self,
        generation: u64,
        original: Account,
        result: Result<(Account, Quota), String>,
    ) -> bool {
        let now = Self::now_secs();
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.generation != generation || state.phase != TaskPhase::Running {
            return false;
        }
        let Some(card) = state
            .accounts
            .iter_mut()
            .find(|card| card.account.id == original.id)
        else {
            return false;
        };
        match result {
            Ok((fresh, quota)) => {
                card.account =
                    AccountSummary::from_account(&fresh, card.account.current, now).into();
                let summary = QuotaSummary::from_quota(&quota, now as i64);
                card.quota_state = if summary.weekly_remaining.is_some() {
                    QuotaLoadState::Success
                } else {
                    QuotaLoadState::Unknown
                };
                card.quota = Some(summary.into());
                card.quota_fetched_at = Some(now as i64);
                card.quota_is_fresh = true;
                card.error_code = None;
            }
            Err(_) => {
                card.quota_state = QuotaLoadState::Failed;
                card.quota_is_fresh = false;
                card.error_code = Some(ErrorCode::External);
            }
        }
        true
    }

    fn finish_success(&self, generation: u64, recommended: Option<&str>) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.generation == generation && state.phase == TaskPhase::Running {
            for card in &mut state.accounts {
                card.recommended = recommended == Some(card.account.id.as_str());
            }
            state.phase = TaskPhase::Succeeded;
        }
    }

    fn finish_with_error(&self, generation: u64, error: OperationErrorCode) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.generation == generation
            && matches!(state.phase, TaskPhase::Pending | TaskPhase::Running)
        {
            state.phase = TaskPhase::Failed;
            state.error_code = Some(error);
        }
    }
}

impl From<OAuthSessionPhase> for LoginPhase {
    fn from(value: OAuthSessionPhase) -> Self {
        match value {
            OAuthSessionPhase::Waiting => Self::Waiting,
            OAuthSessionPhase::Exchanging => Self::Exchanging,
            OAuthSessionPhase::Succeeded => Self::Succeeded,
            OAuthSessionPhase::Failed => Self::Failed,
            OAuthSessionPhase::Cancelled => Self::Cancelled,
            OAuthSessionPhase::TimedOut => Self::TimedOut,
        }
    }
}

fn internal_error(message: String) -> BridgeError {
    BridgeError::Internal { message }
}

fn account_error(error: hangar_core::account::AccountError) -> BridgeError {
    let message = error.to_string();
    match error.kind() {
        AccountErrorKind::State => BridgeError::State { message },
        AccountErrorKind::Auth => BridgeError::Auth { message },
        AccountErrorKind::External => BridgeError::External { message },
        AccountErrorKind::Internal => BridgeError::Internal { message },
    }
}

fn error_code(error: &BridgeError) -> ErrorCode {
    match error {
        BridgeError::State { .. } => ErrorCode::State,
        BridgeError::Auth { .. } => ErrorCode::Auth,
        BridgeError::External { .. } => ErrorCode::External,
        BridgeError::Internal { .. } => ErrorCode::Internal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::RefreshPhase;
    use hangar_core::quota::{NamedWindow, QuotaWindow};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    fn account(id: &str, stale: bool) -> Account {
        Account {
            id: id.into(),
            email: format!("{id}@example.com"),
            access_token: "opaque".into(),
            refresh_token: "rt".into(),
            id_token: String::new(),
            expires_at: u64::MAX,
            stale,
            account_id: None,
            organization_id: None,
        }
    }

    fn quota(value: i32) -> Quota {
        Quota {
            plan: Some("Pro 20x".into()),
            windows: vec![NamedWindow {
                label: "周".into(),
                window: QuotaWindow {
                    remaining: Some(value),
                    limit_secs: Some(604_800),
                    reset_in_secs: None,
                    reset_at: None,
                },
            }],
            reset_available: None,
            reset_detail: None,
        }
    }

    fn wait_terminal(service: &HangarService) -> RefreshSnapshot {
        for _ in 0..100 {
            let snapshot = service.refresh_snapshot();
            if matches!(
                snapshot.phase,
                RefreshPhase::Succeeded | RefreshPhase::Failed | RefreshPhase::Cancelled
            ) {
                return snapshot;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("refresh did not finish");
    }

    #[test]
    fn initial_snapshot_is_idle_so_gui_can_start_first_refresh() {
        let service = HangarService::with_dependencies(
            Arc::new(|| {
                Ok(AccountsFile {
                    accounts: Vec::new(),
                    current_account_id: None,
                })
            }),
            Arc::new(|_| unreachable!()),
        );
        let snapshot = service.refresh_snapshot();
        assert_eq!(snapshot.generation, 0);
        assert_eq!(snapshot.phase, RefreshPhase::Succeeded);
    }

    #[test]
    fn recent_cache_is_visible_without_automatic_request_but_manual_refresh_runs() {
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_fetch = calls.clone();
        let now = HangarService::now_secs();
        let service = HangarService::with_runtime_dependencies(
            Arc::new(|| {
                Ok(AccountsFile {
                    accounts: vec![account("a", false)],
                    current_account_id: Some("a".into()),
                })
            }),
            Arc::new(move |_| {
                calls_for_fetch.fetch_add(1, Ordering::SeqCst);
                Ok((account("a", false), quota(60)))
            }),
            Arc::new(|| {}),
            Arc::new(move || {
                HashMap::from([(
                    "a".into(),
                    CachedQuota {
                        fetched_at: now,
                        quota: quota(55),
                    },
                )])
            }),
        );

        service.clone().start_refresh_due();
        let snapshot = wait_terminal(&service);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            snapshot.accounts[0]
                .quota
                .as_ref()
                .unwrap()
                .weekly_remaining,
            Some(55)
        );
        assert!(snapshot.accounts[0].recommended);

        service.clone().start_refresh_all();
        assert_eq!(service.refresh_snapshot().accounts.len(), 1);
        let snapshot = wait_terminal(&service);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            snapshot.accounts[0]
                .quota
                .as_ref()
                .unwrap()
                .weekly_remaining,
            Some(60)
        );
    }

    #[test]
    fn failed_refresh_keeps_old_quota_visible_and_unrecommended() {
        let now = HangarService::now_secs();
        let service = HangarService::with_runtime_dependencies(
            Arc::new(|| {
                Ok(AccountsFile {
                    accounts: vec![account("a", false)],
                    current_account_id: Some("a".into()),
                })
            }),
            Arc::new(|_| Err("offline".into())),
            Arc::new(|| {}),
            Arc::new(move || {
                HashMap::from([(
                    "a".into(),
                    CachedQuota {
                        fetched_at: now - quota_cache::TTL_SECS - 1,
                        quota: quota(55),
                    },
                )])
            }),
        );

        service.clone().start_refresh_due();
        let snapshot = wait_terminal(&service);
        assert_eq!(
            snapshot.accounts[0]
                .quota
                .as_ref()
                .unwrap()
                .weekly_remaining,
            Some(55)
        );
        assert_eq!(snapshot.accounts[0].quota_state, QuotaLoadState::Failed);
        assert!(!snapshot.accounts[0].quota_is_fresh);
        assert!(!snapshot.accounts[0].recommended);
    }

    #[test]
    fn refreshes_healthy_accounts_sequentially_in_library_order() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let calls_for_fetch = calls.clone();
        let service = HangarService::with_dependencies(
            Arc::new(|| {
                Ok(AccountsFile {
                    accounts: vec![
                        account("a", false),
                        account("stale", true),
                        account("b", false),
                    ],
                    current_account_id: Some("a".into()),
                })
            }),
            Arc::new(move |id| {
                calls_for_fetch.lock().unwrap().push(id.to_string());
                Ok((account(id, false), quota(if id == "a" { 40 } else { 60 })))
            }),
        );
        service.clone().start_refresh_all();
        let snapshot = wait_terminal(&service);
        assert_eq!(snapshot.phase, RefreshPhase::Succeeded);
        assert_eq!(*calls.lock().unwrap(), ["a", "b"]);
        assert_eq!(snapshot.accounts[1].quota_state, QuotaLoadState::Stale);
        assert!(!snapshot.accounts[0].recommended);
        assert!(snapshot.accounts[2].recommended);
    }

    #[test]
    fn newer_generation_rejects_late_results() {
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_fetch = calls.clone();
        let service = HangarService::with_dependencies(
            Arc::new(|| {
                Ok(AccountsFile {
                    accounts: vec![account("a", false)],
                    current_account_id: None,
                })
            }),
            Arc::new(move |id| {
                if calls_for_fetch.fetch_add(1, Ordering::SeqCst) == 0 {
                    std::thread::sleep(Duration::from_millis(40));
                }
                Ok((account(id, false), quota(50)))
            }),
        );
        let old = service.clone().start_refresh_all();
        std::thread::sleep(Duration::from_millis(5));
        let current = service.clone().start_refresh_all();
        assert!(current > old);
        let snapshot = wait_terminal(&service);
        assert_eq!(snapshot.generation, current);
        assert_eq!(snapshot.phase, RefreshPhase::Succeeded);
    }

    #[test]
    fn cancellation_is_terminal_and_idempotent() {
        let service = HangarService::with_dependencies(
            Arc::new(|| {
                Ok(AccountsFile {
                    accounts: vec![account("a", false)],
                    current_account_id: None,
                })
            }),
            Arc::new(|id| {
                std::thread::sleep(Duration::from_millis(30));
                Ok((account(id, false), quota(50)))
            }),
        );
        let generation = service.clone().start_refresh_all();
        assert!(service.cancel_refresh(generation));
        assert!(!service.cancel_refresh(generation));
        assert_eq!(service.refresh_snapshot().phase, RefreshPhase::Cancelled);
    }
}
