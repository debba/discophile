//! The catalog of downloaded songs. The app crate stores it as `catalog.json` next to the audio
//! files, named by [`OfflineRecord::filename`], and reports which files exist.

use std::cmp::Ordering;
use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::md5_hex;
use crate::models::{Album, Credentials, Song, StreamingQuality};

const ALLOWED_EXTENSIONS: [&str; 9] = ["mp3", "m4a", "aac", "flac", "wav", "aiff", "ogg", "opus", "alac"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OfflineState {
    Queued,
    Ready,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OfflineRecord {
    pub id: Uuid,
    /// The account namespace from [`OfflineRecord::account_key`].
    pub account: String,
    pub song: Song,
    #[serde(default)]
    pub album_name: Option<String>,
    pub file_extension: String,
    pub state: OfflineState,
    #[serde(default)]
    pub bytes: i64,
    /// Downloads saved before automatic downloads existed are manual.
    #[serde(default)]
    pub automatic: Option<bool>,
}

impl OfflineRecord {
    pub fn new(account: &str, song: &Song, file_extension: &str, automatic: bool) -> Self {
        Self {
            id: Uuid::new_v4(),
            account: account.to_string(),
            song: song.clone(),
            album_name: song.album.clone(),
            file_extension: safe_extension(file_extension),
            state: OfflineState::Queued,
            bytes: 0,
            automatic: Some(automatic),
        }
    }

    pub fn is_automatic(&self) -> bool {
        self.automatic == Some(true)
    }

    /// The audio file's name: never derived from server data beyond a whitelisted extension.
    pub fn filename(&self) -> String {
        format!("{}.{}", self.id.to_string().to_uppercase(), safe_extension(&self.file_extension))
    }

    /// The temporary name used while a transfer is being written.
    pub fn incoming_filename(&self) -> String {
        format!("{}.incoming", self.id.to_string().to_uppercase())
    }

    /// A namespace only. No password, token, or authenticated URL is persisted.
    pub fn account_key(credentials: &Credentials) -> String {
        credentials.account_key()
    }

    /// Marks a finished transfer as ready with its actual size.
    pub fn mark_ready(&mut self, bytes: i64) {
        self.state = OfflineState::Ready;
        self.bytes = bytes.max(0);
    }

    pub fn mark_failed(&mut self) {
        self.state = OfflineState::Failed;
        self.bytes = 0;
    }
}

fn safe_extension(value: &str) -> String {
    let lower = value.to_lowercase();
    if ALLOWED_EXTENSIONS.contains(&lower.as_str()) { lower } else { "audio".into() }
}

/// The file extension for a download in the given quality: transcodes are MP3.
pub fn file_extension(song: &Song, quality: StreamingQuality) -> String {
    if quality == StreamingQuality::Original {
        song.suffix.clone().unwrap_or_else(|| "audio".into())
    } else {
        "mp3".into()
    }
}

/// Rejects error pages served instead of audio.
pub fn is_audio_response(status: u16, content_type: Option<&str>) -> bool {
    let mime = content_type.unwrap_or_default().to_ascii_lowercase();
    (200..300).contains(&status) && !mime.contains("json") && !mime.contains("xml") && !mime.contains("html")
}

/// The artwork file kept next to downloads for a cover, per account.
pub fn artwork_filename(account: &str, cover: &str) -> String {
    format!("art-{}.jpg", md5_hex(format!("{account}{cover}").as_bytes()))
}

/// What to do when the user (or the automatic planner) asks to download a song.
#[derive(Debug, Clone, PartialEq)]
pub enum DownloadDecision {
    /// Already downloaded or downloading.
    Present,
    /// An automatic download the user asked to keep: clear its automatic flag.
    Keep(Uuid),
    /// Start a transfer for `record`, removing a failed previous attempt first.
    Start { replace: Option<Uuid>, record: Box<OfflineRecord> },
}

/// Catalog operations. Records of every account share one catalog.
pub struct OfflineCatalog;

impl OfflineCatalog {
    /// Reconciles a loaded catalog: ready records whose file is gone, and queued records without a
    /// running transfer, become failed.
    pub fn restore(
        records: &mut [OfflineRecord],
        file_exists: impl Fn(&str) -> bool,
        transferring: impl Fn(Uuid) -> bool,
    ) {
        for record in records.iter_mut() {
            match record.state {
                OfflineState::Ready if !file_exists(&record.filename()) => record.mark_failed(),
                OfflineState::Queued if !transferring(record.id) => record.state = OfflineState::Failed,
                _ => {}
            }
        }
    }

    pub fn find<'a>(records: &'a [OfflineRecord], account: &str, song_id: &str) -> Option<&'a OfflineRecord> {
        records.iter().find(|r| r.account == account && r.song.id == song_id)
    }

    /// The downloaded record for a song, when its file is ready.
    pub fn ready<'a>(records: &'a [OfflineRecord], account: &str, song_id: &str) -> Option<&'a OfflineRecord> {
        Self::find(records, account, song_id).filter(|r| r.state == OfflineState::Ready)
    }

    pub fn songs(records: &[OfflineRecord], account: &str) -> Vec<Song> {
        records
            .iter()
            .filter(|r| r.account == account && r.state == OfflineState::Ready)
            .map(|r| r.song.clone())
            .collect()
    }

    /// Albums of downloaded music (no podcasts), sorted by name as Finder sorts.
    pub fn albums(records: &[OfflineRecord], account: &str) -> Vec<Album> {
        let mut groups: Vec<(String, Vec<Song>)> = Vec::new();
        let mut index: HashMap<String, usize> = HashMap::new();
        for song in Self::songs(records, account).into_iter().filter(|s| !s.is_podcast()) {
            let id = song
                .album_id
                .clone()
                .unwrap_or_else(|| format!("unknown:{}", song.album.as_deref().unwrap_or_default()));
            match index.get(&id) {
                Some(&i) => groups[i].1.push(song),
                None => {
                    index.insert(id.clone(), groups.len());
                    groups.push((id, vec![song]));
                }
            }
        }
        let mut albums: Vec<Album> = groups
            .into_iter()
            .map(|(id, songs)| {
                let first = &songs[0];
                Album {
                    cover_art: first.cover_art.clone(),
                    ..Album::new(id, first.album.clone().unwrap_or_default(), first.artist.clone().unwrap_or_default())
                }
            })
            .collect();
        albums.sort_by(|a, b| natural_cmp(&a.name, &b.name));
        albums
    }

    pub fn total_bytes(records: &[OfflineRecord], account: &str) -> i64 {
        records.iter().filter(|r| r.account == account && r.state == OfflineState::Ready).map(|r| r.bytes).sum()
    }

    /// Queued automatic transfers of an account, cancelled when its settings change.
    pub fn queued_automatic(records: &[OfflineRecord], account: &str) -> Vec<Uuid> {
        records
            .iter()
            .filter(|r| r.account == account && r.is_automatic() && r.state == OfflineState::Queued)
            .map(|r| r.id)
            .collect()
    }

    /// Records of an account, for removing its downloads.
    pub fn account_records(records: &[OfflineRecord], account: &str) -> Vec<Uuid> {
        records.iter().filter(|r| r.account == account).map(|r| r.id).collect()
    }

    /// Whether removing `record` leaves its cover unused, so its artwork file can be deleted.
    pub fn artwork_unused(records: &[OfflineRecord], removed: &OfflineRecord) -> bool {
        let Some(cover) = removed.song.cover_art.as_deref() else { return false };
        !records
            .iter()
            .any(|r| r.id != removed.id && r.account == removed.account && r.song.cover_art.as_deref() == Some(cover))
    }

    pub fn decide(
        records: &[OfflineRecord],
        account: &str,
        song: &Song,
        quality: StreamingQuality,
        automatic: bool,
    ) -> DownloadDecision {
        let mut replace = None;
        if let Some(record) = Self::find(records, account, &song.id) {
            if matches!(record.state, OfflineState::Ready | OfflineState::Queued) {
                return if !automatic && record.is_automatic() {
                    DownloadDecision::Keep(record.id)
                } else {
                    DownloadDecision::Present
                };
            }
            replace = Some(record.id);
        }
        DownloadDecision::Start {
            replace,
            record: Box::new(OfflineRecord::new(account, song, &file_extension(song, quality), automatic)),
        }
    }
}

