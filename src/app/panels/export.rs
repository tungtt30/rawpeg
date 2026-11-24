use std::path::Path;

use eframe::egui::{self, Color32, Label, RichText, Vec2};

use crate::app::state::{AppState, ToastKind};
use crate::app::worker::Worker;
use crate::models::{ExportFormat, ExportSettings, PerformanceSettings};

use super::widgets::{ACCENT, DANGER, MUTED, SUCCESS, format_duration};

pub fn panel(ui: &mut egui::Ui, state: &mut AppState, worker: &Worker) {
    ui.add_space(6.0);
    ui.heading("Export");
    ui.separator();

    let stats = state.selection_stats();
    let jobs = state.export_jobs();
    let count = jobs.len();
    let active = state
        .export
        .as_ref()
        .is_some_and(|session| !session.finished);

    let mut open_dir = false;
    let mut cancel_export = false;
    let mut dismiss_export = false;
    let mut export_again = false;

    if let Some(session) = &state.export {
        export_progress(
            ui,
            session,
            &mut open_dir,
            &mut cancel_export,
            &mut dismiss_export,
            &mut export_again,
        );
    }

    if cancel_export {
        state.cancel_export();
    }
    if dismiss_export {
        state.dismiss_export();
    }
    if open_dir {
        super::open_export_dir(state);
    }
    if export_again {
        state.dismiss_export();
        state.begin_export(worker);
    }

    ui.add_enabled_ui(!active, |ui| {
        ui.add_space(4.0);
        ui.label(
            RichText::new(format!(
                "{} supported · {} unsupported skipped",
                stats.export_count(),
                stats.selected_unsupported
            ))
            .small()
            .color(MUTED),
        );
        if stats.selected_pending > 0 {
            ui.label(
                RichText::new(format!(
                    "{} file(s) still being probed; export may skip any unsupported ones",
                    stats.selected_pending
                ))
                .small()
                .color(MUTED),
            );
        }
        ui.add_space(8.0);

        ui.label(RichText::new("Destination").strong());
        let mut same = state.export_settings.output_dir.is_none();
        if ui
            .checkbox(&mut same, "Same folder as each source file")
            .changed()
            && same
        {
            state.export_settings.output_dir = None;
        }
        let path_text = state
            .export_settings
            .output_dir
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "next to the original RAW files".to_string());
        ui.add(Label::new(RichText::new(path_text).small().color(MUTED)).truncate());
        if ui.button("Choose folder…").clicked()
            && let Some(dir) =
                super::pick_output(state.export_settings.output_dir.as_deref(), &state.last_dir)
        {
            state.export_settings.output_dir = Some(dir);
        }

        ui.add_space(10.0);
        ui.label(RichText::new("Format").strong());
        ui.horizontal(|ui| {
            for format in ExportFormat::ALL {
                ui.selectable_value(&mut state.export_settings.format, format, format.label());
            }
        });
        ui.label(
            RichText::new(match state.export_settings.format {
                ExportFormat::Jpeg => "JPEG · compact, high quality",
                ExportFormat::Png => "PNG · lossless, larger files",
                ExportFormat::Tiff => "TIFF · lossless working file",
            })
            .small()
            .color(MUTED),
        );

        if state.export_settings.format == ExportFormat::Jpeg {
            ui.add_space(4.0);
            let mut quality = state.export_settings.jpeg_quality;
            if ui
                .add(
                    egui::Slider::new(&mut quality, 1..=100)
                        .text("Quality")
                        .fixed_decimals(0),
                )
                .changed()
            {
                state.export_settings.jpeg_quality = quality;
            }
            ui.checkbox(&mut state.export_settings.chroma_444, "4:4:4 chroma")
                .on_hover_text("Keep full chroma resolution. Larger files, best colour detail.");
        }

        ui.add_space(10.0);
        ui.label(RichText::new("Size").strong());
        let mut limit = state.export_settings.max_dim.is_some();
        let mut max_dim = state.export_settings.max_dim.unwrap_or(4000);
        ui.horizontal(|ui| {
            if ui.selectable_label(!limit, "Original").clicked() {
                limit = false;
            }
            if ui.selectable_label(limit, "Long edge").clicked() {
                limit = true;
            }
            if limit {
                ui.add(
                    egui::DragValue::new(&mut max_dim)
                        .range(100..=20_000)
                        .speed(10.0)
                        .suffix(" px"),
                );
            }
        });
        state.export_settings.max_dim = if limit { Some(max_dim) } else { None };
        ui.label(
            RichText::new("Images are never enlarged.")
                .small()
                .color(MUTED),
        );

        ui.add_space(10.0);
        ui.label(RichText::new("Metadata").strong());
        let exif_available = state.export_settings.format.supports_exif();
        ui.add_enabled_ui(exif_available, |ui| {
            ui.checkbox(&mut state.export_settings.keep_exif, "Preserve EXIF");
        });
        if !exif_available {
            ui.label(
                RichText::new("TIFF export currently writes pixels only.")
                    .small()
                    .color(MUTED),
            );
        }

        ui.add_space(10.0);
        ui.label(RichText::new("Existing files").strong());
        ui.horizontal(|ui| {
            ui.radio_value(&mut state.export_settings.overwrite, true, "Overwrite");
            ui.radio_value(&mut state.export_settings.overwrite, false, "Skip");
        });

        ui.add_space(10.0);
        ui.label(RichText::new("Performance").strong());
        let full_resolution = state.export_settings.max_dim.is_none();
        let max_threads = PerformanceSettings::max_threads();
        let perf = &mut state.export_settings.performance;
        ui.checkbox(&mut perf.multithreaded, "Use multiple CPU cores")
            .on_hover_text("Off exports one photo at a time on a single CPU thread.");
        ui.add_enabled_ui(perf.multithreaded, |ui| {
            let mut auto_threads = perf.threads == 0;
            ui.horizontal(|ui| {
                if ui.checkbox(&mut auto_threads, "Auto threads").changed() {
                    perf.threads = if auto_threads { 0 } else { max_threads };
                }
                if auto_threads {
                    ui.label(
                        RichText::new(format!("{max_threads} logical cores"))
                            .small()
                            .color(MUTED),
                    );
                } else {
                    let mut threads = perf.threads.clamp(1, max_threads);
                    if max_threads <= 1 {
                        ui.label(RichText::new("1 logical core").small().color(MUTED));
                    } else if ui
                        .add(
                            egui::Slider::new(&mut threads, 1..=max_threads)
                                .text("threads")
                                .fixed_decimals(0),
                        )
                        .changed()
                    {
                        perf.threads = threads;
                    }
                }
            });

            let mut auto_photos = perf.concurrent_photos == 0;
            let default_photos = if full_resolution { 2 } else { 4 };
            ui.horizontal(|ui| {
                if ui
                    .checkbox(&mut auto_photos, "Auto parallel photos")
                    .changed()
                {
                    perf.concurrent_photos = if auto_photos {
                        0
                    } else {
                        default_photos.min(perf.max_concurrent_photos())
                    };
                }
                if auto_photos {
                    ui.label(
                        RichText::new(format!("{default_photos} for this size"))
                            .small()
                            .color(MUTED),
                    );
                } else {
                    let max = perf.max_concurrent_photos().max(1);
                    let mut photos = perf.concurrent_photos.clamp(1, max);
                    if max <= 1 {
                        ui.label(RichText::new("1 photo at a time").small().color(MUTED));
                    } else if ui
                        .add(
                            egui::Slider::new(&mut photos, 1..=max)
                                .text("photos at once")
                                .fixed_decimals(0),
                        )
                        .changed()
                    {
                        perf.concurrent_photos = photos;
                    }
                }
            });
            ui.label(
                RichText::new("More parallel photos use more RAM. Full-size exports default to 2.")
                    .small()
                    .color(MUTED),
            );
            ui.label(
                RichText::new(format!(
                    "Using {} CPU thread(s) · {} photo(s) at once",
                    perf.resolved_threads(),
                    perf.resolved_concurrency(full_resolution)
                ))
                .small()
                .color(MUTED),
            );
        });

        ui.add_space(14.0);
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
        let label = format!("Export {count} photo(s)");
        let button = egui::Button::new(RichText::new(&label).color(Color32::WHITE).strong())
            .fill(if count > 0 {
                ACCENT
            } else {
                Color32::from_rgb(0x2a, 0x2a, 0x2e)
            })
            .min_size(Vec2::new(0.0, 32.0));
        if ui
            .add_enabled(count > 0, button)
            .on_hover_text("Start the batch export with these settings")
            .clicked()
        {
            state.begin_export(worker);
        }
        ui.add_space(4.0);
        if ui.button("Reset settings").clicked() {
            state.export_settings = ExportSettings::default();
            state.toast("Export settings reset", ToastKind::Info);
        }
    });
}

