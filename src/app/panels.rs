//! egui panels: top bar, library sidebar, viewport and adjustments sidebar.

use eframe::egui::{self, Align, Align2, Color32, FontId, Layout, RichText, Sense, Stroke};

use crate::models::EditStatus;
use crate::raw_engine::decoder::{DecodeStatus, RAW_EXTENSIONS};

use super::state::{edit_status, select_and_load, AppState, RgbHistogram, HIST_BINS};
use super::worker::{Job, Worker};

/// Accent blue from the darkroom palette (#3B82F6).
const ACCENT: Color32 = Color32::from_rgb(0x3b, 0x82, 0xf6);
const DANGER: Color32 = Color32::from_rgb(0xf8, 0x71, 0x71);
const MUTED: Color32 = Color32::from_gray(140);

pub fn top_bar(ui: &mut egui::Ui, state: &mut AppState, worker: &Worker) {
    egui::Panel::top("top_bar").show(ui, |ui| {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.button("Open Folder").clicked()
                && let Some(dir) = pick(&state.last_dir, true)
            {
                state.last_dir = Some(dir.clone());
                state.status = format!("Scanning {}…", dir.display());
                worker.send(Job::ScanDir(dir));
            }
            if ui.button("Add Files").clicked()
                && let Some(files) = FilePicker::files(&state.last_dir)
            {
                let added = state.add_paths(files);
                for &index in &added {
                    let path = state.entries[index].item.path.clone();
                    worker.send(Job::LoadThumbnail { index, path });
                }
                if state.current.is_none()
                    && let Some(&first) = added.first()
                {
                    select_and_load(state, worker, first);
                }
            }

            ui.separator();
            show_exif(ui, state);

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_enabled(false, egui::Button::new("Export Batch"))
                    .on_hover_text("Batch export lands in milestone 4");
            });
        });
        ui.add_space(4.0);
    });
}

fn show_exif(ui: &mut egui::Ui, state: &AppState) {
    let Some(entry) = state.current_entry() else {
        ui.label(RichText::new("No image selected").color(MUTED));
        return;
    };
    let exif = &entry.item.exif;
    let mut parts = Vec::new();
    if let Some(model) = &exif.model {
        parts.push(model.clone());
    }
    if let Some(iso) = &exif.iso {
        parts.push(format!("ISO {iso}"));
    }
    if let Some(aperture) = &exif.aperture {
        parts.push(format!("f/{aperture}"));
    }
    if let Some(shutter) = &exif.shutter {
        parts.push(format!("{shutter}s"));
    }
    if let Some(focal) = &exif.focal_length {
        parts.push(format!("{focal}mm"));
    }
    let text = if parts.is_empty() {
        "EXIF: —".to_string()
    } else {
        parts.join("  ·  ")
    };
    ui.label(RichText::new(text).color(Color32::from_gray(185)));
}

pub fn library_panel(ui: &mut egui::Ui, state: &mut AppState, worker: &Worker) {
    egui::Panel::left("library")
        .resizable(true)
        .default_size(240.0)
        .show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading("Library");
                ui.label(RichText::new(format!("({})", state.entries.len())).color(MUTED));
            });
            ui.separator();
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for index in 0..state.entries.len() {
                        library_row(ui, state, worker, index);
                        ui.separator();
                    }
                });
        });
}

fn library_row(ui: &mut egui::Ui, state: &mut AppState, worker: &Worker, index: usize) {
    let is_current = state.current == Some(index);
    let mut clicked = false;
    ui.horizontal(|ui| {
        let mut selected = state.entries[index].item.selected;
        if ui.checkbox(&mut selected, "").changed() {
            state.entries[index].item.selected = selected;
        }

        let thumb_size = egui::vec2(84.0, 64.0);
        let thumb_response = if let Some(texture) = state.entries[index].thumb.as_ref() {
            ui.add(
                egui::Image::new(texture)
                    .max_size(thumb_size)
                    .sense(Sense::click()),
            )
        } else {
            let (rect, response) = ui.allocate_exact_size(thumb_size, Sense::click());
            ui.painter()
                .rect_filled(rect, 2.0, Color32::from_rgb(0x27, 0x27, 0x2a));
            let placeholder = match &state.entries[index].status {
                Some(status) if status.is_unsupported() => "unsupported",
                None => "…",
                Some(_) => "",
            };
            if !placeholder.is_empty() {
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    placeholder,
                    FontId::proportional(10.0),
                    DANGER,
                );
            }
            response
        };
        clicked |= thumb_response.clicked();

        ui.vertical(|ui| {
            if ui
                .selectable_label(is_current, &state.entries[index].item.file_name)
                .clicked()
            {
                clicked = true;
            }
            let status_label = match &state.entries[index].status {
                Some(status) if status.is_unsupported() => {
                    let response = ui.label(RichText::new(status_short(status)).small().color(DANGER));
                    response.on_hover_text(unsupported_reason(status));
                    None
                }
                _ => Some(state.entries[index].item.status),
            };
            if let Some(status) = status_label {
                match status {
                    EditStatus::Edited => {
                        ui.label(RichText::new("edited").small().color(ACCENT));
                    }
                    EditStatus::Original => {
                        ui.label(RichText::new("original").small().color(MUTED));
                    }
                }
            }
        });
    });

    if clicked {
        select_and_load(state, worker, index);
    }
}

