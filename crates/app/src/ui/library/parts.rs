//! Shared library components: track lists with context menus, album grids, artist and collection
//! rows, and the large detail header. Lists only draw rows inside the visible area, so long
//! libraries scroll smoothly.

use discophile_core::{Album, Artist, Playlist, PodcastChannel, Song, l10n};
use egui::{Align, Color32, CornerRadius, Layout, Rect, Response, Sense, Ui, UiBuilder, vec2};

use crate::app::App;
use crate::features::{discovery, downloads, library};
use crate::nav::Route;
use crate::player;
use crate::theme::{self, Font, Palette, icons};
use crate::ui::widgets::{self, TrackRowOptions};

/// Options for `track_list`.
#[derive(Clone, Debug, Default)]
pub struct TrackList {
    /// Album pages: track numbers instead of artwork.
    pub numbers: bool,
    pub artwork: bool,
    /// Adds the album to the subtitle.
    pub album: bool,
    /// Disc headers when the songs span several discs.
    pub discs: bool,
    /// Enables "Remove from playlist" for this playlist.
    pub playlist: Option<String>,
}

/// The current queue item, for highlighting.
fn current(app: &App) -> Option<String> {
    app.player.current().map(|s| s.stream_identifier().to_string())
}

/// A list of songs. Double-click or Enter plays from that song; right-click or the ellipsis opens
/// the track menu.
pub fn track_list(app: &mut App, ui: &mut Ui, songs: &[Song], options: &TrackList) {
    ui.scope(|ui| track_rows(app, ui, songs, options));
}

fn track_rows(app: &mut App, ui: &mut Ui, songs: &[Song], options: &TrackList) {
    let current = current(app);
    let playing = app.player.is_playing();
    let discs = options.discs && {
        let first = songs.first().and_then(|s| s.disc_number).unwrap_or(1);
        songs.iter().any(|s| s.disc_number.unwrap_or(1) != first)
    };
    let height = if options.artwork { 52.0 } else { 44.0 };
    let mut last_disc = None;
    ui.spacing_mut().item_spacing.y = 2.0;
    for (index, song) in songs.iter().enumerate() {
        if discs {
            let disc = song.disc_number.unwrap_or(1);
            if last_disc != Some(disc) {
                last_disc = Some(disc);
                ui.add_space(if index == 0 { 0.0 } else { 12.0 });
                ui.horizontal(|ui| {
                    ui.label(theme::icon(icons::DISC, 16.0).color(Palette::of_ui(ui).secondary));
                    ui.label(theme::secondary(ui, l10n::tf("Disc %lld", &[&disc]), Font::Headline));
                });
                ui.add_space(4.0);
            }
        }
        let next = Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), height));
        if !ui.is_rect_visible(next) {
            ui.allocate_exact_size(next.size(), Sense::hover());
            continue;
        }
        let is_current = current.as_deref() == Some(song.stream_identifier());
        let row_options = TrackRowOptions {
            number: options.numbers.then(|| song.track.unwrap_or(index as i64 + 1)),
            show_artwork: options.artwork,
            show_album: options.album,
            is_current,
            is_playing: is_current && playing,
        };
        let row = widgets::track_row(ui, app, song, row_options, |ui, app| {
            let menu_button = egui::Button::new(theme::icon(icons::DOTS_THREE, 18.0).color(Palette::of_ui(ui).secondary)).frame(false);
            let (response, _) =
                egui::containers::menu::MenuButton::from_button(menu_button).ui(ui, |ui| track_menu(app, ui, songs, index, options));
            response.on_hover_text(l10n::t("More"));
            downloads::download_button(ui, app, song);
        });
        if row.response.clicked() {
            row.response.request_focus();
        }
        if row.play {
            player::play_songs(app, songs.to_vec(), index, false);
        }
        row.response.context_menu(|ui| track_menu(app, ui, songs, index, options));
    }
}

fn menu_item(ui: &mut Ui, icon: &str, label: &str) -> bool {
    let clicked = ui.button(format!("{icon}  {label}")).clicked();
    if clicked {
        ui.close();
    }
    clicked
}

