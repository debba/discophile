//! Lyrics for the current song: server lyrics first, LRCLIB when online lyrics are enabled and the server
//! has no synced text. Results are cached per library with the core repository's rules (512 entries,
//! seven days for found texts, one day for misses) and persisted under `library/<account>/lyrics`.
//! Lyrics load only when a lyrics view is open; no bulk lookup is made.

use std::sync::Arc;

use discophile_core::Song;
use discophile_core::lrclib::{LrcLibClient, SleepFuture};
use discophile_core::lyrics_repository::{LyricsCache, LyricsRepository, LyricsResult};

use crate::app::App;
use crate::platform::storage;
use crate::player::Mode;
use crate::tasks::update;

#[derive(Clone, Debug, PartialEq)]
pub enum LyricsStatus {
    Loading,
    /// `None` when neither the server nor LRCLIB has lyrics.
    Loaded(Option<LyricsResult>),
    Failed,
}

#[derive(Default)]
pub struct LyricsState {
    repository: Option<Arc<LyricsRepository>>,
    /// The account whose cache `repository` holds.
    account: String,
    saved_revision: u64,
    /// Identifies the song, library and online setting that `status` belongs to.
    pub key: Option<String>,
    pub status: Option<LyricsStatus>,
}

pub fn start(app: &mut App) {
    open_repository(app);
}

pub fn tick(app: &mut App) {
    save_if_changed(app);
}

pub fn windows(_app: &mut App, _ctx: &egui::Context) {}

pub fn on_library_changed(app: &mut App) {
    save_if_changed(app);
    open_repository(app);
}

/// Nothing is fetched here: an open lyrics view requests the new song on its next frame.
pub fn on_track_started(_app: &mut App, _song: &Song) {}

fn open_repository(app: &mut App) {
    let account = app.session.account_key();
    let cache: LyricsCache = if account.is_empty() { None } else { storage::load(&storage_key(&account)) }.unwrap_or_default();
    let sleeper: discophile_core::lrclib::Sleeper = Arc::new(|ms| -> SleepFuture { Box::pin(crate::platform::sleep_ms(ms)) });
    let online = LrcLibClient::new().with_sleeper(sleeper);
    app.lyrics.repository = Some(Arc::new(LyricsRepository::new(cache, online)));
    app.lyrics.account = account;
    app.lyrics.saved_revision = 0;
    app.lyrics.key = None;
    app.lyrics.status = None;
}

fn storage_key(account: &str) -> String {
    format!("library/{account}/lyrics")
}

fn save_if_changed(app: &mut App) {
    let Some(repository) = &app.lyrics.repository else { return };
    let revision = repository.revision();
    if revision == app.lyrics.saved_revision || app.lyrics.account.is_empty() {
        return;
    }
    app.lyrics.saved_revision = revision;
    storage::save(&storage_key(&app.lyrics.account), &repository.snapshot());
}

/// The song lyrics are shown for: music only (not radio, audiobooks or podcasts).
pub fn current_song(app: &App) -> Option<Song> {
    (app.player.mode == Mode::Music).then(|| app.player.current()).flatten().filter(|s| !s.is_podcast())
}

fn view_key(app: &App, song: &Song) -> String {
    format!("{}\u{1f}{}\u{1f}{}", app.session.account_key(), song.id, app.settings.online_lyrics)
}

/// Loads lyrics for the current song if they aren't loaded or loading. Called by lyrics views every frame.
pub fn ensure(app: &mut App) {
    let Some(song) = current_song(app) else {
        app.lyrics.key = None;
        app.lyrics.status = None;
        return;
    };
    let key = view_key(app, &song);
    if app.lyrics.key.as_deref() == Some(key.as_str()) {
        return;
    }
    load(app, song, key, false);
}

/// Try again: forces a fresh server and LRCLIB lookup.
pub fn retry(app: &mut App) {
    if let Some(song) = current_song(app) {
        let key = view_key(app, &song);
        load(app, song, key, true);
    }
}

fn load(app: &mut App, song: Song, key: String, refresh: bool) {
    if app.lyrics.account != app.session.account_key() {
        open_repository(app);
    }
    app.lyrics.key = Some(key.clone());
    let (Some(client), Some(repository)) = (app.client(), app.lyrics.repository.clone()) else {
        app.lyrics.status = Some(LyricsStatus::Loaded(None));
        return;
    };
    app.lyrics.status = Some(LyricsStatus::Loading);
    let account = app.lyrics.account.clone();
    let online = app.settings.online_lyrics;
    app.tasks.spawn(async move {
        let server = client.lyrics(&song);
        let result = repository.lyrics(&song, &account, online, refresh, server).await;
        update(move |app| {
            if app.lyrics.key.as_deref() == Some(key.as_str()) {
                app.lyrics.status = Some(match result {
                    Ok(result) => LyricsStatus::Loaded(result),
                    Err(error) => {
                        log::info!("Lyrics unavailable: {error}");
                        LyricsStatus::Failed
                    }
                });
            }
            save_if_changed(app);
        })
    });
}

/// Turning online lyrics on or off reloads the open view (cached LRCLIB texts are hidden while off).
pub fn set_online(app: &mut App, enabled: bool) {
    app.settings.online_lyrics = enabled;
    app.settings.save();
    app.lyrics.key = None;
}
