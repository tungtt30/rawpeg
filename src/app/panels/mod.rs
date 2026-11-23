//! egui panels: top bar, library, grid, develop viewport, export panel and
//! status bar. The Library and Develop modes share the same left library and
//! the same right-side inspector.

mod develop;
mod export;
mod grid;
mod library;
mod status;
mod top_bar;
mod widgets;

use std::path::{Path, PathBuf};

use eframe::egui::{self, Align2, Color32, FontId, Id, Key, RichText, Vec2};

use crate::app::state::{AppState, ConfirmAction, RightTab, ToastKind, ViewMode, select_and_load};
use crate::app::worker::{Job, Worker};
use crate::raw_engine::decoder::{RAW_EXTENSIONS, is_raw_path};

use widgets::{ACCENT, DANGER, MUTED, danger_button};

pub use develop::panel as develop_panel;
pub use export::panel as export_panel;
pub use grid::central as library_central;
pub use library::panel as library_panel;
pub use status::bar as status_bar;
pub use top_bar::top_bar;
pub use widgets::{
    format_duration, format_exif, open_path, reveal_path, status_short, unsupported_reason,
};

/// Main centre panel dispatch.
pub fn central(ui: &mut egui::Ui, state: &mut AppState, worker: &Worker) {
    egui::CentralPanel::default().show(ui, |ui| {
        if state.entries.is_empty() {
            grid::empty(ui, state, worker);
            return;
        }
        match state.view_mode {
            ViewMode::Library => grid::central(ui, state, worker),
            ViewMode::Develop => develop::central(ui, state, worker),
        }
    });
}

/// Right-side inspector: Develop and Export tabs.
pub fn right_panel(ui: &mut egui::Ui, state: &mut AppState, worker: &Worker) {
    let stats = state.selection_stats();
    egui::Panel::right("inspector")
        .resizable(true)
        .default_size(330.0)
        .min_size(280.0)
        .show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.selectable_value(&mut state.right_tab, RightTab::Develop, "Develop");
                ui.selectable_value(
                    &mut state.right_tab,
                    RightTab::Export,
                    format!("Export {}", stats.export_count()),
                );
            });
            ui.separator();
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| match state.right_tab {
                    RightTab::Develop => develop::panel(ui, state, worker),
                    RightTab::Export => export::panel(ui, state, worker),
                });
        });
}

/// Opens the Develop view for `index` and makes sure its proxy is loading.
pub fn open_develop(state: &mut AppState, worker: &Worker, index: usize) {
    state.view_mode = ViewMode::Develop;
    state.right_tab = RightTab::Develop;
    state.last_clicked = Some(index);
    select_and_load(state, worker, index);
}

pub fn open_export_dir(state: &AppState) {
    let dir = state
        .export
        .as_ref()
        .and_then(|session| session.summary.as_ref())
        .and_then(|summary| summary.output_dir())
        .or_else(|| state.export_settings.output_dir.clone())
        .or_else(|| {
            state
                .entries
                .iter()
                .find(|entry| entry.item.selected)
                .and_then(|entry| entry.item.path.parent().map(Path::to_path_buf))
        });
    if let Some(dir) = dir {
        open_path(&dir);
    }
}

pub fn open_folder(state: &mut AppState, worker: &Worker) {
    if let Some(dir) = pick_folder(&state.last_dir) {
        scan_folder(state, worker, dir);
    }
}

pub fn add_files(state: &mut AppState, worker: &Worker) {
    if let Some(files) = pick_files(&state.last_dir) {
        if let Some(parent) = files.first().and_then(|path| path.parent()) {
            state.last_dir = Some(parent.to_path_buf());
        }
        let added = state.add_imported(worker, files);
        if !added.is_empty() {
            state.status = format!(
                "Added {} file(s) — {} in library",
                added.len(),
                state.entries.len()
            );
        }
    }
}

