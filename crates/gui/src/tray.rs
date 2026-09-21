//! 系统托盘：Linux 走 ksni（SNI/DBus，纯 Rust）；Win/macOS 走 tray-icon。
//!
//! facade：`TrayHandle` 是 App 唯一依赖的类型，App 代码不含 cfg。
//! 托盘点击不直接干活，全部经 `TrayCmd` 通道送回 UI 线程（与 worker::Ev 同路）。
//!
//! 平台差异：
//! - Linux：ksni 服务自带线程，`TrayHandle::spawn` 随时可调。
//! - Win/mac：tray-icon 必须在跑着事件循环的主线程上创建（tray-icon README），
//!   所以 `spawn` 只存通道，真正的图标在 `build_on_main_thread`（eframe 创建回调里）落地；
//!   菜单事件由全局 receiver 的转发线程送进命令通道。

use std::sync::mpsc::Sender;

/// 托盘 → UI 线程的用户动作
#[derive(Debug, Clone)]
pub enum TrayCmd {
    /// 切换到指定账号 id
    Switch(String),
    /// 显示主窗口
    ShowWindow,
    /// 退出程序（含托盘 shutdown）
    Quit,
}

/// 托盘对 UI 侧暴露的最小接口
pub struct TrayHandle {
    inner: Option<PlatformTray>,
}

impl TrayHandle {
    /// Linux：立即启动托盘服务；Win/mac：仅记录通道，图标延后到主线程创建。
    /// 失败只打日志返回 None，主窗口照常工作。
    ///
    /// `ctx` 用于托盘点击后 `request_repaint` 唤醒 egui 事件循环——
    /// egui 按需重绘，托盘点击本身不产生窗口事件，不唤醒则命令永远无人轮询。
    pub fn spawn(cmd_tx: Sender<TrayCmd>, ctx: egui::Context) -> Option<Self> {
        #[cfg(target_os = "linux")]
        {
            let tray = linux::HangarTray {
                accounts: vec![],
                cmd_tx,
                wake: ctx,
                icon: icon_argb32(),
            };
            match ksni::blocking::TrayMethods::spawn(tray) {
                Ok(handle) => Some(Self {
                    inner: Some(PlatformTray::Ksni(handle)),
                }),
                Err(e) => {
                    eprintln!("托盘启动失败（主窗口不受影响）: {}", e);
                    None
                }
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = ctx;
            Some(Self {
                inner: Some(PlatformTray::Deferred {
                    cmd_tx,
                    built: false,
                }),
            })
        }
    }

    /// Win/mac 专用：在主线程事件循环内创建托盘（eframe 创建回调中调用）。
    /// Linux 上 ksni 托盘已在 spawn 时启动，空操作。
    #[cfg(target_os = "linux")]
    pub fn build_on_main_thread(&mut self, _wake: egui::Context) {}

    /// Win/mac 专用：在主线程事件循环内创建托盘（eframe 创建回调中调用）。
    /// Linux 上是空操作。
    #[cfg(not(target_os = "linux"))]
    pub fn build_on_main_thread(&mut self, wake: egui::Context) {
        if let Some(PlatformTray::Deferred { cmd_tx, built }) = &mut self.inner {
            if *built {
                return;
            }
            *built = true;
            match winmac::build(cmd_tx.clone(), wake) {
                Ok(t) => self.inner = Some(PlatformTray::TrayIcon(t)),
                Err(e) => eprintln!("托盘创建失败（主窗口不受影响）: {}", e),
            }
        }
    }

    /// 用账号库最新状态刷新托盘菜单（激活账号打勾）
    #[cfg(target_os = "linux")]
    pub fn update_accounts(&self, items: &[(String, String, bool)]) {
        if let Some(PlatformTray::Ksni(h)) = &self.inner {
            let items = items.to_vec();
            let _ = h.update(move |t| {
                t.accounts = items;
            });
        }
    }

    /// 用账号库最新状态刷新托盘菜单（激活账号打勾）
    #[cfg(not(target_os = "linux"))]
    pub fn update_accounts(&self, items: &[(String, String, bool)]) {
        match &self.inner {
            Some(PlatformTray::TrayIcon(t)) => winmac::update_menu(t, items),
            Some(PlatformTray::Deferred { .. }) => {
                let _ = items; // 托盘尚未在主线程创建，菜单状态随 build 重建
            }
            None => {}
        }
    }

    /// UI 侧主动退出时关停托盘服务
    pub fn shutdown(&self) {
        #[cfg(target_os = "linux")]
        if let Some(PlatformTray::Ksni(h)) = &self.inner {
            h.shutdown().wait();
        }
    }
}

enum PlatformTray {
    #[cfg(target_os = "linux")]
    Ksni(ksni::blocking::Handle<linux::HangarTray>),
    #[cfg(not(target_os = "linux"))]
    TrayIcon(tray_icon::TrayIcon),
    /// Win/mac：等待主线程事件循环就绪后再创建
    #[cfg(not(target_os = "linux"))]
    Deferred {
        cmd_tx: Sender<TrayCmd>,
        built: bool,
    },
}

// ---------- Linux：ksni ----------

#[cfg(target_os = "linux")]
mod linux {
    use super::TrayCmd;
    use std::sync::mpsc::Sender;

