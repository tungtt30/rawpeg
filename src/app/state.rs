//! Darkroom UI state: library, selection, viewport, edit clipboard, export
//! session and the live RGB histogram.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use eframe::egui::{self, TextureHandle};

use crate::models::{EditParams, EditStatus, ExifSummary, ExportSettings, ImageItem};
use crate::raw_engine::decoder::DecodeStatus;
use crate::raw_engine::exporter::{ExportEvent, ExportJob, ExportSummary};

use super::worker::{Job, Message, TexturePayload, Worker};

pub const HIST_BINS: usize = 256;

/// Three 256-bin sRGB histograms (one per channel) of the edited proxy buffer.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RgbHistogram {
    pub r: [u32; HIST_BINS],
    pub g: [u32; HIST_BINS],
    pub b: [u32; HIST_BINS],
}

impl Default for RgbHistogram {
    fn default() -> Self {
        Self {
            r: [0; HIST_BINS],
            g: [0; HIST_BINS],
            b: [0; HIST_BINS],
        }
    }
}

impl RgbHistogram {
    /// Builds a histogram from an edited linear buffer, clamping channels to
    /// `0.0..=1.0` (values above 1.0 land in the last bin).
    pub fn from_linear(data: &[[f32; 3]]) -> Self {
        let mut hist = Self::default();
        for px in data {
            hist.r[bin_of(px[0])] += 1;
            hist.g[bin_of(px[1])] += 1;
            hist.b[bin_of(px[2])] += 1;
        }
        hist
    }

    /// Largest count across the three channels, at least 1 (safe divisor).
    pub fn max(&self) -> u32 {
        let top = |bins: &[u32; HIST_BINS]| bins.iter().copied().max().unwrap_or(0);
        top(&self.r).max(top(&self.g)).max(top(&self.b)).max(1)
    }

    /// Number of pixels represented by the R channel (equals the image area).
    pub fn pixel_count(&self) -> u32 {
        self.r.iter().sum()
    }
}

