//! Background worker: decodes thumbnails/proxies and renders edits off the UI
//! thread, reporting results back over an `mpsc` channel. Batch exports run on
//! their own thread so browsing and editing stay responsive.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use eframe::egui;
use log::{error, warn};

use crate::models::{EditParams, ExifSummary, ExportSettings};
use crate::raw_engine::auto_tone::auto_exposure;
use crate::raw_engine::decoder::{
    DecodeStatus, DecodedImage, decode_linear_scaled, decode_thumbnail_srgb, extract_exif_summary,
    probe_support, scan_directory,
};
use crate::raw_engine::exporter::{self, ExportEvent, ExportJob};
use crate::raw_engine::processor::{apply, encode_srgb};

use super::state::RgbHistogram;

/// Longest proxy side kept in memory for interactive editing.
pub const PROXY_MAX_DIM: usize = 1600;
/// Thumbnail side length for the library list.
pub const THUMB_MAX_DIM: u32 = 256;

/// A decoded sRGB8 image ready to be uploaded as a texture on the UI thread.
#[derive(Clone, Debug)]
pub struct TexturePayload {
    pub width: usize,
    pub height: usize,
    pub rgb: Vec<u8>,
}

/// Work item sent from the UI to the worker thread.
#[derive(Clone, Debug)]
pub enum Job {
    ScanDir {
        dir: PathBuf,
        epoch: u64,
    },
    Probe {
        epoch: u64,
        index: usize,
        path: PathBuf,
    },
    LoadThumbnail {
        epoch: u64,
        index: usize,
        path: PathBuf,
    },
    LoadProxy {
        token: u64,
        index: usize,
        path: PathBuf,
        params: EditParams,
    },
    RenderAfter {
        token: u64,
        params: EditParams,
    },
    AutoTone {
        token: u64,
    },
}

/// Result sent from the worker back to the UI.
#[derive(Clone, Debug)]
pub enum Message {
    Scanned {
        epoch: u64,
        paths: Vec<PathBuf>,
        error: Option<String>,
    },
    Probed {
        epoch: u64,
        index: usize,
        status: DecodeStatus,
        exif: ExifSummary,
    },
    Thumbnail {
        epoch: u64,
        index: usize,
        thumb: Option<TexturePayload>,
    },
    ProxyReady {
        token: u64,
        index: usize,
        width: usize,
        height: usize,
        before: TexturePayload,
        after: TexturePayload,
        histogram: RgbHistogram,
    },
    RenderReady {
        token: u64,
        payload: TexturePayload,
        histogram: RgbHistogram,
    },
    ProxyFailed {
        token: u64,
        index: usize,
        message: String,
    },
    AutoToneReady {
        token: u64,
        params: EditParams,
    },
    Export {
        token: u64,
        event: ExportEvent,
    },
}

/// Handle to the worker thread.
pub struct Worker {
    tx: Sender<Job>,
    rx: Receiver<Message>,
    msg_tx: Sender<Message>,
    ctx: Option<egui::Context>,
}

impl Worker {
    /// Spawns the worker. `ctx` is used to request repaints as results arrive;
    /// pass `None` in tests.
    pub fn spawn(ctx: Option<egui::Context>) -> Self {
        let (job_tx, job_rx) = mpsc::channel();
        let (msg_tx, msg_rx) = mpsc::channel();
        let worker_msg_tx = msg_tx.clone();
        let worker_ctx = ctx.clone();
        thread::Builder::new()
            .name("rawpeg-worker".to_string())
            .spawn(move || run(job_rx, worker_msg_tx, worker_ctx))
            .expect("failed to spawn rawpeg worker thread");
        Self {
            tx: job_tx,
            rx: msg_rx,
            msg_tx,
            ctx,
        }
    }

    pub fn send(&self, job: Job) {
        if let Err(e) = self.tx.send(job) {
            error!("worker channel closed: {e}");
        }
    }

    /// Non-blocking drain of all pending results (used once per UI frame).
    pub fn drain(&self) -> Vec<Message> {
        let mut out = Vec::new();
        while let Ok(message) = self.rx.try_recv() {
            out.push(message);
        }
        out
    }

    /// Blocking receive with a timeout (used by tests).
    pub fn recv_timeout(&self, timeout: Duration) -> Option<Message> {
        self.rx.recv_timeout(timeout).ok()
    }

