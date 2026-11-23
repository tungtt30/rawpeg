use eframe::egui::{
    self, Align, Align2, Color32, FontId, Key, Label, Layout, Rect, RichText, Sense, Vec2,
};

use crate::app::state::{AppState, RightTab, ToastKind, ViewMode, select_and_load};
use crate::app::worker::{Job, Worker};
use crate::models::EditParams;

use super::widgets::{
    ACCENT, DANGER, MUTED, VIEWPORT_BG, draw_histogram, format_exif, reveal_path,
};

pub fn central(ui: &mut egui::Ui, state: &mut AppState, worker: &Worker) {
    let Some(index) = state.current else {
        empty_develop(ui, state);
        return;
    };
    let Some(entry) = state.entries.get(index) else {
        empty_develop(ui, state);
        return;
    };
    let name = entry.item.file_name.clone();
    let exif = entry.item.exif.clone();
    let path = entry.item.path.clone();
    let status = entry.status.clone();
    let count = state.entries.len();
    let current_number = index + 1;

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if ui.small_button("‹").on_hover_text("Previous (←)").clicked() {
            step(state, worker, -1);
        }
        if ui.small_button("›").on_hover_text("Next (→)").clicked() {
            step(state, worker, 1);
        }
        ui.label(
            RichText::new(format!("{current_number}/{count}"))
                .small()
                .color(MUTED),
        );
        ui.add(Label::new(RichText::new(&name).strong()).truncate());
        ui.label(RichText::new(format_exif(&exif)).small().color(MUTED));

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let show_before = state.show_before;
            if ui
                .selectable_label(show_before, "Before")
                .on_hover_text("Toggle Before view (B) — hold Space to peek")
                .clicked()
            {
                state.show_before = !show_before;
            }
            if ui.button("100%").on_hover_text("Actual size (1)").clicked() {
                state.viewport.request_actual = true;
            }
            if ui.button("Fit").on_hover_text("Fit to view (F)").clicked() {
                state.viewport.zoom = 1.0;
                state.viewport.pan = Vec2::ZERO;
                state.viewport.request_actual = false;
            }
            if let Some(status) = &status
                && status.is_unsupported()
            {
                ui.label(RichText::new("UNSUPPORTED").small().color(DANGER));
            } else if state.show_before {
                ui.label(RichText::new("BEFORE").small().color(ACCENT));
            }
        });
    });
    ui.separator();

    let available = ui.available_size();
    let (rect, response) = ui.allocate_exact_size(available, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, VIEWPORT_BG);

    if status
        .as_ref()
        .is_some_and(crate::raw_engine::decoder::DecodeStatus::is_unsupported)
    {
        let reason = status
            .as_ref()
            .map(super::unsupported_reason)
            .unwrap_or_default();
        centered_message(ui, rect, "This RAW file cannot be decoded", &reason, DANGER);
        let button_rect =
            Rect::from_center_size(rect.center() + Vec2::new(0.0, 78.0), Vec2::new(140.0, 28.0));
        if ui
            .put(button_rect, egui::Button::new("Show in Finder"))
            .clicked()
        {
            reveal_path(&path);
        }
        return;
    }

    let space_down = ui.input(|input| input.key_down(Key::Space));
    let show_before = state.show_before || space_down;
    let texture = if show_before {
        state.before.clone()
    } else {
        state.after.clone()
    }
    .or_else(|| state.after.clone());

    if response.double_clicked() {
        state.viewport.reset();
    }
    if response.dragged() {
        state.viewport.pan += response.drag_delta();
    }

    if let Some(texture) = texture {
        let image_size = texture.size_vec2();
        let fit = (rect.width() / image_size.x).min(rect.height() / image_size.y);
        state.viewport.fit = fit;
        if state.viewport.request_actual {
            state.viewport.zoom = (1.0 / fit).clamp(0.1, 16.0);
            state.viewport.request_actual = false;
        }
        if response.hovered() {
            let (scroll, zoom_delta) =
                ui.input(|input| (input.smooth_scroll_delta.y, input.zoom_delta()));
            let factor = if (zoom_delta - 1.0).abs() > 1e-4 {
                zoom_delta
            } else {
                (scroll * 0.0015).exp()
            };
            if (factor - 1.0).abs() > 1e-4 {
                state.viewport.zoom = (state.viewport.zoom * factor).clamp(0.1, 16.0);
            }
        }

        let scale = fit * state.viewport.zoom;
        let display = image_size * scale;
        let max_pan = (display * 0.5 + rect.size() * 0.5).max(rect.size());
        state.viewport.pan = state.viewport.pan.clamp(-max_pan, max_pan);
        let center = rect.center() + state.viewport.pan;
        let destination = Rect::from_center_size(center, display);
        painter.image(
            texture.id(),
            destination,
            Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );

        if show_before {
            painter.text(
                rect.left_top() + Vec2::new(10.0, 8.0),
                Align2::LEFT_TOP,
                "BEFORE — release Space / click Before for After",
                FontId::proportional(13.0),
                Color32::WHITE,
            );
        }
        let zoom_percent = (state.viewport.zoom * fit * 100.0).round();
        painter.text(
            rect.right_bottom() + Vec2::new(-10.0, -8.0),
            Align2::RIGHT_BOTTOM,
            format!("{zoom_percent:.0}%"),
            FontId::proportional(12.0),
            Color32::from_gray(170),
        );
    } else if let Some(thumb) = state
        .entries
        .get(index)
        .and_then(|entry| entry.thumb.clone())
    {
        let image_size = thumb.size_vec2();
        let fit = (rect.width() / image_size.x)
            .min(rect.height() / image_size.y)
            .min(1.0);
        state.viewport.fit = fit;
        if state.viewport.request_actual {
            state.viewport.request_actual = false;
        }
        let display = image_size * fit;
        let destination = Rect::from_center_size(rect.center(), display);
        painter.image(
            thumb.id(),
            destination,
            Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::from_gray(210),
        );
        loading_overlay(ui, rect, &state.status, !state.proxy_failed);
    } else {
        loading_overlay(ui, rect, &state.status, !state.proxy_failed);
        if !state.proxy_failed {
            let spinner_rect =
                Rect::from_center_size(rect.center() + Vec2::new(0.0, -36.0), Vec2::splat(22.0));
            ui.put(spinner_rect, egui::Spinner::new());
        }
    }
}