fn bin_of(value: f32) -> usize {
    ((value.clamp(0.0, 1.0) * (HIST_BINS - 1) as f32).round() as usize).min(HIST_BINS - 1)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    #[default]
    Library,
    Develop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RightTab {
    Develop,
    #[default]
    Export,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LibraryFilter {
    #[default]
    All,
    Selected,
    Edited,
    Issues,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SelectionStats {
    pub total: usize,
    pub selected: usize,
    pub selected_supported: usize,
    pub selected_unsupported: usize,
    pub selected_pending: usize,
    pub edited: usize,
    pub unsupported: usize,
}

impl SelectionStats {
    /// Files that will be attempted by the exporter (known supported + pending).
    pub fn export_count(&self) -> usize {
        self.selected_supported + self.selected_pending
    }

    pub fn all_selected(&self) -> bool {
        self.total > 0 && self.selected == self.total
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Error,
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub message: String,
    pub kind: ToastKind,
    pub expires: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmAction {
    ClearLibrary,
    ResetSelected,
}

/// One file in the library, with its lazily-loaded thumbnail and probe status.
pub struct LibraryEntry {
    pub item: ImageItem,
    /// `None` while the background probe has not returned yet.
    pub status: Option<DecodeStatus>,
    pub thumb: Option<TextureHandle>,
    pub thumb_requested: bool,
}

/// Viewport transform: `zoom` multiplies the "fit to view" scale, `pan` is a
/// pixel offset from the centred position.
#[derive(Clone, Copy, Debug)]
pub struct Viewport {
    pub zoom: f32,
    pub pan: egui::Vec2,
    pub fit: f32,
    pub request_actual: bool,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan: egui::Vec2::ZERO,
            fit: 1.0,
            request_actual: false,
        }
    }
}

impl Viewport {
    pub fn reset(&mut self) {
        self.zoom = 1.0;
        self.pan = egui::Vec2::ZERO;
        self.request_actual = false;
    }
}

/// A freshly decoded proxy: both textures (unedited/edited) plus histogram.
pub struct ProxyRender {
    pub width: usize,
    pub height: usize,
    pub before: TexturePayload,
    pub after: TexturePayload,
    pub histogram: RgbHistogram,
}

/// Live state of a background batch export.
pub struct ExportSession {
    pub token: u64,
    pub total: usize,
    pub done: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub skipped: usize,
    pub current: Option<String>,
    pub started: Instant,
    pub cancel: Arc<AtomicBool>,
    pub cancelling: bool,
    pub finished: bool,
    pub summary: Option<ExportSummary>,
    pub fatal: Option<String>,
}

impl ExportSession {
    pub fn new(token: u64, total: usize, cancel: Arc<AtomicBool>) -> Self {
        Self {
            token,
            total,
            done: 0,
            succeeded: 0,
            failed: 0,
            skipped: 0,
            current: None,
            started: Instant::now(),
            cancel,
            cancelling: false,
            finished: false,
            summary: None,
            fatal: None,
        }
    }

    pub fn progress(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            (self.done as f32 / self.total as f32).clamp(0.0, 1.0)
        }
    }

    pub fn elapsed(&self) -> Duration {
        self.summary
            .as_ref()
            .map(|summary| summary.elapsed)
            .unwrap_or_else(|| self.started.elapsed())
    }

    pub fn apply(&mut self, event: &ExportEvent) {
        match event {
            ExportEvent::Started { total } => self.total = *total,
            ExportEvent::ItemStarted { current } => self.current = Some(current.clone()),
            ExportEvent::ItemFinished { outcome } => {
                self.done = self.done.saturating_add(1).min(self.total);
                if outcome.skipped {
                    self.skipped += 1;
                } else if outcome.result.is_ok() {
                    self.succeeded += 1;
                } else {
                    self.failed += 1;
                }
                self.current = None;
            }
            ExportEvent::Finished { summary } => {
                self.total = summary.total;
                self.done = summary.processed();
                self.succeeded = summary.succeeded;
                self.failed = summary.failed;
                self.skipped = summary.skipped;
                self.current = None;
                self.cancelling = false;
                self.finished = true;
                self.summary = Some(summary.clone());
            }
            ExportEvent::Fatal { message } => {
                self.current = None;
                self.fatal = Some(message.clone());
                self.finished = true;
            }
        }
    }
}

/// Root UI state.
#[derive(Default)]
pub struct AppState {
    pub entries: Vec<LibraryEntry>,
    pub current: Option<usize>,
    /// Texture of the unedited proxy (neutral `EditParams`).
    pub before: Option<TextureHandle>,
    /// Texture of the proxy rendered with the current parameters.
    pub after: Option<TextureHandle>,
    pub image_size: [usize; 2],
    pub histogram: RgbHistogram,
    pub viewport: Viewport,
    pub clipboard: Option<EditParams>,
    /// Incremented on every proxy load; stale worker results are dropped.
    pub proxy_token: u64,
    pub proxy_loading: bool,
    /// True after a proxy decode failed; stops the automatic Develop reload loop.
    pub proxy_failed: bool,
    /// Incremented when the library is replaced/cleared so stale scan/probe/
    /// thumbnail messages from before the clear cannot corrupt the new library.
    pub library_gen: u64,
    pub status: String,
    /// Directory of the most recent folder scan (used as dialog start dir).
    pub last_dir: Option<PathBuf>,
    pub view_mode: ViewMode,
    pub right_tab: RightTab,
    pub filter: LibraryFilter,
    pub last_clicked: Option<usize>,
    pub show_help: bool,
    pub pending_confirm: Option<ConfirmAction>,
    pub show_before: bool,
    pub export_settings: ExportSettings,
    pub export: Option<ExportSession>,
    pub export_token: u64,
    pub toast: Option<Toast>,
}

/// Computes the edit badge for a parameter set.
pub fn edit_status(params: &EditParams) -> EditStatus {
    if params.is_neutral() {
        EditStatus::Original
    } else {
        EditStatus::Edited
    }
}

impl AppState {
    /// Adds paths, skipping duplicates. Returns the indices of new entries.
    pub fn add_paths(&mut self, paths: impl IntoIterator<Item = PathBuf>) -> Vec<usize> {
        let mut added = Vec::new();
        for path in paths {
            if self.entries.iter().any(|e| e.item.path == path) {
                continue;
            }
            self.entries.push(LibraryEntry {
                item: ImageItem::new(path),
                status: None,
                thumb: None,
                thumb_requested: false,
            });
            added.push(self.entries.len() - 1);
        }
        added
    }

    /// Adds paths, queues cheap probes for all of them and selects the first
    /// as current without decoding a proxy (Library-first flow).
    pub fn add_imported(&mut self, worker: &Worker, paths: Vec<PathBuf>) -> Vec<usize> {
        let added = self.add_paths(paths);
        if added.is_empty() {
            return added;
        }
        self.status = format!("{} file(s) in library", self.entries.len());
        for &index in &added {
            let path = self.entries[index].item.path.clone();
            worker.send(Job::Probe {
                epoch: self.library_gen,
                index,
                path,
            });
        }
        if self.current.is_none()
            && let Some(&first) = added.first()
        {
            self.select_only(first);
        }
        added
    }

    /// Selects an entry, resets the viewport and bumps the proxy token.
    ///
    /// Returns `(token, path, params)` so the caller can enqueue a proxy load.
    pub fn select(&mut self, index: usize) -> Option<(u64, PathBuf, EditParams)> {
        let entry = self.entries.get(index)?;
        let path = entry.item.path.clone();
        let params = entry.item.params.clone();
        self.current = Some(index);
        self.proxy_token += 1;
        self.proxy_loading = true;
        self.proxy_failed = false;
        self.before = None;
        self.after = None;
        self.image_size = [0, 0];
        self.histogram = RgbHistogram::default();
        self.viewport.reset();
        self.status = format!("Loading {}…", entry.item.file_name);
        Some((self.proxy_token, path, params))
    }

    /// Selects without touching the proxy/textures. Used by Library clicks.
    pub fn select_only(&mut self, index: usize) -> bool {
        if self.entries.get(index).is_none() {
            return false;
        }
        if self.current == Some(index) {
            return true;
        }
        self.current = Some(index);
        self.proxy_token += 1;
        self.proxy_loading = false;
        self.proxy_failed = false;
        self.before = None;
        self.after = None;
        self.image_size = [0, 0];
        self.histogram = RgbHistogram::default();
        self.viewport.reset();
        true
    }

    pub fn current_entry(&self) -> Option<&LibraryEntry> {
        self.entries.get(self.current?)
    }

    pub fn current_is_selected(&self) -> bool {
        self.current_entry()
            .map(|e| e.item.selected)
            .unwrap_or(false)
    }

    /// Snapshot of the current render request `(token, params)`.
    pub fn render_request(&self) -> Option<(u64, EditParams)> {
        let entry = self.current_entry()?;
        Some((self.proxy_token, entry.item.params.clone()))
    }

    pub fn copy_current(&mut self) {
        if let Some(entry) = self.current_entry() {
            self.clipboard = Some(entry.item.params.clone());
        }
    }

    /// Applies `params` to every selected entry; returns how many were changed.
    pub fn paste_to_selected(&mut self, params: &EditParams) -> usize {
        let status = edit_status(params);
        let mut changed = 0;
        for entry in &mut self.entries {
            if entry.item.selected {
                entry.item.params = params.clone();
                entry.item.status = status;
                changed += 1;
            }
        }
        changed
    }

    /// Applies the current entry's params to the selected entries.
    pub fn sync_to_selected(&mut self) -> usize {
        let Some(index) = self.current else {
            return 0;
        };
        let Some(entry) = self.entries.get(index) else {
            return 0;
        };
        let params = entry.item.params.clone();
        self.paste_to_selected(&params)
    }

    /// Resets the current entry to neutral parameters.
    pub fn reset_current(&mut self) -> Option<(u64, EditParams)> {
        let token = self.proxy_token;
        let entry = self.entries.get_mut(self.current?)?;
        entry.item.params = EditParams::default();
        entry.item.status = EditStatus::Original;
        Some((token, EditParams::default()))
    }

    /// Resets every selected entry to neutral parameters.
    pub fn reset_selected(&mut self) -> usize {
        let mut changed = 0;
        for entry in &mut self.entries {
            if entry.item.selected {
                entry.item.params = EditParams::default();
                entry.item.status = EditStatus::Original;
                changed += 1;
            }
        }
        changed
    }

    /// Stores new parameters for the current entry and refreshes its badge.
    pub fn apply_params(&mut self, params: EditParams) {
        if let Some(index) = self.current
            && let Some(entry) = self.entries.get_mut(index)
        {
            entry.item.status = edit_status(&params);
            entry.item.params = params;
        }
    }

    pub fn set_all_selected(&mut self, selected: bool) {
        for entry in &mut self.entries {
            entry.item.selected = selected;
        }
    }

    /// Selects every file that is not known to be unsupported.
    pub fn select_supported(&mut self) {
        for entry in &mut self.entries {
            let unsupported = entry
                .status
                .as_ref()
                .is_some_and(DecodeStatus::is_unsupported);
            entry.item.selected = !unsupported;
        }
    }

    pub fn deselect_unsupported(&mut self) {
        for entry in &mut self.entries {
            if entry
                .status
                .as_ref()
                .is_some_and(DecodeStatus::is_unsupported)
            {
                entry.item.selected = false;
            }
        }
    }

    pub fn set_selected(&mut self, index: usize, selected: bool) {
        if let Some(entry) = self.entries.get_mut(index) {
            entry.item.selected = selected;
        }
    }

    pub fn toggle_selected(&mut self, index: usize) {
        if let Some(entry) = self.entries.get_mut(index) {
            entry.item.selected = !entry.item.selected;
        }
    }

    pub fn select_range(&mut self, from: usize, to: usize) {
        let (start, end) = if from <= to { (from, to) } else { (to, from) };
        for index in start..=end {
            if let Some(entry) = self.entries.get_mut(index) {
                entry.item.selected = true;
            }
        }
    }

    pub fn selection_stats(&self) -> SelectionStats {
        let mut stats = SelectionStats::default();
        for entry in &self.entries {
            stats.total += 1;
            if entry.item.selected {
                stats.selected += 1;
                match &entry.status {
                    None => stats.selected_pending += 1,
                    Some(status) if status.is_unsupported() => stats.selected_unsupported += 1,
                    Some(_) => stats.selected_supported += 1,
                }
            }
            if entry.item.status == EditStatus::Edited {
                stats.edited += 1;
            }
            if entry
                .status
                .as_ref()
                .is_some_and(DecodeStatus::is_unsupported)
            {
                stats.unsupported += 1;
            }
        }
        stats
    }

    pub fn visible_indices(&self) -> Vec<usize> {
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| match self.filter {
                LibraryFilter::All => true,
                LibraryFilter::Selected => entry.item.selected,
                LibraryFilter::Edited => entry.item.status == EditStatus::Edited,
                LibraryFilter::Issues => entry
                    .status
                    .as_ref()
                    .is_some_and(DecodeStatus::is_unsupported),
            })
            .map(|(index, _)| index)
            .collect()
    }

    pub fn ensure_thumb(&mut self, worker: &Worker, index: usize) {
        let Some(entry) = self.entries.get_mut(index) else {
            return;
        };
        if entry.thumb.is_some() || entry.thumb_requested {
            return;
        }
        if entry
            .status
            .as_ref()
            .is_some_and(DecodeStatus::is_unsupported)
        {
            return;
        }
        entry.thumb_requested = true;
        let path = entry.item.path.clone();
        let epoch = self.library_gen;
        worker.send(Job::LoadThumbnail { epoch, index, path });
    }

    /// Clears the whole library. The running export (if any) keeps its snapshot.
    pub fn clear_library(&mut self) {
        self.entries.clear();
        self.current = None;
        self.library_gen += 1;
        self.proxy_token += 1;
        self.proxy_loading = false;
        self.proxy_failed = false;
        self.before = None;
        self.after = None;
        self.image_size = [0, 0];
        self.histogram = RgbHistogram::default();
        self.viewport.reset();
        self.filter = LibraryFilter::All;
        self.last_clicked = None;
        self.status = "Library cleared".to_string();
    }

    pub fn export_jobs(&self) -> Vec<ExportJob> {
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                entry.item.selected
                    && !entry
                        .status
                        .as_ref()
                        .is_some_and(DecodeStatus::is_unsupported)
            })
            .map(|(index, entry)| ExportJob {
                index,
                source: entry.item.path.clone(),
                output: self.export_settings.output_path(&entry.item.path),
                params: entry.item.params.clone(),
            })
            .collect()
    }

    pub fn begin_export(&mut self, worker: &Worker) {
        if self
            .export
            .as_ref()
            .is_some_and(|session| !session.finished)
        {
            return;
        }
        let jobs = self.export_jobs();
        if jobs.is_empty() {
            self.toast("Select at least one supported photo", ToastKind::Error);
            return;
        }
        self.export_token += 1;
        let token = self.export_token;
        let total = jobs.len();
        let cancel = Arc::new(AtomicBool::new(false));
        self.export = Some(ExportSession::new(token, total, cancel.clone()));
        self.right_tab = RightTab::Export;
        self.toast(format!("Exporting {total} photo(s)…"), ToastKind::Info);
        worker.start_export(jobs, self.export_settings.clone(), token, cancel);
    }

    pub fn cancel_export(&mut self) {
        if let Some(session) = &mut self.export {
            session.cancel.store(true, Ordering::Relaxed);
            session.cancelling = true;
        }
    }

    pub fn dismiss_export(&mut self) {
        self.export = None;
    }

    pub fn toast(&mut self, message: impl Into<String>, kind: ToastKind) {
        self.toast = Some(Toast {
            message: message.into(),
            kind,
            expires: Instant::now() + Duration::from_secs(4),
        });
    }

    pub fn poll_toast(&mut self, ctx: &egui::Context) {
        if let Some(toast) = &self.toast {
            if Instant::now() >= toast.expires {
                self.toast = None;
            } else {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
        }
    }

    pub fn set_probed(&mut self, index: usize, status: DecodeStatus, exif: ExifSummary) {
        let Some(entry) = self.entries.get_mut(index) else {
            return;
        };
        entry.status = Some(status);
        entry.item.exif = exif;
    }

    pub fn set_thumb(
        &mut self,
        ctx: &egui::Context,
        index: usize,
        payload: Option<TexturePayload>,
    ) {
        let Some(entry) = self.entries.get_mut(index) else {
            return;
        };
        entry.thumb_requested = true;
        if let Some(payload) = payload {
            entry.thumb = Some(load_texture(ctx, format!("thumb-{index}"), payload));
        }
    }

    /// Applies a freshly decoded proxy (before + after textures and histogram).
    pub fn set_proxy(&mut self, ctx: &egui::Context, token: u64, render: ProxyRender) -> bool {
        if token != self.proxy_token {
            return false;
        }
        let ProxyRender {
            width,
            height,
            before,
            after,
            histogram,
        } = render;
        self.image_size = [width, height];
        self.before = Some(load_texture(ctx, "before", before));
        self.after = Some(load_texture(ctx, "after", after));
        self.histogram = histogram;
        self.proxy_loading = false;
        self.proxy_failed = false;
        self.viewport.reset();
        self.status = format!("{width} × {height}");
        true
    }

    /// Records a failed proxy load. Stale token/index results are ignored.
    pub fn set_proxy_failed(&mut self, token: u64, index: usize, message: String) {
        if token != self.proxy_token || self.current != Some(index) {
            return;
        }
        self.proxy_loading = false;
        self.proxy_failed = true;
        self.status = message.clone();
        self.toast(message, ToastKind::Error);
    }

    /// Applies a re-rendered "after" texture for the current parameters.
    pub fn set_after(
        &mut self,
        ctx: &egui::Context,
        token: u64,
        payload: TexturePayload,
        histogram: RgbHistogram,
    ) -> bool {
        if token != self.proxy_token {
            return false;
        }
        if let Some(tex) = &mut self.after {
            *tex = load_texture(ctx, "after", payload);
        } else {
            self.after = Some(load_texture(ctx, "after", payload));
        }
        self.histogram = histogram;
        true
    }

    /// Drains worker messages and applies them to the UI state.
    pub fn poll(&mut self, ctx: &egui::Context, worker: &Worker) {
        for message in worker.drain() {
            match message {
                Message::Scanned {
                    epoch,
                    paths,
                    error,
                } => {
                    if epoch != self.library_gen {
                        continue;
                    }
                    if let Some(error) = error {
                        self.status = error.clone();
                        self.toast(error, ToastKind::Error);
                    }
                    self.add_imported(worker, paths);
                }
                Message::Probed {
                    epoch,
                    index,
                    status,
                    exif,
                } => {
                    if epoch == self.library_gen {
                        self.set_probed(index, status, exif);
                    }
                }
                Message::Thumbnail {
                    epoch,
                    index,
                    thumb,
                } => {
                    if epoch == self.library_gen {
                        self.set_thumb(ctx, index, thumb);
                    }
                }
                Message::ProxyReady {
                    token,
                    index,
                    width,
                    height,
                    before,
                    after,
                    histogram,
                } => {
                    if self.current == Some(index) {
                        self.set_proxy(
                            ctx,
                            token,
                            ProxyRender {
                                width,
                                height,
                                before,
                                after,
                                histogram,
                            },
                        );
                    }
                }
                Message::RenderReady {
                    token,
                    payload,
                    histogram,
                } => {
                    self.set_after(ctx, token, payload, histogram);
                }
                Message::AutoToneReady { token, params } => {
                    if token == self.proxy_token && self.current.is_some() {
                        self.apply_params(params.clone());
                        worker.send(Job::RenderAfter { token, params });
                    }
                }
                Message::Export { token, event } => {
                    let mut finished = None;
                    if let Some(session) = &mut self.export
                        && session.token == token
                    {
                        session.apply(&event);
                        if session.finished {
                            finished = Some(session_toast(session));
                        }
                    }
                    if let Some((message, kind)) = finished {
                        self.toast(message, kind);
                    }
                }
                Message::ProxyFailed {
                    token,
                    index,
                    message,
                } => self.set_proxy_failed(token, index, message),
            }
        }
    }
}

