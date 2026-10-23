//! Downloaded audiobooks (desktop): complete books under `<data>/audiobooks/<account>/`, with the
//! core catalog bookkeeping, two transfers at a time, progress, cancellation and retry that keeps
//! completed files. Signed file URLs live only in the transfer queue, never in the catalog.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use discophile_core::l10n::t;
use discophile_core::offline_audiobook::{
    CATALOG_FILENAME, OfflineAudiobook, can_start_download, decode_catalog, encode_catalog, find_file, find_record, prepare_download,
};
use url::Url;
use uuid::Uuid;

use crate::app::App;
use crate::platform::storage;
use crate::tasks::{Tasks, update};

const MAX_ACTIVE: usize = 2;
/// Progress reports from a transfer are throttled to this interval.
const PROGRESS_INTERVAL_MS: i64 = 250;

#[derive(Default)]
pub struct Downloads {
    /// Records of the connected account.
    pub records: Vec<OfflineAudiobook>,
    /// Transfer progress (0–1) by file.
    pub progress: HashMap<Uuid, f64>,
    /// Books being prepared, with the preparation's generation.
    pub preparing: HashMap<String, u64>,
    pub error: Option<String>,
    account: Option<String>,
    queue: VecDeque<(Uuid, Url)>,
    active: HashMap<Uuid, Arc<AtomicBool>>,
    generation: u64,
}

fn directory(account: &str) -> PathBuf {
    storage::data_dir().join("audiobooks").join(account)
}

fn failed_message() -> String {
    t("Audiobook download failed. Check your connection and storage, then retry. Completed files are kept.")
}

/// Reads the connected account's catalog. Transfers interrupted by quitting become failed files
/// that Retry downloads again.
pub fn load(app: &mut App) {
    cancel_all(&mut app.audiobooks.downloads);
    let account = app.audiobooks.account.as_ref().map(|a| a.id());
    let downloads = &mut app.audiobooks.downloads;
    downloads.records.clear();
    downloads.progress.clear();
    downloads.preparing.clear();
    downloads.error = None;
    downloads.account = account.clone();
    let Some(account) = account else { return };
    let dir = directory(&account);
    let Ok(data) = std::fs::read(dir.join(CATALOG_FILENAME)) else { return };
    let size_of = |name: &str| std::fs::metadata(dir.join(name)).ok().map(|m| m.len());
    match decode_catalog(&data, size_of) {
        Ok(mut records) => {
            for record in &mut records {
                record.restore_queued(|_| false, |_| false);
                for file in &record.files {
                    let _ = std::fs::remove_file(dir.join(file.incoming_filename()));
                }
            }
            downloads.records = records;
            persist(downloads);
        }
        Err(_) => downloads.error = Some(t("Unable to read downloaded audiobooks. Check available storage.")),
    }
}

fn persist(downloads: &mut Downloads) -> bool {
    let Some(account) = downloads.account.clone() else { return false };
    let dir = directory(&account);
    let result = encode_catalog(&downloads.records).map_err(|_| ()).and_then(|bytes| {
        std::fs::create_dir_all(&dir).map_err(|_| ())?;
        let temporary = dir.join(format!("{CATALOG_FILENAME}.tmp"));
        std::fs::write(&temporary, bytes).and_then(|_| std::fs::rename(&temporary, dir.join(CATALOG_FILENAME))).map_err(|_| ())
    });
    if result.is_err() {
        downloads.error = Some(t("Unable to save audiobook downloads. Check available storage."));
    }
    result.is_ok()
}

fn account_of(app: &App) -> Option<String> {
    app.audiobooks.account.as_ref().map(|a| a.id())
}

pub fn record<'a>(app: &'a App, book: &str) -> Option<&'a OfflineAudiobook> {
    let account = account_of(app)?;
    find_record(&app.audiobooks.downloads.records, &account, book)
}

/// A complete download of a book.
pub fn ready_record(app: &App, book: &str) -> Option<OfflineAudiobook> {
    record(app, book).filter(|r| r.ready()).cloned()
}

/// The downloaded file for track `index` of a complete book.
pub fn local_path(app: &App, book: &str, index: usize) -> Option<PathBuf> {
    let record = record(app, book)?;
    let path = directory(&record.account).join(record.local_filename(index)?);
    path.exists().then_some(path)
}

pub fn artwork_path(app: &App, book: &str) -> Option<PathBuf> {
    let record = record(app, book)?;
    let path = directory(&record.account).join(record.artwork_filename());
    path.exists().then_some(path)
}

