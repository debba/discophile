//! Library dialogs: Add to playlist, New/Rename playlist, the playlist track editor, Add podcast and
//! delete confirmations. Desktop modals in place of the iOS sheets and alerts.

use discophile_core::podcast_directory::{PodcastCandidate, PodcastDirectory, PodcastSource};
use discophile_core::{MusicError, Song, l10n};
use egui::{Align, CornerRadius, Layout, Sense, Ui, vec2};

use super::{PlaylistChange, keys};
use crate::app::App;
use crate::nav::Route;
use crate::tasks::update;
use crate::theme::{self, Font, Palette, icons};
use crate::ui::widgets;

#[derive(Default)]
pub(super) struct Windows {
    picker: Option<Picker>,
    name: Option<NameDialog>,
    editor: Option<Editor>,
    add_podcast: Option<AddPodcast>,
    confirm: Option<Confirm>,
    token: u64,
}

impl Windows {
    fn next_token(&mut self) -> u64 {
        self.token += 1;
        self.token
    }
}

struct Picker {
    songs: Vec<Song>,
    new_name: String,
    error: Option<String>,
}

struct NameDialog {
    /// The playlist being renamed; `None` creates a new playlist with `songs`.
    playlist: Option<String>,
    songs: Vec<Song>,
    text: String,
    error: Option<String>,
    focus: bool,
}

struct Editor {
    id: String,
    name: String,
    original: Vec<Song>,
    /// Entries with a stable key, so duplicates of a song can be moved independently.
    entries: Vec<(u64, Song)>,
    next_key: u64,
    loading: bool,
    loaded: bool,
    error: Option<String>,
    conflict: bool,
    token: u64,
}

struct AddPodcast {
    link: String,
    candidates: Vec<PodcastCandidate>,
    searching: bool,
    adding: Option<String>,
    error: Option<String>,
    token: u64,
    focus: bool,
}

enum Confirm {
    DeletePlaylist { id: String },
    DeletePodcast { id: String },
}

/// Opens the "Add to playlist" picker for these songs (skips songs already present, then shows a summary).
pub fn open_playlist_picker(app: &mut App, songs: Vec<Song>) {
    let songs: Vec<Song> = songs.into_iter().filter(|s| !s.is_podcast()).collect();
    if songs.is_empty() {
        return;
    }
    if app.client().is_none() {
        return app.show_error(l10n::music_error(MusicError::MissingCredentials));
    }
    super::load_playlists(app);
    app.library.windows.picker = Some(Picker { songs, new_name: String::new(), error: None });
}

/// Asks for a name and creates a playlist, optionally with songs.
pub fn open_new_playlist(app: &mut App, songs: Vec<Song>) {
    app.library.windows.name = Some(NameDialog { playlist: None, songs, text: String::new(), error: None, focus: true });
}

pub fn open_rename_playlist(app: &mut App, id: &str) {
    let text = super::find_playlist(app, id).map(|p| p.name).unwrap_or_default();
    app.library.windows.name = Some(NameDialog { playlist: Some(id.to_string()), songs: Vec::new(), text, error: None, focus: true });
}

/// Opens the playlist track editor (reorder/remove, then Save).
pub fn open_playlist_editor(app: &mut App, playlist_id: &str) {
    let token = app.library.windows.next_token();
    let name = super::find_playlist(app, playlist_id).map(|p| p.name).unwrap_or_default();
    app.library.windows.editor = Some(Editor {
        id: playlist_id.to_string(),
        name,
        original: Vec::new(),
        entries: Vec::new(),
        next_key: 0,
        loading: false,
        loaded: false,
        error: None,
        conflict: false,
        token,
    });
    load_editor(app);
}

pub fn open_add_podcast(app: &mut App) {
    let token = app.library.windows.next_token();
    app.library.windows.add_podcast =
        Some(AddPodcast { link: String::new(), candidates: Vec::new(), searching: false, adding: None, error: None, token, focus: true });
}

pub fn confirm_delete_playlist(app: &mut App, id: &str) {
    app.library.windows.confirm = Some(Confirm::DeletePlaylist { id: id.to_string() });
}

