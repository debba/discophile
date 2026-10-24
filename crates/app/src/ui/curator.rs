//! Saved assistant selections in Listen Now and their detail page.

use chrono::{Datelike, Local, TimeZone};
use discophile_core::Song;
use discophile_core::curated_mix::CuratedMix;
use discophile_core::l10n::{self, t};
use egui::{Align, Color32, CornerRadius, Layout, Sense, Ui, vec2};
use uuid::Uuid;

use crate::app::App;
use crate::features::assistant::{self as feature, SelectionEdit};
use crate::features::library::{self, PlaylistChange};
use crate::features::{discovery, downloads};
use crate::nav::Route;
use crate::theme::{self, Font, Palette, icons};
use crate::ui::widgets;

/// Songs that can play now: only downloaded ones while Downloaded only is on.
fn playable(app: &App, songs: &[Song]) -> Vec<Song> {
    if app.settings.downloaded_only { songs.iter().filter(|s| downloads::is_downloaded(app, s)).cloned().collect() } else { songs.to_vec() }
}

fn same_songs(a: &[Song], b: &[Song]) -> bool {
    a.iter().map(Song::stream_identifier).eq(b.iter().map(Song::stream_identifier))
}

/// "12 Oct 2026".
fn date_label(millis: i64) -> String {
    let Some(date) = Local.timestamp_millis_opt(millis).single() else { return String::new() };
    const EN: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    const IT: [&str; 12] = ["gen", "feb", "mar", "apr", "mag", "giu", "lug", "ago", "set", "ott", "nov", "dic"];
    let months = if l10n::language() == l10n::Language::Italian { IT } else { EN };
    format!("{} {} {}", date.day(), months[date.month0() as usize], date.year())
}

// MARK: Listen Now

/// Saved selections created through the music assistant.
pub fn listen_now_section(app: &mut App, ui: &mut Ui) {
    let mixes: Vec<CuratedMix> = discovery::archive(app)
        .curated
        .iter()
        .filter(|m| !app.settings.downloaded_only || m.songs.iter().any(|s| downloads::is_downloaded(app, s)))
        .cloned()
        .collect();
    if mixes.is_empty() {
        return;
    }
    ui.add_space(12.0);
    widgets::section_heading(ui, &t("Your selections"), None);
    // `widgets::tile_grid` cannot lend the app to tiles, so the columns are laid out here.
    let gap = 14.0;
    let available = ui.available_width();
    let columns = ((available + gap) / (320.0 + gap)).floor().max(1.0) as usize;
    let width = ((available - gap * (columns as f32 - 1.0)) / columns as f32).floor();
    for row in mixes.chunks(columns) {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for mix in row {
                ui.allocate_ui_with_layout(vec2(width, 0.0), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(width);
                    selection_card(app, ui, mix);
                });
            }
        });
        ui.add_space(gap);
    }
}

