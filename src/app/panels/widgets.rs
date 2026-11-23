//! Shared visual helpers and small widgets for the rawpeg UI.

use std::path::Path;
use std::time::Duration;

use eframe::egui::{self, Align2, Color32, FontId, Response, RichText, Sense, Stroke, Vec2};

use crate::models::ExifSummary;
use crate::raw_engine::decoder::DecodeStatus;

use super::super::state::{HIST_BINS, RgbHistogram};

pub const ACCENT: Color32 = Color32::from_rgb(0x3b, 0x82, 0xf6);
pub const DANGER: Color32 = Color32::from_rgb(0xf8, 0x71, 0x71);
pub const SUCCESS: Color32 = Color32::from_rgb(0x4a, 0xde, 0x80);
pub const MUTED: Color32 = Color32::from_gray(140);
pub const ROW_CURRENT: Color32 = Color32::from_rgb(0x22, 0x2b, 0x3a);
pub const VIEWPORT_BG: Color32 = Color32::from_rgb(0x0e, 0x0e, 0x11);
pub const THUMB_BG: Color32 = Color32::from_rgb(0x1f, 0x1f, 0x23);

pub fn primary_button(ui: &mut egui::Ui, label: impl Into<String>) -> Response {
    let button = egui::Button::new(RichText::new(label.into()).color(Color32::WHITE).strong())
        .fill(ACCENT)
        .min_size(Vec2::new(0.0, 28.0));
    ui.add(button)
}

pub fn danger_button(ui: &mut egui::Ui, label: impl Into<String>) -> Response {
    let button = egui::Button::new(RichText::new(label.into()).color(Color32::WHITE))
        .fill(Color32::from_rgb(0x9f, 0x2f, 0x2f))
        .min_size(Vec2::new(0.0, 26.0));
    ui.add(button)
}

pub fn badge(ui: &mut egui::Ui, text: &str, color: Color32) {
    ui.label(RichText::new(text).small().color(color));
}

pub fn format_exif(exif: &ExifSummary) -> String {
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
    if parts.is_empty() {
        "EXIF: —".to_string()
    } else {
        parts.join("  ·  ")
    }
}

pub fn format_duration(duration: Duration) -> String {
    let secs = duration.as_secs();
    if secs >= 3600 {
        format!("{}:{:02}:{:02}", secs / 3600, (secs % 3600) / 60, secs % 60)
    } else {
        format!("{}:{:02}", secs / 60, secs % 60)
    }
}

pub fn status_short(status: &DecodeStatus) -> &'static str {
    match status {
        DecodeStatus::Supported => "ok",
        DecodeStatus::UnsupportedCamera(_) => "unsupported camera",
        DecodeStatus::UnsupportedFormat(_) => "unsupported format",
        DecodeStatus::Error(_) => "decode error",
    }
}

pub fn unsupported_reason(status: &DecodeStatus) -> String {
    match status {
        DecodeStatus::Supported => String::new(),
        DecodeStatus::UnsupportedCamera(reason)
        | DecodeStatus::UnsupportedFormat(reason)
        | DecodeStatus::Error(reason) => reason.clone(),
    }
}

pub fn open_path(path: &Path) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(path).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer").arg(path).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    }
}

pub fn reveal_path(path: &Path) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer")
            .arg(format!("/select,{}", path.display()))
            .spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(parent) = path.parent() {
            let _ = std::process::Command::new("xdg-open").arg(parent).spawn();
        }
    }
}

pub fn draw_histogram(ui: &mut egui::Ui, histogram: &RgbHistogram) {
    let size = egui::vec2(ui.available_width(), 110.0);
    let (response, painter) = ui.allocate_painter(size, Sense::hover());
    let rect = response.rect;
    painter.rect_filled(rect, 3.0, VIEWPORT_BG);
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

pub fn thumb_placeholder(painter: &egui::Painter, rect: egui::Rect, text: &str, color: Color32) {
    painter.rect_filled(rect, 3.0, THUMB_BG);
    if !text.is_empty() {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            text,
            FontId::proportional(10.0),
            color,
        );
    }
}