fn session_toast(session: &ExportSession) -> (String, ToastKind) {
    if let Some(fatal) = &session.fatal {
        return (fatal.clone(), ToastKind::Error);
    }
    match &session.summary {
        Some(summary) if summary.cancelled => (
            format!(
                "Export cancelled after {}/{} photo(s)",
                summary.processed(),
                summary.total
            ),
            ToastKind::Info,
        ),
        Some(summary) if summary.failed > 0 => (
            format!(
                "Export finished: {} ok, {} failed",
                summary.succeeded, summary.failed
            ),
            ToastKind::Error,
        ),
        Some(summary) => (
            format!("Exported {} photo(s)", summary.succeeded),
            ToastKind::Success,
        ),
        None => ("Export finished".to_string(), ToastKind::Info),
    }
}

/// Selects `index` and enqueues a proxy load for it.
pub fn select_and_load(state: &mut AppState, worker: &Worker, index: usize) {
    if state.current == Some(index) && (state.proxy_loading || state.after.is_some()) {
        return;
    }
    if let Some((token, path, params)) = state.select(index) {
        worker.send(Job::LoadProxy {
            token,
            index,
            path,
            params,
        });
    }
}

/// Loads the current proxy if the Develop view needs it.
pub fn ensure_current_loaded(state: &mut AppState, worker: &Worker) {
    if state.view_mode != ViewMode::Develop {
        return;
    }
    let Some(index) = state.current else {
        return;
    };
    if state.after.is_none() && !state.proxy_loading && !state.proxy_failed {
        select_and_load(state, worker, index);
    }
}

