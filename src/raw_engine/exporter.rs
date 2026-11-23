//! Multi-threaded batch exporter: full-resolution decode, edit, sRGB encode and
//! write JPEG/PNG/TIFF with optional EXIF and resizing.

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufReader, Cursor, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use exif::{Field, In, Tag, Value};
use image::ImageEncoder;
use jpeg_encoder::{ColorType, Encoder, SamplingFactor};

use crate::models::{EditParams, ExportFormat, ExportSettings};
use crate::raw_engine::decoder::decode_linear_scaled;
use crate::raw_engine::processor::{apply, encode_srgb};

/// One file scheduled for export.
#[derive(Clone, Debug)]
pub struct ExportJob {
    pub index: usize,
    pub source: PathBuf,
    pub output: PathBuf,
    pub params: EditParams,
}

/// Result of one file in the batch.
#[derive(Clone, Debug)]
pub struct ExportOutcome {
    pub source: PathBuf,
    pub output: PathBuf,
    pub result: Result<(), String>,
    pub skipped: bool,
}

impl ExportOutcome {
    fn written(job: &ExportJob) -> Self {
        Self {
            source: job.source.clone(),
            output: job.output.clone(),
            result: Ok(()),
            skipped: false,
        }
    }

    fn skipped(job: &ExportJob) -> Self {
        Self {
            source: job.source.clone(),
            output: job.output.clone(),
            result: Ok(()),
            skipped: true,
        }
    }

    fn failure(job: &ExportJob, message: impl Into<String>) -> Self {
        Self {
            source: job.source.clone(),
            output: job.output.clone(),
            result: Err(message.into()),
            skipped: false,
        }
    }
}

/// A failed file, kept for the result panel.
#[derive(Clone, Debug)]
pub struct ExportFailure {
    pub source: PathBuf,
    pub output: PathBuf,
    pub message: String,
}

/// Aggregated result of a batch export.
#[derive(Clone, Debug, Default)]
pub struct ExportSummary {
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub skipped: usize,
    pub cancelled: bool,
    pub elapsed: Duration,
    pub outputs: Vec<PathBuf>,
    pub failures: Vec<ExportFailure>,
}

impl ExportSummary {
    pub fn processed(&self) -> usize {
        self.succeeded + self.failed + self.skipped
    }

    pub fn output_dir(&self) -> Option<PathBuf> {
        self.outputs
            .first()
            .and_then(|path| path.parent().map(Path::to_path_buf))
    }
}

/// Progress events sent from the export threads to the UI.
#[derive(Clone, Debug)]
pub enum ExportEvent {
    Started { total: usize },
    ItemStarted { current: String },
    ItemFinished { outcome: ExportOutcome },
    Finished { summary: ExportSummary },
    Fatal { message: String },
}

/// Runs a batch export. `on_event` is called from worker threads.
pub fn export_batch(
    jobs: &[ExportJob],
    settings: &ExportSettings,
    cancel: &AtomicBool,
    on_event: &(dyn Fn(ExportEvent) + Send + Sync),
) -> ExportSummary {
    let started = Instant::now();
    let mut summary = ExportSummary {
        total: jobs.len(),
        ..Default::default()
    };
    on_event(ExportEvent::Started { total: jobs.len() });

    if jobs.is_empty() {
        summary.elapsed = started.elapsed();
        on_event(ExportEvent::Finished {
            summary: summary.clone(),
        });
        return summary;
    }

    // Two selected photos can share a file stem (e.g. IMG_0001.CR2 from two
    // folders). Never let them race on the same output path: report the later
    // jobs as failures instead of silently overwriting another photo.
    let mut seen_outputs = HashSet::new();
    let mut unique_jobs = Vec::new();
    let mut duplicate_outcomes = Vec::new();
    for job in jobs {
        if seen_outputs.insert(job.output.clone()) {
            unique_jobs.push(job);
        } else {
            duplicate_outcomes.push(ExportOutcome::failure(
                job,
                "duplicate output path: another selected photo has the same file name",
            ));
        }
    }

    let run = |job: &ExportJob| -> Option<ExportOutcome> {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        on_event(ExportEvent::ItemStarted {
            current: job.source.to_string_lossy().into_owned(),
        });
        let outcome = export_one_caught(job, settings);
        on_event(ExportEvent::ItemFinished {
            outcome: outcome.clone(),
        });
        Some(outcome)
    };

    let num_threads = match settings.max_dim {
        Some(_) => 4,
        None => 2,
    };
    let num_threads = num_threads
        .min(
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(2),
        )
        .max(1);

    for outcome in &duplicate_outcomes {
        on_event(ExportEvent::ItemStarted {
            current: outcome.source.to_string_lossy().into_owned(),
        });
        on_event(ExportEvent::ItemFinished {
            outcome: outcome.clone(),
        });
    }

    let outcomes: Vec<Option<ExportOutcome>> = match rayon::ThreadPoolBuilder::new()
        .num_threads(num_threads)
        .thread_name(|i| format!("rawpeg-export-{i}"))
        .build()
    {
        Ok(pool) => pool.install(|| {
            use rayon::prelude::*;
            unique_jobs.par_iter().map(|job| run(job)).collect()
        }),
        Err(_) => unique_jobs.iter().map(|job| run(job)).collect(),
    };

    for outcome in outcomes.into_iter().flatten().chain(duplicate_outcomes) {
        if outcome.skipped {
            summary.skipped += 1;
        } else if let Err(message) = outcome.result {
            summary.failed += 1;
            summary.failures.push(ExportFailure {
                source: outcome.source,
                output: outcome.output,
                message,
            });
        } else {
            summary.succeeded += 1;
            summary.outputs.push(outcome.output);
        }
    }

    summary.cancelled = summary.processed() < summary.total;
    summary.elapsed = started.elapsed();
    on_event(ExportEvent::Finished {
        summary: summary.clone(),
    });
    summary
}

