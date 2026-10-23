//! Library sections: Albums, Artists, Songs, Playlists, Favorites and Podcasts. Cached content shows
//! immediately (also offline); each section refreshes from the server once per session.

pub mod parts;

use std::collections::HashSet;

use discophile_core::{Artist, l10n};
use egui::{Align, Layout, Ui};

use crate::app::App;
use crate::features::library::{self, AlbumListType, keys};
use crate::nav::Route;
use crate::theme::{self, Font, Palette, icons};
use crate::ui::widgets;
use parts::TrackList;

pub fn show(app: &mut App, ui: &mut Ui, route: &Route) {
    match route {
        Route::Albums => albums(app, ui),
        Route::Artists => artists(app, ui),
        Route::Songs => songs(app, ui),
        Route::Playlists => playlists(app, ui),
        Route::Favorites => favorites(app, ui),
        Route::Podcasts => podcasts(app, ui),
        _ => {}
    }
}

fn refresh_button(app: &mut App, ui: &mut Ui, key: &str, reload: impl FnOnce(&mut App)) {
    let loading = library::is_loading(app, key);
    if loading {
        ui.spinner();
    } else if !app.settings.downloaded_only && widgets::icon_button(ui, icons::ARROWS_CLOCKWISE, &l10n::t("Refresh"), 17.0).clicked() {
        app.library.errors.remove(key);
        reload(app);
    }
}

/// The "Downloaded only" reminder under page titles.
fn filter_note(app: &App, ui: &mut Ui) {
    if app.settings.downloaded_only {
        let palette = Palette::of_ui(ui);
        ui.label(theme::text(format!("{}  {}", icons::ARROW_CIRCLE_DOWN, l10n::t("Downloaded only")), Font::Callout).color(palette.accent));
        ui.add_space(6.0);
    }
}

// MARK: Albums

fn albums(app: &mut App, ui: &mut Ui) {
    let list = app.library.album_list;
    let key = keys::albums(list.api());
    library::ensure(app, &key);
    widgets::page(ui, "library-albums", |ui| {
        widgets::page_header(ui, &l10n::t("Albums"), |ui| {
            refresh_button(app, ui, &key, |app| library::load_albums(app, false));
            if !app.settings.downloaded_only {
                let mut selected = list;
                egui::ComboBox::from_id_salt("library-album-list").selected_text(selected.title()).width(170.0).show_ui(ui, |ui| {
                    for option in AlbumListType::available(app.session.provider()) {
                        ui.selectable_value(&mut selected, option, option.title());
                    }
                });
                if selected != list {
                    library::set_album_list(app, selected);
                }
            }
        });
        filter_note(app, ui);
        let downloaded_only = app.settings.downloaded_only;
        let albums = if downloaded_only {
            library::downloaded_albums(app)
        } else {
            parts::load_error(app, ui, &key, |app| library::load_albums(app, false));
            app.library.snapshot.albums.get(list.api()).cloned().unwrap_or_default()
        };
        let loading = library::is_loading(app, &key);
        if albums.is_empty() {
            if loading {
                widgets::loading(ui)
            } else {
                parts::empty(app, ui, icons::VINYL_RECORD)
            }
            return;
        }
        parts::album_grid(app, ui, &albums);
        let more = app.library.more_albums.get(list.api()).copied().unwrap_or(albums.len() % library::ALBUM_PAGE == 0);
        if !downloaded_only && more {
            ui.vertical_centered(|ui| {
                ui.add_enabled_ui(!loading, |ui| {
                    if widgets::action_button(ui, l10n::t("Load more albums"), true).clicked() {
                        library::load_albums(app, true);
                    }
                });
                if loading {
                    ui.spinner();
                }
            });
        }
    });
}

// MARK: Artists

fn index_letter(artist: &Artist) -> char {
    library::sort_name(&artist.name).chars().next().map(|c| c.to_ascii_uppercase()).filter(char::is_ascii_alphabetic).unwrap_or('#')
}

