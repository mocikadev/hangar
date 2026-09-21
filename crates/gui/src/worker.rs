//! 后台线程：网络与切换走这里，UI 线程每帧 poll 事件。线程内禁 UI 调用。

use hangar_core::quota::Quota;
use std::sync::mpsc::Sender;

pub enum Ev {
    HarvestDone,
    QuotaOne {
        id: String,
        res: Result<Quota, String>,
    },
    QuotaDone,
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
