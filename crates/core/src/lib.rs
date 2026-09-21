//! hangar-core：Codex 多账号管理业务层（UI 无关）。
//!
//! - `account`  : 账号库（harvest 收敛/切换/复活/删除）、原子写 + 文件锁、JWT 工具
//! - `oauth`    : OAuth PKCE 登录、token 交换/刷新
//! - `quota`    : wham/usage 配额解析、重置卡（只读）、时间格式化
//! - `doctor`   : 离线自检（凭据/权限/一致性/锁）
//! - `login`    : 登录 + 入库 + 切换的组合动作
//! - `process`  : Codex 进程检测
//!
//! 两个前端（crates/cli 的 TUI 与 classic）共享本层全部函数。

pub mod account;
pub mod doctor;
pub mod emit;
pub mod login;
pub mod oauth;
pub mod process;
pub mod quota;
pub mod updater;

pub use account::{
    delete_account, force_refresh_account, fresh_account, harvest, load_accounts, reauth_account,
    switch_account, with_accounts_lock, Account, AccountsFile,
};
pub use login::do_login;
pub use oauth::login_codex;
pub use process::codex_process_running;
pub use quota::{fetch_quota_for_account, fmt_countdown, fmt_ts_local, quota_bar, reset_in_secs};