pub fn adjustments_panel(ui: &mut egui::Ui, state: &mut AppState, worker: &Worker) {
    egui::Panel::right("adjustments")
        .resizable(true)
        .default_size(300.0)
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add_space(6.0);
                    ui.heading("Adjustments");
                    ui.separator();

                    let Some(index) = state.current else {
                        ui.add_space(8.0);
                        ui.label(RichText::new("No image selected.").color(MUTED));
                        return;
                    };

                    let token = state.proxy_token;
                    let mut params = state.entries[index].item.params.clone();
                    let mut changed = false;
                    ui.add_space(4.0);
                    changed |= ui
                        .add(
                            egui::Slider::new(&mut params.exposure, -3.0..=3.0)
                                .text("Exposure (EV)")
                                .fixed_decimals(2),
                        )
                        .changed();
                    changed |= ui
                        .add(egui::Slider::new(&mut params.contrast, -50..=50).text("Contrast"))
                        .changed();
                    changed |= ui
                        .add(
                            egui::Slider::new(&mut params.highlights, -100..=100)
                                .text("Highlights"),
                        )
                        .changed();
                    changed |= ui
                        .add(egui::Slider::new(&mut params.shadows, -100..=100).text("Shadows"))
                        .changed();
                    changed |= ui
                        .add(
                            egui::Slider::new(&mut params.temperature, 2000..=10000)
                                .text("Temp")
                                .suffix(" K"),
                        )
                        .changed();
                    changed |= ui
                        .add(egui::Slider::new(&mut params.tint, -100..=100).text("Tint"))
                        .changed();

                    if changed {
                        params.is_auto_applied = false;
                        state.entries[index].item.status = edit_status(&params);
                        state.entries[index].item.params = params.clone();
                        worker.send(Job::RenderAfter { token, params });
                    }

                    ui.add_space(10.0);
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Auto Balance").clicked() {
                            worker.send(Job::AutoTone { token });
                        }
                        if ui.button("Reset").clicked()
                            && let Some((token, params)) = state.reset_current()
                        {
                            worker.send(Job::RenderAfter { token, params });
                        }
                        if ui.button("Copy").clicked() {
                            state.copy_current();
                        }
                        if ui.button("Paste").clicked()
                            && let Some(params) = state.clipboard.clone()
                        {
                            state.paste_to_selected(&params);
                            if state.current_is_selected() {
                                worker.send(Job::RenderAfter { token, params });
                            }
                        }
                    });

                    ui.add_space(12.0);
                    ui.label(RichText::new("Histogram (RGB)").strong());
                    draw_histogram(ui, &state.histogram);

                    ui.add_space(8.0);
                    let selected = state.entries.iter().filter(|e| e.item.selected).count();
                    ui.label(
                        RichText::new(format!(
                            "{selected} of {} selected for export",
                            state.entries.len()
                        ))
                        .small()
                        .color(MUTED),
                    );
                });
        });
}

