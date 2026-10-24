//! The music library: cached sections per library, detail loads, playlist and podcast changes,
//! favorites and the Downloaded only filter. Port of the library parts of the iOS `MusicSession`.
//!
//! Every section is cached under `library/<account>/<part>` so the library can be browsed offline;
//! pages show cached data immediately and refresh it from the server once per session.

pub mod search;
mod windows;

use std::collections::{HashMap, HashSet};
use std::future::Future;

use discophile_core::client::{MusicClient, SearchLimits};
use discophile_core::listening_history::fold;
use discophile_core::offline_catalog::natural_cmp;
use discophile_core::{
    Album, Artist, MusicError, MusicProvider, MusicResult, Playlist, PlaylistAdditionResult, PodcastChannel, SearchResults, Song,
    album_listening, l10n, podcast_order,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

pub use search::{SearchScope, SearchState};
pub(crate) use windows::candidate_row;
pub use windows::{
    confirm_delete_playlist, confirm_delete_podcast, open_add_podcast, open_new_playlist, open_playlist_editor, open_playlist_picker,
    open_rename_playlist,
};

use crate::app::App;
use crate::features::downloads;
use crate::platform::{MaybeSend, storage};
use crate::player::{self, PlayContext};
use crate::tasks::update;

/// Albums per page in the Albums section, as in the iOS app.
pub const ALBUM_PAGE: usize = 60;
/// Albums in the "Recently added" shelf.
pub const NEWEST_COUNT: usize = 20;
/// Songs in the Songs section (search3 with an empty query).
pub const SONG_LIMIT: usize = 500;
/// Album details kept for offline browsing.
const ALBUM_DETAIL_LIMIT: usize = 100;
/// Subscribed channels with episodes are reused for this long while searching.
const PODCAST_INDEX_MS: i64 = 300_000;

/// Album list orders offered by the Albums section.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum AlbumListType {
    #[default]
    AlphabeticalByName,
    AlphabeticalByArtist,
    Newest,
    Recent,
    Frequent,
    Random,
}

impl AlbumListType {
    pub const ALL: [AlbumListType; 6] =
        [Self::AlphabeticalByName, Self::AlphabeticalByArtist, Self::Newest, Self::Recent, Self::Frequent, Self::Random];

    /// The Subsonic `getAlbumList2` type.
    pub fn api(self) -> &'static str {
        match self {
            Self::AlphabeticalByName => "alphabeticalByName",
            Self::AlphabeticalByArtist => "alphabeticalByArtist",
            Self::Newest => "newest",
            Self::Recent => "recent",
            Self::Frequent => "frequent",
            Self::Random => "random",
        }
    }

    pub fn title(self) -> String {
        l10n::t(match self {
            Self::AlphabeticalByName => "By title",
            Self::AlphabeticalByArtist => "By artist",
            Self::Newest => "Recently added",
            Self::Recent => "Recently played",
            Self::Frequent => "Most played",
            Self::Random => "Random",
        })
    }

    /// Jellyfin only sorts by name or by date added.
    pub fn available(provider: Option<MusicProvider>) -> Vec<AlbumListType> {
        if provider == Some(MusicProvider::Jellyfin) { vec![Self::AlphabeticalByName, Self::Newest] } else { Self::ALL.to_vec() }
    }
}

/// The cached library of the active account.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LibrarySnapshot {
    pub newest: Vec<Album>,
    /// Album pages by list type (`AlbumListType::api`).
    pub albums: HashMap<String, Vec<Album>>,
    pub artists: Vec<Artist>,
    pub playlists: Vec<Playlist>,
    pub podcasts: Vec<PodcastChannel>,
    pub favorites: SearchResults,
    pub songs: Vec<Song>,
    pub playlist_contents: HashMap<String, Vec<Song>>,
    pub album_details: HashMap<String, Album>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    Newest,
    Albums,
    Artists,
    Playlists,
    Podcasts,
    Favorites,
    Songs,
    PlaylistContents,
    AlbumDetails,
}

impl Part {
    const ALL: [Part; 9] = [
        Part::Newest,
        Part::Albums,
        Part::Artists,
        Part::Playlists,
        Part::Podcasts,
        Part::Favorites,
        Part::Songs,
        Part::PlaylistContents,
        Part::AlbumDetails,
    ];
    fn name(self) -> &'static str {
        match self {
            Part::Newest => "newest",
            Part::Albums => "albums",
            Part::Artists => "artists",
            Part::Playlists => "playlists",
            Part::Podcasts => "podcasts",
            Part::Favorites => "favorites",
            Part::Songs => "songs",
            Part::PlaylistContents => "playlist-contents",
            Part::AlbumDetails => "album-details",
        }
    }
}

