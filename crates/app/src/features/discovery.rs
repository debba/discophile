//! Listen Now discovery: the per-library listening archive (history, daily statistics log, library
//! sample and curated mixes), discovery mixes, album suggestions for tonight and personal radio.
//!
//! The archive stays on this device under `library/<account>/discovery`; album metadata used by
//! "Choose a record for tonight" is cached under `library/<account>/discovery-albums`.

mod personal_radio;

use std::collections::{HashMap, HashSet};

use discophile_core::album_listening::{self, AlbumSuggestion};
use discophile_core::listening_history::{
    self as history, DiscoveryArchive, DiscoveryMix, FALLBACK_SAMPLE_ALBUMS, ListeningHistory, ListeningResume, RANDOM_SAMPLE_SIZE,
};
use discophile_core::listening_statistics::{ListeningPeriod, ListeningSummary};
use discophile_core::queue::RepeatMode;
use discophile_core::{Album, MusicError, Song, l10n};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::app::App;
use crate::features::downloads;
use crate::platform::storage;
use crate::player::{self, PlayContext};
use crate::tasks::update;

pub use personal_radio::{
    RadioState, is_radio_active, open_radio_window, radio_reasons, start_personal_radio, stop_personal_radio, why_this_song,
};

/// Listening is written to the archive at least this often while a song plays.
const FLUSH_SECONDS: f64 = 10.0;
/// A failed candidate refresh is retried automatically after five minutes.
const RETRY_MILLIS: i64 = 300_000;
/// Album lists requested for "Choose a record for tonight".
const NEWEST_ALBUMS: usize = 30;
const RANDOM_ALBUMS: usize = 60;
/// Candidate albums whose track lists are checked per refresh, at most this many.
const ALBUM_COMPLETIONS: usize = 9;

/// The song currently being listened to, accumulated between archive writes.
struct ListenSession {
    song: Song,
    id: Uuid,
    listened: f64,
    recorded: f64,
    position: f64,
    duration: f64,
}

/// Album metadata kept for suggestions, including complete track lists used offline.
#[derive(Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct AlbumCache {
    albums: Vec<Album>,
    favorite_ids: Vec<String>,
    details: HashMap<String, Album>,
}

/// A discovery mix or saved selection shown in the track list window.
#[derive(Clone)]
pub struct SelectionWindow {
    pub title: String,
    pub explanation: String,
    pub songs: Vec<Song>,
}

/// Interface state of Listen Now, statistics and the Rediscover settings.
pub struct DiscoveryUi {
    pub selection: Option<SelectionWindow>,
    /// "Choose a record for tonight" duration filter; `None` is any duration.
    pub minutes: Option<i64>,
    /// Albums already shown while browsing tonight's suggestions.
    pub shown_albums: HashSet<String>,
    pub period: ListeningPeriod,
    pub history_expanded: bool,
    pub confirm_clear: bool,
    /// Album being loaded before it plays.
    pub album_loading: Option<String>,
}

impl Default for DiscoveryUi {
    fn default() -> Self {
        Self {
            selection: None,
            minutes: None,
            shown_albums: HashSet::new(),
            period: storage::load("statistics-period").unwrap_or_default(),
            history_expanded: false,
            confirm_clear: false,
            album_loading: None,
        }
    }
}

/// A discovery mix with its songs.
pub type MixSongs = (DiscoveryMix, Vec<Song>);
/// Archive and album revisions, duration filter, 30-second time slot and Downloaded only.
type AlbumKey = (u64, u64, Option<i64>, i64, bool);
/// A value computed for a key (archive revision, day, filters…), reused while the key is unchanged.
type Cached<K, V> = Option<(K, V)>;

#[derive(Default)]
struct Caches {
    mixes: Cached<(u64, i64, bool), Vec<MixSongs>>,
    week: Cached<(u64, i64), ListeningSummary>,
    all_empty: Cached<(u64, i64), bool>,
    summary: Cached<(u64, ListeningPeriod, i64), ListeningSummary>,
    albums: Cached<AlbumKey, Vec<AlbumSuggestion>>,
}

#[derive(Default)]
pub struct DiscoveryState {
    account: String,
    archive: DiscoveryArchive,
    albums: AlbumCache,
    /// Favorite songs from the last refresh (also part of the saved candidates).
    favorites: Vec<Song>,
    pub loading: bool,
    pub error: Option<String>,
    last_refresh: Option<i64>,
    last_attempt: Option<i64>,
    session: Option<ListenSession>,
    /// Bumped whenever the archive changes, to invalidate cached mixes and summaries.
    revision: u64,
    album_revision: u64,
    caches: Caches,
    pub radio: RadioState,
    pub ui: DiscoveryUi,
}

