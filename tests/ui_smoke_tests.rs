mod common;

use std::path::PathBuf;

use eframe::egui;
use rawpeg::app::RawpegApp;
use rawpeg::app::state::{AppState, ViewMode};

use rawpeg::app::worker::Worker;

fn headless_frames(app: &mut RawpegApp, ctx: &egui::Context, frames: usize) {
    for _ in 0..frames {
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| app.draw(ui));
        output.textures_delta.clear();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn app_renders_empty_state_headless() {
    let ctx = egui::Context::default();
    let worker = Worker::spawn(None);
    let mut app = RawpegApp::from_parts(AppState::default(), worker);
    headless_frames(&mut app, &ctx, 3);
}

#[test]
fn app_renders_library_and_develop_headless() {
    let dir = std::env::temp_dir().join(format!("rawpeg-ui-smoke-{}-develop", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dng: PathBuf = dir.join("fixture.dng");
    common::write_fixture_dng(&dng).unwrap();

    let ctx = egui::Context::default();
    let worker = Worker::spawn(None);
    let mut state = AppState::default();
    state.add_imported(&worker, vec![dng]);
    state.view_mode = ViewMode::Develop;
    let mut app = RawpegApp::from_parts(state, worker);
    headless_frames(&mut app, &ctx, 5);
}
