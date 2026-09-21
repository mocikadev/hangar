use crate::worker::{Ev, Paste};
use hangar_core::account::Account;
use hangar_core::quota::Quota;
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// 本二进制版本（gui 包内求值；发版与 cli 一起 bump，见 release 流程）
const GUI_VERSION: &str = env!("CARGO_PKG_VERSION");

/// GUI 下载页（安装包获取地址）
const RELEASES_URL: &str = "https://github.com/mocikadev/hangar/releases/latest";

#[derive(Clone)]
pub enum Dialog {
    Add { url: String, error: String },
    ConfirmDelete { email: String },
    Notice(String),
    About,
    Doctor(Vec<String>),
    UpdateAvailable { version: String },
}

pub struct App {
    pub accounts: Vec<Account>,
    pub current: Option<String>,
    pub selected: Option<String>,
    pub quotas: HashMap<String, Quota>,
    pub quota_now: i64,
    pub status: String,
    pub busy: bool,
    pub dialog: Option<Dialog>,
    pub paste_input: String,
    pub paste_tx: Option<Sender<Paste>>,
    pub login_seq: u64,
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
            dialog: None,
            paste_input: String::new(),
            paste_tx: None,
            login_seq: 0,
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
            Ev::SwitchDone { email, res } => {
                self.busy = false;
                match res {
                    Ok(()) => {
                        self.reload();
                        self.status = format!("✅ 已切换到 {}", email);
                        self.maybe_codex_notice();
                    }
                    Err(e) => {
                        self.status = format!("✗ {}", e);
                    }
                }
            }
            Ev::LoginUrl { url } => {
                self.dialog = Some(Dialog::Add {
                    url,
                    error: String::new(),
                });
            }
            Ev::LoginDone { seq, reauth, res } => {
                // 过时轮次（取消后姗姗来迟）直接丢弃
                if seq != self.login_seq {
                    return;
                }
                self.busy = false;
                self.paste_tx = None;
                match res {
                    Ok(email) => {
                        self.dialog = None;
                        self.reload();
                        self.status = if reauth {
                            format!("✅ 已复活并切换到 {}", email)
                        } else {
                            format!("✅ 已添加账号 {}（未激活，选中后点「切换」）", email)
                        };
                        self.maybe_codex_notice();
                    }
                    Err(e) => {
                        self.dialog = None;
                        self.status = format!("✗ {}", e);
                    }
                }
            }
            Ev::LoginFailed(e) => {
                // 粘贴预检失败：记错但不杀对话框不杀线程
                self.status = format!("✗ {}", e);
                if let Some(Dialog::Add { error, .. }) = self.dialog.as_mut() {
                    *error = e;
                }
            }
            Ev::UpdateDone { res } => {
                self.busy = false;
                match res {
                    Ok(Some(v)) => {
                        self.dialog = Some(Dialog::UpdateAvailable { version: v });
                    }
                    Ok(None) => {
                        self.status = "已是最新版本".to_string();
                    }
                    Err(e) => {
                        self.status = format!("✗ 更新失败：{}（旧版继续可用）", e);
                    }
                }
            }
        }
    }

    /// 手动检查更新（工具栏/关于共用）：后台跑，busy 转圈
    pub fn start_update(&mut self) {
        self.busy = true;
        self.status = "检查更新中…".to_string();
        crate::worker::spawn_update(self.tx.clone(), GUI_VERSION.to_string());
    }

    fn open_doctor(&mut self) {
        match hangar_core::doctor::doctor_lines(GUI_VERSION) {
            Ok((lines, _)) => {
                self.dialog = Some(Dialog::Doctor(lines));
            }
            Err(e) => {
                self.status = format!("✗ {}", e);
            }
        }
    }

    /// 使用中账号禁止删除（UI + 底层双层拦截）
    pub fn can_delete(&self, id: &str) -> bool {
        Some(id) != self.current.as_deref()
    }

    fn maybe_codex_notice(&mut self) {
        if hangar_core::process::codex_process_running() {
            self.dialog = Some(Dialog::Notice(
                "检测到 Codex 正在运行，请重启 Codex 生效".to_string(),
            ));
        }
    }

    pub fn do_switch(&mut self) {
        let sel = self.selected.clone();
        let cur = self.current.clone();
        let Some(id) = sel else { return };
        if Some(id.as_str()) == cur.as_deref() {
            self.status = "已是使用中的账号，无需切换".to_string();
            return;
        }
        if self
            .accounts
            .iter()
            .find(|a| a.id == id)
            .is_some_and(|a| a.stale)
        {
            self.status = "该账号已失效，请先复活".to_string();
            return;
        }
        self.busy = true;
        self.status = "切换中…".to_string();
        crate::worker::spawn_switch(self.tx.clone(), id);
    }

    /// 发起登录：`reauth` 为 Some(目标 id, 邮箱) 时复活，否则添加。
    /// 对话框等 LoginUrl 到达再弹；seq 发号用于丢弃过时事件。
    pub fn start_login(&mut self, reauth: Option<(String, String)>) {
        self.login_seq += 1;
        let seq = self.login_seq;
        let (ptx, prx) = std::sync::mpsc::channel();
        self.paste_tx = Some(ptx);
        self.paste_input.clear();
        self.dialog = None;
        self.busy = true;
        self.status = "等待浏览器授权…".to_string();
        crate::worker::spawn_login(self.tx.clone(), seq, prx, reauth);
    }

    pub fn cancel_login(&mut self) {
        if let Some(tx) = self.paste_tx.take() {
            let _ = tx.send(Paste::Cancel);
        }
        // 发号+1：该轮后续事件（超时报错等）一律丢弃，状态停在"已取消"
        self.login_seq += 1;
        self.dialog = None;
        self.busy = false;
        self.status = "已取消".to_string();
    }

    fn ask_delete(&mut self) {
        let Some(id) = self.selected.clone() else {
            self.status = "先在左侧选中账号".to_string();
            return;
        };
        if !self.can_delete(&id) {
            self.status = "使用中的账号无法删除，请先切换到其他账号".to_string();
            return;
        }
        let email = self
            .accounts
            .iter()
            .find(|a| a.id == id)
            .map(|a| a.email.clone())
            .unwrap_or_default();
        self.dialog = Some(Dialog::ConfirmDelete { email });
    }

    fn confirm_delete(&mut self) {
        let Some(id) = self.selected.clone() else {
            return;
        };
        match hangar_core::account::delete_account(&id) {
            Ok(_) => {
                self.reload();
                self.status = "✅ 已删除".to_string();
            }
            Err(e) => {
                self.status = format!("✗ {}", e);
            }
        }
        self.dialog = None;
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

    fn render_dialogs(&mut self, ctx: &egui::Context) {
        let dlg = self.dialog.clone();
        match dlg {
            None => {}
            Some(Dialog::Notice(msg)) => {
                let mut open = true;
                let mut close = false;
                egui::Window::new("提示").open(&mut open).show(ctx, |ui| {
                    ui.label(&msg);
                    if ui.button("确定").clicked() {
                        close = true;
                    }
                });
                open = open && !close;
                if !open {
                    self.dialog = None;
                }
            }
            Some(Dialog::ConfirmDelete { email }) => {
                let mut open = true;
                let mut close = false;
                let mut yes = false;
                egui::Window::new("删除账号")
                    .open(&mut open)
                    .show(ctx, |ui| {
                        ui.label(format!("确认删除 {}？", email));
                        ui.horizontal(|ui| {
                            if ui.button("删除").clicked() {
                                yes = true;
                            }
                            if ui.button("取消").clicked() {
                                close = true;
                            }
                        });
                    });
                open = open && !close;
                if yes {
                    self.confirm_delete();
                } else if !open {
                    self.dialog = None;
                }
            }
            Some(Dialog::Add { url, error }) => {
                let mut open = true;
                let mut close = false;
                let mut confirm = false;
                egui::Window::new("添加账号")
                    .open(&mut open)
                    .show(ctx, |ui| {
                        ui.label("已打开浏览器完成登录；若跳转被拦截，复制下址到浏览器打开：");
                        ui.horizontal(|ui| {
                            ui.label(&url);
                            if ui.button("复制链接").clicked() {
                                ui.ctx().copy_text(url.clone());
                            }
                        });
                        ui.separator();
                        ui.label("把浏览器地址栏的完整回调 URL 粘贴到下面：");
                        ui.text_edit_singleline(&mut self.paste_input);
                        if !error.is_empty() {
                            ui.colored_label(egui::Color32::RED, &error);
                        }
                        ui.horizontal(|ui| {
                            if ui.button("确认").clicked() {
                                confirm = true;
                            }
                            if ui.button("取消").clicked() {
                                close = true;
                            }
                        });
                    });
                if confirm {
                    let input = std::mem::take(&mut self.paste_input);
                    if let Some(tx) = self.paste_tx.clone() {
                        let _ = tx.send(Paste::Text(input));
                    }
                }
                open = open && !close;
                if !open {
                    self.cancel_login();
                }
            }
            Some(Dialog::About) => {
                let mut open = true;
                let mut close = false;
                let mut update = false;
                egui::Window::new("关于").open(&mut open).show(ctx, |ui| {
                    ui.heading("hangar");
                    ui.label(format!("版本 {}", GUI_VERSION));
                    ui.label("Codex 多账号管理");
                    ui.horizontal(|ui| {
                        if ui.button("检查更新").clicked() {
                            update = true;
                        }
                        if ui.button("关闭").clicked() {
                            close = true;
                        }
                    });
                });
                open = open && !close;
                if update {
                    self.dialog = None;
                    self.start_update();
                } else if !open {
                    self.dialog = None;
                }
            }
            Some(Dialog::Doctor(lines)) => {
                let mut open = true;
                let mut close = false;
                egui::Window::new("自检").open(&mut open).show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("doctor")
                        .max_height(400.0)
                        .show(ui, |ui| {
                            for l in &lines {
                                ui.label(l);
                            }
                        });
                    if ui.button("关闭").clicked() {
                        close = true;
                    }
                });
                open = open && !close;
                if !open {
                    self.dialog = None;
                }
            }
            Some(Dialog::UpdateAvailable { version }) => {
                let mut open = true;
                let mut close = false;
                egui::Window::new("发现新版本")
                    .open(&mut open)
                    .show(ctx, |ui| {
                        ui.label(format!("新版本 {} 可下载，请安装新版安装包升级。", version));
                        ui.horizontal(|ui| {
                            ui.label(RELEASES_URL);
                            if ui.button("复制链接").clicked() {
                                ui.ctx().copy_text(RELEASES_URL.to_string());
                            }
                        });
                        if ui.button("关闭").clicked() {
                            close = true;
                        }
                    });
                open = open && !close;
                if !open {
                    self.dialog = None;
                }
            }
        }
    }

    fn render_list(&mut self, ui: &mut egui::Ui) {
        ui.heading(format!("账号（{}）", self.accounts.len()));
        ui.add_space(4.0);
        if self.accounts.is_empty() {
            ui.weak("（空）点右上「添加」入库");
            return;
        }
        let current = self.current.clone();
        let selected = self.selected.clone();
        let mut pick: Option<String> = None;
        let mut switch_now = false;
        for a in &self.accounts {
            let is_current = Some(a.id.as_str()) == current.as_deref();
            let sel = Some(a.id.as_str()) == selected.as_deref();
            // 行内附配额摘要（有数据才附，无数据不刷"未知"保整洁）
            let summary = quota_summary(self, &a.id);
            let frame_fill = if sel {
                ui.visuals().selection.bg_fill
            } else if is_current {
                ui.visuals().faint_bg_color
            } else {
                ui.visuals().panel_fill
            };
            let resp = egui::Frame::new()
                .fill(frame_fill)
                .inner_margin(egui::Margin::symmetric(10, 8))
                .corner_radius(6)
                .stroke(if sel {
                    ui.visuals().selection.stroke
                } else {
                    egui::Stroke::NONE
                })
                .show(ui, |ui| {
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.strong(&a.email);
                            if is_current {
                                ui.colored_label(
                                    egui::Color32::from_rgb(0x4c, 0xc9, 0x6b),
                                    "● 使用中",
                                );
                            }
                            if a.stale {
                                ui.colored_label(
                                    egui::Color32::from_rgb(0xe5, 0xa5, 0x3d),
                                    "⚠ 失效",
                                );
                            }
                        });
                        if summary != "未知" {
                            ui.weak(&summary);
                        }
                    });
                })
                .response
                .interact(egui::Sense::click());
            if resp.clicked() {
                pick = Some(a.id.clone());
            }
            // 双击直接切换（与工具栏按钮同动作）
            if resp.double_clicked() {
                pick = Some(a.id.clone());
                switch_now = true;
            }
            ui.add_space(4.0);
        }
        if let Some(id) = pick {
            self.selected = Some(id);
            if switch_now {
                self.do_switch();
            }
        }
    }

    fn render_detail(&self, ui: &mut egui::Ui) {
        let Some(a) = self.selected_account().cloned() else {
            ui.weak("← 左侧选择账号查看详情");
            return;
        };
        let is_current = Some(a.id.as_str()) == self.current.as_deref();
        ui.heading(&a.email);
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            if is_current {
                ui.colored_label(egui::Color32::from_rgb(0x4c, 0xc9, 0x6b), "● 使用中");
            }
            if a.stale {
                ui.colored_label(egui::Color32::from_rgb(0xe5, 0xa5, 0x3d), "⚠ 需重新登录");
            }
            if !is_current && !a.stale {
                ui.weak("正常");
            }
        });
        ui.add_space(8.0);
        egui::Frame::new()
            .fill(ui.visuals().faint_bg_color)
            .inner_margin(egui::Margin::same(10))
            .corner_radius(6)
            .show(ui, |ui| {
                ui.label(format!(
                    "刷新令牌：{}",
                    if a.refresh_token.trim().is_empty() {
                        "缺失（无法自动续期）"
                    } else {
                        "有（可自动续期）"
                    }
                ));
            });
        ui.add_space(12.0);
        ui.heading("配额");
        match self.quotas.get(&a.id).cloned() {
            None => {
                ui.weak("尚未查询，点「刷新配额」获取");
            }
            Some(q) if q.windows.is_empty() => {
                ui.weak("无可用窗口");
            }
            Some(q) => {
                if let Some(plan) = &q.plan {
                    ui.weak(format!("套餐：{}", plan));
                }
                for w in &q.windows {
                    let p = w.window.remaining.unwrap_or(0).clamp(0, 100) as f32 / 100.0;
                    let pct = w
                        .window
                        .remaining
                        .map(|v| format!("{}%", v))
                        .unwrap_or_else(|| "未知".to_string());
                    ui.horizontal(|ui| {
                        ui.label(&w.label);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.strong(pct);
                        });
                    });
                    ui.add(
                        egui::ProgressBar::new(p)
                            .desired_height(8.0)
                            .corner_radius(4),
                    );
                    if let Some(reset) = w.window.reset_at {
                        ui.weak(format!(
                            "重置于 {}",
                            hangar_core::quota::fmt_ts_local(reset)
                        ));
                    }
                    ui.add_space(6.0);
                }
                if let Some(n) = q.reset_available {
                    ui.weak(format!("重置卡：{} 张", n));
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

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll();
        // 底部状态栏固定在窗口底部，主区占满剩余高度
        egui::containers::panel::Panel::bottom("status_bar").show(ui, |ui| {
            ui.add_space(4.0);
            ui.label(if self.status.is_empty() {
                "就绪"
            } else {
                &self.status
            });
            ui.add_space(2.0);
        });
        egui::containers::panel::Panel::top("toolbar").show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.heading("hangar");
                ui.weak("Codex 多账号管理");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(!self.busy, egui::Button::new("关于"))
                        .clicked()
                    {
                        self.dialog = Some(Dialog::About);
                    }
                    if ui
                        .add_enabled(!self.busy, egui::Button::new("检查更新"))
                        .clicked()
                    {
                        self.start_update();
                    }
                    if ui
                        .add_enabled(!self.busy, egui::Button::new("自检"))
                        .clicked()
                    {
                        self.open_doctor();
                    }
                    if ui
                        .add_enabled(!self.busy, egui::Button::new("删除"))
                        .clicked()
                    {
                        self.ask_delete();
                    }
                    // 复活仅对选中的失效账号有意义
                    let reauth_target = self.selected.clone().and_then(|id| {
                        self.accounts
                            .iter()
                            .find(|a| a.id == id && a.stale)
                            .map(|a| (a.id.clone(), a.email.clone()))
                    });
                    if ui
                        .add_enabled(
                            !self.busy && reauth_target.is_some(),
                            egui::Button::new("复活"),
                        )
                        .clicked()
                    {
                        if let Some(t) = reauth_target {
                            self.start_login(Some(t));
                        }
                    }
                    if ui
                        .add_enabled(!self.busy, egui::Button::new("添加"))
                        .clicked()
                    {
                        self.start_login(None);
                    }
                    if ui
                        .add_enabled(!self.busy, egui::Button::new("切换"))
                        .clicked()
                    {
                        self.do_switch();
                    }
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
            });
            ui.add_space(2.0);
        });
        egui::containers::panel::Panel::left("sidebar")
            .default_size(340.0)
            .resizable(true)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("accounts")
                    .auto_shrink(false)
                    .show(ui, |ui| self.render_list(ui));
            });
        egui::containers::panel::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("detail")
                .auto_shrink(false)
                .show(ui, |ui| self.render_detail(ui));
        });

        if self.busy {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }

        let ctx = ui.ctx().clone();
        self.render_dialogs(&ctx);
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

    #[test]
    fn delete_current_account_is_blocked_in_gui() {
        let app = App {
            current: Some("id-1".into()),
            ..Default::default()
        };
        assert!(!app.can_delete("id-1"));
        assert!(app.can_delete("id-2"));
    }

    #[test]
    fn login_dialog_survives_state_mismatch() {
        // state 不匹配只记错，不杀对话框：reducer 收到 LoginFailed 仍保持 dialog=Add
        let mut app = App {
            dialog: Some(Dialog::Add {
                url: "http://x".into(),
                error: String::new(),
            }),
            ..Default::default()
        };
        app.reduce(Ev::LoginFailed("state 不匹配".into()));
        assert!(matches!(app.dialog, Some(Dialog::Add { .. })));
        assert!(app.status.contains("state 不匹配"));
    }

    #[test]
    fn no_dialog_when_already_latest() {
        let mut app = App::default();
        app.reduce(Ev::UpdateDone { res: Ok(None) });
        assert!(app.dialog.is_none());
        assert_eq!(app.status, "已是最新版本");
    }

    #[test]
    fn update_failure_keeps_old_version_usable() {
        let mut app = App::default();
        app.reduce(Ev::UpdateDone {
            res: Err("断网".into()),
        });
        assert!(app.dialog.is_none());
        assert!(app.status.contains("旧版继续可用"));
    }

    #[test]
    fn newer_version_opens_download_dialog() {
        // GUI 只发安装包：有新版弹下载指引，不自动替换
        let mut app = App::default();
        app.reduce(Ev::UpdateDone {
            res: Ok(Some("0.4.0".into())),
        });
        assert!(matches!(app.dialog, Some(Dialog::UpdateAvailable { .. })));
    }
}