pub fn confirm_delete_podcast(app: &mut App, id: &str) {
    app.library.windows.confirm = Some(Confirm::DeletePodcast { id: id.to_string() });
}

pub(super) fn show(app: &mut App, ctx: &egui::Context) {
    picker(app, ctx);
    name_dialog(app, ctx);
    editor(app, ctx);
    add_podcast(app, ctx);
    confirm(app, ctx);
}

fn dialog_title(ui: &mut Ui, title: &str) {
    ui.label(theme::text(title, Font::Title3));
    ui.add_space(6.0);
}

fn footer(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.add_space(12.0);
    ui.with_layout(Layout::right_to_left(Align::Center), add);
}

fn inline_error(ui: &mut Ui, error: &Option<String>) {
    if let Some(error) = error {
        ui.add_space(4.0);
        ui.label(theme::text(error, Font::Callout).color(Palette::of_ui(ui).danger));
    }
}

fn text_field(ui: &mut Ui, text: &mut String, hint: &str, focus: &mut bool) -> egui::Response {
    let response = ui.add(egui::TextEdit::singleline(text).hint_text(hint).desired_width(f32::INFINITY).margin(vec2(10.0, 8.0)));
    if std::mem::take(focus) {
        response.request_focus();
    }
    response
}

// MARK: Add to playlist

fn picker(app: &mut App, ctx: &egui::Context) {
    let Some(mut state) = app.library.windows.picker.take() else { return };
    let busy = app.library.changing_playlist;
    let playlists = app.library.snapshot.playlists.clone();
    let loading = super::is_loading(app, keys::PLAYLISTS);
    let mut close = false;
    let mut add_to: Option<String> = None;
    let mut create = false;
    let modal = egui::Modal::new(egui::Id::unique("library-playlist-picker")).show(ctx, |ui| {
        ui.set_width(440.0);
        dialog_title(ui, &l10n::t("Add to playlist"));
        ui.label(theme::secondary(ui, l10n::count("%lld songs", state.songs.len() as i64), Font::Subheadline));
        ui.add_space(10.0);
        ui.add_enabled_ui(!busy, |ui| {
            ui.horizontal(|ui| {
                let response = ui.add(
                    egui::TextEdit::singleline(&mut state.new_name)
                        .hint_text(l10n::t("New playlist"))
                        .desired_width(ui.available_width() - 150.0)
                        .margin(vec2(10.0, 8.0)),
                );
                let enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let valid = !state.new_name.trim().is_empty();
                if (ui.add_enabled(valid, egui::Button::new(l10n::t("Create playlist"))).clicked() || enter) && valid {
                    create = true;
                }
            });
            ui.add_space(8.0);
            if loading && playlists.is_empty() {
                widgets::loading(ui);
            }
            egui::ScrollArea::vertical().max_height(360.0).auto_shrink([false, true]).show(ui, |ui| {
                for playlist in &playlists {
                    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 52.0), Sense::click());
                    let palette = Palette::of_ui(ui);
                    if response.hovered() {
                        ui.painter().rect_filled(rect, CornerRadius::same(8), palette.hover);
                    }
                    let mut child = ui.new_child(
                        egui::UiBuilder::new().max_rect(rect.shrink2(vec2(8.0, 4.0))).layout(Layout::left_to_right(Align::Center)),
                    );
                    widgets::artwork(&mut child, app, &playlist.id, playlist.cover_art.as_deref(), 40.0, 6.0);
                    child.vertical(|ui| {
                        ui.add(egui::Label::new(theme::text(&playlist.name, Font::Body)).truncate().selectable(false));
                        ui.label(theme::secondary(ui, l10n::count("%lld songs", playlist.song_count.unwrap_or(0)), Font::Footnote));
                    });
                    if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        add_to = Some(playlist.id.clone());
                    }
                }
            });
        });
        if busy {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(theme::secondary(ui, l10n::t("Saving…"), Font::Callout));
            });
        }
        if let Some(error) = super::error(app, keys::PLAYLISTS).filter(|_| playlists.is_empty())
            && widgets::error_banner(ui, &error, true) {
                super::load_playlists(app);
            }
        inline_error(ui, &state.error);
        footer(ui, |ui| {
            if ui.add_enabled(!busy, egui::Button::new(l10n::t("Cancel"))).clicked() {
                close = true;
            }
        });
    });
    if close || (modal.should_close() && !busy) {
        return;
    }
    let songs = state.songs.clone();
    let name = state.new_name.trim().to_string();
    if create || add_to.is_some() {
        state.error = None;
    }
    app.library.windows.picker = Some(state);
    if create {
        let added = songs.len() as i64;
        super::change_playlist(app, PlaylistChange::Create(name, songs), move |app, result| match result {
            Ok(_) => {
                app.library.windows.picker = None;
                app.toast(l10n::count("Added: %lld", added));
            }
            Err(error) => picker_error(app, error),
        });
    } else if let Some(id) = add_to {
        super::change_playlist(app, PlaylistChange::Add(id, songs), |app, result| match result {
            Ok(result) => {
                app.library.windows.picker = None;
                let addition = result.addition.unwrap_or_default();
                let mut message = l10n::count("Added: %lld", addition.added_count as i64);
                if addition.skipped_count > 0 {
                    message = format!("{message} · {}", l10n::count("Already present: %lld", addition.skipped_count as i64));
                }
                app.toast(message);
            }
            Err(error) => picker_error(app, error),
        });
    }
}

