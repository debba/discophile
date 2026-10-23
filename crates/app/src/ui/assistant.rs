//! The music assistant interface: the chat in the right side panel (sparkle toolbar button) or as a full
//! page, its dialogs, and Settings > Music assistant.

mod chat;
mod settings;
mod windows;

use discophile_core::l10n::t;
use egui::Ui;

use crate::app::App;
use crate::nav::{Route, SidePanel};
use crate::theme::icons;
use crate::ui::widgets;

pub use windows::reorder_list;

/// The assistant as a full page (`Route::Assistant`).
pub fn page(app: &mut App, ui: &mut Ui) {
    if app.nav.side_panel == SidePanel::Assistant {
        // One chat view at a time: the page replaces the panel.
        app.nav.side_panel = SidePanel::None;
    }
    if !app.session.has_library() {
        widgets::page(ui, "assistant-empty", |ui| {
            widgets::page_title(ui, &t("Music assistant"));
            widgets::empty_state(ui, icons::SPARKLE, &t("Music assistant"), &t("Choose something from your library to start listening."));
        });
        return;
    }
    egui::Frame::new()
        .inner_margin(egui::Margin { left: widgets::PAGE_MARGIN as i8, right: widgets::PAGE_MARGIN as i8, top: 4, bottom: 12 })
        .show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            chat::show(app, ui, true);
        });
}

/// The assistant in the right side panel.
pub fn panel(app: &mut App, ui: &mut Ui) {
    if app.nav.current == Route::Assistant {
        // Opening the panel while the page is shown moves the chat to the panel.
        if app.nav.can_go_back() { app.nav.back() } else { app.nav.select(Route::ListenNow) }
    }
    if !app.session.has_library() {
        widgets::empty_state(ui, icons::SPARKLE, &t("Music assistant"), &t("Choose something from your library to start listening."));
        return;
    }
    chat::show(app, ui, false);
}

/// Settings > Music assistant: provider, ChatGPT connection and local model.
pub fn settings(app: &mut App, ui: &mut Ui) {
    settings::show(app, ui);
}

/// Assistant dialogs, drawn after the main layout.
pub fn windows(app: &mut App, ctx: &egui::Context) {
    windows::show(app, ctx);
}
