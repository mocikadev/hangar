//! 后台线程：网络与切换走这里，UI 线程每帧 poll 事件。线程内禁 UI 调用。

use crate::hooks::GuiHooks;
use hangar_core::quota::Quota;
use std::sync::mpsc::{Receiver, Sender};

pub enum Ev {
    HarvestDone,
    QuotaOne {
        id: String,
        res: Result<Quota, String>,
    },
    QuotaDone,
    SwitchDone {
        email: String,
        res: Result<(), String>,
    },
    LoginUrl {
        url: String,
    },
    LoginDone {
        seq: u64,
        reauth: bool,
        res: Result<String, String>,
    },
    LoginFailed(String),
    UpdateDone {
        res: Result<Option<String>, String>,
    },
}

/// 对话框 → 后台登录线程的粘贴输入
pub enum Paste {
    Text(String),
    Cancel,
}

pub fn spawn_harvest(tx: Sender<Ev>) {
    std::thread::spawn(move || {
        hangar_core::emit::set_quiet(true);
        hangar_core::account::harvest();
        let _ = tx.send(Ev::HarvestDone);
    });
}

pub fn spawn_quota(tx: Sender<Ev>, ids: Vec<String>) {
    std::thread::spawn(move || {
        hangar_core::emit::set_quiet(true);
        for id in ids {
            let res = hangar_core::quota::fetch_quota_for_account(&id).map(|(_, q)| q);
            if tx.send(Ev::QuotaOne { id, res }).is_err() {
                return;
            }
        }
        let _ = tx.send(Ev::QuotaDone);
    });
}

pub fn spawn_switch(tx: Sender<Ev>, id: String) {
    std::thread::spawn(move || {
        hangar_core::emit::set_quiet(true);
        let email = hangar_core::account::load_accounts()
            .ok()
            .and_then(|f| {
                f.accounts
                    .iter()
                    .find(|a| a.id == id)
                    .map(|a| a.email.clone())
            })
            .unwrap_or_default();
        let res = hangar_core::account::switch_account(&id);
        let _ = tx.send(Ev::SwitchDone { email, res });
    });
}

/// 后台检查更新：`current_version` 由调用方传本二进制版本（gui 包内求值即 gui 版本）
pub fn spawn_update(tx: Sender<Ev>, current_version: String) {
    std::thread::spawn(move || {
        hangar_core::emit::set_quiet(true);
        let res = match hangar_core::updater::check_update(
            true,
            &current_version,
            env!("CARGO_PKG_NAME"),
        ) {
            Ok(Some(info)) => hangar_core::updater::apply_update(&info).map(Some),
            Ok(None) => Ok(None),
            Err(e) => Err(e),
        };
        let _ = tx.send(Ev::UpdateDone { res });
    });
}

/// 后台登录：`reauth` 为 Some(目标 id) 时走复活（原位覆盖+切换），否则走添加。
/// seq 由调用方发号，reducer 凭 seq 丢弃过时结果（如取消后姗姗来迟的完成）。
pub fn spawn_login(
    tx: Sender<Ev>,
    seq: u64,
    inbox: Receiver<Paste>,
    reauth: Option<(String, String)>,
) {
    std::thread::spawn(move || {
        hangar_core::emit::set_quiet(true);
        let hooks = GuiHooks {
            tx: tx.clone(),
            inbox,
        };
        let reauth_flag = reauth.is_some();
        let res = match reauth {
            Some((id, _email)) => hangar_core::oauth::login_codex_with(&hooks).and_then(|fresh| {
                hangar_core::account::reauth_account(&id, &fresh)?;
                Ok(fresh.email)
            }),
            None => hangar_core::login::do_login_with(&hooks),
        };
        let _ = tx.send(Ev::LoginDone {
            seq,
            reauth: reauth_flag,
            res,
        });
    });
}