fn picker_error(app: &mut App, error: MusicError) {
    if let Some(picker) = app.library.windows.picker.as_mut() {
        picker.error = Some(l10n::music_error(error));
    }
}

// MARK: New / Rename playlist

fn name_dialog(app: &mut App, ctx: &egui::Context) {
    let Some(mut state) = app.library.windows.name.take() else { return };
    let busy = app.library.changing_playlist;
    let mut close = false;
    let mut save = false;
    let title = if state.playlist.is_some() { l10n::t("Rename playlist") } else { l10n::t("New playlist") };
    let modal = egui::Modal::new(egui::Id::unique("library-playlist-name")).show(ctx, |ui| {
        ui.set_width(380.0);
        dialog_title(ui, &title);
        let response = text_field(ui, &mut state.text, &l10n::t("Playlist name"), &mut state.focus);
        let valid = !state.text.trim().is_empty();
        if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && valid && !busy {
            save = true;
        }
        if busy {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(theme::secondary(ui, l10n::t("Saving…"), Font::Callout));
            });
        }
        inline_error(ui, &state.error);
        footer(ui, |ui| {
            if ui.add_enabled(valid && !busy, egui::Button::new(theme::text(l10n::t("Save"), Font::Headline))).clicked() {
                save = true;
            }
            if ui.add_enabled(!busy, egui::Button::new(l10n::t("Cancel"))).clicked() {
                close = true;
            }
        });
    });
    if close || (modal.should_close() && !busy) {
        return;
    }
    if save {
        state.error = None;
        let name = state.text.trim().to_string();
        let change = match &state.playlist {
            Some(id) => PlaylistChange::Rename(id.clone(), name),
            None => PlaylistChange::Create(name, state.songs.clone()),
        };
        app.library.windows.name = Some(state);
        return super::change_playlist(app, change, |app, result| match result {
            Ok(_) => app.library.windows.name = None,
            Err(error) => {
                if let Some(dialog) = app.library.windows.name.as_mut() {
                    dialog.error = Some(l10n::music_error(error));
                }
            }
        });
    }
    app.library.windows.name = Some(state);
}

// MARK: Edit tracks

fn load_editor(app: &mut App) {
    let Some(editor) = app.library.windows.editor.as_mut() else { return };
    let Some(client) = app.session.client.clone() else { return };
    editor.loading = true;
    editor.error = None;
    editor.conflict = false;
    let (id, token, generation) = (editor.id.clone(), editor.token, app.session.generation);
    app.tasks.spawn(async move {
        let result = client.playlist(&id).await;
        update(move |app: &mut App| {
            if app.session.generation != generation {
                return;
            }
            let Some(editor) = app.library.windows.editor.as_mut().filter(|e| e.token == token) else { return };
            editor.loading = false;
            match result {
                Ok(playlist) => {
                    if !playlist.name.is_empty() {
                        editor.name = playlist.name.clone();
                    }
                    editor.original = playlist.songs().to_vec();
                    editor.entries = editor.original.iter().cloned().enumerate().map(|(i, s)| (i as u64, s)).collect();
                    editor.next_key = editor.entries.len() as u64;
                    editor.loaded = true;
                }
                Err(error) => editor.error = Some(l10n::music_error(error)),
            }
        })
    });
}

