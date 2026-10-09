//! Worker-owned OAuth lifecycle. Credentials never enter a UI event.
use hangar_core::oauth::{OAuthLoginSession, OAuthSessionPhase};
use std::sync::Arc;

pub enum Outcome {
    Saved,
    Cancelled,
}

pub struct Login {
    pub generation: u64,
    pub session: Arc<OAuthLoginSession>,
    target: Option<String>,
    pub target_email: Option<String>,
    finished: bool,
}

impl Login {
    pub fn start(generation: u64, target: Option<String>) -> Result<Self, String> {
        let target_email = if let Some(id) = &target {
            let file = hangar_core::load_accounts().map_err(|_| "无法读取账号库".to_string())?;
            Some(
                file.accounts
                    .iter()
                    .find(|a| &a.id == id)
                    .ok_or_else(|| "账号已不存在，请刷新后重试".to_string())?
                    .email
                    .clone(),
            )
        } else {
            None
        };
        Ok(Self {
            generation,
            session: OAuthLoginSession::start()
                .map_err(|_| "无法启动登录，请检查 1455/1457 端口是否被占用".to_string())?,
            target,
            target_email,
            finished: false,
        })
    }

    pub fn target(&self) -> Option<&str> {
        self.target.as_deref()
    }

    pub fn poll(&mut self) -> Option<Result<Outcome, String>> {
        if self.finished {
            return None;
        }
        let result = match self.session.phase() {
            OAuthSessionPhase::Waiting | OAuthSessionPhase::Exchanging => return None,
            OAuthSessionPhase::Succeeded => self
                .session
                .take_account()
                .ok_or_else(|| "登录结果已被消费".to_string())
                .and_then(|account| match &self.target {
                    Some(id) => hangar_core::reauth_account_checked(id, &account).map_err(|_| {
                        "重新登录未能保存，请运行自检并确认登录的是目标账号".to_string()
                    }),
                    None => hangar_core::store_logged_in_account(account)
                        .map_err(|_| "登录未能保存，请运行自检".to_string()),
                })
                .map(|()| Outcome::Saved),
            OAuthSessionPhase::Cancelled => Ok(Outcome::Cancelled),
            OAuthSessionPhase::TimedOut => Err("登录超时，请重新添加或重新登录".into()),
            OAuthSessionPhase::Failed => Err("登录失败，请重试；未输出认证响应或凭据".into()),
        };
        // Even a failed commit must not consume the same OAuth result again.
        self.finished = true;
        Some(result)
    }
}
