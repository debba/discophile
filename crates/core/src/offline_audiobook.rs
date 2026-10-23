//! Downloaded audiobooks and saved listening positions.
//!
//! An [`OfflineAudiobook`] records a book's expanded item, its local playback session and one
//! file per audio track. File names are random UUIDs, so books with the same ID on two accounts
//! never collide, and the catalog holds no token or authenticated URL. Nothing here touches the
//! filesystem: the app reads and writes `catalog.json` and the audio files, passing file sizes in.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

use crate::audiobookshelf::{Audiobook, AudiobookPlaybackSession, AudiobookProgress, AudiobookTrack, AudiobookshelfClient, media_url};
use crate::error::{MusicError, MusicResult};

/// The catalog file inside the downloads directory.
pub const CATALOG_FILENAME: &str = "catalog.json";
/// The saved-position archive file.
pub const PROGRESS_FILENAME: &str = "progress.json";

const SUPPORTED_EXTENSIONS: [&str; 10] = ["mp3", "m4b", "m4a", "aac", "flac", "wav", "aiff", "alac", "ogg", "opus"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OfflineFileState {
    Queued,
    Ready,
    Failed,
}

fn upper(id: Uuid) -> String {
    id.to_string().to_uppercase()
}

fn extension_for(track: &AudiobookTrack) -> &'static str {
    let path = track.content_url.split(['?', '#']).next().unwrap_or_default();
    let name = path.rsplit('/').next().unwrap_or_default();
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    if let Some(known) = SUPPORTED_EXTENSIONS.iter().find(|e| **e == ext) {
        return known;
    }
    match track.mime_type.as_deref().unwrap_or_default() {
        "audio/mpeg" => "mp3",
        "audio/mp4" | "audio/x-m4b" => "m4b",
        "audio/aac" => "aac",
        "audio/flac" => "flac",
        "audio/x-m4a" => "m4a",
        _ => "audio",
    }
}

/// One downloaded audio file of a book.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OfflineAudiobookFile {
    pub id: Uuid,
    pub filename: String,
    pub state: OfflineFileState,
    pub bytes: u64,
}

impl OfflineAudiobookFile {
    fn for_track(track: &AudiobookTrack) -> Self {
        let id = Uuid::new_v4();
        Self { id, filename: format!("{}.{}", upper(id), extension_for(track)), state: OfflineFileState::Queued, bytes: 0 }
    }

    /// The temporary name a transfer writes to before it is moved to `filename`.
    pub fn incoming_filename(&self) -> String {
        format!("{}.incoming", upper(self.id))
    }

