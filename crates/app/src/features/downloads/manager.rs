//! The desktop download manager: the catalog of every library, the transfer queue and its files.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;

use discophile_core::automatic_downloads::AutomaticDownloads;
use discophile_core::client::MusicClient;
use discophile_core::offline_catalog::{DownloadDecision, OfflineCatalog, OfflineRecord, OfflineState};
use discophile_core::{MusicError, Song, l10n, now_millis};
use uuid::Uuid;

use super::automatic::{self, AutoState};
use super::store;
use super::transfer::{self, Outcome, Progress};
use crate::app::App;
use crate::tasks::update;

/// Transfers running at the same time, like the iOS background session.
pub const CONCURRENT_TRANSFERS: usize = 3;
/// Catalog changes are written at most this often (removals and new downloads are written at once).
const FLUSH_INTERVAL_MS: i64 = 1_000;

#[derive(Default)]
pub struct Manager {
    started: bool,
    /// The active library's account key.
    pub(super) account: String,
    records: Vec<OfflineRecord>,
    /// account → song id → index in `records`.
    index: HashMap<String, HashMap<String, usize>>,
    artwork: HashSet<PathBuf>,
    covers_loading: HashSet<PathBuf>,
    pending: VecDeque<Uuid>,
    active: HashMap<Uuid, Arc<Progress>>,
    dirty: HashSet<String>,
    last_flush: i64,
    /// Changes whenever the catalog changes, so callers can cache derived lists.
    pub(super) revision: u64,
    pub(super) automatic: AutoState,
    pub(super) confirm_remove_all: bool,
    /// Shown in Settings > Offline listening.
    pub(super) error: Option<String>,
    pub(super) list_filter: String,
}

/// What a track's download control shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DownloadState {
    NotDownloaded,
    /// Waiting or transferring, with the fraction transferred so far.
    Downloading(f32),
    Downloaded {
        automatic: bool,
    },
    Failed,
}

impl Manager {
    fn reindex(&mut self) {
        self.index.clear();
        for (i, record) in self.records.iter().enumerate() {
            self.index.entry(record.account.clone()).or_default().insert(record.song.id.clone(), i);
        }
        self.revision += 1;
    }

    pub(super) fn record(&self, account: &str, song_id: &str) -> Option<&OfflineRecord> {
        self.index.get(account)?.get(song_id).map(|&i| &self.records[i])
    }

    fn position(&self, id: Uuid) -> Option<usize> {
        self.records.iter().position(|r| r.id == id)
    }

    pub(super) fn records(&self) -> &[OfflineRecord] {
        &self.records
    }

    /// Records of the active library.
    pub(super) fn account_records(&self) -> impl Iterator<Item = &OfflineRecord> {
        self.records.iter().filter(|r| r.account == self.account)
    }

    pub(super) fn progress(&self, id: Uuid) -> f32 {
        self.active.get(&id).map(|p| p.fraction()).unwrap_or(0.0)
    }

    pub(super) fn state(&self, song: &Song) -> DownloadState {
        match self.record(&self.account, &song.id) {
            None => DownloadState::NotDownloaded,
            Some(record) => match record.state {
                OfflineState::Queued => DownloadState::Downloading(self.progress(record.id)),
                OfflineState::Ready => DownloadState::Downloaded { automatic: record.is_automatic() },
                OfflineState::Failed => DownloadState::Failed,
            },
        }
    }

    pub(super) fn local_file(&self, song: &Song) -> Option<PathBuf> {
        let record = self.record(&self.account, &song.id).filter(|r| r.state == OfflineState::Ready)?;
        let path = store::file_path(record);
        path.exists().then_some(path)
    }

    pub(super) fn local_artwork(&self, cover: &str) -> Option<PathBuf> {
        if self.account.is_empty() {
            return None;
        }
        let path = store::artwork_path(&self.account, cover);
        self.artwork.contains(&path).then_some(path)
    }

    pub(super) fn is_transferring(&self) -> bool {
        !self.active.is_empty() || !self.pending.is_empty()
    }

    fn mark_dirty(&mut self, account: &str) {
        self.dirty.insert(account.to_string());
    }
}

