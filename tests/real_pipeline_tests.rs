use std::path::{Path, PathBuf};

use rawpeg::models::EditParams;
use rawpeg::raw_engine::auto_tone::auto_exposure;
use rawpeg::raw_engine::decoder::{
    decode_linear, decode_linear_scaled, decode_thumbnail_srgb, extract_exif_summary, scan_directory,
};
use rawpeg::raw_engine::processor::{apply, apply_srgb, luminance};

/// Real-sample directory: `RAW_DIR` env var, defaulting to `./rawimage`.
fn sample_dir() -> PathBuf {
    std::env::var("RAW_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("rawimage"))
}

/// Runs the whole pipeline on one file and asserts every invariant.
///
/// Panics on any genuine pipeline violation. Only invoked on files that
/// decoded successfully, so library decode limitations are handled by the
/// caller before this runs.
fn check_full_pipeline(path: &Path) -> String {
    let decoded = decode_linear(path).expect("caller filters decodable files");

    // Full-res linear buffer invariants.
    assert_eq!(
        decoded.data.len(),
        decoded.width * decoded.height,
        "{}: data length mismatch",
        path.display()
    );
    for pixel in &decoded.data {
        for channel in pixel {
            assert!(
                channel.is_finite(),
                "{}: non-finite channel",
                path.display()
            );
            assert!(
                (0.0..=1.0).contains(channel),
                "{}: channel {channel} outside 0..=1",
                path.display()
            );
        }
    }

    // Identity edits must be a no-op.
    let identity = apply(&decoded, &EditParams::default());
    assert_eq!(
        identity, decoded.data,
        "{}: neutral params not identity",
        path.display()
    );

    // Auto-tone produces in-range, deterministic parameters.
    let auto = auto_exposure(&decoded);
    assert!(auto.is_auto_applied, "{}: auto not marked applied", path.display());
    assert!((-3.0..=3.0).contains(&auto.exposure));
    assert!((-100..=100).contains(&auto.highlights));
    assert!((-100..=100).contains(&auto.shadows));
    assert_eq!(auto, auto_exposure(&decoded), "{}: auto-tone not deterministic", path.display());

    // Edited buffer: same length, all finite.
    let edited = apply(&decoded, &auto);
    assert_eq!(edited.len(), decoded.data.len());
    assert!(
        edited.iter().all(|p| p.iter().all(|c| c.is_finite())),
        "{}: edited buffer has non-finite values",
        path.display()
    );

    // sRGB encode: exactly 3 bytes per pixel.
    let srgb = apply_srgb(&decoded, &auto);
    assert_eq!(
        srgb.len(),
        decoded.width * decoded.height * 3,
        "{}: sRGB buffer length mismatch",
        path.display()
    );

    // Proxy decode must respect max dimension.
    let proxy = decode_linear_scaled(path, 1600).expect("proxy decode");
    assert_eq!(proxy.data.len(), proxy.width * proxy.height);
    assert!(
        proxy.width.max(proxy.height) <= 1600,
        "{}: proxy too large ({}x{})",
        path.display(),
        proxy.width,
        proxy.height
    );

    // Thumbnail sRGB: 3 bytes per pixel.
    let (w, h, thumb) = decode_thumbnail_srgb(path, 256).expect("thumb decode");
    assert_eq!(thumb.len(), w as usize * h as usize * 3);

    // EXIF must parse without error.
    extract_exif_summary(path).expect("exif parse");

    let mean_lum = decoded.data.iter().map(|p| luminance(*p)).sum::<f32>()
        / decoded.data.len().max(1) as f32;
    format!(
        "OK   {:28} {:5}x{:<5} ev={:+.2} hs={:+3} sh={:+3} mean_lum={:.3}",
        path.file_name().unwrap().to_string_lossy(),
        decoded.width,
        decoded.height,
        auto.exposure,
        auto.highlights,
        auto.shadows,
        mean_lum,
    )
}

/// Runs the full pipeline against every raw file in the sample directory.
///
/// Files that imagepipe cannot decode (unsupported camera formats in
/// rawloader 0.37.2) are reported as UNSUPPORTED, not as failures. The test
/// only fails on real pipeline violations, or when no file decodes at all.
#[test]
#[ignore]
fn full_pipeline_on_real_samples() {
    let dir = sample_dir();
    let files = scan_directory(&dir).expect("scan sample directory");
    assert!(!files.is_empty(), "no raw files found in '{}'", dir.display());

    let mut ok = 0usize;
    let mut unsupported = 0usize;
    for path in &files {
        // Decode failures here are rawloader 0.37.2 library limitations
        // (e.g. Sony ILCE-7M4/7RM5, Samsung phone DNGs) — report, don't fail.
        if decode_linear(path).is_err() {
            println!("UNSUPPORTED {}", path.display());
            unsupported += 1;
            continue;
        }
        let line = check_full_pipeline(path.as_path());
        println!("{line}");
        ok += 1;
    }
    println!("\n{ok} passed, {unsupported} unsupported -> {}", dir.display());
    assert!(ok > 0, "no sample decoded: {unsupported} unsupported, 0 passed");
}

/// Re-decoding the same file must produce byte-identical linear buffers and
/// identical auto-tone parameters.
#[test]
#[ignore]
fn pipeline_is_deterministic_on_real_samples() {
    let dir = sample_dir();
    let files = scan_directory(&dir).expect("scan sample directory");

    let first = files
        .iter()
        .find(|p| decode_linear(p).is_ok())
        .expect("no decodable sample in the directory");

    let a = decode_linear(first).unwrap();
    let b = decode_linear(first).unwrap();
    assert_eq!(a.data, b.data, "decode not deterministic: {}", first.display());
    assert_eq!(
        auto_exposure(&a),
        auto_exposure(&b),
        "auto-tone not deterministic: {}",
        first.display()
    );
}
