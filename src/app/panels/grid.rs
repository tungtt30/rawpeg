use eframe::egui::{
    self, Align, Align2, Color32, FontId, Label, RichText, Sense, Stroke, StrokeKind, Vec2,
};

use crate::app::state::AppState;
use crate::app::worker::Worker;
use crate::models::EditStatus;

use super::widgets::{ACCENT, DANGER, MUTED, THUMB_BG, badge, primary_button, unsupported_reason};

pub fn empty(ui: &mut egui::Ui, state: &mut AppState, worker: &Worker) {
    ui.vertical_centered(|ui| {
        ui.add_space(130.0);
        ui.label(
            egui::RichText::new("RAW → JPEG · PNG · TIFF")
                .size(30.0)
                .strong(),
        );
        ui.add_space(10.0);
        ui.label(
            RichText::new("Open a folder of RAW files and export the whole batch in a few clicks.")
                .size(15.0)
                .color(MUTED),
        );
        ui.add_space(26.0);
        ui.horizontal(|ui| {
            if primary_button(ui, "Open Folder").clicked() {
                super::open_folder(state, worker);
            }
            if ui.button("Add Files…").clicked() {
                super::add_files(state, worker);
            }
        });
        ui.add_space(22.0);
        ui.label(
            RichText::new(
                "Supported: CR2 · CR3 · NEF · ARW · RAF · ORF · RW2 · DNG · PEF · SRW · …",
            )
            .small()
            .color(MUTED),
        );
        ui.label(
            RichText::new("Tip: drag a folder or RAW files anywhere in this window.")
                .small()
                .color(MUTED),
        );
    });
}

pub fn central(ui: &mut egui::Ui, state: &mut AppState, worker: &Worker) {
    let stats = state.selection_stats();
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("{} photos", stats.total)).strong());
        ui.label(
            RichText::new(format!(
                "· {} selected · {} edited · {} unsupported",
                stats.selected, stats.edited, stats.unsupported
            ))
            .color(MUTED),
        );
        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
            let export_active = state
                .export
                .as_ref()
                .is_some_and(|session| !session.finished);
            let export_count = stats.export_count();
            let export_label = if export_active {
                "Exporting…".to_string()
            } else {
                format!("Export {export_count}")
            };
            let export_button =
                egui::Button::new(RichText::new(export_label).color(Color32::WHITE).strong())
                    .fill(if export_active || export_count > 0 {
                        ACCENT
                    } else {
                        Color32::from_rgb(0x2a, 0x2a, 0x2e)
                    })
                    .min_size(egui::vec2(104.0, 26.0));
            if ui
                .add_enabled(export_active || export_count > 0, export_button)
                .on_hover_text("Export with the current settings")
                .clicked()
            {
                state.begin_export(worker);
            }
            if stats.selected > 0 && ui.button("Reset").clicked() {
                state.pending_confirm = Some(crate::app::state::ConfirmAction::ResetSelected);
            }
            if stats.selected > 0
                && ui
                    .button(format!("Sync current to {}", stats.selected))
                    .on_hover_text("Apply the current photo's settings to all selected photos")
                    .clicked()
            {
                let changed = state.sync_to_selected();
                if changed > 0 {
                    state.toast(
                        format!("Applied settings to {changed} photo(s)"),
                        crate::app::state::ToastKind::Success,
                    );
                }
            }
            if stats.selected == 0 {
                ui.label(RichText::new("Tick photos to export").small().color(MUTED));
            }
        });
    });
    if stats.selected_unsupported > 0 {
        ui.label(
            RichText::new(format!(
                "{} selected unsupported file(s) will be skipped",
                stats.selected_unsupported
            ))
            .small()
            .color(DANGER),
        );
    }
    ui.separator();

    let visible = state.visible_indices();
    if visible.is_empty() {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("No photos match this filter.").color(MUTED));
            if ui.button("Show all").clicked() {
                state.filter = crate::app::state::LibraryFilter::All;
            }
        });
        return;
    }

    let gap = 12.0;
    let available = ui.available_width();
    let target = 164.0;
    let columns = (((available + gap) / (target + gap)).floor() as usize).max(1);
    let cell_w = ((available - gap * columns.saturating_sub(1) as f32) / columns as f32).max(80.0);
    let image_h = (cell_w * 0.62).round();
    let cell_h = image_h + 42.0;
    let row_h = cell_h + gap;
    let total_rows = visible.len().div_ceil(columns);

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(ui, row_h, total_rows, |ui, range| {
            for row in range {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(gap, 0.0);
                    for column in 0..columns {
                        let position = row * columns + column;
                        let Some(&index) = visible.get(position) else {
                            break;
                        };
                        tile(ui, state, worker, index, cell_w, cell_h, image_h);
                    }
                });
            }
        });
}

