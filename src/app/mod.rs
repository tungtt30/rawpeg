//! rawpeg darkroom application shell: state, background worker and egui panels.

pub mod panels;
pub mod state;
pub mod worker;

use eframe::egui;

use crate::app::state::AppState;
use crate::app::worker::Worker;

/// Root application state plus the background decode/render worker.
pub struct RawpegApp {
    state: AppState,
    worker: Worker,
}

impl RawpegApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        configure_theme(&cc.egui_ctx);
        Self {
            state: AppState::default(),
            worker: Worker::spawn(Some(cc.egui_ctx.clone())),
        }
    }
}

impl eframe::App for RawpegApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let RawpegApp { state, worker } = self;
        state.poll(&ctx, worker);

        panels::top_bar(ui, state, worker);
        panels::library_panel(ui, state, worker);
        panels::adjustments_panel(ui, state, worker);
        panels::viewport(ui, state);
    }
}

fn configure_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = egui::Color32::from_rgb(0x18, 0x18, 0x1b);
    visuals.window_fill = egui::Color32::from_rgb(0x18, 0x18, 0x1b);
    visuals.extreme_bg_color = egui::Color32::from_rgb(0x11, 0x11, 0x14);
    visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(0x27, 0x27, 0x2a);
    visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(0x27, 0x27, 0x2a);
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(0x32, 0x32, 0x36);
    visuals.widgets.active.bg_fill = egui::Color32::from_rgb(0x3b, 0x82, 0xf6);
    visuals.selection.bg_fill = egui::Color32::from_rgb(0x3b, 0x82, 0xf6);
    visuals.hyperlink_color = egui::Color32::from_rgb(0x3b, 0x82, 0xf6);
    ctx.set_visuals(visuals);
}
