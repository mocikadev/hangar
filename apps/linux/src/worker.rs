//! One serial worker owns all Core I/O. UI receives only summaries and safe messages.
mod refresh;
use crate::{
    login::{Login, Outcome},
    model::Card,
};
use hangar_core::oauth::OAuthSessionPhase;
use std::collections::HashMap;
use std::sync::{atomic::AtomicBool, mpsc, Arc};
use std::time::Duration;

pub enum Command {
    Refresh {
        generation: u64,
        force: bool,
    },
    Switch {
        generation: u64,
        id: String,
    },
    Delete {
        generation: u64,
        id: String,
    },
    Doctor {
        generation: u64,
    },
    Login {
        generation: u64,
        target: Option<String>,
    },
    Callback(String),
    CancelLogin,
    Shutdown,
}

pub enum Event {
    Committed {
        generation: u64,
        message: String,
        force_refresh: bool,
    },
    RefreshProgress {
        generation: u64,
        completed: usize,
        total: usize,
    },
    Cards {
        generation: u64,
        cards: Vec<Card>,
        terminal: bool,
    },
    Done {
        generation: u64,
        result: Result<String, String>,
    },
    Doctor {
        generation: u64,
        lines: Vec<String>,
        issues: usize,
    },
    LoginStarted {
        generation: u64,
        url: String,
        target_email: Option<String>,
    },
    LoginPhase {
        generation: u64,
        exchanging: bool,
    },
    CallbackRejected,
    Stopped,
}

pub struct Worker {
    pub commands: mpsc::Sender<Command>,
    pub events: mpsc::Receiver<Event>,
    pub cancel: Arc<AtomicBool>,
}

impl Worker {
    pub fn start() -> Self {
        let (commands, receiver) = mpsc::channel();
        let (sender, events) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        std::thread::spawn(move || run(receiver, sender, flag));
        Self {
            commands,
            events,
            cancel,
        }
    }
}

fn run(receiver: mpsc::Receiver<Command>, sender: mpsc::Sender<Event>, cancel: Arc<AtomicBool>) {
    hangar_core::emit::set_quiet(true);
    let mut login: Option<Login> = None;
    let mut attempts = HashMap::new();
    let mut stopping = false;
    let mut last_login_phase = None;
    loop {
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(Command::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                stopping = true;
                if let Some(active) = &login {
                    active.session.cancel();
                }
            }
            Ok(command) if !stopping => {
                dispatch(command, &sender, &cancel, &mut login, &mut attempts);
            }
            _ => {}
        }
        if let Some(active) = &mut login {
            let generation = active.generation;
            if let Some(result) = active.poll() {
                if matches!(result, Ok(Outcome::Saved)) {
                    if let Some(id) = active.target() {
                        attempts.remove(id);
                    }
                }
                refresh::snapshot(&sender, generation, true, &attempts);
                let event = match result {
                    Ok(Outcome::Saved) => Event::Committed {
                        generation,
                        message: "登录已保存，当前账号未改变".into(),
                        force_refresh: true,
                    },
                    Ok(Outcome::Cancelled) => Event::Done {
                        generation,
                        result: Ok("已取消登录".into()),
                    },
                    Err(message) => Event::Done {
                        generation,
                        result: Err(message),
                    },
                };
                let _ = sender.send(event);
                login = None;
                last_login_phase = None;
            } else {
                let exchanging = active.session.phase() == OAuthSessionPhase::Exchanging;
                if last_login_phase != Some((generation, exchanging)) {
                    let _ = sender.send(Event::LoginPhase {
                        generation,
                        exchanging,
                    });
                    last_login_phase = Some((generation, exchanging));
                }
            }
        }
        if stopping && login.is_none() {
            break;
        }
    }
    let _ = sender.send(Event::Stopped);
}

fn dispatch(
    command: Command,
    sender: &mpsc::Sender<Event>,
    cancel: &AtomicBool,
    login: &mut Option<Login>,
    attempts: &mut HashMap<String, refresh::Attempt>,
) {
    // The UI also disables conflicting actions, but enforce serialization here.
    if login.is_some() && !matches!(command, Command::Callback(_) | Command::CancelLogin) {
        return;
    }
    match command {
        Command::Refresh { generation, force } => {
            let result = refresh::run(sender, generation, force, cancel, attempts);
            let _ = sender.send(Event::Done { generation, result });
        }
        Command::Switch { generation, id } => {
            let result = switch(&id);
            refresh::snapshot(sender, generation, true, attempts);
            let event = match result {
                Ok(message) => Event::Committed {
                    generation,
                    message,
                    force_refresh: false,
                },
                Err(message) => Event::Done {
                    generation,
                    result: Err(message),
                },
            };
            let _ = sender.send(event);
        }
        Command::Delete { generation, id } => {
            let result = hangar_core::account::harvest_checked()
                .map_err(|_| "无法收敛官方登录状态，请先运行自检".to_string())
                .and_then(|_| hangar_core::delete_account_checked(&id).map_err(account_error))
                .map(|_| "账号已删除".into());
            refresh::snapshot(sender, generation, true, attempts);
            let _ = sender.send(Event::Done { generation, result });
        }
        Command::Doctor { generation } => {
            match hangar_core::doctor::doctor_lines(env!("CARGO_PKG_VERSION")) {
                Ok((lines, issues)) => {
                    let _ = sender.send(Event::Doctor {
                        generation,
                        lines,
                        issues,
                    });
                }
                Err(_) => {
                    let _ = sender.send(Event::Done {
                        generation,
                        result: Err("无法完成自检，请检查配置目录权限".into()),
                    });
                }
            }
        }
        Command::Login { generation, target } => match Login::start(generation, target) {
            Ok(active) => {
                let _ = sender.send(Event::LoginStarted {
                    generation,
                    url: active.session.authorization_url().into(),
                    target_email: active.target_email.clone(),
                });
                *login = Some(active);
            }
            Err(message) => {
                let _ = sender.send(Event::Done {
                    generation,
                    result: Err(message),
                });
            }
        },
        Command::Callback(url) => {
            if let Some(active) = login {
                if active.session.submit_callback(&url).is_err() {
                    let _ = sender.send(Event::CallbackRejected);
                }
            }
        }
        Command::CancelLogin => {
            if let Some(active) = login {
                active.session.cancel();
            }
        }
        Command::Shutdown => {}
    }
}