pub fn fraction(app: &App, record: &OfflineAudiobook) -> f64 {
    record.fraction(&app.audiobooks.downloads.progress)
}

pub fn is_preparing(app: &App, book: &str) -> bool {
    app.audiobooks.downloads.preparing.contains_key(book)
}

/// Total bytes and number of complete books for the connected account.
pub fn usage(app: &App) -> (u64, usize) {
    let records = &app.audiobooks.downloads.records;
    (records.iter().map(OfflineAudiobook::bytes).sum(), records.iter().filter(|r| r.ready()).count())
}

/// Downloads a complete book, or the files still missing from an earlier attempt.
pub fn download(app: &mut App, book: &str) {
    let (Some(account), Some(client)) = (account_of(app), app.audiobooks.client.clone()) else { return };
    let existing = find_record(&app.audiobooks.downloads.records, &account, book).cloned();
    let downloads = &mut app.audiobooks.downloads;
    if !can_start_download(existing.as_ref()) || downloads.preparing.contains_key(book) {
        return;
    }
    downloads.generation += 1;
    let generation = downloads.generation;
    downloads.preparing.insert(book.to_string(), generation);
    downloads.error = None;
    let book = book.to_string();
    app.tasks.spawn(async move {
        let plan = prepare_download(&client, &account, &book, existing).await;
        update(move |app| {
            if app.audiobooks.downloads.preparing.get(&book) != Some(&generation) {
                return;
            }
            app.audiobooks.downloads.preparing.remove(&book);
            if account_of(app).as_deref() != Some(account.as_str()) {
                return;
            }
            match plan {
                Ok(plan) => {
                    if let Some(remote) = plan.server_progress {
                        super::sync::remember(app, &account, &book, remote);
                    }
                    let dir = directory(&account);
                    let _ = std::fs::create_dir_all(&dir);
                    if let Some(cover) = &plan.cover {
                        let _ = std::fs::write(dir.join(plan.record.artwork_filename()), cover);
                    }
                    let downloads = &mut app.audiobooks.downloads;
                    let record = plan.record;
                    let id = record.id;
                    match downloads.records.iter_mut().find(|r| r.id == id) {
                        Some(existing) => *existing = record,
                        None => downloads.records.push(record),
                    }
                    if !persist(downloads) {
                        downloads.records.retain(|r| r.id != id || r.ready());
                        return;
                    }
                    let Some(record) = downloads.records.iter().find(|r| r.id == id) else { return };
                    for (index, url) in plan.urls {
                        if let Some(file) = record.files.get(index) {
                            downloads.progress.insert(file.id, 0.0);
                            downloads.queue.push_back((file.id, url));
                        }
                    }
                    pump(app);
                }
                Err(_) => {
                    let downloads = &mut app.audiobooks.downloads;
                    let active: Vec<Uuid> = downloads.active.keys().copied().collect();
                    if let Some(record) = downloads.records.iter_mut().find(|r| r.account == account && r.book.id == book) {
                        record.restore_queued(|id| active.contains(&id), |_| false);
                    }
                    persist(downloads);
                    downloads.error = Some(failed_message());
                }
            }
        })
    });
}

fn pump(app: &mut App) {
    let tasks = app.tasks.clone();
    let downloads = &mut app.audiobooks.downloads;
    while downloads.active.len() < MAX_ACTIVE {
        let Some((id, url)) = downloads.queue.pop_front() else { return };
        let Some((i, j)) = find_file(&downloads.records, id) else { continue };
        let record = &downloads.records[i];
        let dir = directory(&record.account);
        let incoming = dir.join(record.files[j].incoming_filename());
        let cancel = Arc::new(AtomicBool::new(false));
        downloads.active.insert(id, cancel.clone());
        let reporter = tasks.clone();
        tasks.spawn(async move {
            let result = transfer(url, &incoming, &cancel, &reporter, id).await;
            update(move |app| finished(app, id, incoming, result, cancel.load(Ordering::Relaxed)))
        });
    }
}

