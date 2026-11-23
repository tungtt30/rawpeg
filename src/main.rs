#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use rawpeg::app::RawpegApp;

fn main() -> eframe::Result {
    env_logger::init();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("rawpeg — RAW batch exporter")
            .with_inner_size([1320.0, 860.0])
            .with_min_inner_size([1040.0, 680.0]),
        ..Default::default()
    };
    eframe::run_native(
        "rawpeg",
        options,
        Box::new(|cc| Ok(Box::new(RawpegApp::new(cc)))),
    )
}