/// The track context menu: playback, playlists, favorites, radio, navigation and downloads.
pub fn track_menu(app: &mut App, ui: &mut Ui, songs: &[Song], index: usize, options: &TrackList) {
    let Some(song) = songs.get(index) else { return };
    ui.set_min_width(220.0);
    if menu_item(ui, icons::PLAY, &l10n::t("Play")) {
        player::play_songs(app, songs.to_vec(), index, false);
    }
    if menu_item(ui, icons::ARROW_BEND_DOWN_RIGHT, &l10n::t("Play next")) {
        player::play_next(app, vec![song.clone()]);
    }
    if menu_item(ui, icons::QUEUE, &l10n::t("Add to queue")) {
        player::add_to_queue(app, vec![song.clone()]);
    }
    ui.separator();
    if !song.is_podcast() {
        ui.add_enabled_ui(app.session.has_library(), |ui| {
            if menu_item(ui, icons::LIST_PLUS, &l10n::t("Add to playlist")) {
                library::open_playlist_picker(app, vec![song.clone()]);
            }
        });
        let favorite = library::is_favorite(app, song);
        let (icon, label) = if favorite { (icons::HEART_BREAK, "Remove from favorites") } else { (icons::HEART, "Add to favorites") };
        if menu_item(ui, icon, &l10n::t(label)) {
            library::toggle_favorite(app, song);
        }
        if menu_item(ui, icons::RADIO, &l10n::t("Start personal radio")) {
            discovery::start_personal_radio(app, song.clone());
        }
        ui.separator();
        if let Some(album_id) = song.album_id.clone().filter(|id| !id.is_empty())
            && menu_item(ui, icons::VINYL_RECORD, &l10n::t("Go to album")) {
                app.nav.push(Route::Album(album_id));
            }
        if let Some(artist_id) = song.artist_id.clone().filter(|id| !id.is_empty())
            && menu_item(ui, icons::MICROPHONE_STAGE, &l10n::t("Go to artist")) {
                app.nav.push(Route::Artist(artist_id));
            }
    } else if let Some(channel) = song.album_id.as_deref().and_then(|id| id.strip_prefix("podcast:"))
        && menu_item(ui, icons::MICROPHONE, &l10n::t("Go to podcast")) {
            app.nav.push(Route::Podcast(channel.to_string()));
        }
    if let Some(playlist) = &options.playlist {
        ui.separator();
        ui.add_enabled_ui(!app.library.changing_playlist, |ui| {
            if menu_item(ui, icons::MINUS_CIRCLE, &l10n::t("Remove from playlist")) {
                let occurrence = songs[..index].iter().filter(|s| s.stream_identifier() == song.stream_identifier()).count();
                library::remove_from_playlist(app, playlist, song, occurrence);
            }
        });
    }
    if !crate::platform::IS_WEB {
        ui.separator();
        downloads::song_menu(ui, app, song);
    }
}

/// Actions for a whole collection (album, playlist, podcast, list of songs).
pub fn collection_menu_items(app: &mut App, ui: &mut Ui, songs: &[Song]) {
    let enabled = !songs.is_empty();
    ui.add_enabled_ui(enabled, |ui| {
        if menu_item(ui, icons::ARROW_BEND_DOWN_RIGHT, &l10n::t("Play next")) {
            player::play_next(app, songs.to_vec());
        }
        if menu_item(ui, icons::QUEUE, &l10n::t("Add to queue")) {
            player::add_to_queue(app, songs.to_vec());
        }
        if songs.iter().all(|s| !s.is_podcast()) && menu_item(ui, icons::LIST_PLUS, &l10n::t("Add to playlist")) {
            library::open_playlist_picker(app, songs.to_vec());
        }
    });
}

// MARK: Albums

/// Tiles in as many columns as fit, each `width + extra` tall. Only visible rows are drawn.
pub fn grid<T>(app: &mut App, ui: &mut Ui, items: &[T], min_tile: f32, extra: f32, mut tile: impl FnMut(&mut App, &mut Ui, &T, f32)) {
    let gap = 20.0;
    let available = ui.available_width();
    let columns = ((available + gap) / (min_tile + gap)).floor().max(1.0) as usize;
    let width = ((available - gap * (columns as f32 - 1.0)) / columns as f32).floor();
    let row_height = width + extra;
    for row in items.chunks(columns) {
        let (rect, _) = ui.allocate_exact_size(vec2(available, row_height), Sense::hover());
        if ui.is_rect_visible(rect) {
            let mut child = ui.new_child(UiBuilder::new().max_rect(rect).layout(Layout::left_to_right(Align::Min)));
            child.spacing_mut().item_spacing.x = gap;
            for item in row {
                child.allocate_ui_with_layout(vec2(width, row_height), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(width);
                    tile(app, ui, item, width);
                });
            }
        }
        ui.add_space(12.0);
    }
}