async fn transfer(url: Url, incoming: &std::path::Path, cancel: &AtomicBool, tasks: &Tasks, id: Uuid) -> Result<u64, ()> {
    use tokio::io::AsyncWriteExt;
    let mut response = discophile_core::http::server_client().get(url).send().await.map_err(|_| ())?;
    if !response.status().is_success() {
        return Err(());
    }
    let total = response.content_length().filter(|t| *t > 0);
    if let Some(parent) = incoming.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|_| ())?;
    }
    let mut file = tokio::fs::File::create(incoming).await.map_err(|_| ())?;
    let mut written: u64 = 0;
    let mut reported = 0;
    while let Some(chunk) = response.chunk().await.map_err(|_| ())? {
        if cancel.load(Ordering::Relaxed) {
            return Err(());
        }
        file.write_all(&chunk).await.map_err(|_| ())?;
        written += chunk.len() as u64;
        let now = discophile_core::now_millis();
        if let Some(total) = total
            && now - reported >= PROGRESS_INTERVAL_MS
        {
            reported = now;
            let fraction = (written as f64 / total as f64).clamp(0.0, 1.0);
            tasks.send(update(move |app| {
                if app.audiobooks.downloads.active.contains_key(&id) {
                    app.audiobooks.downloads.progress.insert(id, fraction);
                }
            }));
        }
    }
    file.flush().await.map_err(|_| ())?;
    if total.is_some_and(|t| t != written) {
        return Err(());
    }
    Ok(written)
}

fn finished(app: &mut App, id: Uuid, incoming: PathBuf, result: Result<u64, ()>, cancelled: bool) {
    let downloads = &mut app.audiobooks.downloads;
    let tracked = downloads.active.remove(&id).is_some();
    downloads.progress.remove(&id);
    let found = find_file(&downloads.records, id);
    let (Some((i, j)), false, true) = (found, cancelled, tracked) else {
        let _ = std::fs::remove_file(&incoming);
        pump(app);
        return;
    };
    let record = &mut downloads.records[i];
    let destination = directory(&record.account).join(&record.files[j].filename);
    let moved = result.ok().and_then(|_| {
        let _ = std::fs::remove_file(&destination);
        std::fs::rename(&incoming, &destination).ok()?;
        std::fs::metadata(&destination).ok().map(|m| m.len())
    });
    match moved {
        Some(bytes) if bytes > 0 => {
            record.complete_file(id, bytes);
        }
        _ => {
            record.fail_file(id);
            let _ = std::fs::remove_file(&incoming);
            let _ = std::fs::remove_file(&destination);
            downloads.error = Some(failed_message());
        }
    }
    let ready = record.ready().then(|| record.book.id.clone());
    persist(downloads);
    if let Some(book) = ready {
        super::playback::use_downloaded_if_available(app, &book);
    }
    pump(app);
}

fn cancel_files(downloads: &mut Downloads, files: &[Uuid]) {
    downloads.queue.retain(|(id, _)| !files.contains(id));
    for id in files {
        if let Some(flag) = downloads.active.remove(id) {
            flag.store(true, Ordering::Relaxed);
        }
        downloads.progress.remove(id);
    }
}

fn cancel_all(downloads: &mut Downloads) {
    downloads.queue.clear();
    for (_, flag) in downloads.active.drain() {
        flag.store(true, Ordering::Relaxed);
    }
    downloads.progress.clear();
    downloads.preparing.clear();
}

/// Stops preparing a download and removes what it started.
pub fn cancel_preparation(app: &mut App, book: &str) {
    app.audiobooks.downloads.preparing.remove(book);
    remove(app, book);
}

/// Removes a book's download: transfers, files, artwork and its catalog entry.
pub fn remove(app: &mut App, book: &str) {
    app.audiobooks.downloads.preparing.remove(book);
    let Some(record) = record(app, book).cloned() else { return };
    if app.audiobooks.playback.as_ref().is_some_and(|p| p.offline && p.book.id == book) {
        crate::player::stop(app);
    }
    let downloads = &mut app.audiobooks.downloads;
    let files: Vec<Uuid> = record.files.iter().map(|f| f.id).collect();
    cancel_files(downloads, &files);
    let dir = directory(&record.account);
    for name in record.owned_filenames() {
        let _ = std::fs::remove_file(dir.join(name));
    }
    downloads.records.retain(|r| r.id != record.id);
    persist(downloads);
    pump(app);
}

/// Removes every downloaded book of an account (on disconnect or from Settings).
pub fn remove_account(app: &mut App, account: &str) {
    if app.audiobooks.playback.as_ref().is_some_and(|p| p.offline && p.account == account) {
        crate::player::stop(app);
    }
    let downloads = &mut app.audiobooks.downloads;
    if downloads.account.as_deref() == Some(account) {
        cancel_all(downloads);
        downloads.records.clear();
        downloads.error = None;
    }
    let _ = std::fs::remove_dir_all(directory(account));
}