fn tile(
    ui: &mut egui::Ui,
    state: &mut AppState,
    worker: &Worker,
    index: usize,
    cell_w: f32,
    cell_h: f32,
    image_h: f32,
) {
    let is_current = state.current == Some(index);
    let selected = state.entries[index].item.selected;
    let name = state.entries[index].item.file_name.clone();
    let status = state.entries[index].status.clone();
    let edit = state.entries[index].item.status;
    let path = state.entries[index].item.path.clone();
    let exif = state.entries[index].item.exif.clone();
    state.ensure_thumb(worker, index);
    let thumb = state.entries[index].thumb.clone();

    ui.allocate_ui_with_layout(
        Vec2::new(cell_w, cell_h),
        egui::Layout::top_down(Align::Min),
        |ui| {
            let (rect, response) =
                ui.allocate_exact_size(Vec2::new(cell_w, image_h), Sense::click());
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 4.0, THUMB_BG);

            if let Some(texture) = &thumb {
                let size = texture.size_vec2();
                if size.x > 0.0 && size.y > 0.0 {
                    let scale = (rect.width() / size.x).min(rect.height() / size.y);
                    let dest = egui::Rect::from_center_size(rect.center(), size * scale);
                    painter.image(
                        texture.id(),
                        dest,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
            } else {
                let (text, color) = match &status {
                    Some(status) if status.is_unsupported() => ("no preview", DANGER),
                    None => ("…", MUTED),
                    _ => ("", MUTED),
                };
                painter.text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    text,
                    FontId::proportional(11.0),
                    color,
                );
            }

            if selected {
                painter.rect_stroke(rect, 4.0, Stroke::new(2.0, ACCENT), StrokeKind::Inside);
            }
            if is_current {
                painter.rect_stroke(
                    rect.shrink(1.0),
                    4.0,
                    Stroke::new(1.0, Color32::from_gray(220)),
                    StrokeKind::Inside,
                );
            }

            let mut checked = selected;
            let checkbox_rect = egui::Rect::from_min_size(
                rect.left_top() + egui::vec2(6.0, 6.0),
                egui::vec2(18.0, 18.0),
            );
            ui.painter().rect_filled(
                checkbox_rect.expand(2.0),
                3.0,
                Color32::from_black_alpha(120),
            );
            let checkbox = ui.put(checkbox_rect, egui::Checkbox::new(&mut checked, ""));
            if checkbox.changed() {
                state.set_selected(index, checked);
            }

            if response.double_clicked() {
                super::open_develop(state, worker, index);
            } else if response.clicked() && !checkbox.clicked() {
                let modifiers = ui.input(|input| input.modifiers);
                if modifiers.shift {
                    if let Some(from) = state.last_clicked {
                        state.select_range(from, index);
                    } else {
                        state.set_selected(index, true);
                    }
                } else {
                    state.toggle_selected(index);
                }
                state.last_clicked = Some(index);
                state.select_only(index);
            }

            response.clone().on_hover_ui(|ui| {
                ui.label(RichText::new(path.display().to_string()).small());
                ui.label(
                    RichText::new(super::format_exif(&exif))
                        .small()
                        .color(MUTED),
                );
                if let Some(status) = &status
                    && status.is_unsupported()
                {
                    ui.label(
                        RichText::new(unsupported_reason(status))
                            .small()
                            .color(DANGER),
                    );
                }
            });
            response.context_menu(|ui| {
                if ui.button("Open in Develop").clicked() {
                    super::open_develop(state, worker, index);
                    ui.close();
                }
                let toggle_label = if selected { "Deselect" } else { "Select" };
                if ui.button(toggle_label).clicked() {
                    state.set_selected(index, !selected);
                    ui.close();
                }
                if ui.button("Show in Finder").clicked() {
                    super::reveal_path(&path);
                    ui.close();
                }
            });

            ui.add_space(2.0);
            ui.add_sized(
                [cell_w, 16.0],
                Label::new(RichText::new(&name).size(12.0)).truncate(),
            );
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                match &status {
                    Some(status) if status.is_unsupported() => {
                        badge(ui, "unsupported", DANGER);
                    }
                    Some(_) => match edit {
                        EditStatus::Edited => badge(ui, "edited", ACCENT),
                        EditStatus::Original => badge(ui, "original", MUTED),
                    },
                    None => {
                        ui.label(RichText::new("checking…").small().color(MUTED));
                    }
                }
            });
        },
    );
}