fn selection_card(app: &mut App, ui: &mut Ui, mix: &CuratedMix) {
    let palette = Palette::of_ui(ui);
    let songs = playable(app, &mix.songs);
    // The card is allocated first so the play button added inside stays clickable on top of it.
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 84.0), Sense::click());
    let fill = if response.hovered() { palette.surface_raised } else { palette.surface };
    ui.painter().rect_filled(rect, CornerRadius::same(widgets::CARD_RADIUS), fill);
    let mut child =
        ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(14.0, 12.0))).layout(Layout::left_to_right(Align::Center)));
    let mut play = false;
    {
        let ui = &mut child;
        let first = mix.songs.first();
        let seed = first.map(|s| s.id.clone()).unwrap_or_else(|| mix.id.to_string());
        let texture = first.and_then(|s| s.cover_art.as_deref()).and_then(|cover| app.artwork_texture(cover));
        let (art, _) = ui.allocate_exact_size(vec2(56.0, 56.0), Sense::hover());
        widgets::paint_artwork(ui, art, &seed, texture.as_ref(), 8.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_enabled_ui(!songs.is_empty(), |ui| {
                if widgets::round_play_button(ui, false, &t("Play selection"), 36.0).clicked() {
                    play = true;
                }
            });
            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                ui.add_space(6.0);
                ui.add(egui::Label::new(theme::text(&mix.title, Font::Headline)).truncate().selectable(false));
                let detail = format!("{} · {}", l10n::count("%lld songs", mix.songs.len() as i64), mix.source);
                ui.add(egui::Label::new(theme::secondary(ui, detail, Font::Footnote)).truncate().selectable(false));
            });
        });
    }
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("{}, {}", mix.title, t("View selection"))));
    if play {
        discovery::play_selection(app, &mix.title, songs.clone(), 0, false);
    } else if response.clicked() {
        app.nav.push(Route::CuratedSelection(mix.id));
    }
    response.context_menu(|ui| {
        if ui.add_enabled(!songs.is_empty(), egui::Button::new(format!("{}  {}", icons::PLAY, t("Play selection")))).clicked() {
            discovery::play_selection(app, &mix.title, songs.clone(), 0, false);
            ui.close();
        }
        if ui.button(format!("{}  {}", icons::LIST, t("View selection"))).clicked() {
            app.nav.push(Route::CuratedSelection(mix.id));
            ui.close();
        }
        if ui.button(format!("{}  {}", icons::SPARKLE, t("Edit with assistant"))).clicked() {
            feature::edit_selection_with_assistant(app, mix.id);
            ui.close();
        }
        ui.separator();
        let remove =
            egui::Button::new(theme::text(format!("{}  {}", icons::TRASH, t("Remove selection")), Font::Body).color(palette.danger));
        if ui.add(remove).clicked() {
            discovery::remove_mix(app, mix.id);
            ui.close();
        }
    });
}

// MARK: Saved selection

pub fn show_selection(app: &mut App, ui: &mut Ui, id: Uuid) {
    let Some(mix) = discovery::archive(app).curated.iter().find(|m| m.id == id).cloned() else {
        widgets::page(ui, ("curated-selection-missing", id), |ui| {
            widgets::page_title(ui, &t("Your selection"));
            widgets::empty_state(ui, icons::SPARKLE, &t("Your selection"), &t("This selection is no longer available."));
        });
        return;
    };
    sync_edit(app, &mix);
    let Some(mut edit) = app.assistant.ui.selection.take() else { return };
    widgets::page(ui, ("curated-selection", id), |ui| {
        ui.set_max_width(820.0);
        selection_page(app, ui, &mix, &mut edit);
    });
    delete_dialog(app, ui.ctx(), &mix, &mut edit);
    if app.assistant.ui.selection.is_none() && discovery::archive(app).curated.iter().any(|m| m.id == id) {
        app.assistant.ui.selection = Some(edit);
    }
}

/// Starts editing a selection, or picks up changes saved elsewhere (a chat edit) when nothing is pending.
fn sync_edit(app: &mut App, mix: &CuratedMix) {
    let fresh = |mix: &CuratedMix| SelectionEdit {
        id: mix.id,
        title: mix.title.clone(),
        songs: mix.songs.clone(),
        base_title: mix.title.clone(),
        base_songs: mix.songs.clone(),
        saving: false,
        notice: false,
        error: None,
        confirm_delete: false,
    };
    match app.assistant.ui.selection.as_mut() {
        Some(edit) if edit.id == mix.id => {
            if edit.base_title != mix.title || !same_songs(&edit.base_songs, &mix.songs) {
                let untouched = edit.title == edit.base_title && same_songs(&edit.songs, &edit.base_songs);
                if untouched {
                    edit.title = mix.title.clone();
                    edit.songs = mix.songs.clone();
                }
                edit.base_title = mix.title.clone();
                edit.base_songs = mix.songs.clone();
            }
        }
        _ => app.assistant.ui.selection = Some(fresh(mix)),
    }
}

