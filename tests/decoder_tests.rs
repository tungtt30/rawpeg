mod common;

use std::fs::File;
use std::path::PathBuf;

use common::write_fixture_dng;
use rawpeg::raw_engine::decoder::{
    decode_linear, decode_linear_scaled, decode_thumbnail_srgb, extract_exif_summary,
    is_raw_path, scan_directory,
};

fn unique_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rawpeg-test-{}-{}",
        std::process::id(),
        name
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn fixture_dng(name: &str) -> PathBuf {
    let dir = unique_dir(name);
    let path = dir.join("fixture.dng");
    write_fixture_dng(&path).unwrap();
    path
}

fn touch(path: PathBuf) {
    File::create(path).unwrap();
}

#[test]
fn is_raw_path_matches_known_extensions_case_insensitively() {
    assert!(is_raw_path(std::path::Path::new("photo.CR2")));
    assert!(is_raw_path(std::path::Path::new("photo.dng")));
    assert!(is_raw_path(std::path::Path::new("photo.NeF")));
    assert!(!is_raw_path(std::path::Path::new("photo.jpg")));
    assert!(!is_raw_path(std::path::Path::new("photo.jpeg")));
    assert!(!is_raw_path(std::path::Path::new("no_extension")));
    assert!(!is_raw_path(std::path::Path::new("photo.tiff")));
}

#[test]
fn scan_directory_returns_only_raw_files_sorted() {
    let dir = unique_dir("scan");
    touch(dir.join("a.CR2"));
    touch(dir.join("b.dng"));
    touch(dir.join("c.jpg"));
    touch(dir.join("d.NEF"));
    touch(dir.join("e.txt"));
    let nested = dir.join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    touch(nested.join("f.raw")); // must be ignored: scan is non-recursive

    let found = scan_directory(&dir).unwrap();
    let names: Vec<String> = found
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["a.CR2", "b.dng", "d.NEF"]);
}

#[test]
fn scan_directory_missing_dir_is_error() {
    let missing = std::env::temp_dir().join("rawpeg-does-not-exist-xyz");
    assert!(scan_directory(&missing).is_err());
}

#[test]
fn decode_fixture_full_resolution_is_linear_rgb() {
    let dng = fixture_dng("full");
    let img = decode_linear(&dng).unwrap();

    assert_eq!((img.width, img.height), (12, 12));
    assert_eq!(img.data.len(), 144);
    for pixel in &img.data {
        for channel in pixel {
            assert!(channel.is_finite());
            assert!(
                (0.0..=1.0).contains(channel),
                "channel {channel} out of range"
            );
        }
    }

    // The red CFA sites carry the highest raw value (40000/65535 ~= 0.61), so
    // at least one pixel must show a strong red component after the demosaic
    // and color round trip.
    assert!(
        img.data.iter().any(|p| p[0] > 0.5),
        "no pixel with a strong red channel"
    );
}

#[test]
fn decode_fixture_is_deterministic() {
    let dng = fixture_dng("determinism");
    let a = decode_linear(&dng).unwrap();
    let b = decode_linear(&dng).unwrap();
    assert_eq!(a.data, b.data);
}

#[test]
fn decode_fixture_scaled_respects_max_dim() {
    let dng = fixture_dng("scaled");
    let img = decode_linear_scaled(&dng, 2).unwrap();
    assert_eq!((img.width, img.height), (2, 2));
    assert_eq!(img.data.len(), 4);
}

#[test]
fn decode_fixture_thumbnail_is_srgb_8bit() {
    let dng = fixture_dng("thumb");
    let (w, h, data) = decode_thumbnail_srgb(&dng, 8).unwrap();
    assert_eq!((w, h), (8, 8));
    assert_eq!(data.len(), 8 * 8 * 3);

    let (w3, h3, _) = decode_thumbnail_srgb(&dng, 3).unwrap();
    assert_eq!((w3, h3), (3, 3));
}

#[test]
fn exif_summary_reads_model_from_fixture() {
    let dng = fixture_dng("exif");
    let summary = extract_exif_summary(&dng).unwrap();
    assert_eq!(summary.model.as_deref(), Some("FixtureCam"));
    assert!(summary.iso.is_none());
    assert!(summary.aperture.is_none());
    assert!(summary.shutter.is_none());
    assert!(summary.focal_length.is_none());
}

/// Requires a real raw file (e.g. from scripts/fetch_samples.sh). Set the
/// RAW_SAMPLE env var to the file path to run it.
#[test]
#[ignore]
fn decode_real_sample_requires_env() {
    let path = std::env::var("RAW_SAMPLE").expect("RAW_SAMPLE env var not set");
    let path = PathBuf::from(path);
    let img = decode_linear(&path).unwrap();
    assert!(img.width > 0 && img.height > 0);
    assert_eq!(img.data.len(), img.width * img.height);
    let _ = extract_exif_summary(&path).unwrap();
}