fn editor(app: &mut App, ctx: &egui::Context) {
    let Some(mut state) = app.library.windows.editor.take() else { return };
    let busy = app.library.changing_playlist;
    let changed = state.entries.iter().map(|(_, s)| s.stream_identifier()).ne(state.original.iter().map(Song::stream_identifier));
    let mut close = false;
    let mut save = false;
    let mut reload = false;
    let modal = egui::Modal::new(egui::Id::unique("library-playlist-editor")).show(ctx, |ui| {
        ui.set_width(600.0);
        dialog_title(ui, &l10n::t("Edit tracks"));
        if !state.name.is_empty() {
            ui.label(theme::secondary(ui, &state.name, Font::Subheadline));
        }
        ui.add_space(8.0);
        if state.loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(theme::secondary(ui, l10n::t("Loading tracks…"), Font::Callout));
            });
        }
        if state.loaded {
            ui.add_enabled_ui(!busy, |ui| editor_list(ui, &mut state));
            ui.add_space(6.0);
            ui.label(theme::secondary(
                ui,
                l10n::t(
                    "Drag to reorder, or use the arrows. Remove a track with the minus button. Changes are saved only when you click Save.",
                ),
                Font::Footnote,
            ));
        }
        if busy {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(theme::secondary(ui, l10n::t("Saving…"), Font::Callout));
            });
        }
        inline_error(ui, &state.error);
        footer(ui, |ui| {
            let can_save = state.loaded && changed && !state.loading && !busy;
            if ui.add_enabled(can_save, egui::Button::new(theme::text(l10n::t("Save"), Font::Headline))).clicked() {
                save = true;
            }
            if ui.add_enabled(!busy, egui::Button::new(l10n::t("Cancel"))).clicked() {
                close = true;
            }
            if (!state.loaded || state.conflict) && !state.loading && ui.button(l10n::t("Try again")).clicked() {
                reload = true;
            }
        });
    });
    if close || (modal.should_close() && !busy) {
        return;
    }
    if save {
        state.error = None;
        let id = state.id.clone();
        let change =
            PlaylistChange::Tracks { id, expected: state.original.clone(), songs: state.entries.iter().map(|(_, s)| s.clone()).collect() };
        let token = state.token;
        app.library.windows.editor = Some(state);
        return super::change_playlist(app, change, move |app, result| match result {
            Ok(_) => {
                app.library.windows.editor = None;
                app.toast(l10n::t("Playlist saved"));
            }
            Err(error) => {
                if let Some(editor) = app.library.windows.editor.as_mut().filter(|e| e.token == token) {
                    editor.conflict = error == MusicError::PlaylistChanged;
                    editor.error = Some(l10n::music_error(error));
                }
            }
        });
    }
    app.library.windows.editor = Some(state);
    if reload {
        load_editor(app);
    }
}