/// Loads every library's catalog and resumes transfers that were queued when Discophile quit.
pub fn start(app: &mut App) {
    if app.downloads.started {
        return;
    }
    app.downloads.started = true;
    app.downloads.account = app.session.account_key();
    let loaded = store::load_all();
    if loaded.unreadable {
        app.downloads.error = Some(l10n::t("Unable to read downloaded content. Check available storage."));
    }
    let active = app.downloads.account.clone();
    let auto_enabled = !active.is_empty() && automatic::saved_settings(&active).enabled;
    let has_credentials = |account: &str| app.session.accounts.iter().any(|a| a.id() == account);
    let mut records = loaded.records;
    let mut changed: HashSet<String> = HashSet::new();
    // Automatic transfers continue only for the active library while automatic downloads stay on.
    records.retain(|r| {
        let keep = !(r.state == OfflineState::Queued && r.is_automatic() && (r.account != active || !auto_enabled));
        if !keep {
            changed.insert(r.account.clone());
        }
        keep
    });
    let resume: HashSet<Uuid> =
        records.iter().filter(|r| r.state == OfflineState::Queued && has_credentials(&r.account)).map(|r| r.id).collect();
    for record in records.iter_mut() {
        let before = record.state;
        let dir = store::account_dir(&record.account);
        OfflineCatalog::restore(std::slice::from_mut(record), |name| dir.join(name).exists(), |id| resume.contains(&id));
        if record.state != before {
            changed.insert(record.account.clone());
        }
    }
    let mut pending: Vec<&OfflineRecord> = records.iter().filter(|r| resume.contains(&r.id)).collect();
    pending.sort_by_key(|r| r.is_automatic());
    let pending: VecDeque<Uuid> = pending.into_iter().map(|r| r.id).collect();
    let manager = &mut app.downloads;
    manager.records = records;
    manager.artwork = loaded.artwork;
    manager.pending = pending;
    manager.dirty = changed;
    manager.reindex();
    automatic::load(app, &active);
    flush(app);
    // The first automatic sync runs from `tick`, after discovery has loaded its archive.
}

/// Every frame: follows library switches, starts queued transfers, saves the catalog and runs
/// periodic automatic syncs.
pub fn tick(app: &mut App) {
    if !app.downloads.started {
        return;
    }
    if app.session.active.as_deref().unwrap_or_default() != app.downloads.account {
        on_library_changed(app);
    }
    while app.downloads.active.len() < CONCURRENT_TRANSFERS {
        let manager = &mut app.downloads;
        let next =
            manager.pending.iter().position(|id| manager.position(*id).is_some_and(|i| !manager.records[i].is_automatic())).unwrap_or(0);
        let Some(id) = manager.pending.remove(next) else { break };
        start_transfer(app, id);
    }
    if !app.downloads.dirty.is_empty() && now_millis() - app.downloads.last_flush >= FLUSH_INTERVAL_MS {
        flush(app);
    }
    if !app.downloads.active.is_empty() {
        app.ctx().request_repaint_after(std::time::Duration::from_millis(250));
    }
    automatic::tick(app);
}

pub fn on_library_changed(app: &mut App) {
    if !app.downloads.started {
        return;
    }
    let account = app.session.account_key();
    automatic::load(app, &account);
    app.downloads.account = account;
    app.downloads.revision += 1;
    // `automatic::tick` syncs the new library on the next frame, after discovery switched too.
}

/// Writes the catalogs of libraries that changed.
pub fn flush(app: &mut App) {
    let manager = &mut app.downloads;
    manager.last_flush = now_millis();
    let mut failed = false;
    for account in std::mem::take(&mut manager.dirty) {
        let records: Vec<&OfflineRecord> = manager.records.iter().filter(|r| r.account == account).collect();
        let result = if records.is_empty() && !manager.artwork.iter().any(|p| p.starts_with(store::account_dir(&account))) {
            store::remove_account_dir(&account);
            Ok(())
        } else {
            store::save(&account, &records)
        };
        if let Err(error) = result {
            log::error!("Unable to save the download catalog: {error}");
            failed = true;
        }
    }
    if failed {
        app.show_error(l10n::t("Unable to save downloads. Check available storage."));
    }
}

