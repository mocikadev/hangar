mod app;
mod fonts;
mod hooks;
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
    let opts = eframe::NativeOptions::default();
    eframe::run_native(
        "hangar",
        opts,
        Box::new(move |cc| {
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