fn archive_key(account: &str) -> String {
    format!("library/{account}/discovery")
}

fn albums_key(account: &str) -> String {
    format!("library/{account}/discovery-albums")
}

pub fn start(app: &mut App) {
    load(app);
}

/// Loads the active library's archive, replacing the previous one.
fn load(app: &mut App) {
    let account = app.session.account_key();
    let state = &mut app.discovery;
    state.account = account.clone();
    state.archive = DiscoveryArchive::default();
    state.albums = AlbumCache::default();
    state.favorites.clear();
    state.loading = false;
    state.error = None;
    state.last_refresh = None;
    state.last_attempt = None;
    state.session = None;
    state.ui.selection = None;
    state.ui.album_loading = None;
    state.ui.confirm_clear = false;
    state.revision += 1;
    state.album_revision += 1;
    state.caches = Caches::default();
    if account.is_empty() {
        return;
    }
    if let Some(archive) = storage::load::<DiscoveryArchive>(&archive_key(&account)) {
        state.archive = archive.normalized();
    }
    if let Some(albums) = storage::load::<AlbumCache>(&albums_key(&account)) {
        state.albums = albums;
    }
}

fn save(app: &mut App) {
    let state = &app.discovery;
    if state.account.is_empty() {
        return;
    }
    storage::save(&archive_key(&state.account), &state.archive);
}

fn save_albums(app: &mut App) {
    let state = &app.discovery;
    if !state.account.is_empty() {
        storage::save(&albums_key(&state.account), &state.albums);
    }
}

pub fn tick(app: &mut App) {
    refresh(app, false);
}

pub fn windows(app: &mut App, ctx: &egui::Context) {
    crate::ui::listen_now::windows(app, ctx);
    crate::ui::statistics::windows(app, ctx);
    personal_radio::window(app, ctx);
}

pub fn on_library_changed(app: &mut App) {
    flush(app, false, false);
    personal_radio::reset(app);
    load(app);
}

// MARK: Listening

/// Adds real listening time (seeks and buffering excluded) for the current song.
pub fn record_listening(app: &mut App, song: &Song, seconds: f64) {
    if song.is_podcast() || !seconds.is_finite() || seconds <= 0.0 || app.discovery.account.is_empty() {
        return;
    }
    if app.discovery.session.as_ref().is_none_or(|s| s.song.stream_identifier() != song.stream_identifier()) {
        begin_session(app, song);
    }
    let position = app.player.status.position;
    let duration = app.player.duration().or(song.duration).unwrap_or(0.0);
    let Some(session) = app.discovery.session.as_mut() else { return };
    session.listened += seconds;
    session.position = position;
    session.duration = duration;
    if session.listened - session.recorded >= FLUSH_SECONDS {
        flush(app, false, false);
    }
}

/// The song passed the 30-second (or half-length) rule: write it now so the play counts.
pub fn on_qualified_listen(app: &mut App, _song: &Song) {
    flush(app, false, false);
}

/// A new item started: the previous session is written and a new one begins.
pub fn on_track_started(app: &mut App, song: &Song) {
    if app.discovery.session.is_some() {
        flush(app, false, false);
        // The queue already moved on; resume from the start of the new song.
        if app.discovery.session.as_ref().is_some_and(|s| s.listened >= 1.0) {
            app.discovery.archive.history.resume = ListeningResume::new(&app.player.queue, 0.0);
            save(app);
        }
    }
    if song.is_podcast() {
        app.discovery.session = None;
    } else {
        begin_session(app, song);
    }
}

fn begin_session(app: &mut App, song: &Song) {
    app.discovery.session =
        Some(ListenSession { song: song.clone(), id: Uuid::new_v4(), listened: 0.0, recorded: 0.0, position: 0.0, duration: 0.0 });
}

/// Writes the current session's listening to the archive with the core `record_playback` rules.
fn flush(app: &mut App, completed: bool, finished_queue: bool) {
    let state = &mut app.discovery;
    let Some(session) = state.session.as_mut() else { return };
    if state.account.is_empty() || session.listened <= session.recorded && !finished_queue {
        return;
    }
    let changed = state.archive.record_playback(
        &session.song,
        session.id,
        session.listened,
        session.position,
        session.duration,
        &app.player.queue,
        completed,
        finished_queue,
        &chrono::Local::now(),
    );
    session.recorded = session.listened;
    if changed {
        state.revision += 1;
        save(app);
    }
}