fn selection_page(app: &mut App, ui: &mut Ui, mix: &CuratedMix, edit: &mut SelectionEdit) {
    let palette = Palette::of_ui(ui);
    widgets::page_title(ui, &t("Your selection"));
    if !mix.explanation.is_empty() {
        ui.add(egui::Label::new(theme::text(&mix.explanation, Font::Body)).wrap());
    }
    ui.label(theme::secondary(ui, format!("{} · {}", mix.source, date_label(mix.created)), Font::Footnote));
    ui.add_space(10.0);

    let title_valid = !edit.title.trim().is_empty();
    let changed = edit.title != mix.title || !same_songs(&edit.songs, &mix.songs);
    let visible = playable(app, &edit.songs);
    let saved_playlist = mix.playlist_id.is_some() || mix.playlist_saved == Some(true);
    let busy = edit.saving || app.library.changing_playlist;
    widgets::card(ui, |ui| {
        ui.add_enabled_ui(!busy, |ui| {
            ui.label(theme::secondary(ui, t("Selection name"), Font::Footnote));
            let response = ui.add(
                egui::TextEdit::singleline(&mut edit.title).align(egui::Align2::LEFT_CENTER).desired_width(f32::INFINITY).margin(vec2(10.0, 8.0)).font(Font::Title3.id()),
            );
            if response.changed() {
                edit.notice = false;
            }
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                let play =
                    ui.add_enabled_ui(!visible.is_empty(), |ui| widgets::pill_button(ui, icons::PLAY, t("Play selection"), true)).inner;
                if play.clicked() {
                    discovery::play_selection(app, &edit.title, visible.clone(), 0, false);
                }
                let shuffle =
                    ui.add_enabled_ui(visible.len() > 1, |ui| widgets::pill_button(ui, icons::SHUFFLE, t("Shuffle"), false)).inner;
                if shuffle.clicked() {
                    discovery::play_selection(app, &edit.title, visible.clone(), 0, true);
                }
                let can_save = changed && title_valid && !edit.songs.is_empty();
                if ui
                    .add_enabled(can_save, egui::Button::new(format!("{}  {}", icons::FLOPPY_DISK, t("Save selection on this computer"))))
                    .clicked()
                {
                    save_selection(app, mix, edit);
                }
                let can_playlist =
                    !saved_playlist && title_valid && !edit.songs.is_empty() && !app.settings.downloaded_only && app.session.has_library();
                if ui.add_enabled(can_playlist, egui::Button::new(format!("{}  {}", icons::LIST_PLUS, t("Save playlist")))).clicked() {
                    save_playlist(app, mix, edit);
                }
                let chat = ui.add_enabled(
                    !changed && app.session.has_library(),
                    egui::Button::new(format!("{}  {}", icons::SPARKLE, t("Edit with assistant"))),
                );
                if chat.clicked() {
                    feature::edit_selection_with_assistant(app, mix.id);
                }
                let remove = egui::Button::new(
                    theme::text(format!("{}  {}", icons::TRASH, t("Remove selection")), Font::Body).color(palette.danger),
                );
                if ui.add(remove).clicked() {
                    edit.confirm_delete = true;
                }
            });
        });
        if busy {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(theme::secondary(ui, t("Saving…"), Font::Callout));
            });
        }
        if let Some(error) = &edit.error {
            ui.label(theme::text(error, Font::Callout).color(palette.accent));
        }
        if edit.notice && !changed {
            ui.label(theme::text(format!("{}  {}", icons::CHECK_CIRCLE, t("Saved")), Font::Callout).color(palette.positive));
        }
        if let Some(playlist) = &mix.playlist_id {
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(theme::secondary(ui, t("Saved as a playlist on your music server."), Font::Footnote));
                if ui.link(t("Open playlist")).clicked() {
                    app.nav.push(Route::Playlist(playlist.clone()));
                }
            });
        }
    });
    ui.add_space(10.0);
    ui.label(theme::secondary(ui, l10n::count("%lld songs", edit.songs.len() as i64), Font::Headline));
    let before = edit.songs.len();
    let ids: Vec<String> = edit.songs.iter().map(|s| s.stream_identifier().to_string()).collect();
    ui.add_enabled_ui(!busy, |ui| crate::ui::assistant::reorder_list(ui, &mut edit.songs, "curated-selection-songs"));
    if edit.songs.len() != before || !edit.songs.iter().map(Song::stream_identifier).eq(ids.iter().map(String::as_str)) {
        edit.notice = false;
    }
    ui.add_space(4.0);
    ui.label(theme::secondary(ui, t("Edit the name, remove or reorder songs, then save your selection or playlist."), Font::Footnote));
}