/// Case-insensitive comparison treating digit runs as numbers, like `localizedStandardCompare`.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut digits = String::new();
                    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
                        digits.push(c);
                        it.next();
                    }
                    digits.trim_start_matches('0').to_string()
                };
                let (left, right) = (take(&mut a), take(&mut b));
                let order = left.len().cmp(&right.len()).then_with(|| left.cmp(&right));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(x), Some(y)) => {
                let order =
                    crate::listening_history::fold(&x.to_string()).cmp(&crate::listening_history::fold(&y.to_string()));
                if order != Ordering::Equal {
                    return order;
                }
                a.next();
                b.next();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{MusicProvider, ServerAccount};

    #[test]
    fn offline_catalog_separates_accounts_and_detects_missing_files() {
        let song = Song {
            artist: Some("Artist".into()),
            album: Some("Album".into()),
            album_id: Some("a1".into()),
            ..Song::new("1", "One")
        };
        let mut record = OfflineRecord::new("account-a", &song, "../../bad", false);
        assert!(!record.filename().contains('/'));
        assert!(record.filename().ends_with(".audio"));
        record.state = OfflineState::Ready;
        let mut records = vec![record.clone()];
        OfflineCatalog::restore(&mut records, |_| false, |_| false);
        assert_eq!(records[0].state, OfflineState::Failed);
        let mut records = vec![record.clone()];
        let name = record.filename();
        OfflineCatalog::restore(&mut records, |file| file == name, |_| false);
        assert_eq!(records[0].state, OfflineState::Ready);
        assert_eq!(OfflineCatalog::songs(&records, "account-a"), [song]);
        assert!(OfflineCatalog::songs(&records, "account-b").is_empty());
        assert_eq!(OfflineCatalog::albums(&records, "account-a")[0].id, "a1");
        let json = serde_json::to_string(&records).unwrap();
        assert!(!json.contains("password") && !json.contains("https://"));
    }

    #[test]
    fn queued_records_without_transfer_fail_on_restore() {
        let mut records = vec![OfflineRecord::new("a", &Song::new("1", "One"), "mp3", false)];
        let id = records[0].id;
        OfflineCatalog::restore(&mut records, |_| true, |t| t == id);
        assert_eq!(records[0].state, OfflineState::Queued);
        OfflineCatalog::restore(&mut records, |_| true, |_| false);
        assert_eq!(records[0].state, OfflineState::Failed);
    }

    #[test]
    fn download_namespace_does_not_depend_on_password() {
        let first = Credentials::simple("https://example.com", "u", "first", false, MusicProvider::Subsonic).unwrap();
        let changed_password =
            Credentials::simple("https://example.com", "u", "second", false, MusicProvider::Subsonic).unwrap();
        let changed_user =
            Credentials::simple("https://example.com", "other", "first", false, MusicProvider::Subsonic).unwrap();
        assert_eq!(OfflineRecord::account_key(&first), OfflineRecord::account_key(&changed_password));
        assert_ne!(OfflineRecord::account_key(&first), OfflineRecord::account_key(&changed_user));
    }

    #[test]
    fn identical_song_ids_remain_separate_across_server_libraries() {
        let home =
            Credentials::simple("https://example.com/home", "user", "fixture", false, MusicProvider::Subsonic).unwrap();
        let work =
            Credentials::simple("https://example.com/work", "user", "fixture", false, MusicProvider::Subsonic).unwrap();
        let (a, b) = (ServerAccount::new(home, ""), ServerAccount::new(work, ""));
        assert_ne!(a.id(), b.id());
        let mut one = OfflineRecord::new(&a.id(), &Song::new("1", "Home track"), "mp3", false);
        let mut two = OfflineRecord::new(&b.id(), &Song::new("1", "Work track"), "mp3", false);
        one.state = OfflineState::Ready;
        two.state = OfflineState::Ready;
        assert_ne!(one.filename(), two.filename());
        let records = [one, two];
        assert_eq!(OfflineCatalog::songs(&records, &a.id())[0].title, "Home track");
        assert_eq!(OfflineCatalog::songs(&records, &b.id())[0].title, "Work track");
    }

    #[test]
    fn manual_request_keeps_automatic_download_and_failed_ones_restart() {
        let song = Song { suffix: Some("FLAC".into()), ..Song::new("1", "One") };
        let mut auto = OfflineRecord::new("a", &song, "mp3", true);
        auto.mark_ready(10);
        assert_eq!(
            OfflineCatalog::decide(&[auto.clone()], "a", &song, StreamingQuality::High, false),
            DownloadDecision::Keep(auto.id)
        );
        assert_eq!(
            OfflineCatalog::decide(&[auto.clone()], "a", &song, StreamingQuality::High, true),
            DownloadDecision::Present
        );
        auto.mark_failed();
        match OfflineCatalog::decide(&[auto.clone()], "a", &song, StreamingQuality::Original, false) {
            DownloadDecision::Start { replace, record } => {
                assert_eq!(replace, Some(auto.id));
                assert_eq!(record.file_extension, "flac");
                assert!(!record.is_automatic());
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn legacy_records_decode_as_manual_and_helpers() {
        let json = r#"{"id":"6F9619FF-8B86-D011-B42D-00CF4FC964FF","account":"a","song":{"id":"1","title":"One"},"fileExtension":"mp3","state":"ready","bytes":5}"#;
        let record: OfflineRecord = serde_json::from_str(json).unwrap();
        assert!(!record.is_automatic());
        assert_eq!(record.filename(), "6F9619FF-8B86-D011-B42D-00CF4FC964FF.mp3");
        assert!(is_audio_response(200, Some("audio/mpeg")));
        assert!(!is_audio_response(200, Some("text/html; charset=utf-8")));
        assert!(!is_audio_response(302, None));
        assert_eq!(natural_cmp("Album 2", "album 10"), Ordering::Less);
        assert_eq!(natural_cmp("Été", "ete"), Ordering::Equal);
        assert!(artwork_filename("a", "cover").starts_with("art-"));
    }
}
