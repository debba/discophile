//! Automatic downloads on unmetered networks: chosen playlists plus bounded mixes, within a byte budget.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::curated_mix::{CuratedMix, fitting};
use crate::listening_history::{DiscoveryMix, ListeningHistory, unique};
use crate::models::{Song, StreamingQuality};
use crate::offline_catalog::{OfflineRecord, OfflineState};

/// Automatic downloads are re-planned at most hourly unless forced.
pub const REFRESH_INTERVAL_MILLIS: i64 = 3_600_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AutomaticDownloadSettings {
    pub enabled: bool,
    pub include_mixes: bool,
    pub minutes: i64,
    #[serde(rename = "limitMB")]
    pub limit_mb: i64,
    #[serde(rename = "playlistIDs")]
    pub playlist_ids: Vec<String>,
}

impl Default for AutomaticDownloadSettings {
    fn default() -> Self {
        Self { enabled: false, include_mixes: true, minutes: 120, limit_mb: 512, playlist_ids: Vec::new() }
    }
}

impl AutomaticDownloadSettings {
    /// The storage limit, clamped to 128 MB – 4 GB.
    pub fn limit_bytes(&self) -> i64 {
        self.limit_mb.clamp(128, 4_096) * 1_024 * 1_024
    }

    /// The listening time of mixes to keep, clamped to 30 – 240 minutes.
    pub fn target_seconds(&self) -> f64 {
        self.minutes.clamp(30, 240) as f64 * 60.0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AutomaticDownloadPlan {
    /// Automatic records to delete.
    pub remove: Vec<Uuid>,
    /// Songs to start downloading automatically.
    pub download: Vec<Song>,
    pub reserved_bytes: i64,
}

pub struct AutomaticDownloads;

impl AutomaticDownloads {
    pub fn refresh_due(last: Option<i64>, now: i64, force: bool) -> bool {
        force || last.is_none_or(|last| now - last >= REFRESH_INTERVAL_MILLIS)
    }

    /// Mix songs in priority order: curated mixes, unheard, worth another listen, then the rest of the pool.
    pub fn mix_songs(curated: &[CuratedMix], pool: &[Song], history: &ListeningHistory, now: i64) -> Vec<Song> {
        let mut songs: Vec<Song> = curated.iter().flat_map(|m| m.songs.iter().cloned()).collect();
        songs.extend(DiscoveryMix::Unheard.songs(pool, history, now));
        songs.extend(DiscoveryMix::Rediscover.songs(pool, history, now));
        songs.extend(pool.iter().cloned());
        songs
    }

    /// Playlist songs first, then mixes up to the configured listening time.
    pub fn desired(playlists: &[Song], mixes: &[Song], settings: &AutomaticDownloadSettings) -> Vec<Song> {
        let mixes = if settings.include_mixes { fitting(mixes, settings.target_seconds()) } else { Vec::new() };
        unique(playlists.iter().chain(&mixes))
    }

    /// Whether a finished automatic transfer of `bytes` fits beside the account's other ready automatic files.
    pub fn can_keep(bytes: i64, record: &OfflineRecord, records: &[OfflineRecord], limit_bytes: i64) -> bool {
        let used: i64 = records
            .iter()
            .filter(|r| {
                r.account == record.account && r.is_automatic() && r.state == OfflineState::Ready && r.id != record.id
            })
            .map(|r| r.bytes.max(0))
            .sum();
        bytes > 0 && bytes <= (limit_bytes - used).max(0)
    }

    /// A conservative size estimate. Actual sizes are checked again before keeping a transfer.
    pub fn estimate(song: &Song, quality: StreamingQuality) -> i64 {
        let duration = song.duration.filter(|d| d.is_finite() && *d > 0.0).unwrap_or(300.0);
        let bitrate = match quality {
            StreamingQuality::Compact => 128.0,
            StreamingQuality::High => 320.0,
            StreamingQuality::Original => song.bit_rate.unwrap_or(1_411).max(1_411) as f64,
        };
        (duration * bitrate * 125.0 * 1.15 + 262_144.0).min((i64::MAX / 4) as f64) as i64
    }

    /// Plans which automatic downloads to keep, remove and start. Manual downloads are never
    /// removed and use no automatic budget; the song playing now is protected.
    pub fn plan(
        desired: &[Song],
        records: &[OfflineRecord],
        account: &str,
        quality: StreamingQuality,
        limit_bytes: i64,
        protected_song_id: Option<&str>,
    ) -> AutomaticDownloadPlan {
        let records: Vec<&OfflineRecord> = records.iter().filter(|r| r.account == account).collect();
        let manual: HashSet<&str> = records
            .iter()
            .filter(|r| !r.is_automatic() && r.state != OfflineState::Failed)
            .map(|r| r.song.stream_identifier())
            .collect();
        let by_song: HashMap<&str, &OfflineRecord> = records.iter().map(|r| (r.song.stream_identifier(), *r)).collect();
        let mut keep: HashSet<Uuid> = HashSet::new();
        let mut download = Vec::new();
        let mut reserved: i64 = 0;
        if let Some(protected) = records.iter().find(|r| {
            r.is_automatic() && r.state == OfflineState::Ready && Some(r.song.stream_identifier()) == protected_song_id
        }) {
            keep.insert(protected.id);
            reserved += protected.bytes;
        }
        for song in unique(desired) {
            if manual.contains(song.stream_identifier()) {
                continue;
            }
            let existing = by_song.get(song.stream_identifier()).copied();
            if existing.is_some_and(|e| keep.contains(&e.id)) {
                continue;
            }
            let bytes = match existing {
                Some(e) if e.state == OfflineState::Ready => e.bytes.max(0),
                _ => Self::estimate(&song, quality),
            };
            if bytes > (limit_bytes - reserved).max(0) {
                continue;
            }
            reserved += bytes;
            match existing {
                Some(e) if e.is_automatic() && e.state != OfflineState::Failed => {
                    keep.insert(e.id);
                }
                _ => download.push(song),
            }
        }
        let remove = records.iter().filter(|r| r.is_automatic() && !keep.contains(&r.id)).map(|r| r.id).collect();
        AutomaticDownloadPlan { remove, download, reserved_bytes: reserved }
    }

    /// Whether some desired songs are neither downloaded nor downloading after applying a plan, so the
    /// UI can explain that they do not fit within the limit.
    pub fn some_do_not_fit(desired: &[Song], records: &[OfflineRecord], account: &str) -> bool {
        let available: HashSet<&str> = records
            .iter()
            .filter(|r| r.account == account && r.state != OfflineState::Failed)
            .map(|r| r.song.stream_identifier())
            .collect();
        desired.iter().any(|s| !available.contains(s.stream_identifier()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: &str, account: &str, automatic: bool, bytes: i64) -> OfflineRecord {
        let mut record =
            OfflineRecord::new(account, &Song { duration: Some(180.0), ..Song::new(id, id) }, "mp3", automatic);
        record.state = OfflineState::Ready;
        record.bytes = bytes;
        record
    }
    fn auto(id: &str) -> OfflineRecord {
        record(id, "library", true, 10_000_000)
    }

    #[test]
    fn legacy_downloads_decode_as_manual_and_survive_automatic_eviction() {
        let mut json = serde_json::to_value(record("manual", "library", false, 10_000_000)).unwrap();
        json.as_object_mut().unwrap().remove("automatic");
        let legacy: OfflineRecord = serde_json::from_value(json).unwrap();
        assert!(!legacy.is_automatic());
        let stale = auto("stale");
        let other = record("other", "another", true, 10_000_000);
        let plan = AutomaticDownloads::plan(
            &[],
            &[legacy, stale.clone(), other],
            "library",
            StreamingQuality::High,
            100_000_000,
            None,
        );
        assert_eq!(plan.remove, [stale.id]);
        assert!(plan.download.is_empty());
    }

    #[test]
    fn plan_prioritizes_desired_songs_and_stays_within_reserved_budget() {
        let first = Song { duration: Some(180.0), ..Song::new("first", "First") };
        let second = Song { duration: Some(180.0), ..Song::new("second", "Second") };
        let bytes = AutomaticDownloads::estimate(&first, StreamingQuality::High);
        let plan = AutomaticDownloads::plan(
            &[first.clone(), first.clone(), second],
            &[],
            "library",
            StreamingQuality::High,
            bytes,
            None,
        );
        assert_eq!(plan.download.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["first"]);
        assert_eq!(plan.reserved_bytes, bytes);
        assert!(AutomaticDownloads::estimate(&first, StreamingQuality::Original) > bytes);
    }

    #[test]
    fn playlist_tracks_precede_bounded_mixes_and_deduplicate_across_sources() {
        let mut settings = AutomaticDownloadSettings { minutes: 30, ..Default::default() };
        let songs: Vec<Song> =
            (0..20).map(|i| Song { duration: Some(180.0), ..Song::new(i.to_string(), i.to_string()) }).collect();
        let desired = AutomaticDownloads::desired(&songs[15..16], &songs, &settings);
        assert_eq!(desired[0].id, "15");
        assert_eq!(desired.len(), 11);
        settings.include_mixes = false;
        let desired = AutomaticDownloads::desired(&songs[15..16], &songs, &settings);
        assert_eq!(desired.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["15"]);
    }

    #[test]
    fn current_automatic_track_is_protected_and_manual_tracks_consume_no_automatic_budget() {
        let current = auto("current");
        let manual = record("manual", "library", false, 900_000_000);
        let stale = auto("stale");
        let plan = AutomaticDownloads::plan(
            std::slice::from_ref(&manual.song),
            &[current.clone(), manual.clone(), stale.clone()],
            "library",
            StreamingQuality::Compact,
            1,
            Some("current"),
        );
        assert_eq!(plan.remove, [stale.id]);
        assert!(plan.download.is_empty());
        assert_eq!(plan.reserved_bytes, current.bytes);
    }

    #[test]
    fn actual_size_check_rejects_larger_than_reserved_transfers_and_ignores_other_accounts() {
        let existing = record("ready", "library", true, 70);
        let incoming = record("incoming", "library", true, 0);
        let manual = record("manual", "library", false, 10_000);
        let other = record("other", "another", true, 10_000);
        assert!(AutomaticDownloads::can_keep(30, &incoming, &[existing.clone(), manual, other], 100));
        assert!(!AutomaticDownloads::can_keep(31, &incoming, &[existing], 100));
        assert!(!AutomaticDownloads::can_keep(0, &incoming, &[], 100));
    }

    #[test]
    fn failed_automatic_track_is_replaced_and_settings_round_trip_is_opt_in() {
        let mut failed = auto("failed");
        failed.state = OfflineState::Failed;
        let plan = AutomaticDownloads::plan(
            &[failed.song.clone()],
            &[failed.clone()],
            "library",
            StreamingQuality::High,
            100_000_000,
            None,
        );
        assert_eq!(plan.remove, [failed.id]);
        assert_eq!(plan.download, [failed.song.clone()]);
        let mut settings = AutomaticDownloadSettings::default();
        assert!(!settings.enabled);
        settings.enabled = true;
        settings.playlist_ids = vec!["one".into(), "two".into()];
        settings.minutes = 60;
        let restored: AutomaticDownloadSettings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(restored, settings);
        assert_eq!(AutomaticDownloadSettings { limit_mb: 1, ..Default::default() }.limit_bytes(), 128 * 1_048_576);
        assert_eq!(AutomaticDownloadSettings { minutes: 999, ..Default::default() }.target_seconds(), 240.0 * 60.0);
    }

    #[test]
    fn missing_songs_are_reported_after_planning() {
        let ready = auto("ready");
        assert!(!AutomaticDownloads::some_do_not_fit(
            std::slice::from_ref(&ready.song),
            std::slice::from_ref(&ready),
            "library"
        ));
        assert!(AutomaticDownloads::some_do_not_fit(&[Song::new("x", "x")], &[ready], "library"));
        assert!(AutomaticDownloads::refresh_due(Some(0), REFRESH_INTERVAL_MILLIS, false));
        assert!(!AutomaticDownloads::refresh_due(Some(1), REFRESH_INTERVAL_MILLIS, false));
    }
}
