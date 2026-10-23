//! Search: a large field bound to the sidebar's search text, filter chips that only hide sections,
//! library and podcast matches, and an on-demand Apple Podcasts directory search.

use discophile_core::l10n;
use egui::{CornerRadius, Margin, Ui};

use crate::app::App;
use crate::features::library::{self, SearchScope, search};
use crate::theme::{self, Font, Palette, icons};
use crate::ui::library::parts::{self, TrackList};
use crate::ui::widgets;

pub fn show(app: &mut App, ui: &mut Ui) {
    search::drive(app);
    widgets::page(ui, "search", |ui| {
        widgets::page_title(ui, &l10n::t("Search"));
        search_box(app, ui);
        ui.add_space(10.0);
        scope_chips(app, ui);
        ui.add_space(6.0);
        if app.settings.downloaded_only {
            let accent = Palette::of_ui(ui).accent;
            ui.label(theme::text(format!("{}  {}", icons::ARROW_CIRCLE_DOWN, l10n::t("Downloaded only")), Font::Callout).color(accent));
        }
        ui.add_space(8.0);
        results(app, ui);
    });
}

/// The page's own field. It shares `nav.search_text` with the sidebar; it takes the shortcut's
/// focus id only while the sidebar (and its field) is collapsed, so the two never clash.
fn search_box(app: &mut App, ui: &mut Ui) {
    let collapsed = app.settings.sidebar_collapsed;
    let id = if collapsed { egui::Id::unique("search-field") } else { egui::Id::unique("search-page-field") };
    let palette = Palette::of_ui(ui);
    let mut text = app.nav.search_text.clone();
    let mut clear = false;
    let response = egui::Frame::new()
        .fill(palette.surface)
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width().min(720.0));
            ui.horizontal(|ui| {
                ui.label(theme::icon(icons::MAGNIFYING_GLASS, 19.0).color(palette.secondary));
                let width = ui.available_width() - if text.is_empty() { 0.0 } else { 40.0 };
                let response = ui.add(
                    egui::TextEdit::singleline(&mut text)
                        .id(id)
                        .hint_text(l10n::t("Songs, artists, albums, podcasts"))
                        .font(Font::Title3.id())
                        .frame(egui::Frame::NONE)
                        .desired_width(width),
                );
                if !text.is_empty() && widgets::icon_button(ui, icons::X_CIRCLE, &l10n::t("Clear search"), 16.0).clicked() {
                    clear = true;
                }
                response
            })
            .inner
        })
        .inner;
    if app.nav.changed && collapsed {
        response.request_focus();
    }
    if clear {
        text.clear();
    }
    if text != app.nav.search_text {
        app.nav.search_text = text;
    }
}

fn scope_chips(app: &mut App, ui: &mut Ui) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        for scope in SearchScope::ALL {
            if scope == SearchScope::Podcasts && !library::supports_podcasts(app) {
                continue;
            }
            let selected = app.library.search.scope == scope;
            let response = widgets::chip(ui, selected, &scope.title()).on_hover_text(l10n::t("Shows only this kind of result"));
            if response.clicked() {
                app.library.search.scope = scope;
            }
        }
    });
}

fn results(app: &mut App, ui: &mut Ui) {
    let state = &app.library.search;
    if app.nav.search_text.trim().is_empty() {
        widgets::empty_state(
            ui,
            icons::MAGNIFYING_GLASS,
            &l10n::t("Find your kind of music."),
            &l10n::t("Search songs, artists, albums, and podcasts in your collection."),
        );
        return;
    }
    if state.searching {
        ui.vertical_centered(|ui| {
            ui.add_space(24.0);
            ui.spinner();
            ui.label(theme::secondary(ui, l10n::t("Searching…"), Font::Callout));
        });
        return;
    }
    let scope = state.scope;
    let results = state.results.clone();
    let podcasts = state.podcasts.clone();
    let error = state.error.clone();
    let has_results = state.has_visible_results();
    if let Some(error) = &error
        && widgets::error_banner(ui, error, true) {
            search::retry(app);
        }
    if scope.includes_artists() && !results.artists().is_empty() {
        widgets::section_heading(ui, &l10n::t("Artists"), None);
        for artist in results.artists() {
            parts::artist_row(app, ui, artist);
        }
    }
    if scope.includes_albums() && !results.albums().is_empty() {
        widgets::section_heading(ui, &l10n::t("Albums"), None);
        parts::album_grid(app, ui, results.albums());
    }
    if scope.includes_songs() && !results.songs().is_empty() {
        widgets::section_heading(ui, &l10n::t("Songs"), None);
        parts::track_list(app, ui, results.songs(), &TrackList { artwork: true, album: true, ..Default::default() });
    }
    if scope.includes_podcasts() && !podcasts.channels.is_empty() {
        widgets::section_heading(ui, &l10n::t("Podcasts"), None);
        for channel in &podcasts.channels {
            parts::podcast_row(app, ui, channel);
        }
    }
    if scope.includes_podcasts() && !podcasts.episodes.is_empty() {
        widgets::section_heading(ui, &l10n::t("Episodes"), None);
        parts::track_list(app, ui, &podcasts.episodes, &TrackList { artwork: true, album: true, ..Default::default() });
    }
    if !has_results && error.is_none() {
        widgets::empty_state(ui, icons::MAGNIFYING_GLASS, &l10n::t("No matches yet"), &l10n::t("Try another title or artist."));
    }
    if scope.includes_podcasts() && library::supports_podcasts(app) && !app.settings.downloaded_only {
        directory(app, ui);
    }
}

/// Apple's directory is searched only on request, so typing never leaves the user's server.
fn directory(app: &mut App, ui: &mut Ui) {
    widgets::section_heading(
        ui,
        &l10n::t("New podcasts"),
        Some(&l10n::t("Searches the public Apple Podcasts directory. Only the search text is sent to Apple.")),
    );
    let state = &app.library.search;
    let candidates = state.directory.clone();
    let searching = state.searching_directory;
    let error = state.directory_error.clone();
    let adding = state.adding.clone();
    match candidates {
        Some(candidates) => {
            if candidates.is_empty() {
                ui.label(theme::secondary(ui, l10n::t("No podcasts found in the directory."), Font::Callout));
            }
            let disabled = adding.is_some() || app.library.changing_podcasts;
            for candidate in &candidates {
                let subscribed = search::is_subscribed(app, candidate);
                if library::candidate_row(ui, app, candidate, adding.as_deref(), disabled, subscribed) {
                    search::add_directory_podcast(app, candidate);
                }
            }
        }
        None if searching => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(theme::secondary(ui, l10n::t("Looking for the podcast…"), Font::Callout));
            });
        }
        None => {
            let label = format!("{}  {}", icons::MAGNIFYING_GLASS, l10n::t("Search Apple Podcasts"));
            if widgets::action_button(ui, label, true).clicked() {
                search::search_directory(app);
            }
        }
    }
    if let Some(error) = error {
        ui.add_space(6.0);
        if widgets::error_banner(ui, &error, true) {
            search::search_directory(app);
        }
    }
}