fn draw_histogram(ui: &mut egui::Ui, histogram: &RgbHistogram) {
    let size = egui::vec2(ui.available_width(), 110.0);
    let (response, painter) = ui.allocate_painter(size, Sense::hover());
    let rect = response.rect;
    painter.rect_filled(rect, 2.0, Color32::from_rgb(0x11, 0x11, 0x14));
    let max = histogram.max() as f32;

    let draw_channel = |bins: &[u32; HIST_BINS], color: Color32| {
        let points: Vec<egui::Pos2> = bins
            .iter()
            .enumerate()
            .map(|(i, &count)| {
                let x = rect.left() + rect.width() * i as f32 / (HIST_BINS - 1) as f32;
                let y = rect.bottom() - rect.height() * count as f32 / max;
                egui::pos2(x, y)
            })
            .collect();
        painter.add(egui::Shape::line(points, Stroke::new(1.0, color)));
    };
    draw_channel(&histogram.r, Color32::from_rgb(0xf8, 0x71, 0x71));
    draw_channel(&histogram.g, Color32::from_rgb(0x4a, 0xde, 0x80));
    draw_channel(&histogram.b, Color32::from_rgb(0x60, 0xa5, 0xfa));
}

pub fn viewport(ui: &mut egui::Ui, state: &mut AppState) {
    egui::CentralPanel::default().show(ui, |ui| {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, Color32::from_rgb(0x0e, 0x0e, 0x11));

        if response.double_clicked() {
            state.viewport.reset();
        }
        if response.dragged() {
            state.viewport.pan += response.drag_delta();
        }
        if response.hovered() {
            let (scroll, zoom_delta) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let factor = if (zoom_delta - 1.0).abs() > 1e-4 {
                zoom_delta
            } else {
                (scroll * 0.0015).exp()
            };
            if (factor - 1.0).abs() > 1e-4 {
                state.viewport.zoom = (state.viewport.zoom * factor).clamp(0.1, 16.0);
            }
        }

        let show_before = ui.input(|i| i.key_down(egui::Key::Space));
        let texture = if show_before {
            state.before.as_ref()
        } else {
            state.after.as_ref()
        };
        let Some(texture) = texture else {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                "Open a folder or add RAW files",
                FontId::proportional(16.0),
                MUTED,
            );
            return;
        };

        let image_size = texture.size_vec2();
        let fit = (rect.width() / image_size.x).min(rect.height() / image_size.y);
        let scale = fit * state.viewport.zoom;
        let display = image_size * scale;
        let max_pan = (display * 0.5 + rect.size() * 0.5).max(rect.size());
        state.viewport.pan = state.viewport.pan.clamp(-max_pan, max_pan);
        let center = rect.center() + state.viewport.pan;
        let dest = egui::Rect::from_center_size(center, display);
        painter.image(
            texture.id(),
            dest,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );

        if show_before {
            painter.text(
                rect.left_top() + egui::vec2(10.0, 8.0),
                Align2::LEFT_TOP,
                "BEFORE — release Space for After",
                FontId::proportional(13.0),
                Color32::WHITE,
            );
        }
        let zoom_percent = (state.viewport.zoom * fit * 100.0).round();
        painter.text(
            rect.right_bottom() + egui::vec2(-10.0, -8.0),
            Align2::RIGHT_BOTTOM,
            format!("{zoom_percent:.0}%"),
            FontId::proportional(12.0),
            Color32::from_gray(170),
        );
    });
}

fn status_short(status: &DecodeStatus) -> &'static str {
    match status {
        DecodeStatus::Supported => "ok",
        DecodeStatus::UnsupportedCamera(_) => "unsupported camera",
        DecodeStatus::UnsupportedFormat(_) => "unsupported format",
        DecodeStatus::Error(_) => "decode error",
    }
}

fn unsupported_reason(status: &DecodeStatus) -> String {
    match status {
        DecodeStatus::Supported => String::new(),
        DecodeStatus::UnsupportedCamera(reason)
        | DecodeStatus::UnsupportedFormat(reason)
        | DecodeStatus::Error(reason) => reason.clone(),
    }
}

/// Opens a native picker. `folder = true` selects directories; otherwise files
/// are filtered to the supported RAW extensions.
fn pick(start: &Option<std::path::PathBuf>, folder: bool) -> Option<std::path::PathBuf> {
    let mut dialog = rfd::FileDialog::new();
    if let Some(dir) = start {
        dialog = dialog.set_directory(dir);
    }
    if folder {
        dialog.pick_folder()
    } else {
        dialog
            .add_filter("RAW image", RAW_EXTENSIONS)
            .pick_file()
    }
}

struct FilePicker;

impl FilePicker {
    fn files(start: &Option<std::path::PathBuf>) -> Option<Vec<std::path::PathBuf>> {
        let mut dialog = rfd::FileDialog::new().add_filter("RAW image", RAW_EXTENSIONS);
        if let Some(dir) = start {
            dialog = dialog.set_directory(dir);
        }
        dialog.pick_files()
    }
}
