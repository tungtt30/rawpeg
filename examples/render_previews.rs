//! Renders full-resolution previews of every raw file in a directory, using
//! the full edit pipeline, so results can be inspected by eye.
//!
//! Usage:
//!   cargo run --example render_previews -- [dir] [--max-dim N]
//!
//! For each raw file `X.<ext>` the following are written to `<dir>/output/`:
//!   X__auto.{png,jpg}      auto-tone -> apply -> encode_srgb
//!   X__neutral.{png,jpg}   apply(default) on the linear decode (identity)
//!   X__ref.{png,jpg}       imagepipe 8-bit as-shot (base curve + gamma)
//!
//! `--max-dim N` caps the longer side at N pixels (default 0 = full size),
//! which keeps disk usage sane when previewing many large raw files.
//!
//! Exit code 1 if any file failed to render.

use std::path::{Path, PathBuf};

use image::{ImageEncoder, RgbImage};
use rawpeg::models::EditParams;
use rawpeg::raw_engine::auto_tone::auto_exposure;
use rawpeg::raw_engine::decoder::{
    decode_linear_scaled, decode_thumbnail_srgb, extract_exif_summary, scan_directory,
};
use rawpeg::raw_engine::processor::{apply, encode_srgb, luminance};

const JPEG_QUALITY: u8 = 98;

fn main() {
    let mut dir = "rawimage".to_string();
    let mut max_dim = 0usize;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--max-dim" => {
                max_dim = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or_else(|| panic!("--max-dim requires a number"))
            }
            other if !other.starts_with("--") => dir = other.to_string(),
            other => panic!("unknown argument '{other}'"),
        }
    }
    let dir = PathBuf::from(&dir);
    let out = dir.join("output");
    std::fs::create_dir_all(&out).expect("create output dir");

    let files = match scan_directory(&dir) {
        Ok(files) => files,
        Err(e) => {
            eprintln!("ERROR: failed to scan '{}': {e}", dir.display());
            std::process::exit(1);
        }
    };
    if files.is_empty() {
        eprintln!("ERROR: no raw files found in '{}'", dir.display());
        std::process::exit(1);
    }

    let mut ok = 0usize;
    let mut failed = 0usize;
    for path in &files {
        match render_one(path, &out, max_dim) {
            Ok(()) => ok += 1,
            Err(e) => {
                eprintln!("FAIL {}", path.file_name().unwrap().to_string_lossy());
                eprintln!("     {e:#}");
                failed += 1;
            }
        }
    }

    println!("\n=== {ok} rendered, {failed} failed -> {}", out.display());
    if failed > 0 {
        std::process::exit(1);
    }
}

fn render_one(path: &Path, out: &Path, max_dim: usize) -> anyhow::Result<()> {
    let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("image");
    let model = extract_exif_summary(path)
        .ok()
        .and_then(|s| s.model)
        .unwrap_or_else(|| "-".to_string());

    // Full-res (or capped) linear decode, shared by the auto and neutral
    // variants.
    let decoded = decode_linear_scaled(path, max_dim)?;

    // Auto-tone variant.
    let auto = auto_exposure(&decoded);
    let mean_lum =
        decoded.data.iter().map(|p| luminance(*p)).sum::<f32>() / decoded.data.len().max(1) as f32;
    let edited = apply(&decoded, &auto);
    save_both(
        &out.join(format!("{name}__auto")),
        decoded.width,
        decoded.height,
        &edited,
    )?;

    // Neutral variant (identity edits).
    let neutral = apply(&decoded, &EditParams::default());
    save_both(
        &out.join(format!("{name}__neutral")),
        decoded.width,
        decoded.height,
        &neutral,
    )?;

    // Reference variant: imagepipe 8-bit as-shot (base curve + gamma).
    let (w, h, data) = decode_thumbnail_srgb(path, max_dim as u32)?;
    save_rgb8(&out.join(format!("{name}__ref.png")), w, h, &data)?;
    save_jpeg(&out.join(format!("{name}__ref.jpg")), w, h, &data)?;

    println!(
        "OK   {name:20} {model:16} {w:>5}x{h:<5} ev={:+.2} hs={:+3} sh={:+3} mean_lum={:.3}",
        auto.exposure,
        auto.highlights,
        auto.shadows,
        mean_lum,
        w = decoded.width,
        h = decoded.height
    );
    Ok(())
}

fn save_both(stem: &Path, w: usize, h: usize, data: &[[f32; 3]]) -> anyhow::Result<()> {
    let rgb = encode_srgb(data);
    save_rgb8(&stem.with_extension("png"), w as u32, h as u32, &rgb)?;
    save_jpeg(&stem.with_extension("jpg"), w as u32, h as u32, &rgb)
}

fn save_rgb8(path: &Path, w: u32, h: u32, data: &[u8]) -> anyhow::Result<()> {
    debug_assert_eq!(data.len(), (w * h * 3) as usize);
    let img = RgbImage::from_raw(w, h, data.to_vec())
        .ok_or_else(|| anyhow::anyhow!("invalid RGB8 buffer for '{}'", path.display()))?;
    img.save(path)
        .map_err(|e| anyhow::anyhow!("failed to save '{}': {e}", path.display()))
}

fn save_jpeg(path: &Path, w: u32, h: u32, data: &[u8]) -> anyhow::Result<()> {
    debug_assert_eq!(data.len(), (w * h * 3) as usize);
    let file = std::fs::File::create(path)
        .map_err(|e| anyhow::anyhow!("failed to create '{}': {e}", path.display()))?;
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(file, JPEG_QUALITY);
    encoder
        .write_image(data, w, h, image::ExtendedColorType::Rgb8)
        .map_err(|e| anyhow::anyhow!("failed to encode '{}': {e}", path.display()))
}
