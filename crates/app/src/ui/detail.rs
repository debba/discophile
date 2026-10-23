//! Album, artist, playlist and podcast pages: a large header with Play, Shuffle and an actions menu,
//! then the track list (or the artist's albums). Cached contents show while the page refreshes.

use discophile_core::{Song, l10n};
use egui::Ui;

use crate::app::App;
use crate::features::info;
use crate::features::library::{self, keys};
use crate::nav::Route;
use crate::theme::{Font, icons};
use crate::ui::library::parts::{self, Header, TrackList};
use crate::ui::widgets;

/// Starts a load when the page opens, or once per session; errors wait for Try again.
fn should_load(app: &mut App, key: &str) -> bool {
    if app.settings.downloaded_only || library::is_loading(app, key) {
        return false;
    }
    if app.nav.changed {
        app.library.errors.remove(key);
        return true;
    }
    !library::is_fresh(app, key) && library::error(app, key).is_none()
}

fn menu_separator(ui: &mut Ui) {
    ui.separator();
}

fn empty_tracks(app: &App, ui: &mut Ui) {
    let message = if app.settings.downloaded_only {
        l10n::t("Turn off Downloaded only to browse your server and save something for later.")
    } else {
        l10n::t("Your server's collection will appear here.")
    };
    widgets::empty_state(ui, icons::MUSIC_NOTE, &l10n::t("No tracks available"), &message);
}

pub fn album(app: &mut App, ui: &mut Ui, id: &str) {
    let key = keys::album(id);
    if should_load(app, &key) {
        library::load_album(app, id);
    }
    let meta = library::find_album(app, id);
    let songs = library::album_tracks(app, id);
    let tracks = songs.clone().unwrap_or_default();
    let first = tracks.first().cloned();
    widgets::page(ui, ("album", id), |ui| {
        let title = meta
            .as_ref()
            .map(|a| a.name.clone())
            .or_else(|| first.as_ref().and_then(|s| s.album.clone()))
            .unwrap_or_else(|| l10n::t("Album"));
        let artist = meta.as_ref().and_then(|a| a.artist.clone()).or_else(|| first.as_ref().and_then(|s| s.artist.clone()));
        let artist_id = meta.as_ref().and_then(|a| a.artist_id.clone()).or_else(|| first.as_ref().and_then(|s| s.artist_id.clone()));
        let year = meta.as_ref().and_then(|a| a.year).filter(|y| *y > 0);
        let header = Header {
            seed: id.to_string(),
            cover: meta.as_ref().and_then(|a| a.cover_art.clone()).or_else(|| first.as_ref().and_then(|s| s.cover_art.clone())),
            round: false,
            kind: l10n::t("Album"),
            title,
            subtitle: Some((artist.unwrap_or_else(|| l10n::t("Unknown artist")), artist_id.filter(|a| !a.is_empty()).map(Route::Artist))),
            details: vec![
                meta.as_ref().and_then(|a| a.genre.clone()).unwrap_or_default(),
                year.map(|y| y.to_string()).unwrap_or_default(),
                if tracks.is_empty() { String::new() } else { widgets::songs_summary(&tracks) },
            ],
            description: None,
        };
        let album_id = id.to_string();
        parts::header(app, ui, &header, |ui, app| {
            parts::play_buttons(app, ui, &tracks, |app, ui| {
                ui.add_enabled_ui(!tracks.is_empty(), |ui| {
                    if ui.button(format!("{}  {}", icons::VINYL_RECORD, l10n::t("Listen to the whole album"))).clicked() {
                        library::listen_to_whole_album(app, &album_id);
                        ui.close();
                    }
                });
                parts::collection_menu_items(app, ui, &tracks);
                parts::download_all_item(app, ui, &tracks);
                menu_separator(ui);
                if ui.button(format!("{}  {}", icons::INFO, l10n::t("About"))).clicked() {
                    about_album(app, &album_id, &tracks);
                    ui.close();
                }
                if let Some(artist) = header.subtitle.as_ref().and_then(|(_, route)| route.clone()) {
                    menu_separator(ui);
                    if ui.button(format!("{}  {}", icons::MICROPHONE_STAGE, l10n::t("Go to artist"))).clicked() {
                        app.nav.push(artist);
                        ui.close();
                    }
                }
            });
        });
        let id = album_id.clone();
        parts::load_error(app, ui, &key, move |app| library::load_album(app, &id));
        match songs {
            None if library::is_loading(app, &key) => widgets::loading(ui),
            Some(songs) if !songs.is_empty() => {
                parts::track_list(app, ui, &songs, &TrackList { numbers: true, discs: true, ..Default::default() });
            }
            _ if library::is_loading(app, &key) => widgets::loading(ui),
            _ => empty_tracks(app, ui),
        }
    });
}