    fn has_safe_filename(&self) -> bool {
        let Some((stem, ext)) = self.filename.rsplit_once('.') else { return false };
        stem == upper(self.id) && !ext.is_empty() && ext.bytes().all(|b| b.is_ascii_alphanumeric())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OfflineAudiobook {
    pub id: Uuid,
    /// `AudiobookshelfAccount::id`.
    pub account: String,
    pub book: Audiobook,
    pub playback: AudiobookPlaybackSession,
    pub files: Vec<OfflineAudiobookFile>,
}

impl OfflineAudiobook {
    /// A new download for an expanded book (with tracks); every file starts queued.
    pub fn new(account: &str, book: Audiobook) -> MusicResult<Self> {
        let playback = AudiobookPlaybackSession::downloaded(&book)?;
        let files = playback.audio_tracks.iter().map(OfflineAudiobookFile::for_track).collect();
        let record = Self { id: Uuid::new_v4(), account: account.to_string(), book, playback, files };
        record.validated()?;
        Ok(record)
    }

    /// Rejects records with a broken timeline, foreign or traversing media routes, or file names
    /// that could reach outside the downloads directory.
    pub fn validated(&self) -> MusicResult<()> {
        self.playback.validated()?;
        let placeholder = Url::parse("https://audiobookshelf.invalid").map_err(|_| MusicError::InvalidResponse)?;
        for track in &self.playback.audio_tracks {
            media_url(&track.content_url, &placeholder, "")?;
        }
        let ids: HashSet<Uuid> = self.files.iter().map(|f| f.id).collect();
        if self.files.len() != self.playback.audio_tracks.len()
            || ids.len() != self.files.len()
            || self.book.id != self.playback.library_item_id
            || self.account.is_empty()
            || !self.files.iter().all(OfflineAudiobookFile::has_safe_filename)
        {
            return Err(MusicError::InvalidResponse);
        }
        Ok(())
    }

    /// Every file is downloaded and non-empty.
    pub fn ready(&self) -> bool {
        !self.files.is_empty()
            && self.files.len() == self.playback.audio_tracks.len()
            && self.files.iter().all(|f| f.state == OfflineFileState::Ready && f.bytes > 0)
    }

    pub fn queued(&self) -> bool {
        self.files.iter().any(|f| f.state == OfflineFileState::Queued)
    }

    pub fn bytes(&self) -> u64 {
        self.files.iter().map(|f| f.bytes).sum()
    }

    pub fn artwork_filename(&self) -> String {
        format!("{}.jpg", upper(self.id))
    }

    /// The local file for track `index`, only once the whole book is ready. The app should still
    /// check that the file exists.
    pub fn local_filename(&self, index: usize) -> Option<&str> {
        if !self.ready() {
            return None;
        }
        self.files.get(index).map(|f| f.filename.as_str())
    }

    /// Every name this record may own in the downloads directory, for removal.
    pub fn owned_filenames(&self) -> Vec<String> {
        let mut names: Vec<String> = self.files.iter().flat_map(|f| [f.filename.clone(), f.incoming_filename()]).collect();
        names.push(self.artwork_filename());
        names
    }

    /// Overall completion, counting ready files as 1 and in-flight files by their transfer progress.
    pub fn fraction(&self, progress: &HashMap<Uuid, f64>) -> f64 {
        if self.files.is_empty() {
            return 0.0;
        }
        let total: f64 = self
            .files
            .iter()
            .map(|f| if f.state == OfflineFileState::Ready { 1.0 } else { progress.get(&f.id).copied().unwrap_or(0.0) })
            .sum();
        total / self.files.len() as f64
    }

    /// Indices of tracks that still need a download (queued or failed).
    pub fn missing_tracks(&self) -> Vec<usize> {
        self.files.iter().enumerate().filter(|(_, f)| f.state != OfflineFileState::Ready).map(|(i, _)| i).collect()
    }

    pub fn mark_queued(&mut self, indices: &[usize]) {
        for &i in indices {
            if let Some(file) = self.files.get_mut(i) {
                file.state = OfflineFileState::Queued;
            }
        }
    }

    pub fn file_index(&self, file: Uuid) -> Option<usize> {
        self.files.iter().position(|f| f.id == file)
    }

    /// Records a finished transfer after the app moved it to `filename`. An empty file fails.
    pub fn complete_file(&mut self, file: Uuid, bytes: u64) -> bool {
        let Some(entry) = self.files.iter_mut().find(|f| f.id == file) else { return false };
        if bytes > 0 {
            entry.state = OfflineFileState::Ready;
            entry.bytes = bytes;
        } else {
            entry.state = OfflineFileState::Failed;
            entry.bytes = 0;
        }
        true
    }

    /// Marks a transfer as failed. The app removes the file and its incoming file.
    pub fn fail_file(&mut self, file: Uuid) -> bool {
        let Some(entry) = self.files.iter_mut().find(|f| f.id == file) else { return false };
        entry.state = OfflineFileState::Failed;
        entry.bytes = 0;
        true
    }

    /// After a failed preparation or a relaunch: queued files without a running transfer fail,
    /// unless their incoming file is complete. Returns the files to finish from `incoming_filename`.
    pub fn restore_queued(&mut self, active: impl Fn(Uuid) -> bool, incoming_exists: impl Fn(&str) -> bool) -> Vec<Uuid> {
        let mut finish = Vec::new();
        for file in &mut self.files {
            if file.state != OfflineFileState::Queued || active(file.id) {
                continue;
            }
            if incoming_exists(&file.incoming_filename()) {
                finish.push(file.id);
            } else {
                file.state = OfflineFileState::Failed;
            }
        }
        finish
    }

    /// Ready files whose size on disk is missing or differs become failed (truncated or deleted).
    pub fn verify_files(&mut self, size_of: impl Fn(&str) -> Option<u64>) {
        for file in &mut self.files {
            if file.state != OfflineFileState::Ready {
                continue;
            }
            let bytes = size_of(&file.filename).unwrap_or(0);
            if bytes == 0 || bytes != file.bytes {
                file.state = OfflineFileState::Failed;
                file.bytes = 0;
            }
        }
    }
}

/// Whether a new download may start: there is no complete or in-progress record.
pub fn can_start_download(existing: Option<&OfflineAudiobook>) -> bool {
    existing.is_none_or(|r| !r.ready() && !r.queued())
}

pub fn find_record<'a>(records: &'a [OfflineAudiobook], account: &str, book: &str) -> Option<&'a OfflineAudiobook> {
    records.iter().find(|r| r.account == account && r.book.id == book)
}