/// The client for a library: the active one, or a new one from its saved credentials.
fn client_for(app: &App, account: &str) -> Option<MusicClient> {
    if app.session.active.as_deref() == Some(account)
        && let Some(client) = app.session.client.clone() {
            return Some(client);
        }
    app.session.accounts.iter().find(|a| a.id() == account).map(|a| MusicClient::new(a.credentials.clone()))
}

/// Starts manual (or automatic) downloads of songs in the active library.
pub fn download(app: &mut App, songs: Vec<Song>, automatic: bool) {
    let account = app.downloads.account.clone();
    let Some(client) = app.session.client.clone().filter(|_| !account.is_empty()) else {
        if !automatic {
            app.show_error(l10n::music_error(MusicError::MissingCredentials));
        }
        return;
    };
    let quality = app.settings.streaming_quality;
    let mut covers: Vec<String> = Vec::new();
    let mut started = false;
    for song in songs {
        if song.id.is_empty() {
            continue;
        }
        match OfflineCatalog::decide(&app.downloads.records, &account, &song, quality, automatic) {
            DownloadDecision::Present => {}
            DownloadDecision::Keep(id) => keep_record(app, id),
            DownloadDecision::Start { replace, record } => {
                if let Some(old) = replace {
                    remove_ids(app, &[old]);
                }
                let manager = &mut app.downloads;
                if let Some(cover) = record.song.cover_art.clone()
                    && !covers.contains(&cover) {
                        covers.push(cover);
                    }
                manager.pending.push_back(record.id);
                manager.records.push(*record);
                manager.reindex();
                manager.mark_dirty(&account);
                started = true;
            }
        }
    }
    if started {
        flush(app);
        for cover in covers {
            fetch_artwork(app, &client, &account, &cover);
        }
        tick(app);
    }
}

fn fetch_artwork(app: &mut App, client: &MusicClient, account: &str, cover: &str) {
    let path = store::artwork_path(account, cover);
    if app.downloads.artwork.contains(&path) || !app.downloads.covers_loading.insert(path.clone()) {
        return;
    }
    let url = client.artwork_url(cover, 600);
    let (account, cover) = (account.to_string(), cover.to_string());
    app.tasks.spawn(async move {
        let saved = transfer::save_artwork(url, path.clone()).await;
        update(move |app: &mut App| {
            let manager = &mut app.downloads;
            manager.covers_loading.remove(&path);
            if !saved {
                return;
            }
            let used = manager.records.iter().any(|r| r.account == account && r.song.cover_art.as_deref() == Some(cover.as_str()));
            if used {
                manager.artwork.insert(path);
                // A placeholder may have been cached before the file existed.
                app.artwork.retry_failed();
            } else {
                store::remove_file(&path);
            }
        })
    });
}

fn start_transfer(app: &mut App, id: Uuid) {
    let Some(i) = app.downloads.position(id) else { return };
    let record = app.downloads.records[i].clone();
    if record.state != OfflineState::Queued {
        return;
    }
    let Some(client) = client_for(app, &record.account) else {
        fail(app, id);
        return;
    };
    let url = client.stream_url(record.song.stream_identifier(), app.settings.streaming_quality);
    let progress = Arc::new(Progress::default());
    app.downloads.active.insert(id, progress.clone());
    app.tasks.spawn(async move {
        let outcome = transfer::run(url, record, progress).await;
        update(move |app: &mut App| finish(app, id, outcome))
    });
}