pub fn artist(app: &mut App, ui: &mut Ui, id: &str) {
    let key = keys::artist(id);
    if should_load(app, &key) {
        library::load_artist(app, id);
    }
    let artist = library::find_artist(app, id);
    let name = artist.as_ref().map(|a| a.name.clone()).unwrap_or_default();
    let mut albums = match app.library.artist_details.get(id) {
        Some(artist) => artist.albums().to_vec(),
        None => {
            let s = &app.library.snapshot;
            let mut seen = std::collections::HashSet::new();
            s.albums
                .values()
                .flatten()
                .chain(s.newest.iter())
                .filter(|a| a.artist_id.as_deref() == Some(id) || (!name.is_empty() && a.artist.as_deref() == Some(name.as_str())))
                .filter(|a| seen.insert(a.id.clone()))
                .cloned()
                .collect()
        }
    };
    if app.settings.downloaded_only {
        albums = library::downloaded_albums(app)
            .into_iter()
            .filter(|a| a.artist_id.as_deref() == Some(id) || (!name.is_empty() && a.artist.as_deref() == Some(name.as_str())))
            .collect();
        if albums.is_empty() {
            // Downloaded albums carry the song's artist, which may differ from the album artist id.
            let ids: Vec<String> = library::downloaded_songs(app)
                .into_iter()
                .filter(|s| s.artist_id.as_deref() == Some(id))
                .filter_map(|s| s.album_id)
                .collect();
            albums = library::downloaded_albums(app).into_iter().filter(|a| ids.contains(&a.id)).collect();
        }
    }
    widgets::page(ui, ("artist", id), |ui| {
        let header = Header {
            seed: id.to_string(),
            cover: artist.as_ref().and_then(|a| a.cover_art.clone()),
            round: true,
            kind: l10n::t("Artist"),
            title: if name.is_empty() { l10n::t("Unknown artist") } else { name.clone() },
            subtitle: None,
            details: vec![if albums.is_empty() { String::new() } else { parts::album_count(albums.len() as i64) }],
            description: None,
        };
        let shuffle_albums = albums.clone();
        let about = artist.clone().unwrap_or_else(|| discophile_core::Artist { id: id.to_string(), name: name.clone(), ..Default::default() });
        parts::header(app, ui, &header, |ui, app| {
            ui.add_enabled_ui(!shuffle_albums.is_empty(), |ui| {
                if widgets::pill_button(ui, icons::SHUFFLE, l10n::t("Shuffle"), true).clicked() {
                    shuffle_artist(app, &shuffle_albums);
                }
            });
            ui.add_enabled_ui(!about.name.is_empty(), |ui| {
                if widgets::pill_button(ui, icons::INFO, l10n::t("About"), false).clicked() {
                    info::about_artist(app, &about);
                }
            });
        });
        let artist_id = id.to_string();
        parts::load_error(app, ui, &key, move |app| library::load_artist(app, &artist_id));
        if albums.is_empty() {
            if library::is_loading(app, &key) {
                widgets::loading(ui);
            } else {
                parts::empty(app, ui, icons::VINYL_RECORD);
            }
            return;
        }
        widgets::section_heading(ui, &l10n::t("Albums"), None);
        parts::album_grid(app, ui, &albums);
    });
}

/// About for an album page: the loaded album, or one built from its first track.
fn about_album(app: &mut App, id: &str, tracks: &[Song]) {
    let album = library::find_album(app, id).or_else(|| {
        let first = tracks.first()?;
        let mut album = discophile_core::Album::new(id, first.album.clone()?, first.artist.clone().unwrap_or_default());
        album.year = first.album_original_year.or(first.year);
        Some(album)
    });
    if let Some(album) = album {
        info::about_album(app, &album);
    }
}

/// Shuffles every album of the artist; albums never opened are fetched first.
fn shuffle_artist(app: &mut App, albums: &[discophile_core::Album]) {
    let ids: Vec<String> = albums.iter().map(|a| a.id.clone()).collect();
    collect_albums(app, ids, Vec::new());
}

fn collect_albums(app: &mut App, mut ids: Vec<String>, mut songs: Vec<Song>) {
    let Some(id) = ids.pop() else {
        return library::play(app, songs, 0, true);
    };
    library::with_album_songs(app, &id, move |app, album| {
        songs.extend(album);
        collect_albums(app, ids, songs);
    });
}