fn export_one_caught(job: &ExportJob, settings: &ExportSettings) -> ExportOutcome {
    match catch_unwind(AssertUnwindSafe(|| export_one(job, settings))) {
        Ok(Ok(ExportOne::Written)) => ExportOutcome::written(job),
        Ok(Ok(ExportOne::Skipped)) => ExportOutcome::skipped(job),
        Ok(Err(error)) => ExportOutcome::failure(job, format!("{error:#}")),
        Err(payload) => ExportOutcome::failure(job, panic_message(&payload)),
    }
}

enum ExportOne {
    Written,
    Skipped,
}

fn export_one(job: &ExportJob, settings: &ExportSettings) -> Result<ExportOne> {
    if !settings.overwrite && job.output.exists() {
        return Ok(ExportOne::Skipped);
    }

    let decoded = decode_linear_scaled(
        &job.source,
        settings.max_dim.map(|dim| dim as usize).unwrap_or(0),
    )
    .with_context(|| format!("decode '{}'", job.source.display()))?;

    if decoded.width == 0 || decoded.height == 0 {
        return Err(anyhow!("decoded image has no area"));
    }

    let edited = apply(&decoded, &job.params);
    let rgb = encode_srgb(&edited);
    let width = decoded.width;
    let height = decoded.height;

    let exif = if settings.keep_exif && settings.format.supports_exif() {
        build_exif_block(&job.source, width as u32, height as u32)
            .ok()
            .flatten()
    } else {
        None
    };

    if let Some(parent) = job.output.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create output directory '{}'", parent.display()))?;
    }

    let temp = temp_path(&job.output, job.index);
    let write_result = write_image(&temp, &rgb, width, height, settings, exif.as_deref());
    if let Err(error) = write_result {
        let _ = std::fs::remove_file(&temp);
        return Err(error);
    }

    // Re-check the target right before replacing it so `Skip` is honoured even
    // if the file appeared while this job was decoding/encoding.
    if !settings.overwrite && job.output.exists() {
        let _ = std::fs::remove_file(&temp);
        return Ok(ExportOne::Skipped);
    }

    if job.output.exists() {
        std::fs::remove_file(&job.output)
            .with_context(|| format!("remove existing '{}'", job.output.display()))?;
    }
    std::fs::rename(&temp, &job.output)
        .with_context(|| format!("move '{}' -> '{}'", temp.display(), job.output.display()))?;
    Ok(ExportOne::Written)
}