pub fn on_music_tick(app: &mut App) {
    personal_radio::on_music_tick(app);
}

pub fn on_queue_finished(app: &mut App) {
    flush(app, true, true);
    app.discovery.session = None;
    personal_radio::on_queue_finished(app);
}

// MARK: Archive access (also used by the curator)

pub fn archive(app: &App) -> &DiscoveryArchive {
    &app.discovery.archive
}

pub fn history(app: &App) -> &ListeningHistory {
    &app.discovery.archive.history
}

/// Saves (or updates) a curated mix first; keeps the 12 most recent. Returns false for empty new mixes.
pub fn save_mix(app: &mut App, mix: discophile_core::curated_mix::CuratedMix) -> bool {
    let saved = app.discovery.archive.save_mix(mix);
    if saved {
        app.discovery.revision += 1;
        save(app);
        downloads::on_mixes_changed(app);
    }
    saved
}

pub fn remove_mix(app: &mut App, id: Uuid) -> bool {
    let removed = app.discovery.archive.remove_mix(id);
    if removed {
        app.discovery.revision += 1;
        save(app);
        downloads::on_mixes_changed(app);
    }
    removed
}

/// Clears this library's local listening history and resume point.
pub fn clear_history(app: &mut App) {
    app.discovery.archive.clear_history();
    app.discovery.session = None;
    app.discovery.revision += 1;
    save(app);
    crate::player::clear_resume(app);
}

/// Favorite albums' identifiers from the last refresh.
pub fn favorite_album_ids(app: &App) -> HashSet<String> {
    app.discovery.albums.favorite_ids.iter().cloned().collect()
}

/// The songs discovery features choose from: the library sample, listening history and favorites,
/// limited to downloaded songs when Downloaded only is on.
pub fn pool(app: &App) -> Vec<Song> {
    let downloaded = downloads::downloaded_songs(app);
    let songs = app.discovery.archive.pool(&downloaded, &app.discovery.favorites);
    if app.settings.downloaded_only { songs.into_iter().filter(|s| downloads::is_downloaded(app, s)).collect() } else { songs }
}

// MARK: Refresh

/// Samples the library (random songs and favorites) at most hourly, plus album lists for tonight's record.
pub fn refresh(app: &mut App, force: bool) {
    let state = &app.discovery;
    if !app.session.has_library() || state.account.is_empty() || app.settings.downloaded_only || state.loading {
        return;
    }
    let now = discophile_core::now_millis();
    if !history::refresh_due(state.last_refresh, now, force) {
        return;
    }
    if !force && state.last_attempt.is_some_and(|t| now - t < RETRY_MILLIS) {
        return;
    }
    let Some(client) = app.client() else { return };
    let generation = app.session.generation;
    let details: HashMap<String, Album> = state.albums.details.clone();
    app.discovery.loading = true;
    app.discovery.last_attempt = Some(now);
    app.tasks.spawn(async move {
        let mut sample = client.random_songs(RANDOM_SAMPLE_SIZE).await.unwrap_or_default();
        let favorites = client.favorites().await;
        let newest = client.albums("newest", 0, NEWEST_ALBUMS).await;
        let random = client.albums("random", 0, RANDOM_ALBUMS).await.unwrap_or_default();
        let mut error: Option<MusicError> = None;
        if sample.is_empty() {
            // Older servers can still supply a sample through albums.
            for album in newest.as_deref().unwrap_or_default().iter().take(FALLBACK_SAMPLE_ALBUMS) {
                match client.album(&album.id).await {
                    Ok(full) => sample.extend(full.song.unwrap_or_default()),
                    Err(e) => {
                        error = Some(e);
                        break;
                    }
                }
            }
        }
        let (favorite_songs, favorite_albums) = match favorites {
            Ok(results) => (results.song.unwrap_or_default(), results.album.unwrap_or_default()),
            Err(e) => {
                error.get_or_insert(e);
                (Vec::new(), Vec::new())
            }
        };
        let newest = newest.unwrap_or_else(|e| {
            error.get_or_insert(e);
            Vec::new()
        });
        let mut seen = HashSet::new();
        let mut albums: Vec<Album> = newest
            .into_iter()
            .chain(random)
            .chain(favorite_albums.iter().cloned())
            .filter(|a| seen.insert(a.id.clone()))
            .map(|a| details.get(&a.id).cloned().unwrap_or(a))
            .collect();
        // Check track durations even when list metadata already reports a total.
        let mut completed = Vec::new();
        for album in albums.iter_mut().filter(|a| !album_listening::is_complete(a)).take(ALBUM_COMPLETIONS) {
            if let Ok(full) = client.album(&album.id).await {
                *album = full.clone();
                completed.push(full);
            }
        }
        update(move |app: &mut App| {
            if app.session.generation != generation {
                return;
            }
            let state = &mut app.discovery;
            state.loading = false;
            if sample.is_empty() && favorite_songs.is_empty() {
                if state.archive.candidates.is_empty() {
                    state.error = Some(l10n::music_error(error.unwrap_or(MusicError::ServerUnavailable)));
                }
                return;
            }
            state.archive.set_candidates(&sample, &favorite_songs);
            state.favorites = favorite_songs;
            state.albums.favorite_ids = favorite_albums.iter().map(|a| a.id.clone()).collect();
            if !albums.is_empty() {
                state.albums.albums = albums;
            }
            for album in completed {
                remember_album(state, album);
            }
            state.last_refresh = Some(discophile_core::now_millis());
            state.error = None;
            state.revision += 1;
            state.album_revision += 1;
            save(app);
            save_albums(app);
        })
    });
}

