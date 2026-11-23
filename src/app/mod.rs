//! rawpeg darkroom application shell: state, background worker and egui panels.

pub mod panels;
pub mod state;
pub mod worker;

use eframe::egui::{self, Key};

use crate::app::state::{
    AppState, RightTab, ToastKind, ViewMode, ensure_current_loaded, select_and_load,
};
use crate::app::worker::Worker;

/// Root application state plus the background decode/render worker.
pub struct RawpegApp {
    state: AppState,
    worker: Worker,
}

impl RawpegApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        configure_theme(&cc.egui_ctx);
        Self {
            state: AppState::default(),
            worker: Worker::spawn(Some(cc.egui_ctx.clone())),
        }
    }

    /// Test/headless constructor.
    pub fn from_parts(state: AppState, worker: Worker) -> Self {
        Self { state, worker }
    }

    pub fn draw(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.state.poll(&ctx, &self.worker);
        self.state.poll_toast(&ctx);
        self.shortcuts(&ctx);
        panels::handle_drop(&ctx, &mut self.state, &self.worker);

        panels::top_bar(ui, &mut self.state, &self.worker);
        ensure_current_loaded(&mut self.state, &self.worker);
        if !self.state.entries.is_empty() {
            panels::library_panel(ui, &mut self.state, &self.worker);
        }
        if !self.state.entries.is_empty() || self.state.export.is_some() {
            panels::right_panel(ui, &mut self.state, &self.worker);
        }
        panels::status_bar(ui, &mut self.state);
        panels::central(ui, &mut self.state, &self.worker);

        panels::toast(&ctx, &self.state);
        panels::help_modal(&ctx, &mut self.state);
        panels::confirm_modal(&ctx, &mut self.state, &self.worker);
        panels::drop_overlay(&ctx);
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if self.state.show_help
            || self.state.pending_confirm.is_some()
            || ctx.egui_wants_keyboard_input()
        {
            return;
        }

        let mut open_folder = false;
        let mut add_files = false;
        let mut export_panel = false;
        let mut quick_export = false;
        let mut select_all = false;
        let mut clear_selection = false;
        let mut toggle_mode = false;
        let mut open_selected = false;
        let mut toggle_selected = false;
        let mut prev = false;
        let mut next = false;
        let mut fit = false;
        let mut actual = false;
        let mut toggle_before = false;
        let mut close = false;
        let mut help = false;
        let mut copy_settings = false;
        let mut paste_settings = false;

        ctx.input(|input| {
            let command = input.modifiers.command;
            let shift = input.modifiers.shift;
            open_folder = command && input.key_pressed(Key::O) && !shift;
            add_files = command && shift && input.key_pressed(Key::O);
            export_panel = command && input.key_pressed(Key::E) && !shift;
            quick_export = command && shift && input.key_pressed(Key::E);
            select_all = command && input.key_pressed(Key::A) && !shift;
            clear_selection = command && shift && input.key_pressed(Key::A);
            copy_settings = command && input.key_pressed(Key::C);
            paste_settings = command && input.key_pressed(Key::V);
            toggle_mode = !command && input.key_pressed(Key::L);
            open_selected = !command && input.key_pressed(Key::Enter);
            toggle_selected = !command && input.key_pressed(Key::Space);
            prev = input.key_pressed(Key::ArrowLeft);
            next = input.key_pressed(Key::ArrowRight);
            fit = !command && input.key_pressed(Key::F);
            actual = !command && input.key_pressed(Key::Num1);
            toggle_before = !command && input.key_pressed(Key::B);
            close = input.key_pressed(Key::Escape);
            help = input.key_pressed(Key::Questionmark);
        });

        if open_folder {
            panels::open_folder(&mut self.state, &self.worker);
        }
        if add_files {
            panels::add_files(&mut self.state, &self.worker);
        }
        if export_panel {
            self.state.right_tab = RightTab::Export;
        }
        if quick_export {
            self.state.begin_export(&self.worker);
        }
        if select_all {
            self.state.set_all_selected(true);
        }
        if clear_selection {
            self.state.set_all_selected(false);
        }
        if toggle_mode && self.state.current.is_some() {
            self.state.view_mode = match self.state.view_mode {
                ViewMode::Library => ViewMode::Develop,
                ViewMode::Develop => ViewMode::Library,
            };
            self.state.right_tab = match self.state.view_mode {
                ViewMode::Library => RightTab::Export,
                ViewMode::Develop => RightTab::Develop,
            };
        }
        if open_selected
            && self.state.view_mode == ViewMode::Library
            && let Some(index) = self.state.current
        {
            panels::open_develop(&mut self.state, &self.worker, index);
        }
        if toggle_selected
            && self.state.view_mode == ViewMode::Library
            && let Some(index) = self.state.current
        {
            self.state.toggle_selected(index);
            self.state.last_clicked = Some(index);
        }
        if self.state.view_mode == ViewMode::Develop {
            if prev {
                step_current(&mut self.state, &self.worker, -1);
            }
            if next {
                step_current(&mut self.state, &self.worker, 1);
            }
            if fit {
                self.state.viewport.zoom = 1.0;
                self.state.viewport.pan = egui::Vec2::ZERO;
                self.state.viewport.request_actual = false;
            }
            if actual {
                self.state.viewport.request_actual = true;
            }
            if toggle_before {
                self.state.show_before = !self.state.show_before;
            }
        }
        if close && self.state.view_mode == ViewMode::Develop {
            self.state.view_mode = ViewMode::Library;
            self.state.right_tab = RightTab::Export;
        }
        if help {
            self.state.show_help = true;
        }
        if copy_settings {
            self.state.copy_current();
            self.state.toast("Copied settings", ToastKind::Info);
        }
        if paste_settings && let Some(params) = self.state.clipboard.clone() {
            let count = self.state.paste_to_selected(&params);
            if count > 0 {
                if self.state.current_is_selected()
                    && let Some((token, params)) = self.state.render_request()
                {
                    self.worker
                        .send(crate::app::worker::Job::RenderAfter { token, params });
                }
                self.state
                    .toast(format!("Applied to {count} photo(s)"), ToastKind::Success);
            }
        }
    }
}

fn step_current(state: &mut AppState, worker: &Worker, delta: isize) {
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

impl eframe::App for RawpegApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
    }
}

fn configure_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = egui::Color32::from_rgb(0x18, 0x18, 0x1b);
    visuals.window_fill = egui::Color32::from_rgb(0x18, 0x18, 0x1b);
    visuals.extreme_bg_color = egui::Color32::from_rgb(0x11, 0x11, 0x14);
    visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(0x27, 0x27, 0x2a);
    visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(0x27, 0x27, 0x2a);
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(0x32, 0x32, 0x36);
    visuals.widgets.active.bg_fill = egui::Color32::from_rgb(0x3b, 0x82, 0xf6);
    visuals.selection.bg_fill = egui::Color32::from_rgb(0x2b, 0x5f, 0xb8);
    visuals.selection.stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(0x60, 0xa5, 0xfa));
    visuals.hyperlink_color = egui::Color32::from_rgb(0x3b, 0x82, 0xf6);
    visuals.window_corner_radius = egui::CornerRadius::same(8);
    ctx.set_visuals(visuals);

    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 5.0);
        style.spacing.interact_size = egui::vec2(28.0, 20.0);
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(13.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(13.0));
        style
            .text_styles
            .insert(egui::TextStyle::Small, egui::FontId::proportional(11.0));
        style
            .text_styles
            .insert(egui::TextStyle::Heading, egui::FontId::proportional(18.0));
    });
}
