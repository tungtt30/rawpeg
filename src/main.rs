#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use rawpeg::app::RawpegApp;

fn main() -> eframe::Result {
    env_logger::init();
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "rawpeg",
        options,
        Box::new(|cc| Ok(Box::new(RawpegApp::new(cc)))),
    )
}