/// Keeps a complete album's track list for offline suggestions (bounded).
fn remember_album(state: &mut DiscoveryState, album: Album) {
    if !album_listening::is_complete(&album) {
        return;
    }
    if state.albums.details.len() >= 300 && !state.albums.details.contains_key(&album.id) {
        let keep: HashSet<&str> = state.albums.albums.iter().map(|a| a.id.as_str()).collect();
        let stale: Vec<String> = state.albums.details.keys().filter(|id| !keep.contains(id.as_str())).take(50).cloned().collect();
        for id in stale {
            state.albums.details.remove(&id);
        }
    }
    if let Some(listed) = state.albums.albums.iter_mut().find(|a| a.id == album.id) {
        *listed = album.clone();
    }
    state.albums.details.insert(album.id.clone(), album);
}

// MARK: Derived data (cached per archive revision)

fn day_now() -> i64 {
    discophile_core::now_millis() / history::DAY_MILLIS
}

/// The three discovery mixes with their songs (empty mixes are included; the page hides them).
pub fn mixes(app: &mut App) -> Vec<MixSongs> {
    let now = discophile_core::now_millis();
    let key = (app.discovery.revision, now / 30_000, app.settings.downloaded_only);
    if let Some((cached, mixes)) = &app.discovery.caches.mixes
        && *cached == key
    {
        return mixes.clone();
    }
    let candidates = pool(app);
    let history = &app.discovery.archive.history;
    let mixes: Vec<MixSongs> = DiscoveryMix::ALL.iter().map(|mix| (*mix, mix.songs(&candidates, history, now))).collect();
    app.discovery.caches.mixes = Some((key, mixes.clone()));
    mixes
}

/// Listening in the last 7 days, for the Listen Now statistics card.
pub fn week_summary(app: &mut App) -> ListeningSummary {
    let key = (app.discovery.revision, day_now());
    if let Some((cached, summary)) = &app.discovery.caches.week
        && *cached == key
    {
        return summary.clone();
    }
    let summary = ListeningSummary::now(&app.discovery.archive.history, ListeningPeriod::Week);
    app.discovery.caches.week = Some((key, summary.clone()));
    summary
}

/// Whether anything was ever counted in this library.
pub fn has_statistics(app: &mut App) -> bool {
    let key = (app.discovery.revision, day_now());
    if let Some((cached, empty)) = app.discovery.caches.all_empty
        && cached == key
    {
        return !empty;
    }
    let empty = ListeningSummary::now(&app.discovery.archive.history, ListeningPeriod::All).is_empty();
    app.discovery.caches.all_empty = Some((key, empty));
    !empty
}

pub fn summary(app: &mut App, period: ListeningPeriod) -> ListeningSummary {
    let key = (app.discovery.revision, period, discophile_core::now_millis() / 60_000);
    if let Some((cached, summary)) = &app.discovery.caches.summary
        && *cached == key
    {
        return summary.clone();
    }
    let summary = ListeningSummary::now(&app.discovery.archive.history, period);
    app.discovery.caches.summary = Some((key, summary.clone()));
    summary
}