fn scan_folder(state: &mut AppState, worker: &Worker, dir: PathBuf) {
    state.last_dir = Some(dir.clone());
    state.status = format!("Scanning {}…", dir.display());
    worker.send(Job::ScanDir {
        dir,
        epoch: state.library_gen,
    });
}

fn pick_folder(start: &Option<PathBuf>) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new();
    if let Some(dir) = start {
        dialog = dialog.set_directory(dir);
    }
    dialog.pick_folder()
}

fn pick_files(start: &Option<PathBuf>) -> Option<Vec<PathBuf>> {
    let mut dialog = rfd::FileDialog::new().add_filter("RAW image", RAW_EXTENSIONS);
    if let Some(dir) = start {
        dialog = dialog.set_directory(dir);
    }
    dialog.pick_files()
}

fn pick_output(start: Option<&Path>, last_dir: &Option<PathBuf>) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new();
    if let Some(dir) = start.or(last_dir.as_deref()) {
        dialog = dialog.set_directory(dir);
    }
    dialog.pick_folder()
}

/// Handles files/folders dropped anywhere on the window.
pub fn handle_drop(ctx: &egui::Context, state: &mut AppState, worker: &Worker) {
    let dropped = ctx.input(|input| input.raw.dropped_files.clone());
    if dropped.is_empty() {
        return;
    }
    let mut files = Vec::new();
    let mut dirs = Vec::new();
    for file in dropped {
        let path = file.path().to_path_buf();
        if path.is_dir() {
            dirs.push(path);
        } else if is_raw_path(&path) {
            files.push(path);
        }
    }
    for dir in dirs {
        scan_folder(state, worker, dir);
    }
    if !files.is_empty() {
        if let Some(parent) = files.first().and_then(|path| path.parent()) {
            state.last_dir = Some(parent.to_path_buf());
        }
        let added = state.add_imported(worker, files);
        if !added.is_empty() {
            state.toast(format!("Added {} file(s)", added.len()), ToastKind::Success);
        }
    }
}

/// Transparent overlay while files are dragged over the window.
pub fn drop_overlay(ctx: &egui::Context) {
    let hovering = ctx.input(|input| !input.raw.hovered_files.is_empty());
    if !hovering {
        return;
    }
    egui::Area::new(Id::unique("rawpeg_drop_overlay"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::Pos2::ZERO)
        .show(ctx, |ui| {
            let rect = ctx.content_rect();
            ui.set_min_size(rect.size());
            let painter = ui.painter();
            painter.rect_filled(rect, 0.0, Color32::from_black_alpha(185));
            painter.rect_stroke(
                rect.shrink(20.0),
                12.0,
                egui::Stroke::new(2.0, ACCENT),
                egui::StrokeKind::Inside,
            );
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                "Drop RAW files to add",
                FontId::proportional(26.0),
                Color32::WHITE,
            );
            painter.text(
                rect.center() + Vec2::new(0.0, 34.0),
                Align2::CENTER_CENTER,
                "Folders are scanned for supported RAW files",
                FontId::proportional(14.0),
                Color32::from_gray(200),
            );
        });
}

pub fn toast(ctx: &egui::Context, state: &AppState) {
    let Some(toast) = &state.toast else {
        return;
    };
    let color = match toast.kind {
        ToastKind::Info => Color32::from_gray(220),
        ToastKind::Success => widgets::SUCCESS,
        ToastKind::Error => DANGER,
    };
    egui::Area::new(Id::unique("rawpeg_toast"))
        .order(egui::Order::Foreground)
        .anchor(Align2::CENTER_BOTTOM, Vec2::new(0.0, -44.0))
        .show(ctx, |ui| {
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(14, 8))
                .corner_radius(6)
                .fill(Color32::from_rgb(0x24, 0x24, 0x29))
                .stroke(egui::Stroke::new(1.0, Color32::from_rgb(0x3a, 0x3a, 0x40)))
                .show(ui, |ui| {
                    ui.label(RichText::new(&toast.message).color(color));
                });
        });
}