fn export_progress(
    ui: &mut egui::Ui,
    session: &crate::app::state::ExportSession,
    open_dir: &mut bool,
    cancel_export: &mut bool,
    dismiss_export: &mut bool,
    export_again: &mut bool,
) {
    ui.add_space(4.0);
    if session.finished {
        let Some(summary) = &session.summary else {
            ui.label(
                RichText::new(
                    session
                        .fatal
                        .clone()
                        .unwrap_or_else(|| "Export failed".into()),
                )
                .color(DANGER),
            );
            if ui.button("Dismiss").clicked() {
                *dismiss_export = true;
            }
            return;
        };
        let ok = summary.failed == 0 && !summary.cancelled;
        ui.label(
            RichText::new(if summary.cancelled {
                format!("Cancelled after {}/{}", summary.processed(), summary.total)
            } else {
                format!(
                    "Exported {} of {} · {} failed · {} skipped",
                    summary.succeeded, summary.total, summary.failed, summary.skipped
                )
            })
            .color(if ok { SUCCESS } else { DANGER })
            .strong(),
        );
        ui.label(
            RichText::new(format!("{} elapsed", format_duration(summary.elapsed)))
                .small()
                .color(MUTED),
        );
        ui.horizontal(|ui| {
            if ui.button("Open folder").clicked() {
                *open_dir = true;
            }
            if ui.button("Export again").clicked() {
                *export_again = true;
            }
            if ui.button("Dismiss").clicked() {
                *dismiss_export = true;
            }
        });
        if !summary.failures.is_empty() {
            egui::CollapsingHeader::new(format!("{} failed file(s)", summary.failures.len())).show(
                ui,
                |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(140.0)
                        .show(ui, |ui| {
                            for failure in &summary.failures {
                                let name = failure
                                    .source
                                    .file_name()
                                    .map(|name| name.to_string_lossy().into_owned())
                                    .unwrap_or_else(|| failure.source.display().to_string());
                                ui.label(
                                    RichText::new(format!("{name}: {}", failure.message))
                                        .small()
                                        .color(DANGER),
                                );
                            }
                        });
                },
            );
        }
        ui.separator();
        return;
    }

    ui.label(
        RichText::new(format!(
            "Exporting {}/{} · {}",
            session.done,
            session.total,
            format_duration(session.elapsed())
        ))
        .small()
        .color(MUTED),
    );
    ui.add(
        egui::ProgressBar::new(session.progress())
            .desired_width(ui.available_width())
            .desired_height(8.0)
            .show_percentage(),
    );
    let current = session
        .current
        .as_deref()
        .map(|current| {
            Path::new(current)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| current.to_string())
        })
        .unwrap_or_else(|| "preparing…".to_string());
    let current = if session.active > 1 {
        format!("{current} (+{} more)", session.active - 1)
    } else {
        current
    };
    ui.label(RichText::new(current).small().color(MUTED));
    ui.horizontal(|ui| {
        if session.cancelling {
            ui.label(RichText::new("Cancelling…").small().color(MUTED));
        } else if ui.button("Cancel").clicked() {
            *cancel_export = true;
        }
        if session.failed > 0 {
            ui.label(
                RichText::new(format!("{} failed", session.failed))
                    .small()
                    .color(DANGER),
            );
        }
    });
    ui.separator();
}