fn load_texture(
    ctx: &egui::Context,
    name: impl Into<String>,
    payload: TexturePayload,
) -> TextureHandle {
    let image = egui::ColorImage::from_rgb([payload.width, payload.height], &payload.rgb);
    ctx.load_texture(name, image, egui::TextureOptions::LINEAR)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ExportFormat;
    use crate::raw_engine::exporter::{ExportOutcome, ExportSummary};

    fn state_with(paths: &[&str]) -> AppState {
        let mut state = AppState::default();
        state.add_paths(paths.iter().map(PathBuf::from));
        state
    }

    #[test]
    fn add_paths_skips_duplicates() {
        let mut state = state_with(&["a.dng", "b.cr2"]);
        let added = state.add_paths([PathBuf::from("b.cr2"), PathBuf::from("c.nef")]);
        assert_eq!(added, vec![2]);
        assert_eq!(state.entries.len(), 3);
    }

    #[test]
    fn select_bumps_token_resets_viewport_and_clears_textures() {
        let mut state = state_with(&["a.dng", "b.cr2"]);
        let (token1, path1, _) = state.select(0).unwrap();
        state.viewport.zoom = 4.0;
        let (token2, path2, _) = state.select(1).unwrap();
        assert_eq!(path1, PathBuf::from("a.dng"));
        assert_eq!(path2, PathBuf::from("b.cr2"));
        assert_eq!(token2, token1 + 1);
        assert_eq!(state.current, Some(1));
        assert_eq!(state.viewport.zoom, 1.0);
        assert!(state.before.is_none());
        assert!(state.after.is_none());
        assert!(state.proxy_loading);
    }

    #[test]
    fn select_only_same_index_keeps_loaded_textures() {
        let mut state = state_with(&["a.dng"]);
        state.select_only(0);
        assert_eq!(state.current, Some(0));
        assert!(!state.proxy_loading);
        assert!(state.select_only(0));
        assert_eq!(state.current, Some(0));
    }

    #[test]
    fn edit_status_tracks_neutral_parameters() {
        let mut state = state_with(&["a.dng"]);
        state.select(0).unwrap();
        state.apply_params(EditParams {
            exposure: 1.0,
            ..Default::default()
        });
        assert_eq!(state.entries[0].item.status, EditStatus::Edited);
        state.apply_params(EditParams::default());
        assert_eq!(state.entries[0].item.status, EditStatus::Original);
    }

    #[test]
    fn paste_only_touches_selected_entries() {
        let mut state = state_with(&["a.dng", "b.dng", "c.dng"]);
        state.entries[0].item.selected = true;
        state.entries[1].item.selected = false;
        state.entries[2].item.selected = true;
        let params = EditParams {
            contrast: 25,
            ..Default::default()
        };
        let changed = state.paste_to_selected(&params);
        assert_eq!(changed, 2);
        assert_eq!(state.entries[0].item.params, params);
        assert!(state.entries[1].item.params.is_neutral());
        assert_eq!(state.entries[2].item.params, params);
    }

    #[test]
    fn sync_to_selected_copies_current_params() {
        let mut state = state_with(&["a.dng", "b.dng"]);
        state.select(0).unwrap();
        state.apply_params(EditParams {
            exposure: 0.75,
            ..Default::default()
        });
        state.entries[1].item.selected = true;
        let changed = state.sync_to_selected();
        assert_eq!(changed, 2);
        assert_eq!(state.entries[1].item.params.exposure, 0.75);
        assert_eq!(state.entries[1].item.status, EditStatus::Edited);
    }

    #[test]
    fn reset_selected_restores_neutral() {
        let mut state = state_with(&["a.dng", "b.dng"]);
        state.entries[0].item.params.shadows = 40;
        state.entries[1].item.params.exposure = 1.0;
        state.entries[1].item.selected = false;
        let changed = state.reset_selected();
        assert_eq!(changed, 1);
        assert!(state.entries[0].item.params.is_neutral());
        assert_eq!(state.entries[1].item.params.exposure, 1.0);
    }

    #[test]
    fn reset_current_restores_neutral_and_original_badge() {
        let mut state = state_with(&["a.dng"]);
        state.select(0).unwrap();
        state.apply_params(EditParams {
            shadows: 40,
            ..Default::default()
        });
        let (token, params) = state.reset_current().unwrap();
        assert_eq!(token, state.proxy_token);
        assert!(params.is_neutral());
        assert!(state.entries[0].item.params.is_neutral());
        assert_eq!(state.entries[0].item.status, EditStatus::Original);
    }

    #[test]
    fn selection_stats_separates_supported_unsupported_and_pending() {
        let mut state = state_with(&["a.dng", "b.dng", "c.dng"]);
        state.set_probed(0, DecodeStatus::Supported, ExifSummary::default());
        state.set_probed(
            1,
            DecodeStatus::UnsupportedCamera("old table".into()),
            ExifSummary::default(),
        );
        state.entries[2].item.selected = false;
        let stats = state.selection_stats();
        assert_eq!(stats.total, 3);
        assert_eq!(stats.selected, 2);
        assert_eq!(stats.selected_supported, 1);
        assert_eq!(stats.selected_unsupported, 1);
        assert_eq!(stats.selected_pending, 0);
        assert_eq!(stats.export_count(), 1);
        assert_eq!(stats.unsupported, 1);
    }

    #[test]
    fn visible_indices_follow_filter() {
        let mut state = state_with(&["a.dng", "b.dng", "c.dng"]);
        state.entries[0].item.selected = false;
        state.entries[1].item.status = EditStatus::Edited;
        state.set_probed(
            2,
            DecodeStatus::UnsupportedFormat("bad".into()),
            ExifSummary::default(),
        );
        state.filter = LibraryFilter::Selected;
        assert_eq!(state.visible_indices(), vec![1, 2]);
        state.filter = LibraryFilter::Edited;
        assert_eq!(state.visible_indices(), vec![1]);
        state.filter = LibraryFilter::Issues;
        assert_eq!(state.visible_indices(), vec![2]);
        state.filter = LibraryFilter::All;
        assert_eq!(state.visible_indices(), vec![0, 1, 2]);
    }

    #[test]
    fn export_jobs_skip_unsupported_and_use_output_settings() {
        let mut state = state_with(&["/a.dng", "/b.dng", "/c.dng"]);
        state.export_settings = ExportSettings {
            format: ExportFormat::Png,
            output_dir: Some(PathBuf::from("/out")),
            ..Default::default()
        };
        state.set_probed(
            0,
            DecodeStatus::UnsupportedCamera("no".into()),
            ExifSummary::default(),
        );
        state.entries[2].item.selected = false;
        let jobs = state.export_jobs();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].source, PathBuf::from("/b.dng"));
        assert_eq!(jobs[0].output, PathBuf::from("/out/b.png"));
    }

    #[test]
    fn export_session_applies_progress_events() {
        let cancel = Arc::new(AtomicBool::new(false));
        let mut session = ExportSession::new(3, 3, cancel);
        session.apply(&ExportEvent::Started { total: 3 });
        session.apply(&ExportEvent::ItemStarted {
            current: "a.dng".into(),
        });
        assert_eq!(session.current.as_deref(), Some("a.dng"));
        session.apply(&ExportEvent::ItemFinished {
            outcome: ExportOutcome {
                source: PathBuf::from("a.dng"),
                output: PathBuf::from("a.jpg"),
                result: Ok(()),
                skipped: false,
            },
        });
        assert_eq!(session.succeeded, 1);
        assert_eq!(session.done, 1);
        let summary = ExportSummary {
            total: 3,
            succeeded: 1,
            failed: 1,
            skipped: 1,
            elapsed: Duration::from_secs(2),
            ..Default::default()
        };
        session.apply(&ExportEvent::Finished {
            summary: summary.clone(),
        });
        assert!(session.finished);
        assert_eq!(session.done, 3);
        assert_eq!(session.elapsed(), Duration::from_secs(2));
        assert_eq!(session.progress(), 1.0);
    }

    #[test]
    fn histogram_totals_match_pixel_count() {
        let data = vec![[0.0, 0.5, 1.0], [1.0, 0.5, 0.0], [0.25, 0.25, 0.25]];
        let hist = RgbHistogram::from_linear(&data);
        assert_eq!(hist.pixel_count(), 3);
        assert_eq!(hist.r.iter().sum::<u32>(), 3);
        assert_eq!(hist.g.iter().sum::<u32>(), 3);
        assert_eq!(hist.b.iter().sum::<u32>(), 3);
    }

    #[test]
    fn histogram_extremes_land_in_edge_bins() {
        let hist = RgbHistogram::from_linear(&[[0.0, 0.0, 0.0], [4.0, 1.0, 2.0]]);
        assert_eq!(hist.r[0], 1, "black in bin 0");
        assert_eq!(hist.r[HIST_BINS - 1], 1, "over-range clamps to last bin");
        assert_eq!(hist.g[HIST_BINS - 1], 1, "exactly 1.0 in last bin");
    }

    #[test]
    fn empty_histogram_has_unit_max() {
        let hist = RgbHistogram::default();
        assert_eq!(hist.pixel_count(), 0);
        assert_eq!(hist.max(), 1);
    }
}