    /// ksni Tray 实现：菜单即账号列表
    pub struct HangarTray {
        /// (账号 id, email, 是否激活)
        pub accounts: Vec<(String, String, bool)>,
        pub cmd_tx: Sender<TrayCmd>,
        /// egui 上下文：命令入队后唤醒重绘循环（egui 按需重绘，不唤醒无人轮询）
        pub wake: egui::Context,
        /// ARGB32 原始像素（128×128）
        pub icon: Vec<u8>,
    }

    impl ksni::Tray for HangarTray {
        fn id(&self) -> String {
            "hangar".into()
        }

        fn icon_pixmap(&self) -> Vec<ksni::Icon> {
            vec![ksni::Icon {
                width: 128,
                height: 128,
                data: self.icon.clone(),
            }]
        }

        fn title(&self) -> String {
            "hangar".into()
        }

        fn tool_tip(&self) -> ksni::ToolTip {
            ksni::ToolTip {
                title: "hangar".into(),
                description: self
                    .accounts
                    .iter()
                    .find(|(_, _, active)| *active)
                    .map(|(_, email, _)| email.clone())
                    .unwrap_or_default(),
                ..Default::default()
            }
        }

        fn menu(&self) -> Vec<ksni::menu::MenuItem<Self>> {
            let mut menu: Vec<ksni::menu::MenuItem<Self>> = self
                .accounts
                .iter()
                .map(|(id, email, active)| {
                    let id = id.clone();
                    ksni::menu::CheckmarkItem {
                        label: email.clone(),
                        checked: *active,
                        activate: Box::new(move |t: &mut Self| {
                            // 无阻塞操作：发命令即返回（StandardItem 文档要求 handler 轻量）
                            let _ = t.cmd_tx.send(TrayCmd::Switch(id.clone()));
                            t.wake.request_repaint();
                        }),
                        ..Default::default()
                    }
                    .into()
                })
                .collect();
            if !menu.is_empty() {
                menu.push(ksni::menu::MenuItem::Separator);
            }
            menu.push(
                ksni::menu::StandardItem {
                    label: "显示主窗口".into(),
                    activate: Box::new(|t: &mut Self| {
                        let _ = t.cmd_tx.send(TrayCmd::ShowWindow);
                        t.wake.request_repaint();
                    }),
                    ..Default::default()
                }
                .into(),
            );
            menu.push(
                ksni::menu::StandardItem {
                    label: "退出".into(),
                    activate: Box::new(|t: &mut Self| {
                        let _ = t.cmd_tx.send(TrayCmd::Quit);
                        t.wake.request_repaint();
                    }),
                    ..Default::default()
                }
                .into(),
            );
            menu
        }
    }
}

// ---------- Win/macOS：tray-icon ----------

#[cfg(not(target_os = "linux"))]
mod winmac {
    use super::{icon_rgba, TrayCmd};
    use std::sync::mpsc::Sender;
    use tray_icon::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
    use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