    /// Starts a batch export on a dedicated thread. Progress and results are
    /// delivered as [`Message::Export`] values by [`Worker::drain`].
    pub fn start_export(
        &self,
        jobs: Vec<ExportJob>,
        settings: ExportSettings,
        token: u64,
        cancel: Arc<AtomicBool>,
    ) {
        let tx = self.msg_tx.clone();
        let ctx = self.ctx.clone();
        let tx_events = tx.clone();
        let ctx_events = ctx.clone();
        let spawn = thread::Builder::new()
            .name("rawpeg-export".to_string())
            .spawn(move || {
                let sender = Mutex::new(tx_events);
                let send = move |event: ExportEvent| {
                    if let Ok(sender) = sender.lock() {
                        let _ = sender.send(Message::Export { token, event });
                    }
                    if let Some(ctx) = &ctx_events {
                        ctx.request_repaint();
                    }
                };
                let result = catch_unwind(AssertUnwindSafe(|| {
                    exporter::export_batch(&jobs, &settings, &cancel, &send);
                }));
                if result.is_err() {
                    let _ = tx.send(Message::Export {
                        token,
                        event: ExportEvent::Fatal {
                            message: "export worker panicked".to_string(),
                        },
                    });
                    if let Some(ctx) = &ctx {
                        ctx.request_repaint();
                    }
                }
            });

        if let Err(error) = spawn {
            let _ = self.msg_tx.send(Message::Export {
                token,
                event: ExportEvent::Fatal {
                    message: format!("failed to start export thread: {error}"),
                },
            });
            if let Some(ctx) = &self.ctx {
                ctx.request_repaint();
            }
        }
    }
}

fn run(rx: Receiver<Job>, tx: Sender<Message>, ctx: Option<egui::Context>) {
    let mut proxy: Option<DecodedImage> = None;
    while let Ok(first) = rx.recv() {
        let mut batch = vec![first];
        while let Ok(job) = rx.try_recv() {
            batch.push(job);
        }

        for job in coalesce(batch) {
            if let Some(message) = execute(job, &mut proxy)
                && tx.send(message).is_err()
            {
                return;
            }
            if let Some(ctx) = &ctx {
                ctx.request_repaint();
            }
        }
    }
}

/// Collapses a batch of jobs: all probes/thumbnails/scans are kept in order,
/// while only the newest proxy load, auto-tone and render request survive. The
/// proxy load is executed before any render/auto-tone so it applies to the new
/// image.
fn coalesce(jobs: Vec<Job>) -> Vec<Job> {
    let mut others = Vec::new();
    let mut proxy = None;
    let mut auto_tone = None;
    let mut render = None;
    for job in jobs {
        match job {
            Job::LoadProxy { .. } => proxy = Some(job),
            Job::AutoTone { .. } => auto_tone = Some(job),
            Job::RenderAfter { .. } => render = Some(job),
            other => others.push(other),
        }
    }
    let mut out = others;
    out.extend(proxy);
    out.extend(auto_tone);
    out.extend(render);
    out
}

