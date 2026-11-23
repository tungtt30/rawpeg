mod common;

use std::collections::HashSet;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

use common::write_fixture_dng;
use exif::{In, Tag, Value};
use rawpeg::models::{EditParams, ExportFormat, ExportSettings};
use rawpeg::raw_engine::exporter::{ExportEvent, ExportJob, ExportSummary, export_batch};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rawpeg-export-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn fixture(dir: &Path) -> PathBuf {
    let path = dir.join("fixture.dng");
    write_fixture_dng(&path).unwrap();
    path
}

fn settings(format: ExportFormat, dir: &Path) -> ExportSettings {
    ExportSettings {
        format,
        output_dir: Some(dir.to_path_buf()),
        ..Default::default()
    }
}

fn job(index: usize, source: &Path, settings: &ExportSettings) -> ExportJob {
    ExportJob {
        index,
        source: source.to_path_buf(),
        output: settings.output_path(source),
        params: EditParams::default(),
    }
}

fn run(
    jobs: &[ExportJob],
    settings: &ExportSettings,
    cancel: &AtomicBool,
) -> (ExportSummary, Vec<ExportEvent>) {
    let events = Mutex::new(Vec::new());
    let summary = export_batch(jobs, settings, cancel, &|event| {
        events.lock().unwrap().push(event);
    });
    (summary, events.into_inner().unwrap())
}

fn image_size(path: &Path) -> (u32, u32) {
    let image = image::open(path).unwrap();
    (image.width(), image.height())
}

fn exif_model(path: &Path) -> Option<String> {
    let file = File::open(path).ok()?;
    let exif = exif::Reader::new()
        .read_from_container(&mut BufReader::new(file))
        .ok()?;
    let field = exif.get_field(Tag::Model, In::PRIMARY)?;
    match &field.value {
        Value::Ascii(parts) => parts.first().map(|part| {
            String::from_utf8_lossy(part)
                .trim_matches('\0')
                .trim()
                .to_string()
        }),
        _ => Some(field.display_value().to_string()),
    }
}

fn count_events(events: &[ExportEvent]) -> (usize, usize, usize) {
    let mut started = 0;
    let mut finished = 0;
    let mut items = 0;
    for event in events {
        match event {
            ExportEvent::Started { .. } => started += 1,
            ExportEvent::Finished { .. } => finished += 1,
            ExportEvent::ItemFinished { .. } => items += 1,
            _ => {}
        }
    }
    (started, finished, items)
}

#[test]
fn jpeg_export_writes_rgb_and_exif() {
    let dir = temp_dir("jpeg");
    let source = fixture(&dir);
    let settings = settings(ExportFormat::Jpeg, &dir);
    let jobs = vec![job(0, &source, &settings)];
    let (summary, events) = run(&jobs, &settings, &AtomicBool::new(false));

    assert_eq!(summary.succeeded, 1, "summary: {summary:?}");
    assert_eq!(summary.failed, 0);
    assert_eq!(summary.outputs.len(), 1);
    assert_eq!(image_size(&summary.outputs[0]), (12, 12));
    assert_eq!(
        exif_model(&summary.outputs[0]).as_deref(),
        Some("FixtureCam")
    );
    assert_eq!(count_events(&events), (1, 1, 1));
    assert!(summary.output_dir().is_some());
}

#[test]
fn png_export_writes_lossless_file_and_exif() {
    let dir = temp_dir("png");
    let source = fixture(&dir);
    let settings = settings(ExportFormat::Png, &dir);
    let jobs = vec![job(0, &source, &settings)];
    let (summary, _) = run(&jobs, &settings, &AtomicBool::new(false));

    assert_eq!(summary.succeeded, 1, "summary: {summary:?}");
    assert_eq!(image_size(&summary.outputs[0]), (12, 12));
    assert_eq!(
        exif_model(&summary.outputs[0]).as_deref(),
        Some("FixtureCam")
    );
}

#[test]
fn tiff_export_writes_lossless_file() {
    let dir = temp_dir("tiff");
    let source = fixture(&dir);
    let settings = settings(ExportFormat::Tiff, &dir);
    let jobs = vec![job(0, &source, &settings)];
    let (summary, _) = run(&jobs, &settings, &AtomicBool::new(false));

    assert_eq!(summary.succeeded, 1, "summary: {summary:?}");
    assert_eq!(image_size(&summary.outputs[0]), (12, 12));
    assert_eq!(
        summary.outputs[0].extension().and_then(|ext| ext.to_str()),
        Some("tif")
    );
}

#[test]
fn max_dim_caps_the_long_edge() {
    let dir = temp_dir("resize");
    let source = fixture(&dir);
    let settings = ExportSettings {
        max_dim: Some(10),
        ..settings(ExportFormat::Jpeg, &dir)
    };
    let jobs = vec![job(0, &source, &settings)];
    let (summary, _) = run(&jobs, &settings, &AtomicBool::new(false));

    assert_eq!(summary.succeeded, 1, "summary: {summary:?}");
    let (width, height) = image_size(&summary.outputs[0]);
    assert!(width <= 10 && height <= 10, "got {width}x{height}");
}

