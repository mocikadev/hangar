mod app;
mod fonts;
mod hooks;
mod tray;
mod worker;

fn main() -> eframe::Result<()> {
    // 启动收敛 + 列表加载 + 全量配额静默刷（TUI 同款，只读）
    let mut app = app::App::default();
    worker::spawn_harvest(app.tx.clone());
    app.reload();
    let ids = app.pending_quota_ids();
    if !ids.is_empty() {
        app.busy = true;
        worker::spawn_quota(app.tx.clone(), ids);
    }
    // 托盘命令通道先建好；托盘本体在 eframe 创建回调里启动（需要 egui::Context
    // 在点击后 request_repaint 唤醒重绘循环，否则命令无人轮询）
    let (tray_tx, tray_rx) = std::sync::mpsc::channel();
    app.tray_rx = tray_rx;

    // Wayland 下 winit 不支持客户端隐藏窗口（Visible(false) 被忽略），
    // 强制走 XWayland/X11 后端，隐藏/显示行为与 Qt 应用（QQ 等）一致
    #[cfg(target_os = "linux")]
    let opts = {
        use winit::platform::x11::EventLoopBuilderExtX11;
        eframe::NativeOptions {
            event_loop_builder: Some(Box::new(|builder| {
                builder.with_x11();
            })),
            ..Default::default()
        }
    };
    #[cfg(not(target_os = "linux"))]
    let opts = eframe::NativeOptions::default();

    eframe::run_native(
        "hangar",
        opts,
        Box::new(move |cc| {
            app.tray = tray::TrayHandle::spawn(tray_tx, cc.egui_ctx.clone());
            // Win/mac：托盘须在跑着事件循环的主线程上创建（tray-icon README），
            // eframe 创建回调正是该线程；Linux 上此调用为空操作
            if let Some(t) = app.tray.as_mut() {
                t.build_on_main_thread(cc.egui_ctx.clone());
            }
            app.sync_tray();
            // 深色主题 + 现代化控件样式
            cc.egui_ctx.set_theme(egui::Theme::Dark);
            cc.egui_ctx.all_styles_mut(|style| {
                style.visuals = egui::Visuals::dark();
                style.visuals.window_corner_radius = 10.0.into();
                style.visuals.menu_corner_radius = 8.0.into();
                style.visuals.widgets.noninteractive.corner_radius = 6.0.into();
                style.visuals.widgets.inactive.corner_radius = 6.0.into();
                style.visuals.widgets.hovered.corner_radius = 6.0.into();
                style.visuals.widgets.active.corner_radius = 6.0.into();
                style.spacing.item_spacing = egui::vec2(8.0, 6.0);
            });
            match fonts::install_cjk(&cc.egui_ctx) {
                Some(path) => {
                    app.status = format!("就绪（中文字体：{}）", path);
                }
                None => {
                    app.status = "就绪（未找到中文字体，中文可能显示为方框）".to_string();
                }
            }
            Ok(Box::new(app))
        }),
    )
}