fn execute(job: Job, proxy: &mut Option<DecodedImage>) -> Option<Message> {
    match job {
        Job::ScanDir { dir, epoch } => Some(match scan_directory(&dir) {
            Ok(paths) => Message::Scanned {
                epoch,
                paths,
                error: None,
            },
            Err(e) => Message::Scanned {
                epoch,
                paths: Vec::new(),
                error: Some(format!("Scan failed: {e:#}")),
            },
        }),
        Job::Probe { epoch, index, path } => {
            let status = probe_support(&path);
            let exif = extract_exif_summary(&path).unwrap_or_default();
            Some(Message::Probed {
                epoch,
                index,
                status,
                exif,
            })
        }
        Job::LoadThumbnail { epoch, index, path } => {
            let thumb = match decode_thumbnail_srgb(&path, THUMB_MAX_DIM) {
                Ok((width, height, rgb)) => Some(TexturePayload {
                    width: width as usize,
                    height: height as usize,
                    rgb,
                }),
                Err(e) => {
                    warn!("thumbnail failed for '{}': {e:#}", path.display());
                    None
                }
            };
            Some(Message::Thumbnail {
                epoch,
                index,
                thumb,
            })
        }
        Job::LoadProxy {
            token,
            index,
            path,
            params,
        } => match decode_linear_scaled(&path, PROXY_MAX_DIM) {
            Ok(image) => {
                let width = image.width;
                let height = image.height;
                let before = encode_srgb(&image.data);
                let edited = apply(&image, &params);
                let histogram = RgbHistogram::from_linear(&edited);
                let after = encode_srgb(&edited);
                *proxy = Some(image);
                Some(Message::ProxyReady {
                    token,
                    index,
                    width,
                    height,
                    before: TexturePayload {
                        width,
                        height,
                        rgb: before,
                    },
                    after: TexturePayload {
                        width,
                        height,
                        rgb: after,
                    },
                    histogram,
                })
            }
            Err(e) => {
                *proxy = None;
                Some(Message::ProxyFailed {
                    token,
                    index,
                    message: format!("Failed to decode '{}': {e:#}", path.display()),
                })
            }
        },
        Job::RenderAfter { token, params } => {
            let image = proxy.as_ref()?;
            let edited = apply(image, &params);
            let histogram = RgbHistogram::from_linear(&edited);
            let rgb = encode_srgb(&edited);
            Some(Message::RenderReady {
                token,
                payload: TexturePayload {
                    width: image.width,
                    height: image.height,
                    rgb,
                },
                histogram,
            })
        }
        Job::AutoTone { token } => {
            let image = proxy.as_ref()?;
            Some(Message::AutoToneReady {
                token,
                params: auto_exposure(image),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(token: u64) -> Job {
        Job::RenderAfter {
            token,
            params: EditParams::default(),
        }
    }

    fn thumb(index: usize) -> Job {
        Job::LoadThumbnail {
            epoch: 0,
            index,
            path: PathBuf::from("x.dng"),
        }
    }

    fn probe(index: usize) -> Job {
        Job::Probe {
            epoch: 0,
            index,
            path: PathBuf::from("x.dng"),
        }
    }

    fn proxy(token: u64) -> Job {
        Job::LoadProxy {
            token,
            index: 0,
            path: PathBuf::from("x.dng"),
            params: EditParams::default(),
        }
    }

    fn job_key(job: &Job) -> u8 {
        match job {
            Job::ScanDir { .. } => 0,
            Job::Probe { .. } => 1,
            Job::LoadThumbnail { .. } => 2,
            Job::LoadProxy { .. } => 3,
            Job::RenderAfter { .. } => 4,
            Job::AutoTone { .. } => 5,
        }
    }

    #[test]
    fn coalesce_keeps_only_the_newest_render() {
        let out = coalesce(vec![render(1), render(2), render(3)]);
        assert_eq!(out.len(), 1);
        match &out[0] {
            Job::RenderAfter { token, .. } => assert_eq!(*token, 3),
            other => panic!("expected render, got {other:?}"),
        }
    }

    #[test]
    fn coalesce_keeps_all_thumbnails_in_order() {
        let out = coalesce(vec![thumb(0), thumb(1), thumb(2)]);
        assert_eq!(out.len(), 3);
        for (i, job) in out.iter().enumerate() {
            match job {
                Job::LoadThumbnail { index, .. } => assert_eq!(*index, i),
                other => panic!("expected thumbnail, got {other:?}"),
            }
        }
    }

    #[test]
    fn coalesce_keeps_probes_in_order() {
        let out = coalesce(vec![probe(0), probe(1), probe(2)]);
        assert_eq!(out.len(), 3);
        for (i, job) in out.iter().enumerate() {
            match job {
                Job::Probe { index, .. } => assert_eq!(*index, i),
                other => panic!("expected probe, got {other:?}"),
            }
        }
    }

    #[test]
    fn coalesce_keeps_last_proxy_and_runs_it_before_render() {
        let out = coalesce(vec![render(1), proxy(2), proxy(3), thumb(9)]);
        assert_eq!(out.len(), 3, "one thumbnail + one proxy + one render");
        assert_eq!(job_key(&out[0]), 2, "thumbnail first");
        match &out[1] {
            Job::LoadProxy { token, .. } => assert_eq!(*token, 3, "newest proxy wins"),
            other => panic!("expected proxy, got {other:?}"),
        }
        match &out[2] {
            Job::RenderAfter { token, .. } => assert_eq!(*token, 1),
            other => panic!("expected render last, got {other:?}"),
        }
    }

    #[test]
    fn coalesce_keeps_last_auto_tone() {
        let out = coalesce(vec![Job::AutoTone { token: 1 }, Job::AutoTone { token: 2 }]);
        assert_eq!(out.len(), 1);
        match &out[0] {
            Job::AutoTone { token } => assert_eq!(*token, 2),
            other => panic!("expected auto-tone, got {other:?}"),
        }
    }
}