fn empty_develop(ui: &mut egui::Ui, state: &mut AppState) {
    ui.vertical_centered(|ui| {
        ui.add_space(140.0);
        ui.label(RichText::new("No photo selected").size(20.0).strong());
        ui.add_space(8.0);
        ui.label(RichText::new("Pick a photo in Library to develop it.").color(MUTED));
        ui.add_space(20.0);
        if ui.button("Open Library").clicked() {
            state.view_mode = ViewMode::Library;
            state.right_tab = RightTab::Export;
        }
    });
}

fn centered_message(ui: &mut egui::Ui, rect: Rect, title: &str, detail: &str, color: Color32) {
    let painter = ui.painter_at(rect);
    painter.text(
        rect.center() + Vec2::new(0.0, -36.0),
        Align2::CENTER_CENTER,
        title,
        FontId::proportional(18.0),
        color,
    );
    painter.text(
        rect.center() + Vec2::new(0.0, -8.0),
        Align2::CENTER_CENTER,
        detail,
        FontId::proportional(12.0),
        Color32::from_gray(180),
    );
}

fn loading_overlay(ui: &egui::Ui, rect: Rect, status: &str, animate: bool) {
    let painter = ui.painter_at(rect);
    let text = if status.is_empty() {
        "Loading preview…"
    } else {
        status
    };
    painter.text(
        rect.center() + Vec2::new(0.0, 20.0),
        Align2::CENTER_CENTER,
        text,
        FontId::proportional(13.0),
        Color32::from_gray(190),
    );
    if animate {
        ui.ctx().request_repaint();
    }
}

fn step(state: &mut AppState, worker: &Worker, delta: isize) {
    let Some(current) = state.current else {
        return;
    };
    if state.entries.is_empty() {
        return;
    }
    let last = state.entries.len() as isize - 1;
    let index = (current as isize + delta).clamp(0, last) as usize;
    if index != current {
        select_and_load(state, worker, index);
    }
}