fn write_image(
    path: &Path,
    rgb: &[u8],
    width: usize,
    height: usize,
    settings: &ExportSettings,
    exif: Option<&[u8]>,
) -> Result<()> {
    let file = File::create(path).with_context(|| format!("create '{}'", path.display()))?;
    let sync_handle = file
        .try_clone()
        .with_context(|| format!("duplicate handle for '{}'", path.display()))?;
    match settings.format {
        ExportFormat::Jpeg => {
            if width > u16::MAX as usize || height > u16::MAX as usize {
                return Err(anyhow!(
                    "JPEG dimensions {width}x{height} exceed the 65535px limit"
                ));
            }
            let mut encoder = Encoder::new(file, settings.clamped_quality());
            encoder.set_sampling_factor(if settings.chroma_444 {
                SamplingFactor::F_1_1
            } else {
                SamplingFactor::F_2_2
            });
            if let Some(exif) = exif
                && let Err(error) = encoder.add_exif_metadata(exif)
            {
                log::warn!("skipping EXIF for '{}': {error}", path.display());
            }
            encoder
                .encode(rgb, width as u16, height as u16, ColorType::Rgb)
                .with_context(|| format!("encode JPEG '{}'", path.display()))?;
        }
        ExportFormat::Png => {
            let mut encoder = image::codecs::png::PngEncoder::new_with_quality(
                file,
                image::codecs::png::CompressionType::Fast,
                image::codecs::png::FilterType::Adaptive,
            );
            if let Some(exif) = exif
                && let Err(error) = encoder.set_exif_metadata(exif.to_vec())
            {
                log::warn!("skipping EXIF for '{}': {error}", path.display());
            }
            encoder
                .write_image(
                    rgb,
                    width as u32,
                    height as u32,
                    image::ExtendedColorType::Rgb8,
                )
                .with_context(|| format!("encode PNG '{}'", path.display()))?;
        }
        ExportFormat::Tiff => {
            let encoder = image::codecs::tiff::TiffEncoder::new(file);
            encoder
                .write_image(
                    rgb,
                    width as u32,
                    height as u32,
                    image::ExtendedColorType::Rgb8,
                )
                .with_context(|| format!("encode TIFF '{}'", path.display()))?;
        }
    }
    // Flush errors (disk full etc.) must not disappear when the encoder is
    // dropped; surface them before the caller renames the temp file.
    sync_handle
        .sync_all()
        .with_context(|| format!("flush '{}'", path.display()))?;
    Ok(())
}

fn temp_path(output: &Path, index: usize) -> PathBuf {
    let name = output
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "image".to_string());
    output.with_file_name(format!(".{name}.rawpeg-{index}.tmp"))
}

fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "unknown panic while exporting".to_string()
    }
}

/// Builds a compact TIFF/EXIF block for embedding into JPEG/PNG.
fn build_exif_block(source: &Path, width: u32, height: u32) -> Result<Option<Vec<u8>>> {
    let file = File::open(source).with_context(|| format!("open '{}'", source.display()))?;
    let mut reader = BufReader::new(file);
    let parsed = exif::Reader::new()
        .continue_on_error(true)
        .read_from_container(&mut reader);
    let exif = match parsed {
        Ok(exif) => exif,
        Err(error) => match error.distill_partial_result(|_| {}) {
            Ok(exif) => exif,
            Err(error) => {
                log::warn!("EXIF unavailable for '{}': {error}", source.display());
                return Ok(None);
            }
        },
    };

    let mut fields: Vec<Field> = exif
        .fields()
        .filter(|field| {
            !matches!(
                field.tag,
                Tag::Orientation
                    | Tag::PixelXDimension
                    | Tag::PixelYDimension
                    | Tag::MakerNote
                    | Tag::UserComment
            ) && !matches!(field.value, Value::Unknown(..))
        })
        .cloned()
        .collect();

    fields.push(Field {
        tag: Tag::Orientation,
        ifd_num: In::PRIMARY,
        value: Value::Short(vec![1]),
    });
    fields.push(Field {
        tag: Tag::PixelXDimension,
        ifd_num: In::PRIMARY,
        value: Value::Long(vec![width]),
    });
    fields.push(Field {
        tag: Tag::PixelYDimension,
        ifd_num: In::PRIMARY,
        value: Value::Long(vec![height]),
    });

    let mut writer = exif::experimental::Writer::new();
    for field in &fields {
        writer.push_field(field);
    }

    let mut cursor = Cursor::new(Vec::new());
    writer
        .write(&mut cursor, exif.little_endian())
        .with_context(|| format!("serialize EXIF for '{}'", source.display()))?;
    cursor.flush().ok();
    Ok(Some(cursor.into_inner()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_processed_counts_all_non_pending() {
        let summary = ExportSummary {
            total: 5,
            succeeded: 2,
            failed: 1,
            skipped: 1,
            ..Default::default()
        };
        assert_eq!(summary.processed(), 4);
        assert!(summary.processed() < summary.total);
    }

    #[test]
    fn summary_output_dir_uses_first_parent() {
        let summary = ExportSummary {
            outputs: vec![PathBuf::from("/tmp/out/a.jpg")],
            ..Default::default()
        };
        assert_eq!(summary.output_dir(), Some(PathBuf::from("/tmp/out")));
    }

    #[test]
    fn missing_source_is_an_error_not_a_panic() {
        let job = ExportJob {
            index: 0,
            source: PathBuf::from("/definitely/missing.dng"),
            output: std::env::temp_dir().join("rawpeg-missing.jpg"),
            params: EditParams::default(),
        };
        let outcome = export_one_caught(&job, &ExportSettings::default());
        assert!(outcome.result.is_err());
        assert!(!outcome.skipped);
    }
}
