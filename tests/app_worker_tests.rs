mod common;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use common::write_fixture_dng;
use rawpeg::app::state::AppState;
use rawpeg::app::worker::{Job, Message, Worker, PROXY_MAX_DIM, THUMB_MAX_DIM};
use rawpeg::models::EditParams;
use rawpeg::raw_engine::decoder::DecodeStatus;

const TIMEOUT: Duration = Duration::from_secs(30);

fn fixture_dng(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rawpeg-worker-{}-{}",
        std::process::id(),
        name
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("fixture.dng");
    write_fixture_dng(&path).unwrap();
    path
}

#[test]
fn proxy_load_returns_textures_and_histogram() {
    let dng = fixture_dng("proxy");
    let worker = Worker::spawn(None);
    worker.send(Job::LoadProxy {
        token: 7,
        index: 0,
        path: dng,
        params: EditParams::default(),
    });

    match worker.recv_timeout(TIMEOUT) {
        Some(Message::ProxyReady {
            token,
            index,
            width,
            height,
            before,
            after,
            histogram,
        }) => {
            assert_eq!(token, 7);
            assert_eq!(index, 0);
            assert!(width > 0 && height > 0, "image has area");
            assert!(
                width <= PROXY_MAX_DIM && height <= PROXY_MAX_DIM,
                "proxy is capped at {PROXY_MAX_DIM}px"
            );
            assert_eq!(before.rgb.len(), width * height * 3);
            assert_eq!(after.rgb.len(), width * height * 3);
            assert_eq!(histogram.pixel_count() as usize, width * height);
        }
        other => panic!("unexpected message: {other:?}"),
    }
}

#[test]
fn render_after_reuses_proxy_and_matches_dimensions() {
    let dng = fixture_dng("render");
    let worker = Worker::spawn(None);
    worker.send(Job::LoadProxy {
        token: 1,
        index: 0,
        path: dng,
        params: EditParams::default(),
    });
    let Some(Message::ProxyReady { width, height, .. }) = worker.recv_timeout(TIMEOUT) else {
        panic!("expected ProxyReady");
    };

    worker.send(Job::RenderAfter {
        token: 1,
        params: EditParams {
            exposure: 1.0,
            contrast: 20,
            ..Default::default()
        },
    });
    match worker.recv_timeout(TIMEOUT) {
        Some(Message::RenderReady {
            token,
            payload,
            histogram,
        }) => {
            assert_eq!(token, 1);
            assert_eq!(payload.width, width);
            assert_eq!(payload.height, height);
            assert_eq!(payload.rgb.len(), width * height * 3);
            assert_eq!(histogram.pixel_count() as usize, width * height);
        }
        other => panic!("unexpected message: {other:?}"),
    }
}

#[test]
fn auto_tone_job_returns_applied_parameters() {
    let dng = fixture_dng("auto");
    let worker = Worker::spawn(None);
    worker.send(Job::LoadProxy {
        token: 2,
        index: 0,
        path: dng,
        params: EditParams::default(),
    });
    assert!(matches!(
        worker.recv_timeout(TIMEOUT),
        Some(Message::ProxyReady { .. })
    ));

    worker.send(Job::AutoTone { token: 2 });
    match worker.recv_timeout(TIMEOUT) {
        Some(Message::AutoToneReady { token, params }) => {
            assert_eq!(token, 2);
            assert!(params.is_auto_applied);
            assert!((-3.0..=3.0).contains(&params.exposure));
        }
        other => panic!("unexpected message: {other:?}"),
    }
}

#[test]
fn thumbnail_job_probes_supported_fixture() {
    let dng = fixture_dng("thumb");
    let worker = Worker::spawn(None);
    worker.send(Job::LoadThumbnail {
        index: 3,
        path: dng,
    });

    match worker.recv_timeout(TIMEOUT) {
        Some(Message::Thumbnail {
            index,
            thumb,
            status,
            exif,
        }) => {
            assert_eq!(index, 3);
            assert_eq!(status, DecodeStatus::Supported);
            let thumb = thumb.expect("supported fixture should have a thumbnail");
            assert!(thumb.width as u32 <= THUMB_MAX_DIM);
            assert!(thumb.height as u32 <= THUMB_MAX_DIM);
            assert_eq!(thumb.rgb.len(), thumb.width * thumb.height * 3);
            assert_eq!(exif.model.as_deref(), Some("FixtureCam"));
        }
        other => panic!("unexpected message: {other:?}"),
    }
}

/// Drives `AppState::poll` end-to-end with a headless egui context so the
/// texture-upload and token-filtering path is covered without a window.
#[test]
fn state_poll_applies_proxy_and_auto_tone() {
    let dng = fixture_dng("poll");
    let ctx = eframe::egui::Context::default();
    let worker = Worker::spawn(None);

    let mut state = AppState::default();
    state.add_paths([dng]);
    let (token, path, params) = state.select(0).expect("entry exists");
    worker.send(Job::LoadProxy {
        token,
        index: 0,
        path,
        params,
    });

    let wait_for = |state: &mut AppState, done: &dyn Fn(&AppState) -> bool| {
        let deadline = Instant::now() + TIMEOUT;
        while !done(state) && Instant::now() < deadline {
            state.poll(&ctx, &worker);
            std::thread::sleep(Duration::from_millis(10));
        }
    };

    wait_for(&mut state, &|s| s.after.is_some());
    assert!(state.after.is_some(), "after texture applied");
    assert!(state.before.is_some(), "before texture applied");
    assert_eq!(
        state.image_size[0] * state.image_size[1],
        state.histogram.pixel_count() as usize
    );

    worker.send(Job::AutoTone { token });
    wait_for(&mut state, &|s| s.entries[0].item.params.is_auto_applied);
    assert!(
        state.entries[0].item.params.is_auto_applied,
        "auto-tone parameters applied through poll"
    );
}