fn artists(app: &mut App, ui: &mut Ui) {
    library::ensure(app, keys::ARTISTS);
    let mut artists = app.library.snapshot.artists.clone();
    if app.settings.downloaded_only {
        let names: HashSet<String> = library::downloaded_songs(app).iter().filter_map(|s| s.artist.clone()).collect();
        let ids: HashSet<String> = library::downloaded_songs(app).iter().filter_map(|s| s.artist_id.clone()).collect();
        artists.retain(|a| ids.contains(&a.id) || names.contains(&a.name));
    }
    let scroll_id = egui::Id::unique("library-artists-scroll-to");
    widgets::page(ui, "library-artists", |ui| {
        widgets::page_header(ui, &l10n::t("Artists"), |ui| refresh_button(app, ui, keys::ARTISTS, library::load_artists));
        filter_note(app, ui);
        parts::load_error(app, ui, keys::ARTISTS, library::load_artists);
        if artists.is_empty() {
            if library::is_loading(app, keys::ARTISTS) {
                widgets::loading(ui)
            } else {
                parts::empty(app, ui, icons::MICROPHONE_STAGE)
            }
            return;
        }
        // A–Z index: jumps to the first artist of a letter.
        let mut seen = HashSet::new();
        let letters: Vec<char> = artists.iter().map(index_letter).filter(|l| seen.insert(*l)).collect();
        if letters.len() > 1 {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for letter in &letters {
                    let text = theme::text(letter.to_string(), Font::Footnote).color(Palette::of_ui(ui).accent);
                    if ui.add(egui::Button::new(text).frame(false).min_size(egui::vec2(22.0, 22.0))).clicked() {
                        ui.data_mut(|d| d.insert_temp(scroll_id, *letter));
                    }
                }
            });
            ui.add_space(8.0);
        }
        let target: Option<char> = ui.data(|d| d.get_temp(scroll_id));
        let mut current = None;
        for artist in &artists {
            let letter = index_letter(artist);
            if current != Some(letter) {
                current = Some(letter);
                ui.add_space(6.0);
                let response = ui.label(theme::text(letter.to_string(), Font::Title3).color(Palette::of_ui(ui).accent));
                if target == Some(letter) && ui.is_visible() {
                    response.scroll_to_me(Some(Align::TOP));
                    ui.data_mut(|d| d.remove::<char>(scroll_id));
                }
            }
            parts::artist_row(app, ui, artist);
        }
    });
}

// MARK: Songs

fn songs(app: &mut App, ui: &mut Ui) {
    library::ensure(app, keys::SONGS);
    let downloaded_only = app.settings.downloaded_only;
    let songs = if downloaded_only {
        library::downloaded_songs(app).into_iter().filter(|s| !s.is_podcast()).collect()
    } else {
        app.library.snapshot.songs.clone()
    };
    widgets::page(ui, "library-songs", |ui| {
        widgets::page_header(ui, &l10n::t("Songs"), |ui| refresh_button(app, ui, keys::SONGS, library::load_songs));
        filter_note(app, ui);
        if !downloaded_only {
            parts::load_error(app, ui, keys::SONGS, library::load_songs);
        }
        if songs.is_empty() {
            if library::is_loading(app, keys::SONGS) {
                widgets::loading(ui)
            } else {
                parts::empty(app, ui, icons::MUSIC_NOTES)
            }
            return;
        }
        ui.horizontal(|ui| {
            parts::play_buttons(app, ui, &songs, |app, ui| {
                parts::collection_menu_items(app, ui, &songs);
                parts::download_all_item(app, ui, &songs);
            });
            ui.label(theme::secondary(ui, widgets::songs_summary(&songs), Font::Subheadline));
        });
        ui.add_space(10.0);
        parts::track_list(app, ui, &songs, &TrackList { artwork: true, album: true, ..Default::default() });
        if !downloaded_only {
            ui.add_space(10.0);
            ui.label(theme::secondary(
                ui,
                l10n::t("Showing the first 500 songs. Use Search to find more in your library."),
                Font::Footnote,
            ));
        }
    });
}

// MARK: Playlists

fn playlists(app: &mut App, ui: &mut Ui) {
    library::ensure(app, keys::PLAYLISTS);
    let mut playlists = app.library.snapshot.playlists.clone();
    if app.settings.downloaded_only {
        playlists.retain(|p| {
            let contents = app.library.snapshot.playlist_contents.get(&p.id);
            contents.is_some_and(|songs| songs.iter().any(|s| crate::features::downloads::is_downloaded(app, s)))
        });
    }
    widgets::page(ui, "library-playlists", |ui| {
        widgets::page_header(ui, &l10n::t("Playlists"), |ui| {
            refresh_button(app, ui, keys::PLAYLISTS, library::load_playlists);
            let enabled = app.session.has_library() && !app.library.changing_playlist;
            ui.add_enabled_ui(enabled, |ui| {
                if widgets::pill_button(ui, icons::PLUS, l10n::t("New playlist"), false).clicked() {
                    library::open_new_playlist(app, Vec::new());
                }
            });
        });
        filter_note(app, ui);
        parts::load_error(app, ui, keys::PLAYLISTS, library::load_playlists);
        if playlists.is_empty() {
            if library::is_loading(app, keys::PLAYLISTS) {
                widgets::loading(ui);
            } else if app.settings.downloaded_only {
                parts::empty(app, ui, icons::PLAYLIST);
            } else {
                widgets::empty_state(ui, icons::PLAYLIST, &l10n::t("Nothing here yet"), &l10n::t("Your playlists will appear here."));
            }
            return;
        }
        parts::playlist_grid(app, ui, &playlists);
    });
}