    const ID_SHOW: &str = "show";
    const ID_QUIT: &str = "quit";
    /// 账号切换项 id 前缀（后接账号 id）
    const ACT_PREFIX: &str = "act:";

    /// 主线程事件循环内创建托盘。
    /// 点击/菜单事件经 set_event_handler 直推进命令通道（全局 receiver 轮询
    /// 线程在 winit 事件循环被阻塞时同样收不到分发，handler 更可靠）。
    pub fn build(
        cmd_tx: Sender<TrayCmd>,
        wake: egui::Context,
    ) -> Result<TrayIcon, Box<dyn std::error::Error>> {
        let icon = tray_icon::Icon::from_rgba(icon_rgba(), 128, 128)?;
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(build_menu(&[])))
            .with_tooltip("hangar")
            .with_icon(icon)
            .with_menu_on_left_click(false)
            .build()?;

        // 菜单事件（含账号切换/显示/退出）→ 命令通道 + 唤醒重绘
        let menu_tx = cmd_tx.clone();
        let menu_wake = wake.clone();
        tray_icon::menu::MenuEvent::set_event_handler(Some(
            move |ev: tray_icon::menu::MenuEvent| {
                let id = ev.id().0.clone();
                let cmd = match id.as_str() {
                    ID_SHOW => Some(TrayCmd::ShowWindow),
                    ID_QUIT => Some(TrayCmd::Quit),
                    other => other
                        .strip_prefix(ACT_PREFIX)
                        .map(|a| TrayCmd::Switch(a.to_string())),
                };
                if let Some(cmd) = cmd {
                    let _ = menu_tx.send(cmd);
                    menu_wake.request_repaint();
                }
            },
        ));

        // 左键单击图标 → 显示主窗口
        let click_tx = cmd_tx;
        let click_wake = wake;
        TrayIconEvent::set_event_handler(Some(move |ev| {
            if matches!(
                ev,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                let _ = click_tx.send(TrayCmd::ShowWindow);
                click_wake.request_repaint();
            }
        }));
        Ok(tray)
    }

    /// 重建菜单：账号勾选项 + 显示主窗口 + 退出
    pub fn update_menu(tray: &TrayIcon, items: &[(String, String, bool)]) {
        let _ = tray.set_menu(Some(Box::new(build_menu(items))));
    }

    fn build_menu(items: &[(String, String, bool)]) -> Menu {
        let menu = Menu::new();
        for (id, email, active) in items {
            let item =
                CheckMenuItem::with_id(format!("{}{}", ACT_PREFIX, id), email, true, *active, None);
            let _ = menu.append(&item);
        }
        if !items.is_empty() {
            let _ = menu.append(&PredefinedMenuItem::separator());
        }
        let _ = menu.append(&MenuItem::with_id(ID_SHOW, "显示主窗口", true, None));
        let _ = menu.append(&MenuItem::with_id(ID_QUIT, "退出", true, None));
        menu
    }
}

// ---------- 图标资产 ----------

/// 内嵌 128×128 RGBA 资产
fn icon_rgba() -> Vec<u8> {
    include_bytes!("../assets/tray.rgba").to_vec()
}

/// ksni 要 ARGB32（网络字节序）：rgba 每像素字节右旋 1 位
#[cfg(target_os = "linux")]
fn icon_argb32() -> Vec<u8> {
    let mut data = icon_rgba();
    for px in data.as_chunks_mut::<4>().0 {
        px.rotate_right(1);
    }
    data
}