/// Album tiles that reflow with the window width.
pub fn album_grid(app: &mut App, ui: &mut Ui, albums: &[Album]) {
    grid(app, ui, albums, 170.0, 52.0, album_card);
}

/// Playlist tiles that reflow with the window width.
pub fn playlist_grid(app: &mut App, ui: &mut Ui, playlists: &[Playlist]) {
    grid(app, ui, playlists, 170.0, 52.0, playlist_card);
}

/// An album tile: artwork with a hover play button, title, artist and year.
pub fn album_card(app: &mut App, ui: &mut Ui, album: &Album, width: f32) {
    ui.spacing_mut().item_spacing.y = 4.0;
    let art = widgets::artwork(ui, app, &album.id, album.cover_art.as_deref(), width, 12.0);
    ui.add(egui::Label::new(theme::text(&album.name, Font::Headline)).truncate().selectable(false));
    let artist = album.artist.clone().unwrap_or_else(|| l10n::t("Unknown artist"));
    let subtitle = match album.year {
        Some(year) if year > 0 => format!("{artist} · {year}"),
        _ => artist,
    };
    ui.add(egui::Label::new(theme::secondary(ui, subtitle.clone(), Font::Subheadline)).truncate().selectable(false));
    let rect = art.rect.union(ui.min_rect());
    let response = widgets::tile_target(ui, rect, ("album-card", &album.id), &format!("{}, {subtitle}", album.name));
    let play = hover_play_button(ui, art.rect, response.hovered() || ui.rect_contains_pointer(art.rect));
    if play {
        library::with_album_songs(app, &album.id, |app, songs| library::play(app, songs, 0, false));
    } else if response.clicked() {
        app.nav.push(Route::Album(album.id.clone()));
    }
    response.context_menu(|ui| album_menu(app, ui, album));
}

/// The round play button over artwork while the pointer is on the tile. Returns true when pressed.
fn hover_play_button(ui: &mut Ui, art: Rect, hovered: bool) -> bool {
    if !hovered {
        return false;
    }
    ui.painter().rect_filled(art, CornerRadius::same(12), Color32::from_black_alpha(40));
    let size = 44.0;
    let button = Rect::from_min_size(art.right_bottom() - vec2(size + 10.0, size + 10.0), vec2(size, size));
    ui.put(button, |ui: &mut Ui| widgets::round_play_button(ui, false, &l10n::t("Play"), size)).clicked()
}

/// Album actions; the tracks are fetched first when the album was never opened.
pub fn album_menu(app: &mut App, ui: &mut Ui, album: &Album) {
    ui.set_min_width(220.0);
    let id = album.id.clone();
    if menu_item(ui, icons::PLAY, &l10n::t("Play")) {
        library::with_album_songs(app, &id, |app, songs| library::play(app, songs, 0, false));
    }
    if menu_item(ui, icons::SHUFFLE, &l10n::t("Shuffle")) {
        library::with_album_songs(app, &id, |app, songs| library::play(app, songs, 0, true));
    }
    if menu_item(ui, icons::ARROW_BEND_DOWN_RIGHT, &l10n::t("Play next")) {
        library::with_album_songs(app, &id, player::play_next);
    }
    if menu_item(ui, icons::QUEUE, &l10n::t("Add to queue")) {
        library::with_album_songs(app, &id, player::add_to_queue);
    }
    if menu_item(ui, icons::VINYL_RECORD, &l10n::t("Listen to the whole album")) {
        library::listen_to_whole_album(app, &id);
    }
    ui.separator();
    if menu_item(ui, icons::LIST_PLUS, &l10n::t("Add to playlist")) {
        library::with_album_songs(app, &id, library::open_playlist_picker);
    }
    if let Some(artist) = album.artist_id.clone().filter(|a| !a.is_empty())
        && menu_item(ui, icons::MICROPHONE_STAGE, &l10n::t("Go to artist")) {
            app.nav.push(Route::Artist(artist));
        }
}

// MARK: Rows