/// Saves the edited name and tracks on this computer.
fn save_selection(app: &mut App, mix: &CuratedMix, edit: &mut SelectionEdit) {
    let mut updated = mix.clone();
    updated.title = edit.title.trim().to_string();
    updated.songs = edit.songs.clone();
    edit.notice = discovery::save_mix(app, updated);
    edit.error = (!edit.notice).then(|| t("The selection could not be saved."));
    if edit.notice {
        edit.title = edit.title.trim().to_string();
        edit.base_title = edit.title.clone();
        edit.base_songs = edit.songs.clone();
    }
}

/// Creates a server playlist from the edited selection and keeps its identifier.
fn save_playlist(app: &mut App, mix: &CuratedMix, edit: &mut SelectionEdit) {
    if app.library.changing_playlist {
        return;
    }
    let title = edit.title.trim().to_string();
    let songs = edit.songs.clone();
    let id = mix.id;
    let original = mix.clone();
    edit.saving = true;
    edit.error = None;
    library::change_playlist(app, PlaylistChange::Create(title.clone(), songs.clone()), move |app: &mut App, result| {
        let finish = |app: &mut App, error: Option<String>, notice: bool| {
            if let Some(edit) = app.assistant.ui.selection.as_mut().filter(|e| e.id == id) {
                edit.saving = false;
                edit.error = error;
                edit.notice = notice;
                if notice {
                    edit.title = title.clone();
                    edit.base_title = title.clone();
                    edit.base_songs = songs.clone();
                }
            }
        };
        match result {
            Ok(result) => {
                let mut mix = discovery::archive(app).curated.iter().find(|m| m.id == id).cloned().unwrap_or(original);
                mix.title = title.clone();
                mix.songs = songs.clone();
                mix.playlist_id = result.created.map(|p| p.id);
                mix.playlist_saved = Some(true);
                let saved = discovery::save_mix(app, mix);
                finish(app, (!saved).then(|| t("The selection could not be saved.")), saved);
                app.toast(t("Playlist saved"));
            }
            Err(error) => finish(app, Some(l10n::music_error(error)), false),
        }
    });
    // Nothing started (for example without a library): the page must not keep waiting.
    if !app.library.changing_playlist {
        edit.saving = false;
    }
}

fn delete_dialog(app: &mut App, ctx: &egui::Context, mix: &CuratedMix, edit: &mut SelectionEdit) {
    if !edit.confirm_delete {
        return;
    }
    let mut close = false;
    let mut confirm = false;
    let modal = crate::ui::widgets::modal(ctx, egui::Id::unique(("curated-selection-delete", mix.id))).show(ctx, |ui| {
        ui.set_width(380.0);
        ui.label(theme::text(t("Remove selection"), Font::Title3));
        ui.add_space(6.0);
        ui.label(theme::text(t("This removes the selection from this computer. Saved playlists are kept."), Font::Body));
        ui.add_space(12.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let palette = Palette::of_ui(ui);
            let button = egui::Button::new(theme::text(t("Remove selection"), Font::Headline).color(Color32::WHITE))
                .fill(palette.danger_fill)
                .corner_radius(CornerRadius::same(10));
            if ui.add(button).clicked() {
                confirm = true;
            }
            if ui.button(t("Cancel")).clicked() {
                close = true;
            }
        });
    });
    if confirm {
        discovery::remove_mix(app, mix.id);
        app.assistant.ui.selection = None;
        if app.nav.can_go_back() {
            app.nav.back();
        } else {
            app.nav.select(Route::ListenNow);
        }
    }
    if confirm || close || modal.should_close() {
        edit.confirm_delete = false;
    }
}
