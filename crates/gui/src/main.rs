mod app;

fn main() -> eframe::Result<()> {
    let opts = eframe::NativeOptions::default();
    eframe::run_native(
        "hangar",
        opts,
        Box::new(|_cc| Ok(Box::new(app::App::default()))),
    )
}