fn finish(app: &mut App, id: Uuid, outcome: Outcome) {
    app.downloads.active.remove(&id);
    let Some(i) = app.downloads.position(id) else {
        // Removed while transferring.
        if let Outcome::Done(done) = outcome {
            store::remove_file(&done.path);
        }
        return;
    };
    match outcome {
        Outcome::Cancelled => {}
        Outcome::Failed => fail(app, id),
        Outcome::Done(done) => {
            let manager = &mut app.downloads;
            let record = &manager.records[i];
            if record.is_automatic() {
                let settings = if record.account == manager.automatic.account {
                    manager.automatic.settings.clone()
                } else {
                    automatic::saved_settings(&record.account)
                };
                if !settings.enabled || !AutomaticDownloads::can_keep(done.bytes, record, &manager.records, settings.limit_bytes()) {
                    store::remove_file(&done.path);
                    let account = record.account.clone();
                    manager.records[i].mark_failed();
                    manager.mark_dirty(&account);
                    manager.revision += 1;
                    if settings.enabled {
                        manager.automatic.transfer_error =
                            Some(l10n::t("Automatic downloads reached the storage limit. Increase the limit or choose fewer playlists."));
                    }
                    return;
                }
            }
            let record = &mut manager.records[i];
            record.file_extension = done.extension;
            record.mark_ready(done.bytes);
            let account = record.account.clone();
            manager.mark_dirty(&account);
            manager.revision += 1;
        }
    }
}

fn fail(app: &mut App, id: Uuid) {
    let manager = &mut app.downloads;
    manager.active.remove(&id);
    let Some(i) = manager.position(id) else { return };
    let record = &mut manager.records[i];
    store::remove_file(&store::part_path(record));
    store::remove_file(&store::file_path(record));
    record.mark_failed();
    let (account, automatic) = (record.account.clone(), record.is_automatic());
    manager.mark_dirty(&account);
    manager.revision += 1;
    let message = l10n::t("Download failed. Check your connection and available storage, then retry.");
    if automatic {
        manager.automatic.transfer_error = Some(message);
    } else if account == manager.account {
        app.show_error(message);
    }
}

/// Removes records: cancels transfers and deletes their files and unused artwork. Never touches the server.
pub fn remove_ids(app: &mut App, ids: &[Uuid]) {
    if ids.is_empty() {
        return;
    }
    let manager = &mut app.downloads;
    for id in ids {
        if let Some(progress) = manager.active.remove(id) {
            progress.cancel();
        }
        manager.pending.retain(|p| p != id);
        let Some(i) = manager.position(*id) else { continue };
        let record = manager.records.remove(i);
        store::remove_file(&store::file_path(&record));
        store::remove_file(&store::part_path(&record));
        if OfflineCatalog::artwork_unused(&manager.records, &record)
            && let Some(cover) = record.song.cover_art.as_deref() {
                let path = store::artwork_path(&record.account, cover);
                if manager.artwork.remove(&path) {
                    store::remove_file(&path);
                }
            }
        manager.dirty.insert(record.account);
    }
    manager.reindex();
    flush(app);
}

/// Turns an automatic download into a permanent manual one.
pub fn keep_record(app: &mut App, id: Uuid) {
    let manager = &mut app.downloads;
    let Some(i) = manager.position(id) else { return };
    manager.records[i].automatic = Some(false);
    let account = manager.records[i].account.clone();
    manager.mark_dirty(&account);
    manager.revision += 1;
}

/// Removes every download of a library and its folder.
pub fn remove_account(app: &mut App, account: &str) {
    if !app.downloads.started {
        store::remove_account_dir(account);
        return;
    }
    let ids = OfflineCatalog::account_records(&app.downloads.records, account);
    remove_ids(app, &ids);
    let dir = store::account_dir(account);
    app.downloads.artwork.retain(|p| !p.starts_with(&dir));
    app.downloads.dirty.remove(account);
    store::remove_account_dir(account);
    automatic::forget(app, account);
}

/// Restarts failed downloads of the active library; completed files are kept.
pub fn retry_failed(app: &mut App) {
    let songs: Vec<(Song, bool)> =
        app.downloads.account_records().filter(|r| r.state == OfflineState::Failed).map(|r| (r.song.clone(), r.is_automatic())).collect();
    let (automatic, manual): (Vec<_>, Vec<_>) = songs.into_iter().partition(|(_, automatic)| *automatic);
    download(app, manual.into_iter().map(|(s, _)| s).collect(), false);
    download(app, automatic.into_iter().map(|(s, _)| s).collect(), true);
}