#[cfg(test)]
mod review_regression_tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn proxy_failure_stops_auto_reload() {
        let mut state = AppState::default();
        state.add_paths([PathBuf::from("a.dng")]);
        state.view_mode = ViewMode::Develop;
        state.select(0).unwrap();
        let token = state.proxy_token;
        state.set_proxy_failed(token, 0, "cannot decode".into());

        assert!(state.proxy_failed);
        assert!(!state.proxy_loading);

        let worker = Worker::spawn(None);
        ensure_current_loaded(&mut state, &worker);
        assert!(!state.proxy_loading);
        assert!(state.proxy_failed);

        state.select(0).unwrap();
        assert!(state.proxy_loading);
        assert!(!state.proxy_failed);
    }

    #[test]
    fn clear_library_ignores_stale_probe_results() {
        let mut state = AppState::default();
        state.add_paths([PathBuf::from("old.dng")]);
        let old_epoch = state.library_gen;
        let worker = Worker::spawn(None);
        let ctx = egui::Context::default();

        state.clear_library();
        state.add_paths([PathBuf::from("new.dng")]);
        worker.send(Job::Probe {
            epoch: old_epoch,
            index: 0,
            path: PathBuf::from("old.dng"),
        });

        for _ in 0..20 {
            state.poll(&ctx, &worker);
            std::thread::sleep(Duration::from_millis(5));
        }

        assert!(
            state.entries[0].status.is_none(),
            "stale probe corrupted the new library entry"
        );
    }

    #[test]
    fn finished_export_can_be_replaced_by_a_new_one() {
        let mut state = AppState::default();
        state.add_paths([PathBuf::from("a.dng")]);
        let cancel = Arc::new(AtomicBool::new(false));
        let mut old = ExportSession::new(99, 1, cancel);
        old.finished = true;
        state.export = Some(old);

        let worker = Worker::spawn(None);
        state.begin_export(&worker);
        let session = state.export.as_ref().expect("new export session");
        assert!(!session.finished);
        assert_ne!(session.token, 99);
        assert_eq!(session.total, 1);
    }
}