fn editor_list(ui: &mut Ui, state: &mut Editor) {
    let palette = Palette::of_ui(ui);
    let ctx = ui.ctx().clone();
    let dragging = egui::DragAndDrop::has_payload_of_type::<usize>(&ctx);
    let pointer = ctx.pointer_interact_pos();
    let released = ui.input(|i| i.pointer.any_released());
    let mut action: Option<(usize, Option<usize>)> = None;
    let count = state.entries.len();
    egui::ScrollArea::vertical().max_height(420.0).auto_shrink([false, true]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        for index in 0..count {
            let (key, song) = &state.entries[index];
            let id = egui::Id::unique(("playlist-editor-row", *key));
            let row = ui.dnd_drag_source(id, index, |ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 46.0), Sense::hover());
                ui.painter().rect_filled(rect, CornerRadius::same(8), palette.surface_raised.gamma_multiply(0.6));
                let mut child = ui
                    .new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(10.0, 4.0))).layout(Layout::left_to_right(Align::Center)));
                child.label(theme::icon(icons::DOTS_SIX_VERTICAL, 16.0).color(palette.secondary));
                child.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if widgets::icon_button_tinted(ui, icons::MINUS_CIRCLE, &l10n::t("Remove"), 16.0, Some(palette.danger)).clicked() {
                        action = Some((index, None));
                    }
                    ui.add_enabled_ui(index + 1 < count, |ui| {
                        if widgets::icon_button(ui, icons::ARROW_DOWN, &l10n::t("Move down"), 15.0).clicked() {
                            action = Some((index, Some(index + 2)));
                        }
                    });
                    ui.add_enabled_ui(index > 0, |ui| {
                        if widgets::icon_button(ui, icons::ARROW_UP, &l10n::t("Move up"), 15.0).clicked() {
                            action = Some((index, Some(index - 1)));
                        }
                    });
                    ui.with_layout(Layout::top_down(Align::Min), |ui| {
                        ui.add_space(2.0);
                        ui.add(egui::Label::new(theme::text(&song.title, Font::Body)).truncate().selectable(false));
                        let artist = song.artist.clone().unwrap_or_else(|| l10n::t("Unknown artist"));
                        ui.add(egui::Label::new(theme::secondary(ui, artist, Font::Footnote)).truncate().selectable(false));
                    });
                });
            });
            let rect = row.response.rect;
            if dragging && pointer.is_some_and(|p| rect.contains(p) || (p.y >= rect.top() && p.y < rect.bottom() + 2.0)) {
                let after = pointer.is_some_and(|p| p.y > rect.center().y);
                let y = if after { rect.bottom() + 1.0 } else { rect.top() - 1.0 };
                ui.painter().hline(rect.x_range(), y, egui::Stroke::new(2.0, palette.accent));
                if released
                    && let Some(from) = egui::DragAndDrop::take_payload::<usize>(&ctx) {
                        action = Some((*from, Some(if after { index + 1 } else { index })));
                    }
            }
        }
    });
    match action {
        Some((index, None)) => {
            state.entries.remove(index);
        }
        Some((from, Some(to))) => move_entry(&mut state.entries, from, to),
        _ => {}
    }
}

/// Moves `from` to the insertion index `to`, counted in the list before removal.
fn move_entry<T>(entries: &mut Vec<T>, from: usize, to: usize) {
    if from >= entries.len() {
        return;
    }
    let entry = entries.remove(from);
    let to = if to > from { to - 1 } else { to };
    entries.insert(to.min(entries.len()), entry);
}

// MARK: Add podcast

fn find_podcast(app: &mut App) {
    let Some(state) = app.library.windows.add_podcast.as_mut() else { return };
    let link = state.link.trim().to_string();
    if link.is_empty() || state.adding.is_some() {
        return;
    }
    state.candidates.clear();
    state.error = None;
    let source = match PodcastSource::parse(&link) {
        Ok(source) => source,
        Err(error) => {
            state.error = Some(error.message());
            return;
        }
    };
    state.searching = true;
    let token = state.token;
    app.tasks.spawn(async move {
        let result = PodcastDirectory::new().candidates(&source).await;
        update(move |app: &mut App| {
            let Some(state) = app.library.windows.add_podcast.as_mut().filter(|s| s.token == token) else { return };
            state.searching = false;
            match result {
                Ok(found) => state.candidates = found,
                Err(error) => state.error = Some(error.message()),
            }
        })
    });
}