pub fn find_record_mut<'a>(records: &'a mut [OfflineAudiobook], account: &str, book: &str) -> Option<&'a mut OfflineAudiobook> {
    records.iter_mut().find(|r| r.account == account && r.book.id == book)
}

/// The record owning a transfer, and the file's index in it.
pub fn find_file(records: &[OfflineAudiobook], file: Uuid) -> Option<(usize, usize)> {
    records.iter().enumerate().find_map(|(i, r)| r.file_index(file).map(|j| (i, j)))
}

/// Removes an account's downloads from the catalog, returning them so the app can delete their files.
pub fn remove_account(records: &mut Vec<OfflineAudiobook>, account: &str) -> Vec<OfflineAudiobook> {
    let (removed, kept) = std::mem::take(records).into_iter().partition(|r| r.account == account);
    *records = kept;
    removed
}

/// Reads `catalog.json`, validating every record and checking ready files against their size on
/// disk (`size_of(filename)`, `None` when missing).
pub fn decode_catalog(data: &[u8], size_of: impl Fn(&str) -> Option<u64>) -> MusicResult<Vec<OfflineAudiobook>> {
    let mut records: Vec<OfflineAudiobook> = serde_json::from_slice(data).map_err(|_| MusicError::InvalidResponse)?;
    for record in &mut records {
        record.validated()?;
        record.verify_files(&size_of);
    }
    Ok(records)
}

/// Serializes the catalog, refusing to write an invalid record.
pub fn encode_catalog(records: &[OfflineAudiobook]) -> MusicResult<Vec<u8>> {
    for record in records {
        record.validated()?;
    }
    serde_json::to_vec(records).map_err(|_| MusicError::InvalidResponse)
}

/// Everything needed to start downloading a book. The signed `urls` must only be handed to the
/// transfer and never persisted.
#[derive(Debug, Clone)]
pub struct AudiobookDownloadPlan {
    /// The record to store (new or existing), with the missing files marked queued.
    pub record: OfflineAudiobook,
    /// The server position, to pass to [`AudiobookProgressArchive::remember`].
    pub server_progress: Option<AudiobookProgress>,
    /// Cover bytes for `record.artwork_filename()`, when available.
    pub cover: Option<Vec<u8>>,
    /// Track index and signed URL for every file to download.
    pub urls: Vec<(usize, Url)>,
}

/// Fetches the expanded book, its position, cover and signed file URLs. An existing incomplete
/// record is reused so completed files are kept.
pub async fn prepare_download(
    client: &AudiobookshelfClient,
    account: &str,
    book: &str,
    existing: Option<OfflineAudiobook>,
) -> MusicResult<AudiobookDownloadPlan> {
    let detail = client.book(book).await?;
    let server_progress = client.progress(book).await?;
    let mut record = match existing {
        Some(record) => record,
        None => OfflineAudiobook::new(account, detail)?,
    };
    let cover = client.cover(book).await.ok();
    let missing = record.missing_tracks();
    let paths: Vec<&str> = missing.iter().map(|&i| record.playback.audio_tracks[i].content_url.as_str()).collect();
    let urls = if paths.is_empty() { Vec::new() } else { client.media_urls(&paths).await? };
    record.mark_queued(&missing);
    Ok(AudiobookDownloadPlan { record, server_progress, cover, urls: missing.into_iter().zip(urls).collect() })
}

// MARK: Saved positions

/// Positions by `"{account}:{item}"`. `saved` is the last known position for offline resume;
/// `pending` holds positions not yet confirmed by the server.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AudiobookProgressArchive {
    pub saved: BTreeMap<String, AudiobookProgress>,
    pub pending: BTreeMap<String, AudiobookProgress>,
}

impl AudiobookProgressArchive {
    pub fn key(account: &str, item: &str) -> String {
        format!("{account}:{item}")
    }

    /// Reads the archive, accepting the older format that stored only pending positions.
    pub fn decode(data: &[u8]) -> MusicResult<Self> {
        if let Ok(archive) = serde_json::from_slice::<Self>(data) {
            return Ok(archive);
        }
        let pending: BTreeMap<String, AudiobookProgress> = serde_json::from_slice(data).map_err(|_| MusicError::InvalidResponse)?;
        Ok(Self { saved: pending.clone(), pending })
    }