/// Albums offered for tonight. Offline, only complete albums with every track downloaded qualify.
fn album_sources(app: &App) -> Vec<Album> {
    let cache = &app.discovery.albums;
    if app.settings.downloaded_only {
        let mut albums: Vec<Album> = cache
            .details
            .values()
            .filter(|album| {
                album_listening::is_complete(album) && album_listening::tracks(album).iter().all(|s| downloads::is_downloaded(app, s))
            })
            .cloned()
            .collect();
        albums.sort_by(|a, b| a.id.cmp(&b.id));
        return albums;
    }
    cache.albums.iter().map(|a| cache.details.get(&a.id).cloned().unwrap_or_else(|| a.clone())).collect()
}

/// Up to three album suggestions for the selected duration.
pub fn album_suggestions(app: &mut App) -> Vec<AlbumSuggestion> {
    let now = discophile_core::now_millis();
    let minutes = app.discovery.ui.minutes;
    let key = (app.discovery.revision, app.discovery.album_revision, minutes, now / 30_000, app.settings.downloaded_only);
    if let Some((cached, suggestions)) = &app.discovery.caches.albums
        && *cached == key
    {
        return suggestions.clone();
    }
    let albums: Vec<Album> = album_sources(app).into_iter().filter(|a| !app.discovery.ui.shown_albums.contains(&a.id)).collect();
    let favorites = favorite_album_ids(app);
    let suggestions = album_listening::suggest(&albums, &app.discovery.archive.history, &favorites, minutes, now);
    app.discovery.caches.albums = Some((key, suggestions.clone()));
    suggestions
}

/// Moves through eligible albums without repeating a pick until the sample is exhausted.
pub fn more_album_suggestions(app: &mut App) {
    let current: HashSet<String> = album_suggestions(app).iter().map(|s| s.album.id.clone()).collect();
    app.discovery.ui.shown_albums.extend(current.iter().cloned());
    app.discovery.caches.albums = None;
    if album_suggestions(app).is_empty() {
        app.discovery.ui.shown_albums = current;
        app.discovery.caches.albums = None;
        if album_suggestions(app).is_empty() {
            app.discovery.ui.shown_albums.clear();
            app.discovery.caches.albums = None;
        }
    }
}

pub fn set_album_minutes(app: &mut App, minutes: Option<i64>) {
    if app.discovery.ui.minutes != minutes {
        app.discovery.ui.minutes = minutes;
        app.discovery.ui.shown_albums.clear();
        app.discovery.caches.albums = None;
    }
}

/// Plays a whole album in disc and track order, loading its track list first when needed.
/// Playback stops at the end of the album.
pub fn play_album(app: &mut App, album: Album) {
    let cached = app.discovery.albums.details.get(&album.id).cloned();
    let complete = cached.or_else(|| album_listening::is_complete(&album).then(|| album.clone()));
    if let Some(full) = complete {
        start_album(app, &full);
        return;
    }
    if app.discovery.ui.album_loading.is_some() {
        return;
    }
    let Some(client) = app.client() else { return };
    let generation = app.session.generation;
    app.discovery.ui.album_loading = Some(album.id.clone());
    app.tasks.spawn(async move {
        let result = client.album(&album.id).await;
        update(move |app: &mut App| {
            if app.session.generation != generation {
                return;
            }
            app.discovery.ui.album_loading = None;
            match result {
                Ok(full) => {
                    remember_album(&mut app.discovery, full.clone());
                    app.discovery.album_revision += 1;
                    save_albums(app);
                    start_album(app, &full);
                }
                Err(error) => app.show_error(l10n::music_error(error)),
            }
        })
    });
}

fn start_album(app: &mut App, album: &Album) {
    let mut songs = album_listening::tracks(album);
    if app.settings.downloaded_only {
        songs.retain(|s| downloads::is_downloaded(app, s));
    }
    if songs.is_empty() {
        app.show_error(l10n::music_error(MusicError::EmptyQueue));
        return;
    }
    let context = PlayContext::AlbumFocus { album_id: album.id.clone(), title: album.name.clone() };
    player::play_songs_with_context(app, songs, 0, false, Some(context));
    app.player.queue.repeat_mode = RepeatMode::Off;
    app.player.shuffled = false;
}

/// Plays a discovery mix or selection from `index`.
pub fn play_selection(app: &mut App, title: &str, songs: Vec<Song>, index: usize, shuffled: bool) {
    player::play_songs_with_context(app, songs, index, shuffled, Some(PlayContext::Mix { title: title.to_string() }));
}

pub fn set_period(app: &mut App, period: ListeningPeriod) {
    app.discovery.ui.period = period;
    storage::save("statistics-period", &period);
}
