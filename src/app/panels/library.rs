use eframe::egui::{self, Align, Layout, Response, RichText, Sense, TextureHandle, Vec2};

use crate::app::state::{AppState, LibraryFilter, ViewMode, select_and_load};
use crate::app::worker::Worker;
use crate::models::EditStatus;
use crate::raw_engine::decoder::DecodeStatus;

use super::widgets::{
    ACCENT, DANGER, MUTED, ROW_CURRENT, badge, status_short, thumb_placeholder, unsupported_reason,
};

const ROW_H: f32 = 66.0;

pub fn panel(ui: &mut egui::Ui, state: &mut AppState, worker: &Worker) {
    egui::Panel::left("library")
        .resizable(true)
        .default_size(276.0)
        .min_size(236.0)
        .show(ui, |ui| {
            ui.add_space(6.0);
            let stats = state.selection_stats();
            ui.horizontal(|ui| {
                ui.heading("Library");
                ui.label(RichText::new(format!("({})", stats.total)).color(MUTED));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.menu_button("…", |ui| {
                        if ui.button("Select all supported").clicked() {
                            state.select_supported();
                            ui.close();
                        }
                        if ui.button("Deselect unsupported").clicked() {
                            state.deselect_unsupported();
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("Clear library…").clicked() {
                            state.pending_confirm =
                                Some(crate::app::state::ConfirmAction::ClearLibrary);
                            ui.close();
                        }
                    });
                });
            });
            ui.separator();

            ui.add_space(2.0);
            ui.horizontal_wrapped(|ui| {
                let filters = [
                    (LibraryFilter::All, format!("All {}", stats.total)),
                    (
                        LibraryFilter::Selected,
                        format!("Selected {}", stats.selected),
                    ),
                    (LibraryFilter::Edited, format!("Edited {}", stats.edited)),
                    (
                        LibraryFilter::Issues,
                        format!("Issues {}", stats.unsupported),
                    ),
                ];
                for (filter, label) in filters {
                    ui.selectable_value(&mut state.filter, filter, label);
                }
            });

            ui.horizontal(|ui| {
                let mut all = stats.all_selected();
                let indeterminate = stats.selected > 0 && !all;
                let response = ui
                    .add(egui::Checkbox::new(&mut all, "Select all").indeterminate(indeterminate));
                if response.changed() {
                    state.set_all_selected(all);
                }
                if ui.button("Clear").clicked() {
                    state.set_all_selected(false);
                }
            });
            ui.separator();

            let visible = state.visible_indices();
            if visible.is_empty() {
                ui.add_space(24.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("No photos match this filter.").color(MUTED));
                    if ui.button("Show all").clicked() {
                        state.filter = LibraryFilter::All;
                    }
                });
                return;
            }

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show_rows(ui, ROW_H, visible.len(), |ui, range| {
                    for row in range {
                        let index = visible[row];
                        row_widget(ui, state, worker, index);
                    }
                });
        });
}

fn row_widget(ui: &mut egui::Ui, state: &mut AppState, worker: &Worker, index: usize) {
    let is_current = state.current == Some(index);
    let selected = state.entries[index].item.selected;
    let name = state.entries[index].item.file_name.clone();
    let status = state.entries[index].status.clone();
    let edit = state.entries[index].item.status;
    let path = state.entries[index].item.path.clone();
    let exif = state.entries[index].item.exif.clone();
    state.ensure_thumb(worker, index);
    let thumb = state.entries[index].thumb.clone();

    let rect = ui.available_rect_before_wrap();
    let row_rect = egui::Rect::from_min_size(rect.min, Vec2::new(rect.width(), ROW_H));
    if is_current {
        ui.painter().rect_filled(row_rect, 3.0, ROW_CURRENT);
    }

    let mut clicked = false;
    let mut double_clicked = false;

    let horizontal = ui.horizontal(|ui| {
        ui.set_min_height(ROW_H);
        let mut checked = selected;
        if ui.checkbox(&mut checked, "").changed() {
            state.set_selected(index, checked);
        }

        let thumb_response = thumb_widget(ui, &thumb, Vec2::new(82.0, 54.0), status.as_ref());
        if thumb_response.clicked() {
            clicked = true;
        }
        if thumb_response.double_clicked() {
            double_clicked = true;
        }

        ui.vertical(|ui| {
            ui.add_space(3.0);
            let name_response = ui.selectable_label(is_current, RichText::new(&name).size(12.5));
            if name_response.clicked() {
                clicked = true;
            }
            if name_response.double_clicked() {
                double_clicked = true;
            }
            match &status {
                Some(status) if status.is_unsupported() => {
                    ui.label(RichText::new(status_short(status)).small().color(DANGER));
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
    });

    horizontal.response.on_hover_ui(|ui| {
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

    if double_clicked {
        super::open_develop(state, worker, index);
    } else if clicked {
        let modifiers = ui.input(|input| input.modifiers);
        if modifiers.shift {
            if let Some(from) = state.last_clicked {
                state.select_range(from, index);
            } else {
                state.set_selected(index, true);
            }
        } else if modifiers.command {
            state.toggle_selected(index);
        }
        state.last_clicked = Some(index);
        if state.view_mode == ViewMode::Develop {
            select_and_load(state, worker, index);
        } else {
            state.select_only(index);
        }
    }
    ui.add_space(1.0);
}

fn thumb_widget(
    ui: &mut egui::Ui,
    thumb: &Option<TextureHandle>,
    size: Vec2,
    status: Option<&DecodeStatus>,
) -> Response {
    if let Some(texture) = thumb {
        ui.add(
            egui::Image::new(texture)
                .max_size(size)
                .sense(Sense::click()),
        )
    } else {
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        let (text, color) = match status {
            Some(status) if status.is_unsupported() => ("unsupported", DANGER),
            None => ("…", MUTED),
            _ => ("", MUTED),
        };
        thumb_placeholder(ui.painter(), rect, text, color);
        if status.is_some_and(DecodeStatus::is_unsupported) {
            ui.painter().rect_stroke(
                rect,
                3.0,
                egui::Stroke::new(1.0, DANGER),
                egui::StrokeKind::Inside,
            );
        }
        response
    }
}
