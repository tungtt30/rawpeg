use eframe::egui::{self, Align, Color32, Layout, RichText};

use crate::app::state::{AppState, RightTab, ViewMode};
use crate::app::worker::Worker;

use super::widgets::{ACCENT, MUTED};

pub fn top_bar(ui: &mut egui::Ui, state: &mut AppState, worker: &Worker) {
    egui::Panel::top("top_bar").show(ui, |ui| {
        ui.add_space(5.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("rawpeg").strong().size(16.0).color(ACCENT));
            ui.separator();
            if ui.button("Open Folder").clicked() {
                super::open_folder(state, worker);
            }
            if ui.button("Add Files").clicked() {
                super::add_files(state, worker);
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let stats = state.selection_stats();
                let export_active = state
                    .export
                    .as_ref()
                    .is_some_and(|session| !session.finished);
                let count = stats.export_count();
                let enabled = export_active || count > 0;
                let label = if export_active {
                    "Exporting…".to_string()
                } else {
                    format!("Export {count}")
                };
                let fill = if enabled {
                    ACCENT
                } else {
                    Color32::from_rgb(0x2a, 0x2a, 0x2e)
                };
                let button = egui::Button::new(RichText::new(label).color(Color32::WHITE).strong())
                    .fill(fill)
                    .min_size(egui::vec2(98.0, 28.0));
                let response = ui.add_enabled(enabled, button);
                if response.clicked() {
                    state.right_tab = RightTab::Export;
                }
                if count == 0 && !export_active {
                    response.on_hover_text("Select at least one supported photo");
                } else {
                    response.on_hover_text("Open export settings");
                }
                ui.menu_button("▾", |ui| {
                    if ui.button("Export settings…").clicked() {
                        state.right_tab = RightTab::Export;
                        ui.close();
                    }
                    if ui.button("Quick export").clicked() {
                        state.begin_export(worker);
                        ui.close();
                    }
                });

                ui.add_space(6.0);
                ui.separator();
                ui.add_space(6.0);

                let previous_mode = state.view_mode;
                let mut mode = state.view_mode;
                ui.add_enabled_ui(state.current.is_some(), |ui| {
                    ui.selectable_value(&mut mode, ViewMode::Develop, "Develop");
                });
                ui.selectable_value(&mut mode, ViewMode::Library, "Library");
                if mode != previous_mode {
                    state.view_mode = mode;
                    state.right_tab = match mode {
                        ViewMode::Library => RightTab::Export,
                        ViewMode::Develop => RightTab::Develop,
                    };
                }

                if ui.button("?").on_hover_text("Keyboard shortcuts").clicked() {
                    state.show_help = true;
                }

                ui.add_space(6.0);
                ui.separator();
                ui.add_space(6.0);

                let current = state
                    .current_entry()
                    .map(|entry| {
                        format!(
                            "{}  ·  {}",
                            entry.item.file_name,
                            super::format_exif(&entry.item.exif)
                        )
                    })
                    .unwrap_or_else(|| "No photo selected".to_string());
                ui.add(egui::Label::new(RichText::new(current).small().color(MUTED)).truncate());
            });
        });
        ui.add_space(5.0);
    });
}
