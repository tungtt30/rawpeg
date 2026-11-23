use eframe::egui;

/// Root application state. Fleshed out in Milestone 3 (darkroom UI).
pub struct RawpegApp;

impl RawpegApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        configure_theme(&cc.egui_ctx);
        Self
    }
}

impl eframe::App for RawpegApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.centered_and_justified(|ui| {
                ui.label("rawpeg");
            });
        });
    }
}

fn configure_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = egui::Color32::from_rgb(0x18, 0x18, 0x1b);
    visuals.window_fill = egui::Color32::from_rgb(0x18, 0x18, 0x1b);
    ctx.set_visuals(visuals);
}
