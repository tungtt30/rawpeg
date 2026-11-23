//! Darkroom UI state: library, selection, viewport, edit clipboard and the
//! live RGB histogram.
//!
//! The state layer is intentionally free of background-thread concerns; it
//! only reacts to [`Message`]s delivered by the worker (see `app::worker`) and
//! exposes small helpers the panels call.

use std::path::PathBuf;

use eframe::egui::{self, TextureHandle};

use crate::models::{EditParams, EditStatus, ExifSummary, ImageItem};
use crate::raw_engine::decoder::DecodeStatus;

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

/// One file in the library, with its lazily-loaded thumbnail and probe status.
pub struct LibraryEntry {
    pub item: ImageItem,
    /// `None` while the background probe/thumbnail job has not returned yet.
    pub status: Option<DecodeStatus>,
    pub thumb: Option<TextureHandle>,
}

/// Viewport transform: `zoom` multiplies the "fit to view" scale, `pan` is a
/// pixel offset from the centred position.
#[derive(Clone, Copy, Debug)]
pub struct Viewport {
    pub zoom: f32,
    pub pan: egui::Vec2,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan: egui::Vec2::ZERO,
        }
    }
}

impl Viewport {
    pub fn reset(&mut self) {
        self.zoom = 1.0;
        self.pan = egui::Vec2::ZERO;
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
    pub status: String,
    /// Directory of the most recent folder scan (used as dialog start dir).
    pub last_dir: Option<PathBuf>,
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
            });
            added.push(self.entries.len() - 1);
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
        self.before = None;
        self.after = None;
        self.image_size = [0, 0];
        self.histogram = RgbHistogram::default();
        self.viewport.reset();
        self.status = format!("Loading {}…", entry.item.file_name);
        Some((self.proxy_token, path, params))
    }

    pub fn current_entry(&self) -> Option<&LibraryEntry> {
        self.entries.get(self.current?)
    }

    pub fn current_is_selected(&self) -> bool {
        self.current_entry().map(|e| e.item.selected).unwrap_or(false)
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

    /// Resets the current entry to neutral parameters.
    pub fn reset_current(&mut self) -> Option<(u64, EditParams)> {
        let token = self.proxy_token;
        let entry = self.entries.get_mut(self.current?)?;
        entry.item.params = EditParams::default();
        entry.item.status = EditStatus::Original;
        Some((token, EditParams::default()))
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

    pub fn set_thumbnail(
        &mut self,
        ctx: &egui::Context,
        index: usize,
        payload: Option<TexturePayload>,
        status: DecodeStatus,
        exif: ExifSummary,
    ) {
        let Some(entry) = self.entries.get_mut(index) else {
            return;
        };
        entry.status = Some(status);
        entry.item.exif = exif;
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
        self.viewport.reset();
        self.status = format!("{width} × {height}");
        true
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
                Message::Scanned { paths, error } => {
                    if let Some(error) = error {
                        self.status = error;
                    }
                    let added = self.add_paths(paths);
                    if !added.is_empty() {
                        self.status = format!("{} file(s) in library", self.entries.len());
                    }
                    for &index in &added {
                        let path = self.entries[index].item.path.clone();
                        worker.send(Job::LoadThumbnail { index, path });
                    }
                    if self.current.is_none()
                        && let Some(&first) = added.first()
                    {
                        select_and_load(self, worker, first);
                    }
                }
                Message::Thumbnail {
                    index,
                    thumb,
                    status,
                    exif,
                } => self.set_thumbnail(ctx, index, thumb, status, exif),
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
                Message::Error(error) => self.status = error,
            }
        }
    }
}

/// Selects `index` and enqueues a proxy load for it.
pub fn select_and_load(state: &mut AppState, worker: &Worker, index: usize) {
    if let Some((token, path, params)) = state.select(index) {
        worker.send(Job::LoadProxy {
            token,
            index,
            path,
            params,
        });
    }
}

fn load_texture(ctx: &egui::Context, name: impl Into<String>, payload: TexturePayload) -> TextureHandle {
    let image = egui::ColorImage::from_rgb([payload.width, payload.height], &payload.rgb);
    ctx.load_texture(name, image, egui::TextureOptions::LINEAR)
}

#[cfg(test)]
mod tests {
    use super::*;

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
