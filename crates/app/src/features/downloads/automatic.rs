//! Automatic downloads (Settings > Offline listening): per-library settings and syncs that keep selected
//! playlists and discovery mixes downloaded within a space limit. Off by default.
//!
//! Syncs run on activation (launch and library switch), after a library refresh
//! (`downloads::on_library_refreshed`), when settings change and hourly while Discophile runs.

use discophile_core::automatic_downloads::{AutomaticDownloadSettings, AutomaticDownloads, REFRESH_INTERVAL_MILLIS};
use discophile_core::curated_mix::CuratedMix;
use discophile_core::listening_history::{ListeningHistory, unique};
use discophile_core::offline_catalog::OfflineCatalog;
use discophile_core::{MusicError, Playlist, Song, l10n, now_millis};

use super::manager;
use crate::app::App;
use crate::platform::storage;
use crate::player::Mode;
use crate::tasks::update;

/// Songs requested from the server for the mix pool when discovery has none.
const POOL_SAMPLE: usize = 150;
const POOL_LIMIT: usize = 500;
/// Periodic syncs that fail wait this long before trying again.
const RETRY_INTERVAL_MILLIS: i64 = 5 * 60_000;

#[derive(Default)]
pub struct AutoState {
    pub account: String,
    pub settings: AutomaticDownloadSettings,
    pub busy: bool,
    /// Sync errors and "Some tracks do not fit…".
    pub error: Option<String>,
    /// Transfer errors of automatic downloads.
    pub transfer_error: Option<String>,
    revision: u64,
    last_refresh: Option<i64>,
    last_attempt: Option<i64>,
    /// Server sample for mixes: (account, fetched at, songs).
    pool: Option<(String, i64, Vec<Song>)>,
    /// Playlists offered in settings, loaded when the settings are shown.
    pub playlists: Option<Vec<Playlist>>,
    playlists_account: String,
    pub playlists_loading: bool,
    pub playlists_error: Option<String>,
}

/// What discovery offers to the mix planner.
#[derive(Default)]
pub struct MixInputs {
    pub curated: Vec<CuratedMix>,
    pub pool: Vec<Song>,
    pub history: ListeningHistory,
}

fn key(account: &str) -> String {
    format!("library/{account}/automatic-downloads")
}

pub fn saved_settings(account: &str) -> AutomaticDownloadSettings {
    if account.is_empty() {
        return AutomaticDownloadSettings::default();
    }
    storage::load(&key(account)).unwrap_or_default()
}

fn cancel_queued(app: &mut App, account: &str) {
    let ids = OfflineCatalog::queued_automatic(app.downloads.records(), account);
    manager::remove_ids(app, &ids);
}

/// Makes `account` the library whose settings are shown and synced. Queued automatic transfers of
/// the previous library are cancelled.
pub fn load(app: &mut App, account: &str) {
    let state = &app.downloads.automatic;
    if state.account == account && (state.revision > 0 || account.is_empty()) {
        return;
    }
    let previous = state.account.clone();
    if !previous.is_empty() && previous != account {
        cancel_queued(app, &previous);
    }
    let state = &mut app.downloads.automatic;
    *state =
        AutoState { revision: state.revision + 1, settings: saved_settings(account), account: account.to_string(), ..Default::default() };
}

/// Forgets a removed library's settings.
pub fn forget(app: &mut App, account: &str) {
    storage::remove(&key(account));
    if app.downloads.automatic.account == account {
        let revision = app.downloads.automatic.revision + 1;
        app.downloads.automatic = AutoState { revision, ..Default::default() };
    }
}

/// Changes the active library's settings, cancels queued automatic transfers and syncs again.
pub fn update_settings(app: &mut App, change: impl FnOnce(&mut AutomaticDownloadSettings)) {
    let account = app.downloads.automatic.account.clone();
    if account.is_empty() {
        return;
    }
    let state = &mut app.downloads.automatic;
    state.revision += 1;
    state.busy = false;
    state.last_refresh = None;
    change(&mut state.settings);
    storage::save(&key(&account), &state.settings);
    cancel_queued(app, &account);
    refresh(app, true);
}

/// Periodic sync while Discophile runs.
pub fn tick(app: &mut App) {
    let state = &app.downloads.automatic;
    if !state.settings.enabled || state.busy || state.account.is_empty() {
        return;
    }
    let now = now_millis();
    let due = AutomaticDownloads::refresh_due(state.last_refresh, now, false)
        && state.last_attempt.is_none_or(|last| now - last >= RETRY_INTERVAL_MILLIS);
    if due {
        refresh(app, false);
    }
}