    pub fn encode(&self) -> MusicResult<Vec<u8>> {
        serde_json::to_vec(self).map_err(|_| MusicError::InvalidResponse)
    }

    pub fn pending(&self, account: &str, item: &str) -> Option<&AudiobookProgress> {
        self.pending.get(&Self::key(account, item))
    }

    pub fn saved(&self, account: &str, item: &str) -> Option<&AudiobookProgress> {
        self.saved.get(&Self::key(account, item))
    }

    /// A position from the player: saved locally and queued for upload.
    pub fn record(&mut self, account: &str, item: &str, progress: AudiobookProgress) {
        let key = Self::key(account, item);
        self.pending.insert(key.clone(), progress);
        self.saved.insert(key, progress);
    }

    /// A server position seen while browsing or downloading; an unsent newer local one wins.
    pub fn remember(&mut self, account: &str, item: &str, remote: AudiobookProgress) {
        let key = Self::key(account, item);
        if let Some(latest) = AudiobookProgress::resuming(Some(remote), self.pending.get(&key).copied()) {
            self.saved.insert(key, latest);
        }
    }

    /// An upload of `sent` succeeded. A newer position recorded meanwhile stays pending.
    pub fn sent(&mut self, account: &str, item: &str, sent: &AudiobookProgress) {
        let key = Self::key(account, item);
        if self.pending.get(&key) == Some(sent) {
            self.pending.remove(&key);
        }
    }

    /// Pending positions of one account, as `(item, position)`, for retrying.
    pub fn pending_items(&self, account: &str) -> Vec<(String, AudiobookProgress)> {
        let prefix = format!("{account}:");
        self.pending.iter().filter_map(|(k, v)| k.strip_prefix(&prefix).map(|item| (item.to_string(), *v))).collect()
    }

    /// A retried position was reconciled with the server (see
    /// [`AudiobookshelfClient::reconcile_progress`]).
    pub fn reconciled(&mut self, account: &str, item: &str, sent: &AudiobookProgress, latest: AudiobookProgress) {
        let key = Self::key(account, item);
        if self.pending.get(&key) == Some(sent) {
            self.pending.remove(&key);
            self.saved.insert(key, latest);
        }
    }