pub fn playlist(app: &mut App, ui: &mut Ui, id: &str) {
    let key = keys::playlist(id);
    if should_load(app, &key) {
        library::load_playlist(app, id);
    }
    let meta = library::find_playlist(app, id);
    let songs = library::playlist_tracks(app, id);
    let tracks = songs.clone().unwrap_or_default();
    let loading = library::is_loading(app, &key);
    widgets::page(ui, ("playlist", id), |ui| {
        let header = Header {
            seed: id.to_string(),
            cover: meta.as_ref().and_then(|p| p.cover_art.clone()),
            round: false,
            kind: l10n::t("Playlist"),
            title: meta.as_ref().map(|p| p.name.clone()).unwrap_or_else(|| l10n::t("Playlist")),
            subtitle: None,
            details: vec![if songs.is_some() { widgets::songs_summary(&tracks) } else { String::new() }],
            description: meta.as_ref().and_then(|p| p.comment.clone()),
        };
        parts::header(app, ui, &header, |ui, app| {
            parts::play_buttons(app, ui, &tracks, |app, ui| {
                parts::collection_menu_items(app, ui, &tracks);
                parts::download_all_item(app, ui, &tracks);
                if let Some(playlist) = &meta {
                    menu_separator(ui);
                    parts::playlist_menu(app, ui, playlist, false);
                }
            });
        });
        let playlist_id = id.to_string();
        parts::load_error(app, ui, &key, move |app| library::load_playlist(app, &playlist_id));
        match songs {
            Some(songs) if !songs.is_empty() => {
                // Removing needs the full server order, so it is off while filtering or reloading.
                let removable = !app.settings.downloaded_only && !loading && meta.is_some();
                let options = TrackList { artwork: true, album: true, playlist: removable.then(|| id.to_string()), ..Default::default() };
                parts::track_list(app, ui, &songs, &options);
            }
            _ if loading => widgets::loading(ui),
            _ => empty_tracks(app, ui),
        }
    });
}

pub fn podcast(app: &mut App, ui: &mut Ui, id: &str) {
    let key = keys::podcast(id);
    if should_load(app, &key) {
        library::load_podcast(app, id);
    }
    let channel = library::find_podcast(app, id);
    let songs = library::podcast_episodes(app, id);
    let episodes = songs.clone().unwrap_or_default();
    widgets::page(ui, ("podcast", id), |ui| {
        let header = Header {
            seed: id.to_string(),
            cover: channel.as_ref().and_then(|c| c.cover_art.clone()),
            round: false,
            kind: l10n::t("Podcast"),
            title: channel.as_ref().map(|c| c.title.clone()).unwrap_or_else(|| l10n::t("Podcast")),
            subtitle: None,
            details: vec![if episodes.is_empty() { String::new() } else { widgets::songs_summary(&episodes) }],
            description: channel.as_ref().and_then(|c| c.description.as_deref()).map(parts::plain_text),
        };
        let channel_id = id.to_string();
        parts::header(app, ui, &header, |ui, app| {
            parts::play_buttons(app, ui, &episodes, |app, ui| {
                parts::collection_menu_items(app, ui, &episodes);
                parts::download_all_item(app, ui, &episodes);
                if let Some(channel) = &channel {
                    menu_separator(ui);
                    if ui.button(format!("{}  {}", icons::INFO, l10n::t("About"))).clicked() {
                        info::about_podcast(app, channel);
                        ui.close();
                    }
                }
                if app.session.has_library() {
                    menu_separator(ui);
                    ui.add_enabled_ui(!app.library.changing_podcasts, |ui| {
                        if ui.button(format!("{}  {}", icons::TRASH, l10n::t("Delete podcast"))).clicked() {
                            library::confirm_delete_podcast(app, &channel_id);
                            ui.close();
                        }
                    });
                }
            });
        });
        ui.label(widgets_secondary(ui, &l10n::t("Episodes available for streaming on your server, newest first.")));
        ui.add_space(8.0);
        let podcast_id = id.to_string();
        parts::load_error(app, ui, &key, move |app| library::load_podcast(app, &podcast_id));
        match songs {
            Some(songs) if !songs.is_empty() => parts::track_list(app, ui, &songs, &TrackList::default()),
            _ if library::is_loading(app, &key) => widgets::loading(ui),
            _ => widgets::empty_state(
                ui,
                icons::MICROPHONE,
                &l10n::t("No playable episodes"),
                &l10n::t(
                    "Episodes must already be available to stream on your server. Discophile does not ask the server to fetch new episodes.",
                ),
            ),
        }
    });
}

fn widgets_secondary(ui: &Ui, text: &str) -> egui::RichText {
    crate::theme::secondary(ui, text, Font::Footnote)
}