/// A server change to a playlist (iOS `PlaylistChange`).
#[derive(Clone, Debug)]
pub enum PlaylistChange {
    Create(String, Vec<Song>),
    Rename(String, String),
    Add(String, Vec<Song>),
    Tracks { id: String, expected: Vec<Song>, songs: Vec<Song> },
    Remove { id: String, song: Song, occurrence: usize, expected: Vec<Song> },
    Delete(String),
}

#[derive(Clone, Debug, Default)]
pub struct PlaylistChangeResult {
    pub addition: Option<PlaylistAdditionResult>,
    pub created: Option<Playlist>,
}

/// Section load keys, also used for errors.
pub mod keys {
    pub const NEWEST: &str = "newest";
    pub const ARTISTS: &str = "artists";
    pub const PLAYLISTS: &str = "playlists";
    pub const PODCASTS: &str = "podcasts";
    pub const FAVORITES: &str = "favorites";
    pub const SONGS: &str = "songs";
    pub fn albums(list: &str) -> String {
        format!("albums:{list}")
    }
    pub fn album(id: &str) -> String {
        format!("album:{id}")
    }
    pub fn artist(id: &str) -> String {
        format!("artist:{id}")
    }
    pub fn playlist(id: &str) -> String {
        format!("playlist:{id}")
    }
    pub fn podcast(id: &str) -> String {
        format!("podcast:{id}")
    }
}