    /// Before playing: the reconciled position replaces the saved one, and the pending one is
    /// cleared unless a newer position was recorded meanwhile.
    pub fn prepared(&mut self, account: &str, item: &str, sent: Option<&AudiobookProgress>, latest: Option<AudiobookProgress>) {
        let key = Self::key(account, item);
        if let Some(sent) = sent
            && self.pending.get(&key) == Some(sent)
        {
            self.pending.remove(&key);
        }
        match latest {
            Some(latest) => self.saved.insert(key, latest),
            None => self.saved.remove(&key),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"{"id":"book","libraryId":"library","mediaType":"book","media":{"metadata":{"title":"Synthetic book","authors":[{"name":"Synthetic author"}]},"duration":180,"chapters":[{"id":1,"start":0,"end":180,"title":"Synthetic chapter"}],"tracks":[{"startOffset":0,"duration":60,"contentUrl":"/api/items/book/file/1","mimeType":"audio/mpeg"},{"startOffset":60,"duration":120,"contentUrl":"/api/items/book/file/2","mimeType":"audio/mp4"}]}}"#;

    fn book(text: &str) -> Audiobook {
        serde_json::from_str(text).unwrap()
    }

    /// A stand-in for a downloads directory: filename → size.
    fn disk(record: &OfflineAudiobook, sizes: &[Option<u64>]) -> HashMap<String, u64> {
        record.files.iter().zip(sizes).filter_map(|(f, s)| s.map(|s| (f.filename.clone(), s))).collect()
    }

    #[test]
    fn complete_book_requires_every_nonempty_audio_file() {
        let mut download = OfflineAudiobook::new("account", book(FIXTURE)).unwrap();
        assert!(!download.ready());
        assert!(download.queued());
        download.files[0].state = OfflineFileState::Ready;
        download.files[0].bytes = 30;
        assert!(!download.ready());
        download.files[1].state = OfflineFileState::Failed;
        assert!(!download.queued());
        assert!(!download.ready());
        download.files[1].state = OfflineFileState::Ready;
        download.files[1].bytes = 40;
        assert!(download.ready());
        assert_eq!(download.bytes(), 70);
        assert!(download.files[0].filename.ends_with(".mp3"));
        assert!(download.files[1].filename.ends_with(".m4b"));
        let location = download.playback.location(75.0).unwrap();
        assert_eq!((location.index, location.offset), (1, 15.0));
    }

    #[test]
    fn download_metadata_preserves_chapters_and_contains_no_authenticated_urls() {
        let download = OfflineAudiobook::new("account", book(FIXTURE)).unwrap();
        let data = serde_json::to_vec(&download).unwrap();
        let restored: OfflineAudiobook = serde_json::from_slice(&data).unwrap();
        assert_eq!(restored, download);
        assert_eq!(restored.book.author(), "Synthetic author");
        assert_eq!(restored.playback.chapters.as_ref().unwrap()[0].title, "Synthetic chapter");
        let text = String::from_utf8(data).unwrap();
        for private in ["token", "password", "Authorization", "https://"] {
            assert!(!text.contains(private), "{private}");
        }
    }

    #[test]
    fn missing_or_truncated_file_makes_book_incomplete_after_relaunch() {
        for truncate in [false, true] {
            let mut download = OfflineAudiobook::new("account", book(FIXTURE)).unwrap();
            for file in &mut download.files {
                file.state = OfflineFileState::Ready;
                file.bytes = 4;
            }
            let data = encode_catalog(std::slice::from_ref(&download)).unwrap();
            let files = disk(&download, &[Some(4), Some(4)]);
            let ready = decode_catalog(&data, |name| files.get(name).copied()).unwrap();
            assert!(ready[0].ready());
            let files = disk(&download, &[Some(4), if truncate { Some(1) } else { None }]);
            let restored = decode_catalog(&data, |name| files.get(name).copied()).unwrap();
            assert!(!restored[0].ready());
            assert_eq!(restored[0].files[1].state, OfflineFileState::Failed);
            assert_eq!(restored[0].files[0].state, OfflineFileState::Ready);
            assert_eq!(restored[0].files[0].bytes, 4);
        }
    }

    #[test]
    fn book_downloads_with_identical_ids_are_separated_by_account_and_file_names() {
        let first = OfflineAudiobook::new("one", book(FIXTURE)).unwrap();
        let second = OfflineAudiobook::new("two", book(FIXTURE)).unwrap();
        assert_ne!(first.id, second.id);
        let names = |r: &OfflineAudiobook| r.files.iter().map(|f| f.filename.clone()).collect::<Vec<_>>();
        assert_ne!(names(&first), names(&second));
        let data = encode_catalog(&[first, second]).unwrap();
        let restored = decode_catalog(&data, |_| None).unwrap();
        assert_eq!(restored.iter().map(|r| r.account.as_str()).collect::<Vec<_>>(), ["one", "two"]);
    }

    #[test]
    fn download_rejects_signed_foreign_and_traversing_media_urls_before_saving() {
        for path in ["https://other.test/book?token=private", "/api/items/book/file/1?token=private", "/s/../private"] {
            let invalid = FIXTURE.replace("/api/items/book/file/1", path);
            assert!(OfflineAudiobook::new("account", book(&invalid)).is_err(), "{path}");
        }
    }

    #[test]
    fn download_rejects_broken_timelines_and_incomplete_metadata() {
        let overlapping = FIXTURE.replace("\"startOffset\":60", "\"startOffset\":30");
        assert!(OfflineAudiobook::new("account", book(&overlapping)).is_err());
        let minimal = r#"{"id":"book","libraryId":"library","mediaType":"book","media":{"metadata":{"title":"No audio"}}}"#;
        assert_eq!(OfflineAudiobook::new("account", book(minimal)), Err(MusicError::Unsupported));
        assert!(OfflineAudiobook::new("", book(FIXTURE)).is_err());
    }

    #[test]
    fn catalog_rejects_unsafe_file_names_before_using_them() {
        let record = OfflineAudiobook::new("account", book(FIXTURE)).unwrap();
        let mut json = serde_json::to_value(&record).unwrap();
        json["files"][0]["filename"] = "../../another-account.mp3".into();
        let decoded: OfflineAudiobook = serde_json::from_value(json.clone()).unwrap();
        assert!(decoded.validated().is_err());
        assert!(decode_catalog(serde_json::to_vec(&vec![json]).unwrap().as_slice(), |_| None).is_err());
    }

    #[test]
    fn progress_archive_keeps_last_synced_position_for_offline_resume_and_pending_retry() {
        let progress = AudiobookProgress::at(75.0, 180.0, 1000.0);
        let mut archive = AudiobookProgressArchive::default();
        archive.saved.insert("account:book".into(), progress);
        archive.pending.insert("account:book".into(), progress);
        let mut restored = AudiobookProgressArchive::decode(&archive.encode().unwrap()).unwrap();
        restored.pending.remove("account:book");
        let synced = AudiobookProgressArchive::decode(&restored.encode().unwrap()).unwrap();
        assert_eq!(synced.saved["account:book"].current_time, 75.0);
        assert!(synced.pending.is_empty());
        let legacy_map: BTreeMap<String, AudiobookProgress> = [("account:book".to_string(), progress)].into();
        let legacy = AudiobookProgressArchive::decode(&serde_json::to_vec(&legacy_map).unwrap()).unwrap();
        assert_eq!(legacy.saved, legacy.pending);
        assert_eq!(legacy.saved["account:book"], progress);
        assert!(AudiobookProgressArchive::decode(b"[]").is_err());
    }

    #[test]
    fn progress_archive_merge_rules() {
        let mut archive = AudiobookProgressArchive::default();
        let local = AudiobookProgress::at(75.0, 180.0, 1000.0);
        archive.record("acc", "book", local);
        archive.record("other", "book", AudiobookProgress::at(5.0, 180.0, 1000.0));
        // An older server position does not replace an unsent newer local one.
        archive.remember("acc", "book", AudiobookProgress::at(20.0, 180.0, 500.0));
        assert_eq!(archive.saved("acc", "book"), Some(&local));
        let newer = AudiobookProgress::at(20.0, 180.0, 2000.0);
        archive.remember("acc", "book", newer);
        assert_eq!(archive.saved("acc", "book"), Some(&newer));
        assert_eq!(archive.pending_items("acc"), vec![("book".to_string(), local)]);
        // A position recorded while an upload was in flight stays pending.
        let later = AudiobookProgress::at(80.0, 180.0, 3000.0);
        archive.record("acc", "book", later);
        archive.sent("acc", "book", &local);
        assert_eq!(archive.pending("acc", "book"), Some(&later));
        archive.reconciled("acc", "book", &later, later);
        assert!(archive.pending("acc", "book").is_none());
        archive.prepared("acc", "book", None, None);
        assert!(archive.saved("acc", "book").is_none());
        archive.prepared("other", "book", Some(&AudiobookProgress::at(5.0, 180.0, 1000.0)), Some(newer));
        assert!(archive.pending.is_empty());
        assert_eq!(archive.saved("other", "book"), Some(&newer));
    }

    #[test]
    fn transfers_complete_fail_and_restore() {
        let mut record = OfflineAudiobook::new("account", book(FIXTURE)).unwrap();
        assert!(!can_start_download(Some(&record)));
        let (first, second) = (record.files[0].id, record.files[1].id);
        let progress: HashMap<Uuid, f64> = [(second, 0.5)].into();
        assert!(record.complete_file(first, 10));
        assert_eq!(record.fraction(&progress), 0.75);
        assert_eq!(record.local_filename(0), None);
        let incoming = record.files[1].incoming_filename();
        assert_eq!(record.restore_queued(|_| false, |name| name == incoming), vec![second]);
        assert!(record.complete_file(second, 0));
        assert_eq!(record.files[1].state, OfflineFileState::Failed);
        assert!(can_start_download(Some(&record)));
        assert_eq!(record.missing_tracks(), vec![1]);
        record.mark_queued(&[1]);
        assert!(record.queued());
        assert_eq!(record.restore_queued(|_| false, |_| false), Vec::<Uuid>::new());
        assert_eq!(record.files[1].state, OfflineFileState::Failed);
        assert!(record.complete_file(second, 20));
        assert!(record.ready());
        assert_eq!(record.local_filename(1), Some(record.files[1].filename.as_str()));
        assert_eq!(record.owned_filenames().len(), 5);
        let mut records = vec![record, OfflineAudiobook::new("other", book(FIXTURE)).unwrap()];
        assert_eq!(find_file(&records, second), Some((0, 1)));
        assert!(find_record(&records, "other", "book").is_some());
        assert_eq!(remove_account(&mut records, "account").len(), 1);
        assert_eq!(records.len(), 1);
    }
}
