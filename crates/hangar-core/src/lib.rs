//! hangar-core：Codex 多账号管理业务层（UI 无关）。
//!
//! - `account`  : 账号库（harvest 收敛/切换/复活/删除）、原子写 + 文件锁、JWT 工具
//! - `oauth`    : OAuth PKCE 登录、token 交换/刷新
//! - `quota`    : wham/usage 配额解析、重置卡（只读）、时间格式化
//! - `doctor`   : 离线自检（凭据/权限/一致性/锁）
//! - `login`    : 登录 + 入库（不自动切换）的组合动作
//! - `process`  : Codex 进程检测
//! - `recommendation`: 基于周剩余额度的稳定推荐规则
//!
//! CLI/TUI 与后续原生 GUI 共享本层业务规则。

pub mod account;
pub mod doctor;
pub mod emit;
pub mod http;
pub mod login;
pub mod oauth;
pub mod operation;
pub mod process;
pub mod quota;
pub mod quota_cache;
pub mod recommendation;
pub mod summary;
pub mod token_health;
pub mod updater;

pub use account::{
    delete_account, delete_account_checked, force_refresh_account, fresh_account, harvest,
    load_accounts, reauth_account, reauth_account_checked, switch_account, switch_account_checked,
    with_accounts_lock, Account, AccountError, AccountErrorKind, AccountsFile,
};
pub use login::do_login;
pub use login::do_login_with;
pub use login::store_logged_in_account;
pub use oauth::login_codex;
pub use oauth::{login_codex_with, LoginHooks, OAuthLoginSession, OAuthSessionPhase};
pub use process::codex_process_running;
pub use quota::{fetch_quota_for_account, fmt_countdown, fmt_ts_local, quota_bar, reset_in_secs};