/// Plans and applies automatic downloads when enabled and due (or forced).
pub fn refresh(app: &mut App, force: bool) {
    let account = app.session.account_key();
    load(app, &account);
    let state = &mut app.downloads.automatic;
    if force && state.busy {
        state.revision += 1;
        state.busy = false;
    }
    let now = now_millis();
    if !state.settings.enabled || state.busy || account.is_empty() || !AutomaticDownloads::refresh_due(state.last_refresh, now, force) {
        return;
    }
    let Some(client) = app.session.client.clone() else { return };
    let state = &mut app.downloads.automatic;
    state.busy = true;
    state.error = None;
    state.last_attempt = Some(now);
    let settings = state.settings.clone();
    let revision = state.revision;
    let cached_pool = state
        .pool
        .as_ref()
        .filter(|(a, fetched, _)| *a == account && now - fetched < REFRESH_INTERVAL_MILLIS)
        .map(|(_, _, songs)| songs.clone());
    let inputs = mix_inputs(app);
    let downloaded_only = app.settings.downloaded_only;
    let local = OfflineCatalog::songs(app.downloads.records(), &account);
    app.tasks.spawn(async move {
        let result: Result<(Vec<Song>, Option<Vec<Song>>), MusicError> = async {
            let mut playlist_songs = Vec::new();
            for id in &settings.playlist_ids {
                match client.playlist(id).await {
                    Ok(playlist) => playlist_songs.extend(playlist.entry.unwrap_or_default()),
                    Err(MusicError::NotFound) => {}
                    Err(error) => return Err(error),
                }
            }
            let mut fetched = None;
            let mut mixes = Vec::new();
            if settings.include_mixes {
                let sample = match cached_pool {
                    _ if !inputs.pool.is_empty() || downloaded_only => Vec::new(),
                    Some(songs) => songs,
                    None => {
                        let mut sample = client.random_songs(POOL_SAMPLE).await.unwrap_or_default();
                        if let Ok(favorites) = client.favorites().await {
                            sample.extend(favorites.song.unwrap_or_default());
                        }
                        let sample: Vec<Song> = unique(&sample).into_iter().filter(|s| !s.is_podcast()).take(POOL_LIMIT).collect();
                        fetched = Some(sample.clone());
                        sample
                    }
                };
                let history_songs = inputs.history.entries().iter().map(|e| &e.song);
                let pool: Vec<Song> = unique(inputs.pool.iter().chain(&sample).chain(&local).chain(history_songs))
                    .into_iter()
                    .filter(|s| !s.is_podcast())
                    .collect();
                mixes = AutomaticDownloads::mix_songs(&inputs.curated, &pool, &inputs.history, now_millis());
            }
            Ok((AutomaticDownloads::desired(&playlist_songs, &mixes, &settings), fetched))
        }
        .await;
        update(move |app: &mut App| apply(app, account, revision, result))
    });
}

fn apply(app: &mut App, account: String, revision: u64, result: Result<(Vec<Song>, Option<Vec<Song>>), MusicError>) {
    let state = &mut app.downloads.automatic;
    if revision != state.revision || account != app.session.account_key() {
        return;
    }
    state.busy = false;
    let (desired, fetched) = match result {
        Ok(value) => value,
        Err(error) => {
            state.error = Some(l10n::music_error(error));
            return;
        }
    };
    let now = now_millis();
    if let Some(songs) = fetched {
        state.pool = Some((account.clone(), now, songs));
    }
    if !state.settings.enabled {
        return;
    }
    let limit = state.settings.limit_bytes();
    let protected =
        if app.player.mode == Mode::Music { app.player.queue.current().map(|s| s.stream_identifier().to_string()) } else { None };
    let quality = app.settings.streaming_quality;
    let plan = AutomaticDownloads::plan(&desired, app.downloads.records(), &account, quality, limit, protected.as_deref());
    manager::remove_ids(app, &plan.remove);
    app.downloads.automatic.transfer_error = None;
    manager::download(app, plan.download, true);
    let missing = AutomaticDownloads::some_do_not_fit(&desired, app.downloads.records(), &account);
    let state = &mut app.downloads.automatic;
    state.error = missing
        .then(|| l10n::t("Some tracks do not fit within the automatic download limit. Increase the limit or choose fewer playlists."));
    state.last_refresh = Some(now);
}

/// Inputs from the discovery feature: curated mixes, its song pool (already limited by Downloaded only)
/// and listening history. When discovery has no pool yet, the sync samples the server itself.
fn mix_inputs(app: &App) -> MixInputs {
    use crate::features::discovery;
    MixInputs { curated: discovery::archive(app).curated.clone(), pool: discovery::pool(app), history: discovery::history(app).clone() }
}

/// Loads the playlists offered in the settings once per library.
pub fn load_playlists(app: &mut App, force: bool) {
    let account = app.session.account_key();
    let state = &mut app.downloads.automatic;
    if state.playlists_loading || (!force && state.playlists.is_some() && state.playlists_account == account) {
        return;
    }
    let Some(client) = app.session.client.clone() else { return };
    state.playlists_loading = true;
    state.playlists_account = account.clone();
    app.tasks.spawn(async move {
        let result = client.playlists().await;
        update(move |app: &mut App| {
            let state = &mut app.downloads.automatic;
            if state.playlists_account != account {
                return;
            }
            state.playlists_loading = false;
            match result {
                Ok(playlists) => {
                    state.playlists = Some(playlists);
                    state.playlists_error = None;
                }
                Err(error) => state.playlists_error = Some(l10n::music_error(error)),
            }
        })
    });
}