// MARK: Favorites

fn favorites(app: &mut App, ui: &mut Ui) {
    library::ensure(app, keys::FAVORITES);
    let favorites = app.library.snapshot.favorites.clone();
    let mut songs = favorites.songs().to_vec();
    let mut albums = favorites.albums().to_vec();
    let mut artists = favorites.artists().to_vec();
    if app.settings.downloaded_only {
        songs.retain(|s| crate::features::downloads::is_downloaded(app, s));
        let downloaded: HashSet<String> = library::downloaded_albums(app).into_iter().map(|a| a.id).collect();
        albums.retain(|a| downloaded.contains(&a.id));
        let names: HashSet<String> = library::downloaded_songs(app).iter().filter_map(|s| s.artist.clone()).collect();
        artists.retain(|a| names.contains(&a.name));
    }
    widgets::page(ui, "library-favorites", |ui| {
        widgets::page_header(ui, &l10n::t("Favorites"), |ui| refresh_button(app, ui, keys::FAVORITES, library::load_favorites));
        filter_note(app, ui);
        parts::load_error(app, ui, keys::FAVORITES, library::load_favorites);
        if songs.is_empty() && albums.is_empty() && artists.is_empty() {
            if library::is_loading(app, keys::FAVORITES) {
                widgets::loading(ui)
            } else {
                parts::empty(app, ui, icons::HEART)
            }
            return;
        }
        if !songs.is_empty() {
            widgets::section_heading(ui, &l10n::t("Songs"), None);
            ui.horizontal(|ui| {
                parts::play_buttons(app, ui, &songs, |app, ui| {
                    parts::collection_menu_items(app, ui, &songs);
                    parts::download_all_item(app, ui, &songs);
                });
                ui.label(theme::secondary(ui, widgets::songs_summary(&songs), Font::Subheadline));
            });
            ui.add_space(8.0);
            parts::track_list(app, ui, &songs, &TrackList { artwork: true, album: true, ..Default::default() });
        }
        if !albums.is_empty() {
            widgets::section_heading(ui, &l10n::t("Albums"), None);
            parts::album_grid(app, ui, &albums);
        }
        if !artists.is_empty() {
            widgets::section_heading(ui, &l10n::t("Artists"), None);
            for artist in &artists {
                parts::artist_row(app, ui, artist);
            }
        }
    });
}

// MARK: Podcasts

fn podcasts(app: &mut App, ui: &mut Ui) {
    let supported = library::supports_podcasts(app);
    if supported {
        library::ensure(app, keys::PODCASTS);
    }
    let mut channels = app.library.snapshot.podcasts.clone();
    if app.settings.downloaded_only {
        let albums: HashSet<String> = library::downloaded_songs(app).into_iter().filter_map(|s| s.album_id).collect();
        channels.retain(|c| albums.contains(&format!("podcast:{}", c.id)));
    }
    widgets::page(ui, "library-podcasts", |ui| {
        widgets::page_header(ui, &l10n::t("Podcasts"), |ui| {
            if !supported {
                return;
            }
            refresh_button(app, ui, keys::PODCASTS, library::load_podcasts);
            let enabled = app.session.has_library() && !app.library.changing_podcasts;
            ui.add_enabled_ui(enabled, |ui| {
                if widgets::pill_button(ui, icons::PLUS, l10n::t("Add podcast"), false).clicked() {
                    library::open_add_podcast(app);
                }
            });
        });
        if !supported {
            widgets::empty_state(
                ui,
                icons::MICROPHONE,
                &l10n::t("Podcasts unavailable"),
                &l10n::t("This provider does not offer podcast subscriptions."),
            );
            return;
        }
        filter_note(app, ui);
        parts::load_error(app, ui, keys::PODCASTS, library::load_podcasts);
        if channels.is_empty() {
            if library::is_loading(app, keys::PODCASTS) {
                widgets::loading(ui)
            } else {
                parts::empty(app, ui, icons::MICROPHONE)
            }
            return;
        }
        ui.with_layout(Layout::top_down(Align::Min), |ui| {
            for channel in &channels {
                parts::podcast_row(app, ui, channel);
            }
        });
    });
}