#[derive(Default)]
pub struct LibraryState {
    pub snapshot: LibrarySnapshot,
    /// The account the snapshot belongs to.
    account: String,
    pub album_list: AlbumListType,
    /// Whether another page may exist, per album list type.
    pub more_albums: HashMap<String, bool>,
    loading: HashSet<String>,
    /// Inline load errors by key (see `keys`).
    pub errors: HashMap<String, String>,
    /// Keys refreshed from the server in this session.
    fresh: HashSet<String>,
    refreshed: bool,
    pub changing_playlist: bool,
    pub changing_podcasts: bool,
    /// Songs unstarred in this session whose cached copies still carry a star.
    unstarred: HashSet<String>,
    /// Artist pages loaded in this session.
    pub artist_details: HashMap<String, Artist>,
    /// Playable episodes (newest first) and channel metadata, by channel id.
    pub episodes: HashMap<String, (PodcastChannel, Vec<Song>)>,
    /// Playlists opened directly (for example from search) that are not in the list yet.
    pub opened_playlists: HashMap<String, Playlist>,
    /// Subscribed channels with episodes and the time they were fetched, for search.
    podcast_index: Option<(Vec<PodcastChannel>, i64)>,
    /// A podcast index request is in flight; bumping the epoch discards its result.
    podcast_index_loading: bool,
    podcast_index_epoch: u64,
    /// Downloaded songs and albums by downloads revision.
    downloaded: Option<(u64, Vec<Song>)>,
    downloaded_albums: Option<(u64, Vec<Album>)>,
    pub search: SearchState,
    windows: windows::Windows,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct ViewPreferences {
    album_list: AlbumListType,
}

pub fn start(app: &mut App) {
    let preferences: ViewPreferences = storage::load("library-view").unwrap_or_default();
    app.library.album_list = preferences.album_list;
    load_cache(app);
}

pub fn tick(app: &mut App) {
    if app.library.account != app.session.account_key() {
        load_cache(app);
    }
    if !app.library.refreshed && app.session.has_library() {
        app.library.refreshed = true;
        refresh(app);
    }
}

pub fn windows(app: &mut App, ctx: &egui::Context) {
    windows::show(app, ctx);
}

pub fn on_library_changed(app: &mut App) {
    let album_list = app.library.album_list;
    app.library = LibraryState { album_list, ..Default::default() };
    load_cache(app);
}

/// The Downloaded only filter changed.
pub fn on_filter_changed(app: &mut App) {
    app.library.downloaded = None;
    app.library.downloaded_albums = None;
    app.library.search.invalidate();
    // Detail pages fall back to the server again when the filter is turned off.
    app.library.fresh.retain(|key| !key.contains(':') || key.starts_with("albums:"));
}

fn load_cache(app: &mut App) {
    let account = app.session.account_key();
    let mut snapshot = LibrarySnapshot::default();
    if !account.is_empty() {
        fn part<T: DeserializeOwned + Default>(account: &str, part: Part) -> T {
            storage::load(&format!("library/{account}/{}", part.name())).unwrap_or_default()
        }
        snapshot = LibrarySnapshot {
            newest: part(&account, Part::Newest),
            albums: part(&account, Part::Albums),
            artists: part(&account, Part::Artists),
            playlists: part(&account, Part::Playlists),
            podcasts: part(&account, Part::Podcasts),
            favorites: part(&account, Part::Favorites),
            songs: part(&account, Part::Songs),
            playlist_contents: part(&account, Part::PlaylistContents),
            album_details: part(&account, Part::AlbumDetails),
        };
    }
    app.library.snapshot = snapshot;
    app.library.account = account;
}

fn save(app: &App, part: Part) {
    let account = &app.library.account;
    if account.is_empty() || *account != app.session.account_key() {
        return;
    }
    let key = format!("library/{account}/{}", part.name());
    let s = &app.library.snapshot;
    match part {
        Part::Newest => storage::save(&key, &s.newest),
        Part::Albums => storage::save(&key, &s.albums),
        Part::Artists => storage::save(&key, &s.artists),
        Part::Playlists => storage::save(&key, &s.playlists),
        Part::Podcasts => storage::save(&key, &s.podcasts),
        Part::Favorites => storage::save(&key, &s.favorites),
        Part::Songs => storage::save(&key, &s.songs),
        Part::PlaylistContents => storage::save(&key, &s.playlist_contents),
        Part::AlbumDetails => storage::save(&key, &s.album_details),
    }
}

/// Clears every cached part of the active library (used by settings to free space).
pub fn clear_cache(app: &mut App) {
    let account = app.session.account_key();
    for part in Part::ALL {
        storage::remove(&format!("library/{account}/{}", part.name()));
    }
    on_library_changed(app);
}

// MARK: Loading

pub fn is_loading(app: &App, key: &str) -> bool {
    app.library.loading.contains(key)
}

pub fn error(app: &App, key: &str) -> Option<String> {
    app.library.errors.get(key).cloned()
}

/// Whether `key` was refreshed from the server in this session.
pub fn is_fresh(app: &App, key: &str) -> bool {
    app.library.fresh.contains(key)
}

/// Runs a server request once per key at a time. Results of a previous library are dropped.
fn load<T, Fut>(app: &mut App, key: String, request: impl FnOnce(MusicClient) -> Fut, apply: impl FnOnce(&mut App, T) + MaybeSend + 'static)
where
    T: MaybeSend + 'static,
    Fut: Future<Output = MusicResult<T>> + MaybeSend + 'static,
{
    let Some(client) = app.client() else { return };
    if !app.library.loading.insert(key.clone()) {
        return;
    }
    app.library.errors.remove(&key);
    let generation = app.session.generation;
    let future = request(client);
    app.tasks.spawn(async move {
        let result = future.await;
        update(move |app: &mut App| {
            if app.session.generation != generation {
                return;
            }
            app.library.loading.remove(&key);
            app.library.fresh.insert(key.clone());
            match result {
                Ok(value) => apply(app, value),
                Err(error) => {
                    app.library.errors.insert(key, l10n::music_error(error));
                }
            }
        })
    });
}

/// Refreshes the library overview: newest albums, the current album list, artists, playlists and
/// favorites. Each section refreshes independently, so an optional API cannot hide the others.
pub fn refresh(app: &mut App) {
    load_newest(app);
    load_albums(app, false);
    load_artists(app);
    load_playlists(app);
    load_favorites(app);
}

/// Loads a section if it was not refreshed in this session.
pub fn ensure(app: &mut App, key: &str) {
    if app.settings.downloaded_only || is_fresh(app, key) || is_loading(app, key) || app.library.errors.contains_key(key) {
        return;
    }
    match key {
        keys::NEWEST => load_newest(app),
        keys::ARTISTS => load_artists(app),
        keys::PLAYLISTS => load_playlists(app),
        keys::PODCASTS => load_podcasts(app),
        keys::FAVORITES => load_favorites(app),
        keys::SONGS => load_songs(app),
        other if other.starts_with("albums:") => load_albums(app, false),
        _ => {}
    }
}

pub fn load_newest(app: &mut App) {
    load(
        app,
        keys::NEWEST.into(),
        |c| async move { c.albums("newest", 0, NEWEST_COUNT).await },
        |app, albums| {
            app.library.snapshot.newest = albums;
            save(app, Part::Newest);
        },
    );
}

/// Loads the first page of the current album list, or the next one with `more`.
pub fn load_albums(app: &mut App, more: bool) {
    let list = app.library.album_list.api();
    let offset = if more { app.library.snapshot.albums.get(list).map_or(0, Vec::len) } else { 0 };
    load(
        app,
        keys::albums(list),
        move |c| async move { c.albums(list, offset, ALBUM_PAGE).await },
        move |app, page: Vec<Album>| {
            app.library.more_albums.insert(list.to_string(), page.len() == ALBUM_PAGE);
            let albums = app.library.snapshot.albums.entry(list.to_string()).or_default();
            if offset == 0 {
                *albums = page;
            } else {
                let known: HashSet<String> = albums.iter().map(|a| a.id.clone()).collect();
                albums.extend(page.into_iter().filter(|a| !known.contains(&a.id)));
            }
            save(app, Part::Albums);
        },
    );
}

pub fn set_album_list(app: &mut App, list: AlbumListType) {
    if app.library.album_list == list {
        return;
    }
    app.library.album_list = list;
    storage::save("library-view", &ViewPreferences { album_list: list });
}

pub fn load_artists(app: &mut App) {
    load(
        app,
        keys::ARTISTS.into(),
        |c| async move { c.artists().await },
        |app, mut artists: Vec<Artist>| {
            artists.sort_by(|a, b| natural_cmp(&sort_name(&a.name), &sort_name(&b.name)));
            app.library.snapshot.artists = artists;
            save(app, Part::Artists);
        },
    );
}

pub fn load_playlists(app: &mut App) {
    load(
        app,
        keys::PLAYLISTS.into(),
        |c| async move { c.playlists().await },
        |app, playlists| {
            if !app.library.changing_playlist {
                app.library.snapshot.playlists = playlists;
                save(app, Part::Playlists);
                downloads::on_library_refreshed(app);
            }
        },
    );
}

pub fn load_favorites(app: &mut App) {
    load(
        app,
        keys::FAVORITES.into(),
        |c| async move { c.favorites().await },
        |app, favorites| {
            app.library.snapshot.favorites = favorites;
            save(app, Part::Favorites);
        },
    );
}

pub fn load_songs(app: &mut App) {
    let limits = SearchLimits { song_count: SONG_LIMIT, album_count: 0, artist_count: 0, ..Default::default() };
    load(
        app,
        keys::SONGS.into(),
        move |c| async move { c.search("", limits).await },
        |app, results: SearchResults| {
            app.library.snapshot.songs = results.song.unwrap_or_default();
            save(app, Part::Songs);
        },
    );
}

pub fn supports_podcasts(app: &App) -> bool {
    app.session.provider().is_some_and(MusicProvider::supports_podcasts)
}

pub fn load_podcasts(app: &mut App) {
    if !supports_podcasts(app) {
        app.library.snapshot.podcasts.clear();
        return;
    }
    load(
        app,
        keys::PODCASTS.into(),
        |c| async move { c.podcasts(None, Some(false)).await },
        |app, channels| {
            app.library.snapshot.podcasts = channels;
            save(app, Part::Podcasts);
        },
    );
}

pub fn load_album(app: &mut App, id: &str) {
    let id = id.to_string();
    let request_id = id.clone();
    load(app, keys::album(&id), move |c| async move { c.album(&request_id).await }, remember_album);
}

fn remember_album(app: &mut App, album: Album) {
    let id = album.id.clone();
    let details = &mut app.library.snapshot.album_details;
    details.insert(id.clone(), album);
    if details.len() > ALBUM_DETAIL_LIMIT {
        // Keep albums that are complete on this computer; drop the first other one.
        let mut ids: Vec<String> = details.keys().filter(|k| **k != id).cloned().collect();
        ids.sort();
        let removable = ids.into_iter().find(|key| {
            let album = &app.library.snapshot.album_details[key];
            !album_listening::is_complete(album) || !album.songs().iter().all(|s| downloads::is_downloaded(app, s))
        });
        if let Some(key) = removable {
            app.library.snapshot.album_details.remove(&key);
        }
    }
    save(app, Part::AlbumDetails);
}

pub fn load_artist(app: &mut App, id: &str) {
    let id = id.to_string();
    let request_id = id.clone();
    load(
        app,
        keys::artist(&id),
        move |c| async move { c.artist(&request_id).await },
        move |app, artist| {
            app.library.artist_details.insert(id, artist);
        },
    );
}

pub fn load_playlist(app: &mut App, id: &str) {
    let id = id.to_string();
    let request_id = id.clone();
    load(
        app,
        keys::playlist(&id),
        move |c| async move { c.playlist(&request_id).await },
        move |app, mut playlist: Playlist| {
            if app.library.changing_playlist {
                return;
            }
            let songs = playlist.entry.take().unwrap_or_default();
            playlist.song_count = Some(songs.len() as i64);
            if let Some(existing) = app.library.snapshot.playlists.iter_mut().find(|p| p.id == id) {
                *existing = playlist;
                save(app, Part::Playlists);
            } else {
                app.library.opened_playlists.insert(id.clone(), playlist);
            }
            app.library.snapshot.playlist_contents.insert(id, songs);
            save(app, Part::PlaylistContents);
        },
    );
}

pub fn load_podcast(app: &mut App, id: &str) {
    let id = id.to_string();
    let request_id = id.clone();
    load(
        app,
        keys::podcast(&id),
        move |c| async move { c.podcasts(Some(&request_id), Some(true)).await },
        move |app, channels| {
            let Some(mut channel) = channels.into_iter().next() else {
                app.library.errors.insert(keys::podcast(&id), l10n::music_error(MusicError::NotFound));
                return;
            };
            let playable: Vec<_> = channel.episodes().iter().filter(|e| e.is_playable()).cloned().collect();
            let songs = podcast_order::newest_first(&playable).iter().map(|e| e.song(&channel)).collect();
            channel.episode = None;
            app.library.episodes.insert(id, (channel, songs));
        },
    );
}

// MARK: Lookups

/// Album metadata from any loaded list, for headers and menus.
pub fn find_album(app: &mut App, id: &str) -> Option<Album> {
    let s = &app.library.snapshot;
    if let Some(album) = s.album_details.get(id) {
        let mut album = album.clone();
        album.song = None;
        return Some(album);
    }
    let found = s
        .newest
        .iter()
        .chain(s.albums.values().flatten())
        .chain(s.favorites.albums())
        .chain(app.library.artist_details.values().flat_map(|a| a.albums()))
        .chain(app.library.search.results.albums())
        .find(|a| a.id == id)
        .cloned();
    found.or_else(|| downloaded_albums(app).into_iter().find(|a| a.id == id))
}

pub fn find_artist(app: &App, id: &str) -> Option<Artist> {
    if let Some(artist) = app.library.artist_details.get(id) {
        return Some(artist.clone());
    }
    let s = &app.library.snapshot;
    s.artists.iter().chain(s.favorites.artists()).chain(app.library.search.results.artists()).find(|a| a.id == id).cloned()
}

/// The artist page for an artist ID or, when the server sent none, an artist with exactly that name.
pub fn artist_route(app: &App, id: Option<&str>, name: Option<&str>) -> Option<crate::nav::Route> {
    if let Some(id) = id.filter(|id| !id.is_empty()) {
        return Some(crate::nav::Route::Artist(id.to_string()));
    }
    let name = name.filter(|n| !n.is_empty())?;
    app.library.snapshot.artists.iter().find(|a| a.name == name).map(|a| crate::nav::Route::Artist(a.id.clone()))
}

pub fn find_playlist(app: &App, id: &str) -> Option<Playlist> {
    app.library.snapshot.playlists.iter().find(|p| p.id == id).or_else(|| app.library.opened_playlists.get(id)).cloned()
}

pub fn find_podcast(app: &App, id: &str) -> Option<PodcastChannel> {
    if let Some((channel, _)) = app.library.episodes.get(id) {
        return Some(channel.clone());
    }
    app.library
        .snapshot
        .podcasts
        .iter()
        .chain(app.library.search.podcasts.channels.iter())
        .find(|c| c.id == id)
        .map(|c| PodcastChannel { episode: None, ..c.clone() })
}

/// The album's tracks in disc and track order, honoring Downloaded only. `None` until loaded.
pub fn album_tracks(app: &mut App, id: &str) -> Option<Vec<Song>> {
    let cached = app.library.snapshot.album_details.get(id).map(album_listening::tracks);
    if app.settings.downloaded_only {
        let mut songs: Vec<Song> = match cached {
            Some(songs) => songs.into_iter().filter(|s| downloads::is_downloaded(app, s)).collect(),
            None => Vec::new(),
        };
        if songs.is_empty() {
            songs = downloaded_songs(app).into_iter().filter(|s| s.album_id.as_deref() == Some(id)).collect();
            songs.sort_by_key(|s| (s.disc_number.unwrap_or(1), s.track.unwrap_or(0)));
        }
        return Some(songs);
    }
    cached
}

/// The playlist's entries in server order, honoring Downloaded only. `None` until loaded.
pub fn playlist_tracks(app: &mut App, id: &str) -> Option<Vec<Song>> {
    let songs = app.library.snapshot.playlist_contents.get(id).cloned();
    if app.settings.downloaded_only {
        return Some(songs.unwrap_or_default().into_iter().filter(|s| downloads::is_downloaded(app, s)).collect());
    }
    songs
}

pub fn podcast_episodes(app: &mut App, id: &str) -> Option<Vec<Song>> {
    let prefix = format!("podcast:{id}");
    let loaded = app.library.episodes.get(id).map(|(_, songs)| songs.clone());
    if app.settings.downloaded_only {
        return Some(downloaded_songs(app).into_iter().filter(|s| s.album_id.as_deref() == Some(prefix.as_str())).collect());
    }
    loaded
}

/// Favorite song ids of the active library.
pub fn favorite_ids(app: &App) -> HashSet<String> {
    app.library.snapshot.favorites.songs().iter().map(|s| s.id.clone()).collect()
}

/// The favorites list is authoritative once refreshed; before that, the song's own star counts too.
pub fn is_favorite(app: &App, song: &Song) -> bool {
    let listed = app.library.snapshot.favorites.songs().iter().any(|s| s.id == song.id);
    listed || (!is_fresh(app, keys::FAVORITES) && song.is_starred() && !app.library.unstarred.contains(&song.id))
}

/// Downloaded songs of the active library (the Downloaded only filter's content).
pub fn downloaded_songs(app: &mut App) -> Vec<Song> {
    let revision = downloads::revision(app);
    if let Some((cached, songs)) = &app.library.downloaded
        && *cached == revision {
            return songs.clone();
        }
    let songs = downloads::downloaded_songs(app);
    app.library.downloaded = Some((revision, songs.clone()));
    app.library.downloaded_albums = None;
    songs
}

/// Albums of downloaded music, sorted by name.
pub fn downloaded_albums(app: &mut App) -> Vec<Album> {
    let revision = downloads::revision(app);
    if let Some((cached, albums)) = &app.library.downloaded_albums
        && *cached == revision {
            return albums.clone();
        }
    let albums = downloads::downloaded_albums(app);
    app.library.downloaded_albums = Some((revision, albums.clone()));
    albums
}

/// A name for alphabetical sorting and the A–Z index: folded, without a leading "The ".
pub fn sort_name(name: &str) -> String {
    let folded = fold(name.trim());
    folded.strip_prefix("the ").map(str::to_string).unwrap_or(folded)
}

// MARK: Actions

/// Runs `action` with an album's tracks in disc order, loading them first when needed.
pub fn with_album_songs(app: &mut App, id: &str, action: impl FnOnce(&mut App, Vec<Song>) + MaybeSend + 'static) {
    if let Some(songs) = album_tracks(app, id).filter(|s| !s.is_empty()) {
        return action(app, songs);
    }
    if app.settings.downloaded_only {
        return action(app, Vec::new());
    }
    let id = id.to_string();
    with_request(
        app,
        move |c| async move { c.album(&id).await },
        move |app, album: Album| {
            let id = album.id.clone();
            remember_album(app, album);
            let songs = album_tracks(app, &id).unwrap_or_default();
            action(app, songs);
        },
    );
}

/// Runs `action` with a playlist's entries, loading them first when needed.
pub fn with_playlist_songs(app: &mut App, id: &str, action: impl FnOnce(&mut App, Vec<Song>) + MaybeSend + 'static) {
    if let Some(songs) = playlist_tracks(app, id)
        && (!songs.is_empty() || app.settings.downloaded_only) {
            return action(app, songs);
        }
    let id = id.to_string();
    with_request(
        app,
        move |c| async move { c.playlist(&id).await },
        move |app, playlist: Playlist| {
            let songs = playlist.songs().to_vec();
            app.library.snapshot.playlist_contents.insert(playlist.id.clone(), songs);
            save(app, Part::PlaylistContents);
            let songs = playlist_tracks(app, &playlist.id).unwrap_or_default();
            action(app, songs);
        },
    );
}

/// A one-off request whose failure is shown in the attention alert.
fn with_request<T, Fut>(app: &mut App, request: impl FnOnce(MusicClient) -> Fut, apply: impl FnOnce(&mut App, T) + MaybeSend + 'static)
where
    T: MaybeSend + 'static,
    Fut: Future<Output = MusicResult<T>> + MaybeSend + 'static,
{
    let Some(client) = app.client() else { return app.show_error(l10n::music_error(MusicError::MissingCredentials)) };
    let generation = app.session.generation;
    let future = request(client);
    app.tasks.spawn(async move {
        let result = future.await;
        update(move |app: &mut App| {
            if app.session.generation != generation {
                return;
            }
            match result {
                Ok(value) => apply(app, value),
                Err(error) => app.show_error(l10n::music_error(error)),
            }
        })
    });
}

/// Plays songs from `index`; an empty list explains why nothing plays.
pub fn play(app: &mut App, songs: Vec<Song>, index: usize, shuffled: bool) {
    let index = if shuffled && !songs.is_empty() { rand::random_range(0..songs.len()) } else { index };
    player::play_songs(app, songs, index, shuffled);
}

/// "Listen to the whole album": the complete album in disc and track order, stopping at the end.
pub fn listen_to_whole_album(app: &mut App, album_id: &str) {
    let downloaded_only = app.settings.downloaded_only;
    let id = album_id.to_string();
    let start = move |app: &mut App, album: Album| {
        let songs = album_listening::tracks(&album);
        if downloaded_only && !(album_listening::is_complete(&album) && songs.iter().all(|s| downloads::is_downloaded(app, s))) {
            return app.show_error(l10n::t("Download every track of an album to use this mode offline."));
        }
        let context = PlayContext::AlbumFocus { album_id: album.id.clone(), title: album.name.clone() };
        player::play_songs_with_context(app, songs, 0, false, Some(context));
    };
    let cached = app.library.snapshot.album_details.get(&id).cloned();
    match cached {
        Some(album) if downloaded_only || is_fresh(app, &keys::album(&id)) => start(app, album),
        cached => {
            let Some(client) = app.client() else { return };
            let generation = app.session.generation;
            app.tasks.spawn(async move {
                let result = client.album(&id).await;
                update(move |app: &mut App| {
                    if app.session.generation != generation {
                        return;
                    }
                    match result {
                        Ok(album) if album_listening::is_complete(&album) => {
                            remember_album(app, album.clone());
                            start(app, album);
                        }
                        Ok(_) => app.show_error(l10n::music_error(MusicError::InvalidResponse)),
                        Err(error) => match cached {
                            Some(album) => start(app, album),
                            None => app.show_error(l10n::music_error(error)),
                        },
                    }
                })
            });
        }
    }
}

pub fn toggle_favorite(app: &mut App, song: &Song) {
    let Some(client) = app.client() else { return app.show_error(l10n::music_error(MusicError::MissingCredentials)) };
    let star = !is_favorite(app, song);
    let generation = app.session.generation;
    let song = song.clone();
    app.tasks.spawn(async move {
        let result = client.favorite(song.stream_identifier(), star).await;
        update(move |app: &mut App| {
            if app.session.generation != generation {
                return;
            }
            if let Err(error) = result {
                return app.show_error(l10n::music_error(error));
            }
            if star {
                app.library.unstarred.remove(&song.id);
            } else {
                app.library.unstarred.insert(song.id.clone());
            }
            let favorites = app.library.snapshot.favorites.song.get_or_insert_with(Vec::new);
            favorites.retain(|s| s.id != song.id);
            if star {
                let mut song = song;
                song.starred = Some(chrono::Utc::now().to_rfc3339());
                favorites.push(song);
            }
            save(app, Part::Favorites);
        })
    });
}

/// Applies a playlist change on the server, then reads the playlists back. `done` receives the
/// mutation's result; a failed read-back never reports a successful change as failed.
pub fn change_playlist(
    app: &mut App,
    change: PlaylistChange,
    done: impl FnOnce(&mut App, MusicResult<PlaylistChangeResult>) + MaybeSend + 'static,
) {
    let Some(client) = app.client() else { return done(app, Err(MusicError::MissingCredentials)) };
    if app.library.changing_playlist {
        return;
    }
    app.library.changing_playlist = true;
    let generation = app.session.generation;
    app.tasks.spawn(async move {
        let mut result = PlaylistChangeResult::default();
        let mut contents: Option<(String, Option<Vec<Song>>)> = None;
        let outcome: MusicResult<()> = async {
            match &change {
                PlaylistChange::Create(name, songs) => result.created = client.create_playlist(name, songs).await?,
                PlaylistChange::Rename(id, name) => {
                    client.rename_playlist(id, name).await?;
                    contents = Some((id.clone(), None));
                }
                PlaylistChange::Add(id, songs) => {
                    result.addition = Some(client.add_to_playlist(id, songs).await?);
                    contents = Some((id.clone(), None));
                }
                PlaylistChange::Tracks { id, expected, songs } => {
                    client.replace_playlist_songs(id, expected, songs).await?;
                    contents = Some((id.clone(), Some(songs.clone())));
                }
                PlaylistChange::Remove { id, song, occurrence, expected } => {
                    let remaining = client.remove_from_playlist(id, song, *occurrence, expected).await?;
                    contents = Some((id.clone(), Some(remaining)));
                }
                PlaylistChange::Delete(id) => {
                    client.delete_playlist(id).await?;
                    contents = Some((id.clone(), None));
                }
            }
            Ok(())
        }
        .await;
        let readback = if outcome.is_ok() { Some(client.playlists().await) } else { None };
        update(move |app: &mut App| {
            if app.session.generation != generation {
                return;
            }
            app.library.changing_playlist = false;
            if let Err(error) = outcome {
                return done(app, Err(error));
            }
            let library = &mut app.library;
            if let Some((id, songs)) = contents {
                match songs {
                    Some(songs) => {
                        library.snapshot.playlist_contents.insert(id, songs);
                    }
                    None => {
                        // Unknown contents: reload when the playlist opens again.
                        library.snapshot.playlist_contents.remove(&id);
                        library.fresh.remove(&keys::playlist(&id));
                    }
                }
            }
            if let PlaylistChange::Delete(id) = &change {
                library.snapshot.playlists.retain(|p| &p.id != id);
                library.opened_playlists.remove(id);
            }
            if let Some(created) = &result.created {
                library.snapshot.playlists.retain(|p| p.id != created.id);
                let mut entry = created.clone();
                library.snapshot.playlist_contents.insert(created.id.clone(), entry.entry.take().unwrap_or_default());
                library.snapshot.playlists.push(entry);
            }
            match readback {
                Some(Ok(playlists)) => {
                    library.snapshot.playlists = playlists;
                    library.errors.remove(keys::PLAYLISTS);
                }
                _ => {
                    library.errors.insert(
                        keys::PLAYLISTS.into(),
                        l10n::t("Your change was saved. Refresh playlists to load the latest information."),
                    );
                }
            }
            save(app, Part::Playlists);
            save(app, Part::PlaylistContents);
            downloads::on_library_refreshed(app);
            done(app, Ok(result));
        })
    });
}

/// Removes one occurrence of a song from a playlist right away, checking the server still has the
/// contents shown.
pub fn remove_from_playlist(app: &mut App, playlist_id: &str, song: &Song, occurrence: usize) {
    let Some(expected) = app.library.snapshot.playlist_contents.get(playlist_id).cloned() else { return };
    let change = PlaylistChange::Remove { id: playlist_id.to_string(), song: song.clone(), occurrence, expected };
    change_playlist(app, change, |app, result| {
        if let Err(error) = result {
            app.show_error(l10n::music_error(error));
        }
    });
}

/// Subscribes the server to a podcast feed, then reloads the channel list.
pub fn add_podcast(app: &mut App, feed: url::Url, done: impl FnOnce(&mut App, MusicResult<()>) + MaybeSend + 'static) {
    let Some(client) = app.client() else { return done(app, Err(MusicError::MissingCredentials)) };
    if app.library.changing_podcasts {
        return;
    }
    app.library.changing_podcasts = true;
    let generation = app.session.generation;
    app.tasks.spawn(async move {
        let result = client.create_podcast_channel(&feed).await;
        let channels = if result.is_ok() { Some(client.podcasts(None, Some(false)).await) } else { None };
        update(move |app: &mut App| {
            if app.session.generation != generation {
                return;
            }
            app.library.changing_podcasts = false;
            invalidate_podcast_index(app);
            if let Some(Ok(channels)) = channels {
                app.library.snapshot.podcasts = channels;
                save(app, Part::Podcasts);
            }
            done(app, result);
        })
    });
}

/// Removes the channel from the server and its downloaded episodes from this computer.
pub fn delete_podcast(app: &mut App, id: &str, done: impl FnOnce(&mut App, MusicResult<()>) + MaybeSend + 'static) {
    let Some(client) = app.client() else { return done(app, Err(MusicError::MissingCredentials)) };
    if app.library.changing_podcasts {
        return;
    }
    app.library.changing_podcasts = true;
    let generation = app.session.generation;
    let id = id.to_string();
    app.tasks.spawn(async move {
        let result = client.delete_podcast_channel(&id).await;
        update(move |app: &mut App| {
            if app.session.generation != generation {
                return;
            }
            app.library.changing_podcasts = false;
            if result.is_ok() {
                app.library.snapshot.podcasts.retain(|c| c.id != id);
                app.library.episodes.remove(&id);
                invalidate_podcast_index(app);
                save(app, Part::Podcasts);
                let album = format!("podcast:{id}");
                downloads::remove_matching(app, |song| song.album_id.as_deref() == Some(album.as_str()));
            }
            done(app, result);
        })
    });
}

/// Whether the subscribed channels with episodes were fetched less than five minutes ago, so typing
/// a search does not refetch every feed.
fn podcast_index_is_fresh(app: &App) -> bool {
    app.library.podcast_index.as_ref().is_some_and(|(_, loaded)| discophile_core::now_millis() - loaded < PODCAST_INDEX_MS)
}

/// Drops the podcast index after subscriptions change, including any request still in flight.
fn invalidate_podcast_index(app: &mut App) {
    app.library.podcast_index = None;
    app.library.podcast_index_loading = false;
    app.library.podcast_index_epoch += 1;
}