/// A clickable row with hover highlight. `content` draws inside a left-to-right layout.
pub fn row(ui: &mut Ui, height: f32, content: impl FnOnce(&mut Ui)) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    if !ui.is_rect_visible(rect) {
        return response;
    }
    if response.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(10), Palette::of_ui(ui).hover);
    }
    let mut child = ui.new_child(UiBuilder::new().max_rect(rect.shrink2(vec2(10.0, 4.0))).layout(Layout::left_to_right(Align::Center)));
    content(&mut child);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn chevron(ui: &mut Ui) {
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.label(theme::icon(icons::CARET_RIGHT, 14.0).color(Palette::of_ui(ui).secondary));
    });
}

pub const ARTIST_ROW: f32 = 60.0;

/// An artist with round artwork; opens the artist page.
pub fn artist_row(app: &mut App, ui: &mut Ui, artist: &Artist) {
    let response = row(ui, ARTIST_ROW, |ui| {
        widgets::artist_artwork(ui, app, &artist.id, artist.cover_art.as_deref(), 44.0);
        ui.add_space(4.0);
        ui.vertical(|ui| {
            ui.add_space(6.0);
            ui.add(egui::Label::new(theme::text(&artist.name, Font::Headline)).truncate().selectable(false));
            if let Some(count) = artist.album_count.filter(|c| *c > 0) {
                ui.label(theme::secondary(ui, album_count(count), Font::Footnote));
            }
        });
        chevron(ui);
    });
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &artist.name));
    if response.clicked() {
        app.nav.push(Route::Artist(artist.id.clone()));
    }
}

/// A playlist tile for grids: artwork, name and song count.
pub fn playlist_card(app: &mut App, ui: &mut Ui, playlist: &Playlist, width: f32) {
    ui.spacing_mut().item_spacing.y = 4.0;
    let art = widgets::artwork(ui, app, &playlist.id, playlist.cover_art.as_deref(), width, 12.0);
    ui.add(egui::Label::new(theme::text(&playlist.name, Font::Headline)).truncate().selectable(false));
    let songs = l10n::count("%lld songs", playlist.song_count.unwrap_or(0));
    ui.add(egui::Label::new(theme::secondary(ui, songs.clone(), Font::Subheadline)).truncate().selectable(false));
    let rect = art.rect.union(ui.min_rect());
    let response = widgets::tile_target(ui, rect, ("playlist-card", &playlist.id), &format!("{}, {songs}", playlist.name));
    let play = hover_play_button(ui, art.rect, response.hovered() || ui.rect_contains_pointer(art.rect));
    let id = playlist.id.clone();
    if play {
        library::with_playlist_songs(app, &id, |app, songs| library::play(app, songs, 0, false));
    } else if response.clicked() {
        app.nav.push(Route::Playlist(id.clone()));
    }
    response.context_menu(|ui| playlist_menu(app, ui, playlist, true));
}

/// Playlist actions. `playback` adds Play/Shuffle/queue items (tiles); detail pages have buttons.
pub fn playlist_menu(app: &mut App, ui: &mut Ui, playlist: &Playlist, playback: bool) {
    ui.set_min_width(220.0);
    let id = playlist.id.clone();
    if playback {
        if menu_item(ui, icons::PLAY, &l10n::t("Play")) {
            library::with_playlist_songs(app, &id, |app, songs| library::play(app, songs, 0, false));
        }
        if menu_item(ui, icons::SHUFFLE, &l10n::t("Shuffle")) {
            library::with_playlist_songs(app, &id, |app, songs| library::play(app, songs, 0, true));
        }
        if menu_item(ui, icons::ARROW_BEND_DOWN_RIGHT, &l10n::t("Play next")) {
            library::with_playlist_songs(app, &id, player::play_next);
        }
        if menu_item(ui, icons::QUEUE, &l10n::t("Add to queue")) {
            library::with_playlist_songs(app, &id, player::add_to_queue);
        }
        ui.separator();
    }
    ui.add_enabled_ui(!app.library.changing_playlist, |ui| {
        if menu_item(ui, icons::SPARKLE, &l10n::t("Edit with assistant")) {
            crate::features::assistant::edit_playlist_with_assistant(app, playlist);
        }
        if menu_item(ui, icons::PENCIL_SIMPLE, &l10n::t("Rename playlist")) {
            library::open_rename_playlist(app, &id);
        }
        if menu_item(ui, icons::LIST_DASHES, &l10n::t("Edit tracks")) {
            library::open_playlist_editor(app, &id);
        }
        ui.separator();
        if menu_item(ui, icons::TRASH, &l10n::t("Delete playlist")) {
            library::confirm_delete_playlist(app, &id);
        }
    });
}