fn add_podcast(app: &mut App, ctx: &egui::Context) {
    let Some(mut state) = app.library.windows.add_podcast.take() else { return };
    let mut close = false;
    let mut find = false;
    let mut add: Option<PodcastCandidate> = None;
    let busy = state.adding.is_some();
    let previous_link = state.link.clone();
    let modal = egui::Modal::new(egui::Id::unique("library-add-podcast")).show(ctx, |ui| {
        ui.set_width(520.0);
        dialog_title(ui, &l10n::t("Add podcast"));
        ui.horizontal(|ui| {
            let mut focus = state.focus;
            ui.add_enabled_ui(!busy, |ui| {
                let response = ui.add(
                    egui::TextEdit::singleline(&mut state.link)
                        .hint_text(l10n::t("Apple Podcasts, Spotify, or RSS link"))
                        .desired_width(ui.available_width() - 130.0)
                        .margin(vec2(10.0, 8.0)),
                );
                if std::mem::take(&mut focus) {
                    response.request_focus();
                }
                if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    find = true;
                }
            });
            state.focus = focus;
            let enabled = !state.link.trim().is_empty() && !state.searching && !busy;
            if ui.add_enabled(enabled, egui::Button::new(theme::text(l10n::t("Find podcast"), Font::Headline))).clicked() {
                find = true;
            }
        });
        ui.add_space(4.0);
        ui.label(theme::secondary(
            ui,
            l10n::t(
                "Spotify does not share podcast feeds, so Discophile looks for the same show in the Apple Podcasts directory. Spotify exclusives cannot be added. The link is sent to Apple or Spotify only to find the feed.",
            ),
            Font::Footnote,
        ));
        if state.searching {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(theme::secondary(ui, l10n::t("Looking for the podcast…"), Font::Callout));
            });
        }
        inline_error(ui, &state.error);
        if !state.candidates.is_empty() {
            ui.add_space(10.0);
            ui.label(theme::text(l10n::t("Choose the podcast"), Font::Headline));
            ui.add_space(4.0);
            egui::ScrollArea::vertical().max_height(320.0).auto_shrink([false, true]).show(ui, |ui| {
                for candidate in &state.candidates {
                    if candidate_row(ui, app, candidate, state.adding.as_deref(), busy || app.library.changing_podcasts, false) {
                        add = Some(candidate.clone());
                    }
                }
            });
            ui.label(theme::secondary(
                ui,
                l10n::t("Your server downloads the feed and its episodes. New episodes may take a few minutes to appear."),
                Font::Footnote,
            ));
        }
        footer(ui, |ui| {
            if ui.add_enabled(!busy, egui::Button::new(l10n::t("Cancel"))).clicked() {
                close = true;
            }
        });
    });
    if close || (modal.should_close() && !busy) {
        return;
    }
    if state.link != previous_link {
        state.candidates.clear();
        state.error = None;
    }
    app.library.windows.add_podcast = Some(state);
    if find {
        find_podcast(app);
    }
    if let Some(candidate) = add {
        let token = app.library.windows.add_podcast.as_ref().map_or(0, |s| s.token);
        if let Some(state) = app.library.windows.add_podcast.as_mut() {
            state.adding = Some(candidate.id().to_string());
            state.error = None;
        }
        super::add_podcast(app, candidate.feed_url.clone(), move |app, result| {
            let Some(state) = app.library.windows.add_podcast.as_mut().filter(|s| s.token == token) else { return };
            state.adding = None;
            match result {
                Ok(()) => app.library.windows.add_podcast = None,
                Err(error) => state.error = Some(discophile_core::podcast_directory::PodcastError::Music(error).message()),
            }
        });
    }
}

