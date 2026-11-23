use std::path::Path;

use eframe::egui::{self, Align, Layout, RichText};

use crate::app::state::AppState;

use super::widgets::{ACCENT, DANGER, MUTED, SUCCESS, format_duration};

pub fn bar(ui: &mut egui::Ui, state: &mut AppState) {
    egui::Panel::bottom("status_bar").show(ui, |ui| {
        ui.add_space(2.0);
        let mut cancel_export = false;
        let mut dismiss_export = false;
        let mut open_export = false;
        let mut request_open = false;

        ui.horizontal(|ui| {
            if state.proxy_loading {
                ui.spinner();
            }
            let label = if state.proxy_loading {
                "Loading preview…".to_string()
            } else if state.status.is_empty() {
                "Ready".to_string()
            } else {
                state.status.clone()
            };
            ui.label(RichText::new(label).small().color(MUTED));

            let stats = state.selection_stats();
            ui.separator();
            ui.label(
                RichText::new(format!("{} of {} selected", stats.selected, stats.total))
                    .small()
                    .color(MUTED),
            );
            if stats.unsupported > 0 {
                ui.label(
                    RichText::new(format!("· {} unsupported", stats.unsupported))
                        .small()
                        .color(DANGER),
                );
            }
            if stats.edited > 0 {
                ui.label(
                    RichText::new(format!("· {} edited", stats.edited))
                        .small()
                        .color(ACCENT),
                );
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let Some(session) = &state.export else {
                    return;
                };
                if session.finished {
                    if let Some(summary) = &session.summary {
                        let color = if summary.failed > 0 { DANGER } else { SUCCESS };
                        ui.label(
                            RichText::new(format!(
                                "Exported {}/{} · {} failed · {}",
                                summary.succeeded,
                                summary.total,
                                summary.failed,
                                format_duration(summary.elapsed)
                            ))
                            .small()
                            .color(color),
                        );
                        if ui.small_button("Open folder").clicked() {
                            request_open = true;
                        }
                        if ui.small_button("Dismiss").clicked() {
                            dismiss_export = true;
                        }
                    } else {
                        ui.label(
                            RichText::new(
                                session
                                    .fatal
                                    .clone()
                                    .unwrap_or_else(|| "Export failed".to_string()),
                            )
                            .small()
                            .color(DANGER),
                        );
                        if ui.small_button("Dismiss").clicked() {
                            dismiss_export = true;
                        }
                    }
                } else {
                    if session.cancelling {
                        ui.label(RichText::new("Cancelling…").small().color(MUTED));
                    } else if ui.small_button("Cancel").clicked() {
                        cancel_export = true;
                    }
                    let file = session
                        .current
                        .as_deref()
                        .map(|current| {
                            Path::new(current)
                                .file_name()
                                .map(|name| name.to_string_lossy().into_owned())
                                .unwrap_or_else(|| current.to_string())
                        })
                        .unwrap_or_else(|| "preparing…".to_string());
                    ui.label(
                        RichText::new(format!("{file} · {}", format_duration(session.elapsed())))
                            .small()
                            .color(MUTED),
                    );
                    if session.failed > 0 {
                        ui.label(
                            RichText::new(format!("{} failed", session.failed))
                                .small()
                                .color(DANGER),
                        );
                    }
                    ui.add(
                        egui::ProgressBar::new(session.progress())
                            .desired_width(160.0)
                            .desired_height(6.0)
                            .text(format!("{}/{}", session.done, session.total)),
                    );
                }
            });
        });
        ui.add_space(2.0);

        if cancel_export {
            state.cancel_export();
        }
        if dismiss_export {
            state.dismiss_export();
        }
        if request_open {
            open_export = true;
        }
        if open_export {
            super::open_export_dir(state);
        }
    });
}
