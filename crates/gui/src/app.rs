use crate::worker::Ev;
use hangar_core::account::Account;
use hangar_core::quota::Quota;
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub struct App {
    pub accounts: Vec<Account>,
    pub current: Option<String>,
    pub selected: Option<String>,
    pub quotas: HashMap<String, Quota>,
    pub quota_now: i64,
    pub status: String,
    pub busy: bool,
    pub tx: Sender<Ev>,
    pub rx: Receiver<Ev>,
}

impl App {
    pub fn new(tx: Sender<Ev>, rx: Receiver<Ev>) -> Self {
        Self {
            accounts: vec![],
            current: None,
            selected: None,
            quotas: HashMap::new(),
            quota_now: now_secs(),
            status: String::new(),
            busy: false,
            tx,
            rx,
        }
    }

    /// 从账号库重载列表（harvest 后调用），选中保持有效否则落到首个
    pub fn reload(&mut self) {
        if let Ok(f) = hangar_core::account::load_accounts() {
            self.current = f.current_account_id;
            self.accounts = f.accounts;
        }
        let alive = self
            .selected
            .as_deref()
            .map(|s| self.accounts.iter().any(|a| a.id == s))
            .unwrap_or(false);
        if !alive {
            self.selected = self.accounts.first().map(|a| a.id.clone());
        }
    }

    pub fn pending_quota_ids(&self) -> Vec<String> {
        self.accounts
            .iter()
            .filter(|a| !a.stale)
            .map(|a| a.id.clone())
            .collect()
    }

    /// 纯收敛：事件 → 状态（单测覆盖，不做 IO）
    pub fn reduce(&mut self, ev: Ev) {
        match ev {
            Ev::HarvestDone => {
                self.reload();
                self.status = "已收敛".to_string();
            }
            Ev::QuotaOne { id, res } => match res {
                Ok(q) => {
                    self.quotas.insert(id, q);
                }
                Err(e) => {
                    self.status = format!("配额失败：{}", e);
                }
            },
            Ev::QuotaDone => {
                self.busy = false;
                self.quota_now = now_secs();
                self.status = "配额已更新".to_string();
            }
        }
    }

    fn poll(&mut self) {
        while let Ok(ev) = self.rx.try_recv() {
            self.reduce(ev);
        }
    }

    fn selected_account(&self) -> Option<&Account> {
        self.selected
            .as_deref()
            .and_then(|s| self.accounts.iter().find(|a| a.id == s))
    }

    fn render_list(&mut self, ui: &mut egui::Ui) {
        ui.heading(format!("账号 ({})", self.accounts.len()));
        if self.accounts.is_empty() {
            ui.label("（空）按 添加 入库");
            return;
        }
        let current = self.current.clone();
        let mut pick: Option<String> = None;
        for a in &self.accounts {
            // 行内附配额摘要（有数据才附，无数据不刷"未知"保整洁）
            let summary = quota_summary(self, &a.id);
            let extra = if summary == "未知" {
                String::new()
            } else {
                format!(" · {}", summary)
            };
            let label = format!(
                "{}{}{}",
                a.email,
                account_badges(a, current.as_deref()),
                extra
            );
            let sel = Some(a.id.as_str()) == self.selected.as_deref();
            if ui.selectable_label(sel, label).clicked() {
                pick = Some(a.id.clone());
            }
        }
        if let Some(id) = pick {
            self.selected = Some(id);
        }
    }

    fn render_detail(&self, ui: &mut egui::Ui) {
        let Some(a) = self.selected_account().cloned() else {
            ui.label("左侧选择账号");
            return;
        };
        ui.heading(&a.email);
        ui.label(format!(
            "状态：{}",
            if a.stale {
                "⚠ 需重新登录"
            } else {
                "正常"
            }
        ));
        ui.label(format!(
            "刷新令牌：{}",
            if a.refresh_token.trim().is_empty() {
                "缺失（无法自动续期）"
            } else {
                "有（可自动续期）"
            }
        ));
        ui.separator();
        ui.heading("配额");
        match self.quotas.get(&a.id).cloned() {
            None => {
                ui.label("尚未查询，按 刷新配额");
            }
            Some(q) if q.windows.is_empty() => {
                ui.label("无可用窗口");
            }
            Some(q) => {
                for w in &q.windows {
                    let p = w.window.remaining.unwrap_or(0).clamp(0, 100) as f32 / 100.0;
                    let pct = w
                        .window
                        .remaining
                        .map(|v| format!("{}%", v))
                        .unwrap_or_else(|| "未知".to_string());
                    ui.label(format!("{} {}", w.label, pct));
                    ui.add(egui::ProgressBar::new(p));
                }
            }
        }
    }
}

impl Default for App {
    fn default() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        Self::new(tx, rx)
    }
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// 配额摘要行：无数据/无窗口显示"未知"（非公开契约容错）
pub fn quota_summary(app: &App, id: &str) -> String {
    match app.quotas.get(id) {
        None => "未知".to_string(),
        Some(q) if q.windows.is_empty() => "未知".to_string(),
        Some(q) => q
            .windows
            .iter()
            .map(|w| {
                let pct = w
                    .window
                    .remaining
                    .map(|p| format!("{}%", p))
                    .unwrap_or_else(|| "未知".to_string());
                format!("{} {}", w.label, pct)
            })
            .collect::<Vec<_>>()
            .join(" · "),
    }
}

fn account_badges(acc: &Account, current: Option<&str>) -> String {
    let mut s = String::new();
    if Some(acc.id.as_str()) == current {
        s.push_str(" ●使用中");
    }
    if acc.stale {
        s.push_str(" ⚠失效");
    }
    s
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll();
        ui.horizontal(|ui| {
            ui.heading("hangar");
            ui.separator();
            let refresh = ui.add_enabled(!self.busy, egui::Button::new("刷新配额"));
            if refresh.clicked() {
                let ids = self.pending_quota_ids();
                if !ids.is_empty() {
                    self.busy = true;
                    self.status = "查询配额中…".to_string();
                    crate::worker::spawn_quota(self.tx.clone(), ids);
                }
            }
            if self.busy {
                ui.spinner();
            }
        });
        ui.separator();
        // 主区：左右分栏，各自滚动（面板已移除时代的手动布局）
        let main_h = (ui.available_height() - 30.0).max(120.0);
        ui.columns(2, |uis| {
            egui::ScrollArea::vertical()
                .id_salt("accounts")
                .max_height(main_h)
                .show(&mut uis[0], |ui| self.render_list(ui));
            egui::ScrollArea::vertical()
                .id_salt("detail")
                .max_height(main_h)
                .show(&mut uis[1], |ui| self.render_detail(ui));
        });
        ui.separator();
        ui.label(if self.status.is_empty() {
            "就绪"
        } else {
            &self.status
        });

        if self.busy {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worker::Ev;

    #[test]
    fn reducer_quota_error_never_panics() {
        let mut app = App::default();
        app.reduce(Ev::QuotaOne {
            id: "x".into(),
            res: Err("断网".into()),
        });
        assert!(app.status.contains("断网"));
        assert!(!app.quotas.contains_key("x"));
    }

    #[test]
    fn reducer_empty_windows_renders_unknown() {
        let mut app = App::default();
        app.reduce(Ev::QuotaOne {
            id: "x".into(),
            res: Ok(hangar_core::quota::Quota::default()),
        });
        assert_eq!(quota_summary(&app, "x"), "未知");
    }
}