fn switch(id: &str) -> Result<String, String> {
    hangar_core::account::harvest_checked()
        .map_err(|_| "无法收敛官方登录状态，请先运行自检".to_string())?;
    let file = hangar_core::load_accounts().map_err(|_| "无法读取账号库".to_string())?;
    if file.current_account_id.as_deref() == Some(id) {
        return Err("该账号正在使用中".into());
    }
    let running = hangar_core::codex_process_running();
    hangar_core::switch_account_checked(id).map_err(account_error)?;
    Ok(if running {
        "切换成功，请重启正在运行的 Codex"
    } else {
        "切换成功，下次启动 Codex 时生效"
    }
    .into())
}

fn account_error(error: hangar_core::AccountError) -> String {
    use hangar_core::AccountErrorKind;
    match error.kind() {
        AccountErrorKind::State => "操作被阻止：请检查当前账号及凭据存储模式（仅支持 file）",
        AccountErrorKind::Auth => "凭据不可用，请重新登录该账号",
        AccountErrorKind::External => "外部服务不可用，请稍后重试",
        AccountErrorKind::Internal => "读写失败，请运行自检检查路径、权限与锁",
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;
    #[test]
    fn empty_isolated_worker_refresh_doctor_oauth_cancel_and_shutdown() {
        // This integration test only accepts an explicitly isolated empty HOME.
        let home = std::env::var("HANGAR_TEST_HOME").expect("run scripts/check-linux.sh");
        assert_eq!(std::env::var("HOME").unwrap(), home);
        assert!(std::path::Path::new(&home).starts_with(std::env::temp_dir()));
        assert!(hangar_core::load_accounts().unwrap().accounts.is_empty());
        let worker = Worker::start();
        worker
            .commands
            .send(Command::Refresh {
                generation: 1,
                force: false,
            })
            .unwrap();
        let mut snapshot_seen = false;
        let mut empty_progress_seen = false;
        loop {
            match worker.events.recv_timeout(Duration::from_secs(15)).unwrap() {
                Event::RefreshProgress {
                    generation: 1,
                    completed: 0,
                    total: 0,
                } => {
                    empty_progress_seen = true;
                }
                Event::Cards { cards, .. } => {
                    assert!(cards.is_empty());
                    snapshot_seen = true;
                }
                Event::Done {
                    generation: 1,
                    result,
                } => {
                    assert!(result.is_ok());
                    break;
                }
                _ => {}
            }
        }
        assert!(snapshot_seen);
        assert!(empty_progress_seen);
        worker
            .commands
            .send(Command::Doctor { generation: 2 })
            .unwrap();
        assert!(matches!(
            worker.events.recv_timeout(Duration::from_secs(15)).unwrap(),
            Event::Doctor { generation: 2, .. }
        ));
        worker
            .commands
            .send(Command::Login {
                generation: 3,
                target: None,
            })
            .unwrap();
        match worker.events.recv_timeout(Duration::from_secs(15)).unwrap() {
            Event::LoginStarted { url, .. } => {
                assert!(url.starts_with("https://chatgpt.com/codex/desktop-auth?"))
            }
            _ => panic!("OAuth session did not start; ensure callback ports are available"),
        }
        worker
            .commands
            .send(Command::Callback(
                "http://localhost:1455/auth/callback?state=wrong&code=invalid".into(),
            ))
            .unwrap();
        loop {
            if matches!(
                worker.events.recv_timeout(Duration::from_secs(15)).unwrap(),
                Event::CallbackRejected
            ) {
                break;
            }
        }
        worker.commands.send(Command::CancelLogin).unwrap();
        loop {
            if let Event::Done {
                generation: 3,
                result,
            } = worker.events.recv_timeout(Duration::from_secs(15)).unwrap()
            {
                assert!(result.unwrap().contains("取消"));
                break;
            }
        }
        worker.cancel.store(true, Ordering::Release);
        worker
            .commands
            .send(Command::Refresh {
                generation: 4,
                force: true,
            })
            .unwrap();
        loop {
            if let Event::Done {
                generation: 4,
                result,
            } = worker.events.recv_timeout(Duration::from_secs(15)).unwrap()
            {
                assert!(result.unwrap().contains("停止"));
                break;
            }
        }
        worker.commands.send(Command::Shutdown).unwrap();
        assert!(matches!(
            worker.events.recv_timeout(Duration::from_secs(15)).unwrap(),
            Event::Stopped
        ));
        assert!(!std::path::Path::new(&std::env::var("CODEX_HOME").unwrap())
            .join("auth.json")
            .exists());
    }
}