/// A subscribed podcast with artwork and description; opens the podcast page.
pub fn podcast_row(app: &mut App, ui: &mut Ui, channel: &PodcastChannel) {
    let response = row(ui, 84.0, |ui| {
        widgets::artwork(ui, app, &channel.id, channel.cover_art.as_deref(), 68.0, 10.0);
        ui.add_space(6.0);
        ui.allocate_ui_with_layout(vec2(ui.available_width() - 30.0, 72.0), Layout::top_down(Align::Min), |ui| {
            ui.add_space(10.0);
            ui.add(egui::Label::new(theme::text(&channel.title, Font::Headline)).truncate().selectable(false));
            if let Some(description) = channel.description.as_deref().map(plain_text).filter(|d| !d.is_empty()) {
                ui.add(egui::Label::new(theme::secondary(ui, description, Font::Footnote)).truncate().selectable(false));
            }
        });
        chevron(ui);
    });
    if response.clicked() {
        app.nav.push(Route::Podcast(channel.id.clone()));
    }
    let id = channel.id.clone();
    response.context_menu(|ui| {
        ui.add_enabled_ui(!app.library.changing_podcasts, |ui| {
            if menu_item(ui, icons::TRASH, &l10n::t("Delete podcast")) {
                library::confirm_delete_podcast(app, &id);
            }
        });
    });
}

/// Feed descriptions often carry HTML; show them as plain text on one line.
pub fn plain_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    discophile_core::podcast_directory::decode_entities(&out).split_whitespace().collect::<Vec<_>>().join(" ")
}

// MARK: Headers

/// Contents of the large detail header.
pub struct Header {
    pub seed: String,
    pub cover: Option<String>,
    pub round: bool,
    /// "Album", "Playlist", "Podcast", "Artist".
    pub kind: String,
    pub title: String,
    /// Accent line, optionally linking to a page (the album's artist).
    pub subtitle: Option<(String, Option<Route>)>,
    pub details: Vec<String>,
    pub description: Option<String>,
}

/// Large artwork beside the title, details and the action row (stacked on narrow windows).
pub fn header(app: &mut App, ui: &mut Ui, header: &Header, actions: impl FnOnce(&mut Ui, &mut App)) {
    let wide = ui.available_width() > 640.0;
    let size = if wide { 220.0 } else { 180.0 };
    let radius = if header.round { size / 2.0 } else { 18.0 };
    let mut actions = Some(actions);
    let mut text = |ui: &mut Ui, app: &mut App| {
        ui.label(theme::secondary(ui, header.kind.to_uppercase(), Font::Footnote).strong());
        ui.add(egui::Label::new(theme::text(&header.title, Font::LargeTitle)).wrap());
        if let Some((subtitle, route)) = &header.subtitle {
            let label = theme::text(subtitle, Font::Title3).color(Palette::of_ui(ui).accent);
            match route {
                Some(route) => {
                    let response = ui.add(egui::Label::new(label).sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand);
                    if response.clicked() {
                        app.nav.push(route.clone());
                    }
                }
                None => {
                    ui.label(label);
                }
            }
        }
        let details: Vec<&str> = header.details.iter().map(String::as_str).filter(|d| !d.is_empty()).collect();
        if !details.is_empty() {
            ui.label(theme::secondary(ui, details.join(" · "), Font::Subheadline));
        }
        if let Some(description) = header.description.as_deref().filter(|d| !d.is_empty()) {
            ui.add_space(2.0);
            ui.add(egui::Label::new(theme::secondary(ui, description, Font::Callout)).wrap().truncate());
        }
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if let Some(actions) = actions.take() {
                actions(ui, app);
            }
        });
    };
    if wide {
        ui.horizontal_top(|ui| {
            header_artwork(app, ui, header, size, radius);
            ui.add_space(14.0);
            ui.allocate_ui_with_layout(vec2(ui.available_width(), size), Layout::top_down(Align::Min), |ui| {
                ui.add_space((size - 150.0).max(0.0) / 2.0);
                text(ui, app);
            });
        });
    } else {
        header_artwork(app, ui, header, size, radius);
        ui.add_space(10.0);
        text(ui, app);
    }
    ui.add_space(16.0);
}

