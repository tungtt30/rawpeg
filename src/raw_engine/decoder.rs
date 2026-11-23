use std::io::BufReader;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use imagepipe::Pipeline;
use exif::{In, Reader, Tag, Value};

use crate::models::ExifSummary;

/// File extensions accepted by the raw engine (case-insensitive).
pub const RAW_EXTENSIONS: &[&str] = &[
    "3fr", "ari", "arw", "cr2", "cr3", "dcr", "dng", "erf", "iiq", "kdc", "mef", "mos",
    "mrw", "nef", "nrw", "orf", "pef", "raf", "raw", "rw2", "srw", "x3f",
];

/// A decoded image stored as linear RGB.
///
/// All edit operations (exposure, contrast, white balance, ...) run on this
/// buffer, in linear light space, before an sRGB gamma encoding is applied at
/// the end for display or export.
#[derive(Debug, Clone)]
pub struct DecodedImage {
    pub width: usize,
    pub height: usize,
    /// Row-major pixels, each channel normalized to `0.0..=1.0`.
    pub data: Vec<[f32; 3]>,
}

/// Returns true when the file path has a raw extension.
pub fn is_raw_path(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match ext {
        Some(ext) => RAW_EXTENSIONS.iter().any(|candidate| *candidate == ext),
        None => false,
    }
}

/// Scans a directory (non-recursive) and returns the sorted list of raw files.
pub fn scan_directory(dir: &Path) -> Result<Vec<PathBuf>> {
    let entries = std::fs::read_dir(dir)
        .with_context(|| format!("failed to read directory '{}'", dir.display()))?;
    let mut files = Vec::new();
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && is_raw_path(&path) {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

/// Decodes the full-resolution image into linear RGB (base tone curve disabled).
pub fn decode_linear(path: &Path) -> Result<DecodedImage> {
    decode_linear_scaled(path, 0)
}

/// Decodes the image into linear RGB, constraining the longer side to `max_dim`
/// pixels (0 means full resolution). The camera base tone curve is disabled so
/// highlight detail stays in the linear range for later editing.
///
/// Note: imagepipe's built-in demosaic is a coarse fast path (neighbor
/// averaging, no AHD/DCB). For previews and interactive editing this is fine;
/// the Milestone 4 exporter should evaluate a proper demosaic on the raw CFA
/// data for maximum export quality.
pub fn decode_linear_scaled(path: &Path, max_dim: usize) -> Result<DecodedImage> {
    let mut pipeline = new_pipeline(path, max_dim)?;
    pipeline.ops.basecurve.points = vec![];
    pipeline.ops.basecurve.exposure = 0.0;

    let img = pipeline
        .output_16bit(None)
        .map_err(|e| anyhow!("imagepipe failed to process '{}': {}", path.display(), e))?;

    let inv = 1.0 / 65535.0;
    let data = img
        .data
        .chunks_exact(3)
        .map(|p| [p[0] as f32 * inv, p[1] as f32 * inv, p[2] as f32 * inv])
        .collect();
    Ok(DecodedImage {
        width: img.width,
        height: img.height,
        data,
    })
}

/// Decodes a small sRGB 8-bit preview (embedded preview via the pipeline at a
/// reduced demosaic size). Returns `(width, height, rgb8 data)`.
pub fn decode_thumbnail_srgb(path: &Path, max_dim: u32) -> Result<(u32, u32, Vec<u8>)> {
    let mut pipeline = new_pipeline(path, max_dim as usize)?;
    let img = pipeline
        .output_8bit(None)
        .map_err(|e| anyhow!("imagepipe failed to process '{}': {}", path.display(), e))?;
    Ok((img.width as u32, img.height as u32, img.data))
}

/// Reads a short EXIF summary from the file (used by the top bar).
pub fn extract_exif_summary(path: &Path) -> Result<ExifSummary> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("failed to open '{}'", path.display()))?;
    let exif = Reader::new()
        .read_from_container(&mut BufReader::new(file))
        .with_context(|| format!("failed to parse EXIF from '{}'", path.display()))?;

    let get = |tag: Tag| -> Option<String> {
        exif.get_field(tag, In::PRIMARY).map(|f| match &f.value {
            Value::Ascii(parts) => parts
                .first()
                .map(|s| {
                    String::from_utf8_lossy(s)
                        .trim_matches('\0')
                        .trim()
                        .to_string()
                })
                .unwrap_or_default(),
            _ => f.display_value().to_string(),
        })
    };

    Ok(ExifSummary {
        model: get(Tag::Model),
        iso: get(Tag::PhotographicSensitivity),
        aperture: get(Tag::FNumber),
        shutter: get(Tag::ExposureTime),
        focal_length: get(Tag::FocalLength),
    })
}

fn new_pipeline(path: &Path, max_dim: usize) -> Result<Pipeline> {
    let mut pipeline = Pipeline::new_from_file(path)
        .map_err(|e| anyhow!("imagepipe failed to decode '{}': {}", path.display(), e))?;
    pipeline.globals.settings.maxwidth = max_dim;
    pipeline.globals.settings.maxheight = max_dim;
    Ok(pipeline)
}