/// A directory result row with artwork, title, author and host. Returns true when Add is pressed.
pub(crate) fn candidate_row(
    ui: &mut Ui,
    app: &mut App,
    candidate: &PodcastCandidate,
    adding: Option<&str>,
    disabled: bool,
    subscribed: bool,
) -> bool {
    let mut pressed = false;
    ui.horizontal(|ui| {
        ui.set_min_height(60.0);
        widgets::artwork_url(
            ui,
            app,
            candidate.id(),
            &format!("podcast-directory:{}", candidate.id()),
            candidate.artwork_url.clone(),
            52.0,
            10.0,
        );
        let trailing = 48.0;
        ui.allocate_ui_with_layout(vec2(ui.available_width() - trailing, 56.0), Layout::top_down(Align::Min), |ui| {
            ui.add_space(4.0);
            ui.add(egui::Label::new(theme::text(&candidate.title, Font::Headline)).truncate());
            if let Some(author) = candidate.author.as_deref().filter(|a| !a.is_empty()) {
                ui.add(egui::Label::new(theme::secondary(ui, author, Font::Subheadline)).truncate());
            }
            let host = candidate.feed_url.host_str().unwrap_or(candidate.feed_url.as_str()).to_string();
            ui.add(egui::Label::new(theme::secondary(ui, host, Font::Footnote)).truncate());
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if adding == Some(candidate.id()) {
                ui.spinner();
            } else if subscribed {
                ui.label(theme::icon_filled(icons::CHECK_CIRCLE, 22.0).color(Palette::of_ui(ui).secondary))
                    .on_hover_text(l10n::t("Already in your podcasts"));
            } else {
                let label = format!("{}: {}", l10n::t("Add podcast"), candidate.title);
                ui.add_enabled_ui(!disabled, |ui| {
                    if widgets::icon_button_tinted(ui, icons::PLUS_CIRCLE, &label, 22.0, Some(Palette::of_ui(ui).accent)).clicked() {
                        pressed = true;
                    }
                });
            }
        });
    });
    pressed
}

// MARK: Confirmations

fn confirm(app: &mut App, ctx: &egui::Context) {
    let Some(state) = app.library.windows.confirm.take() else { return };
    let (title, message, action) = match &state {
        Confirm::DeletePlaylist { .. } => (
            l10n::t("Delete playlist?"),
            l10n::t("This deletes the playlist from your server. Your music files and downloads are kept."),
            l10n::t("Delete playlist"),
        ),
        Confirm::DeletePodcast { .. } => (
            l10n::t("Delete podcast?"),
            l10n::t("This removes the podcast and its episodes from your server and deletes its downloads from this computer."),
            l10n::t("Delete podcast"),
        ),
    };
    let mut close = false;
    let mut confirmed = false;
    let modal = egui::Modal::new(egui::Id::unique("library-confirm")).show(ctx, |ui| {
        ui.set_width(400.0);
        dialog_title(ui, &title);
        ui.label(theme::text(message, Font::Body));
        footer(ui, |ui| {
            let palette = Palette::of_ui(ui);
            let button =
                egui::Button::new(theme::text(action, Font::Headline).color(egui::Color32::WHITE)).fill(palette.danger.gamma_multiply(0.9));
            if ui.add(button).clicked() {
                confirmed = true;
            }
            if ui.button(l10n::t("Cancel")).clicked() {
                close = true;
            }
        });
    });
    if close || modal.should_close() {
        return;
    }
    if !confirmed {
        app.library.windows.confirm = Some(state);
        return;
    }
    match state {
        Confirm::DeletePlaylist { id } => {
            super::change_playlist(app, PlaylistChange::Delete(id.clone()), move |app, result| match result {
                Ok(_) => {
                    if app.nav.current == Route::Playlist(id) {
                        app.nav.select(Route::Playlists);
                    }
                }
                Err(error) => app.show_error(l10n::music_error(error)),
            });
        }
        Confirm::DeletePodcast { id } => {
            let route = Route::Podcast(id.clone());
            super::delete_podcast(app, &id, move |app, result| match result {
                Ok(()) => {
                    if app.nav.current == route {
                        app.nav.select(Route::Podcasts);
                    }
                }
                Err(error) => app.show_error(discophile_core::podcast_directory::PodcastError::Music(error).message()),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::move_entry;

    #[test]
    fn moves_by_insertion_index() {
        let mut list = vec!['a', 'b', 'c', 'd'];
        move_entry(&mut list, 0, 2);
        assert_eq!(list, ['b', 'a', 'c', 'd']);
        move_entry(&mut list, 3, 0);
        assert_eq!(list, ['d', 'b', 'a', 'c']);
        move_entry(&mut list, 1, 4);
        assert_eq!(list, ['d', 'a', 'c', 'b']);
        move_entry(&mut list, 2, 2);
        assert_eq!(list, ['d', 'a', 'c', 'b']);
        move_entry(&mut list, 9, 0);
        assert_eq!(list, ['d', 'a', 'c', 'b']);
    }
}
