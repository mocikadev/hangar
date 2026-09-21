//! LoginHooks 的 GUI 实现：URL 进对话框展示，粘贴经通道收。
//!
//! 粘贴预检与主流程共用 `parse_code_from_callback_url`：
//! state 不匹配只发 `LoginFailed` 记错，后台监听不死，对话框不关。

use crate::worker::{Ev, Paste};
use hangar_core::oauth::LoginHooks;
use std::sync::mpsc::{Receiver, Sender};

pub struct GuiHooks {
    pub tx: Sender<Ev>,
    pub inbox: Receiver<Paste>,
}

impl LoginHooks for GuiHooks {
    fn show_auth_url(&self, url: &str) {
        let _ = self.tx.send(Ev::LoginUrl {
            url: url.to_string(),
        });
    }

    fn prompt_callback(&self, state: &str) -> Option<String> {
        loop {
            match self.inbox.recv() {
                Ok(Paste::Text(s)) => {
                    let s = s.trim().to_string();
                    if s.is_empty() {
                        continue;
                    }
                    match hangar_core::oauth::parse_code_from_callback_url(&s, state) {
                        Ok(_) => return Some(s),
                        Err(e) => {
                            let _ = self.tx.send(Ev::LoginFailed(e));
                        }
                    }
                }
                // 取消 / 通道断开：结束等待，主流程随后报错，reducer 按 seq 决定是否展示
                _ => return None,
            }
        }
    }
}