pub fn help_modal(ctx: &egui::Context, state: &mut AppState) {
    if !state.show_help {
        return;
    }
    let mut close = false;
    let response = egui::Modal::new(Id::unique("rawpeg_help")).show(ctx, |ui| {
        ui.set_width(560.0);
        ui.heading("Keyboard shortcuts");
        ui.label(
            RichText::new(
                "Tips: click a tile to toggle export selection, double-click to develop.",
            )
            .small()
            .color(MUTED),
        );
        ui.add_space(8.0);
        egui::Grid::new("rawpeg_help_grid")
            .num_columns(2)
            .spacing([18.0, 6.0])
            .striped(true)
            .show(ui, |ui| {
                let rows = [
                    ("Cmd/Ctrl + O", "Open folder"),
                    ("Cmd/Ctrl + Shift + O", "Add files"),
                    ("Cmd/Ctrl + E", "Open Export panel"),
                    ("Cmd/Ctrl + Shift + E", "Quick export with current settings"),
                    ("Cmd/Ctrl + A", "Select all"),
                    ("Cmd/Ctrl + Shift + A", "Clear selection"),
                    ("L", "Toggle Library / Develop"),
                    ("Enter", "Open selected photo in Develop"),
                    (
                        "Space",
                        "Toggle selection (Library) / hold Before (Develop)",
                    ),
                    ("← / →", "Previous / next photo in Develop"),
                    ("F / 1", "Fit / actual size"),
                    ("B", "Toggle Before view"),
                    ("Cmd/Ctrl + C / V", "Copy / paste edit settings"),
                    ("?", "Show this help"),
                    ("Esc", "Back to Library / close dialog"),
                ];
                for (keys, action) in rows {
                    ui.label(RichText::new(keys).monospace().color(ACCENT));
                    ui.label(action);
                    ui.end_row();
                }
            });
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button("Close").clicked() {
                close = true;
            }
        });
    });
    if close || response.backdrop_response.clicked() || ctx.input(|i| i.key_pressed(Key::Escape)) {
        state.show_help = false;
    }
}

pub fn confirm_modal(ctx: &egui::Context, state: &mut AppState, worker: &Worker) {
    let Some(action) = state.pending_confirm else {
        return;
    };
    let mut close = false;
    let mut confirmed = false;
    let (title, message, confirm_label) = match action {
        ConfirmAction::ClearLibrary => (
            "Clear library?",
            "Remove all photos from the library. Edits are kept only in memory and will be lost.",
            "Clear library",
        ),
        ConfirmAction::ResetSelected => (
            "Reset selected photos?",
            "Reset every selected photo to the original settings. This cannot be undone.",
            "Reset selected",
        ),
    };
    let response = egui::Modal::new(Id::unique("rawpeg_confirm")).show(ctx, |ui| {
        ui.set_width(400.0);
        ui.heading(title);
        ui.label(RichText::new(message).color(MUTED));
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() {
                close = true;
            }
            if danger_button(ui, confirm_label).clicked() {
                confirmed = true;
            }
        });
    });

    if confirmed {
        match action {
            ConfirmAction::ClearLibrary => {
                state.clear_library();
                state.toast("Library cleared", ToastKind::Info);
            }
            ConfirmAction::ResetSelected => {
                let count = state.reset_selected();
                if state.current_is_selected()
                    && let Some((token, params)) = state.render_request()
                {
                    worker.send(Job::RenderAfter { token, params });
                }
                if count > 0 {
                    state.toast(format!("Reset {count} photo(s)"), ToastKind::Info);
                }
            }
        }
        state.pending_confirm = None;
    } else if close
        || response.backdrop_response.clicked()
        || ctx.input(|i| i.key_pressed(Key::Escape))
    {
        state.pending_confirm = None;
    }
}