/// Large artwork with a soft drop shadow, as on iOS.
fn header_artwork(app: &mut App, ui: &mut Ui, header: &Header, size: f32, radius: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    let shadow = egui::Shadow { offset: [0, 10], blur: 28, spread: 0, color: Color32::from_black_alpha(60) };
    let corner = CornerRadius::same(radius.round().clamp(0.0, 255.0) as u8);
    ui.painter().add(shadow.as_shape(rect, corner));
    let texture = header.cover.as_deref().and_then(|cover| app.artwork_texture(cover));
    // Only artists use round artwork.
    let placeholder = if header.round { icons::USER } else { icons::MUSIC_NOTE };
    widgets::paint_artwork_or(ui, rect, &header.seed, texture.as_ref(), radius, placeholder);
}

/// Play and Shuffle pills followed by an ellipsis menu.
pub fn play_buttons(app: &mut App, ui: &mut Ui, songs: &[Song], menu: impl FnOnce(&mut App, &mut Ui)) {
    ui.add_enabled_ui(!songs.is_empty(), |ui| {
        if widgets::pill_button(ui, icons::PLAY, l10n::t("Play"), true).clicked() {
            library::play(app, songs.to_vec(), 0, false);
        }
        if widgets::pill_button(ui, icons::SHUFFLE, l10n::t("Shuffle"), false).clicked() {
            library::play(app, songs.to_vec(), 0, true);
        }
    });
    more_menu(ui, &l10n::t("Collection actions"), |ui| menu(app, ui));
}

/// The round ellipsis button that opens a menu.
pub fn more_menu(ui: &mut Ui, label: &str, content: impl FnOnce(&mut Ui)) {
    let palette = Palette::of_ui(ui);
    let button = egui::Button::new(theme::icon(icons::DOTS_THREE, 20.0).color(palette.accent))
        .fill(palette.surface_raised)
        .corner_radius(CornerRadius::same(12))
        .min_size(vec2(40.0, 36.0));
    let (response, _) = egui::containers::menu::MenuButton::from_button(button).ui(ui, |ui| {
        ui.set_min_width(230.0);
        content(ui);
    });
    response.on_hover_text(label);
}

/// Download all, for detail menus (desktop only).
pub fn download_all_item(app: &mut App, ui: &mut Ui, songs: &[Song]) {
    if crate::platform::IS_WEB || songs.is_empty() {
        return;
    }
    let all = downloads::downloaded_count(app, songs) == songs.len();
    ui.add_enabled_ui(!all, |ui| {
        if menu_item(ui, icons::ARROW_CIRCLE_DOWN, &l10n::t("Download all")) {
            downloads::download(app, songs);
        }
    });
}

/// An inline load error with Try again, shown above cached content.
pub fn load_error(app: &mut App, ui: &mut Ui, key: &str, retry: impl FnOnce(&mut App)) {
    if let Some(error) = library::error(app, key) {
        if widgets::error_banner(ui, &error, true) {
            app.library.errors.remove(key);
            retry(app);
        }
        ui.add_space(8.0);
    }
}

/// The empty state for library sections, adapted to Downloaded only.
pub fn empty(app: &App, ui: &mut Ui, icon: &str) {
    if app.settings.downloaded_only {
        widgets::empty_state(
            ui,
            icon,
            &l10n::t("Nothing downloaded here yet"),
            &l10n::t("Turn off Downloaded only to browse your server and save something for later."),
        );
    } else {
        widgets::empty_state(ui, icon, &l10n::t("Nothing here yet"), &l10n::t("Your server's collection will appear here."));
    }
}

/// "1 album" / "%lld albums" (the shared plural table has no album entry).
pub fn album_count(count: i64) -> String {
    if count == 1 { l10n::t("1 album") } else { l10n::tf("%lld albums", &[&count]) }
}

#[cfg(test)]
mod tests {
    #[test]
    fn plain_text_strips_markup() {
        assert_eq!(super::plain_text("<p>Hello&nbsp;<b>world</b></p>\n &amp; more"), "Hello world & more");
    }
}
