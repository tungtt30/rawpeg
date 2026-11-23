mod common;

use std::path::PathBuf;

use common::write_fixture_dng;
use rawpeg::models::EditParams;
use rawpeg::raw_engine::auto_tone::auto_exposure;
use rawpeg::raw_engine::decoder::decode_linear;
use rawpeg::raw_engine::processor::{apply, apply_srgb, encode_srgb, luminance};

fn fixture_dng(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rawpeg-test-{}-{}",
        std::process::id(),
        name
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("fixture.dng");
    write_fixture_dng(&path).unwrap();
    path
}

fn mean_luminance(data: &[[f32; 3]]) -> f32 {
    data.iter().map(|p| luminance(*p)).sum::<f32>() / data.len() as f32
}

#[test]
fn apply_with_neutral_params_is_identity_on_fixture() {
    let dng = fixture_dng("neutral");
    let decoded = decode_linear(&dng).unwrap();
    assert_eq!(apply(&decoded, &EditParams::default()), decoded.data);
}

#[test]
fn exposure_plus_one_ev_doubles_fixture_luminance() {
    let dng = fixture_dng("ev");
    let decoded = decode_linear(&dng).unwrap();
    let before = mean_luminance(&decoded.data);
    let params = EditParams {
        exposure: 1.0,
        ..Default::default()
    };
    let after = mean_luminance(&apply(&decoded, &params));
    assert!(
        (after / before - 2.0).abs() < 1e-3,
        "expected 2x luminance, got {before} -> {after}"
    );
}

#[test]
fn apply_srgb_on_fixture_produces_valid_u8_buffer() {
    let dng = fixture_dng("srgb");
    let decoded = decode_linear(&dng).unwrap();
    let params = EditParams {
        exposure: 0.3,
        contrast: 20,
        highlights: -10,
        shadows: 15,
        temperature: 7000,
        tint: 5,
        ..Default::default()
    };
    let out = apply_srgb(&decoded, &params);
    assert_eq!(out.len(), decoded.width * decoded.height * 3);
}

#[test]
fn encode_srgb_on_fixture_preserves_layout() {
    let dng = fixture_dng("encode");
    let decoded = decode_linear(&dng).unwrap();
    let encoded = encode_srgb(&decoded.data);
    assert_eq!(encoded.len(), decoded.data.len() * 3);
}

#[test]
fn auto_exposure_runs_on_fixture_and_is_deterministic() {
    let dng = fixture_dng("auto");
    let decoded = decode_linear(&dng).unwrap();
    let params = auto_exposure(&decoded);
    assert!((-3.0..=3.0).contains(&params.exposure));
    assert!((-100..=100).contains(&params.highlights));
    assert!((-100..=100).contains(&params.shadows));
    assert!(params.is_auto_applied);
    assert_eq!(params.temperature, 6500, "WB untouched by auto-tone");
    assert_eq!(params.tint, 0);
    assert_eq!(auto_exposure(&decoded), params);
}

#[test]
fn auto_tone_then_apply_keeps_output_in_sane_range() {
    let dng = fixture_dng("autoapply");
    let decoded = decode_linear(&dng).unwrap();
    let params = auto_exposure(&decoded);
    let edited = apply(&decoded, &params);
    assert_eq!(edited.len(), decoded.data.len());
    assert!(edited.iter().all(|p| p.iter().all(|c| c.is_finite())));
}