#[test]
fn existing_file_is_skipped_without_overwrite() {
    let dir = temp_dir("skip");
    let source = fixture(&dir);
    let mut settings = settings(ExportFormat::Jpeg, &dir);
    settings.overwrite = false;
    let jobs = vec![job(0, &source, &settings)];
    std::fs::write(&jobs[0].output, b"sentinel").unwrap();

    let (summary, _) = run(&jobs, &settings, &AtomicBool::new(false));
    assert_eq!(summary.skipped, 1, "summary: {summary:?}");
    assert_eq!(summary.succeeded, 0);
    assert_eq!(std::fs::read(&jobs[0].output).unwrap(), b"sentinel");
}

#[test]
fn overwrite_replaces_existing_file() {
    let dir = temp_dir("overwrite");
    let source = fixture(&dir);
    let settings = settings(ExportFormat::Jpeg, &dir);
    let jobs = vec![job(0, &source, &settings)];
    std::fs::write(&jobs[0].output, b"sentinel").unwrap();

    let (summary, _) = run(&jobs, &settings, &AtomicBool::new(false));
    assert_eq!(summary.succeeded, 1, "summary: {summary:?}");
    assert_ne!(std::fs::read(&jobs[0].output).unwrap(), b"sentinel");
}

#[test]
fn cancelled_export_processes_nothing() {
    let dir = temp_dir("cancel");
    let source = fixture(&dir);
    let settings = settings(ExportFormat::Jpeg, &dir);
    let jobs = vec![job(0, &source, &settings)];
    let cancel = AtomicBool::new(true);

    let (summary, events) = run(&jobs, &settings, &cancel);
    assert!(summary.cancelled);
    assert_eq!(summary.processed(), 0);
    assert!(!jobs[0].output.exists());
    assert_eq!(count_events(&events), (1, 1, 0));
}

#[test]
fn one_bad_file_does_not_stop_the_batch() {
    let dir = temp_dir("failure");
    let source = fixture(&dir);
    let settings = settings(ExportFormat::Jpeg, &dir);
    let missing = ExportJob {
        index: 9,
        source: dir.join("missing.dng"),
        output: settings.output_path(&dir.join("missing.dng")),
        params: EditParams::default(),
    };
    let good = job(0, &source, &settings);
    let jobs = vec![good, missing];
    let (summary, _) = run(&jobs, &settings, &AtomicBool::new(false));

    assert_eq!(summary.succeeded, 1, "summary: {summary:?}");
    assert_eq!(summary.failed, 1);
    assert_eq!(summary.failures.len(), 1);
    assert!(!summary.cancelled);
}

#[test]
fn keep_exif_false_omits_metadata() {
    let dir = temp_dir("no-exif");
    let source = fixture(&dir);
    let settings = ExportSettings {
        keep_exif: false,
        ..settings(ExportFormat::Jpeg, &dir)
    };
    let jobs = vec![job(0, &source, &settings)];
    let (summary, _) = run(&jobs, &settings, &AtomicBool::new(false));

    assert_eq!(summary.succeeded, 1, "summary: {summary:?}");
    assert!(exif_model(&summary.outputs[0]).is_none());
}

#[test]
fn temp_files_are_cleaned_up() {
    let dir = temp_dir("temp");
    let source = fixture(&dir);
    let settings = settings(ExportFormat::Jpeg, &dir);
    let jobs = vec![job(0, &source, &settings)];
    let (summary, _) = run(&jobs, &settings, &AtomicBool::new(false));
    assert_eq!(summary.succeeded, 1);

    let leftovers: HashSet<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "leftover temp files: {leftovers:?}");
}

#[test]
fn duplicate_outputs_are_reported_not_overwritten() {
    let dir = temp_dir("duplicate");
    let a_dir = dir.join("a");
    let b_dir = dir.join("b");
    std::fs::create_dir_all(&a_dir).unwrap();
    std::fs::create_dir_all(&b_dir).unwrap();
    let a = a_dir.join("photo.dng");
    let b = b_dir.join("photo.dng");
    write_fixture_dng(&a).unwrap();
    write_fixture_dng(&b).unwrap();

    let out = dir.join("out");
    std::fs::create_dir_all(&out).unwrap();
    let settings = ExportSettings {
        output_dir: Some(out.clone()),
        ..Default::default()
    };
    let jobs = vec![
        ExportJob {
            index: 0,
            source: a.clone(),
            output: settings.output_path(&a),
            params: EditParams::default(),
        },
        ExportJob {
            index: 1,
            source: b.clone(),
            output: settings.output_path(&b),
            params: EditParams::default(),
        },
    ];

    let (summary, _) = run(&jobs, &settings, &AtomicBool::new(false));
    assert_eq!(summary.succeeded, 1, "summary: {summary:?}");
    assert_eq!(summary.failed, 1, "summary: {summary:?}");
    assert_eq!(summary.failures.len(), 1);
    assert!(
        summary.failures[0].message.contains("duplicate"),
        "unexpected failure: {}",
        summary.failures[0].message
    );
    assert!(out.join("photo.jpg").exists());
}