pub fn panel(ui: &mut egui::Ui, state: &mut AppState, worker: &Worker) {
    ui.add_space(6.0);
    ui.heading("Develop");
    ui.separator();

    let Some(index) = state.current else {
        ui.add_space(8.0);
        ui.label(RichText::new("No photo selected.").color(MUTED));
        if ui.button("Open Library").clicked() {
            state.view_mode = ViewMode::Library;
            state.right_tab = RightTab::Export;
        }
        return;
    };
    let Some(entry) = state.entries.get(index) else {
        ui.label(RichText::new("No photo selected.").color(MUTED));
        return;
    };
    let name = entry.item.file_name.clone();
    let exif = entry.item.exif.clone();
    let path = entry.item.path.clone();
    let mut params = entry.item.params.clone();
    let status = entry.status.clone();
    let auto_applied = params.is_auto_applied;
    let token = state.proxy_token;
    let stats = state.selection_stats();
    let selected = state.current_is_selected();
    let loaded = state.after.is_some();

    ui.label(RichText::new(&name).strong());
    ui.label(RichText::new(format_exif(&exif)).small().color(MUTED));
    if auto_applied {
        ui.label(RichText::new("Auto balance applied").small().color(ACCENT));
    }
    if let Some(status) = &status
        && status.is_unsupported()
    {
        ui.add_space(8.0);
        ui.label(
            RichText::new(super::unsupported_reason(status))
                .small()
                .color(DANGER),
        );
        if ui.button("Show in Finder").clicked() {
            reveal_path(&path);
        }
        return;
    }

    ui.add_space(8.0);
    ui.horizontal_wrapped(|ui| {
        if ui
            .add_enabled(loaded, egui::Button::new("Auto Balance"))
            .on_hover_text("Analyse the histogram and balance this photo")
            .clicked()
        {
            worker.send(Job::AutoTone { token });
        }
        if ui.button("Reset").clicked()
            && let Some((token, params)) = state.reset_current()
        {
            worker.send(Job::RenderAfter { token, params });
            state.toast("Reset photo", ToastKind::Info);
        }
        if ui.button("Copy").clicked() {
            state.copy_current();
            state.toast("Copied settings", ToastKind::Info);
        }
        if ui
            .add_enabled(
                state.clipboard.is_some() && stats.selected > 0,
                egui::Button::new(format!("Paste to {}", stats.selected)),
            )
            .clicked()
            && let Some(clipboard) = state.clipboard.clone()
        {
            let changed = state.paste_to_selected(&clipboard);
            if state.current_is_selected() {
                worker.send(Job::RenderAfter {
                    token: state.proxy_token,
                    params: clipboard,
                });
            }
            if changed > 0 {
                state.toast(
                    format!("Applied settings to {changed} photo(s)"),
                    ToastKind::Success,
                );
            }
        }
    });

    let changed = sliders(ui, &mut params);
    if changed {
        params.is_auto_applied = false;
        state.apply_params(params.clone());
        worker.send(Job::RenderAfter {
            token: state.proxy_token,
            params,
        });
    }

    ui.add_space(8.0);
    ui.horizontal_wrapped(|ui| {
        let count = stats.selected;
        let sync = ui.add_enabled(
            count > 0,
            egui::Button::new(format!("Sync to selected ({count})"))
                .fill(super::widgets::ACCENT)
                .min_size(Vec2::new(0.0, 28.0)),
        );
        if sync.clicked() {
            let changed = state.sync_to_selected();
            if changed > 0 {
                state.toast(
                    format!("Applied settings to {changed} photo(s)"),
                    ToastKind::Success,
                );
            }
        }
        if selected {
            ui.label(
                RichText::new("current photo is in selection")
                    .small()
                    .color(MUTED),
            );
        }
    });

    if !loaded {
        ui.add_space(6.0);
        if state.proxy_failed {
            ui.label(
                RichText::new("Preview failed to load")
                    .small()
                    .color(DANGER),
            );
            ui.label(RichText::new(&state.status).small().color(MUTED));
        } else {
            ui.label(RichText::new("Loading full preview…").small().color(MUTED));
        }
    }
    ui.add_space(10.0);
    ui.label(RichText::new("Histogram (RGB)").strong());
    draw_histogram(ui, &state.histogram);
    ui.add_space(8.0);
    if ui.button("Open in Library").clicked() {
        state.view_mode = ViewMode::Library;
        state.right_tab = RightTab::Export;
    }
}

fn sliders(ui: &mut egui::Ui, params: &mut EditParams) -> bool {
    let mut changed = false;
    ui.add_space(6.0);
    ui.label(
        RichText::new("Light")
            .strong()
            .color(Color32::from_gray(210)),
    );
    changed |= slider_f32(
        ui,
        "Exposure (EV)",
        &mut params.exposure,
        -3.0..=3.0,
        0.0,
        " EV",
    );
    changed |= slider_i32(ui, "Contrast", &mut params.contrast, -50..=50, 0);
    changed |= slider_i32(ui, "Highlights", &mut params.highlights, -100..=100, 0);
    changed |= slider_i32(ui, "Shadows", &mut params.shadows, -100..=100, 0);

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("White balance")
                .strong()
                .color(Color32::from_gray(210)),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .small_button("As Shot")
                .on_hover_text("Back to the camera white balance")
                .clicked()
            {
                params.temperature = 6500;
                params.tint = 0;
                changed = true;
            }
        });
    });
    changed |= slider_u16(
        ui,
        "Temp",
        &mut params.temperature,
        2000..=10000,
        6500,
        " K",
    );
    changed |= slider_i32(ui, "Tint", &mut params.tint, -100..=100, 0);
    changed
}

fn slider_f32(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    default: f32,
    suffix: &str,
) -> bool {
    let response = ui.add(
        egui::Slider::new(value, range)
            .text(label)
            .suffix(suffix)
            .fixed_decimals(2),
    );
    if response.double_clicked() {
        *value = default;
        return true;
    }
    response.changed()
}

fn slider_i32(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut i32,
    range: std::ops::RangeInclusive<i32>,
    default: i32,
) -> bool {
    let response = ui.add(egui::Slider::new(value, range).text(label));
    if response.double_clicked() {
        *value = default;
        return true;
    }
    response.changed()
}

fn slider_u16(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut u16,
    range: std::ops::RangeInclusive<u16>,
    default: u16,
    suffix: &str,
) -> bool {
    let response = ui.add(egui::Slider::new(value, range).text(label).suffix(suffix));
    if response.double_clicked() {
        *value = default;
        return true;
    }
    response.changed()
}
