mod app;
mod fonts;
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
