#[derive(Default)]
pub struct App {
    status: String,
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("hangar");
            ui.label(if self.status.is_empty() {
                "就绪"
            } else {
                &self.status
            });
        });
    }
}
