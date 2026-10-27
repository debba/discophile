//! Listening positions of podcast episodes and long tracks, kept per library and synced with the server:
//! Subsonic bookmarks, or Jellyfin's resume position. Audiobooks keep their own Audiobookshelf progress.
//!
//! Positions are saved on the device first and uploaded later. A newer server position wins over an unsent
//! local one; a synced position that disappeared from the server was finished or cleared on another device.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::error::{MusicError, MusicResult};
use crate::models::Song;

/// Tracks at least this long remember their position, like podcast episodes.
pub const LONG_TRACK_SECONDS: f64 = 20.0 * 60.0;
/// Positions closer to the start begin again from the start, and are not kept on the server.
pub const MINIMUM_RESUME_SECONDS: f64 = 10.0;
/// The end of an item counts as finished: its last 5%, at most this many seconds.
pub const MAXIMUM_FINISH_MARGIN: f64 = 30.0;
/// Positions kept per library; the oldest synced ones go first.
pub const ARCHIVE_LIMIT: usize = 1_000;
/// Remote positions read at once from Jellyfin, most recently played first.
pub const REMOTE_LIMIT: usize = 500;

/// Whether playback of `song` saves and resumes its position.
pub fn remembers_position(song: &Song) -> bool {
    song.is_podcast() || song.duration.is_some_and(|d| d.is_finite() && d >= LONG_TRACK_SECONDS)
}

/// How close to the end an item of `duration` seconds counts as finished.
pub fn finish_margin(duration: f64) -> f64 {
    (duration * 0.05).min(MAXIMUM_FINISH_MARGIN)
}

/// A saved position. `duration` is zero when unknown; `updated` is Unix milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackProgress {
    pub position: f64,
    #[serde(default)]
    pub duration: f64,
    #[serde(default)]
    pub finished: bool,
    #[serde(default)]
    pub updated: i64,
}

impl PlaybackProgress {
    /// A position reached at `updated`. Non-finite values become zero; the end of the item counts as finished.
    pub fn at(position: f64, duration: f64, updated: i64) -> Self {
        let duration = if duration.is_finite() { duration.max(0.0) } else { 0.0 };
        let mut position = if position.is_finite() { position.max(0.0) } else { 0.0 };
        if duration > 0.0 {
            position = position.min(duration);
        }
        let finished = duration > 0.0 && position >= duration - finish_margin(duration);
        Self { position, duration, finished, updated }
    }

    /// Played to the end, or marked as played.
    pub fn played(duration: f64, updated: i64) -> Self {
        let duration = if duration.is_finite() { duration.max(0.0) } else { 0.0 };
        Self { position: duration, duration, finished: true, updated }
    }

    /// Marked as unplayed: the next playback starts from the beginning.
    pub fn unplayed(duration: f64, updated: i64) -> Self {
        Self { finished: false, position: 0.0, ..Self::played(duration, updated) }
    }

    /// Where playback continues, or `None` to start from the beginning.
    pub fn resume_position(&self) -> Option<f64> {
        (!self.finished && self.position >= MINIMUM_RESUME_SECONDS).then_some(self.position)
    }

    /// Completion in 0...1 for progress bars: finished items are complete, unstarted ones absent.
    pub fn fraction(&self) -> Option<f64> {
        if self.finished {
            return Some(1.0);
        }
        let position = self.resume_position()?;
        (self.duration > 0.0).then(|| (position / self.duration).clamp(0.0, 1.0))
    }

    /// Seconds left from the resume position, when the duration is known.
    pub fn remaining(&self) -> Option<f64> {
        let position = self.resume_position()?;
        (self.duration > 0.0).then(|| (self.duration - position).max(0.0))
    }
}

/// A position the server keeps for an item, keyed by the item's stream identifier.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteProgress {
    pub id: String,
    pub position: f64,
    pub duration: Option<f64>,
    /// Unix milliseconds of the server's last change, when it reports one.
    pub updated: Option<i64>,
}

/// Parses a server timestamp. Values without a time zone are read as UTC.
pub fn parse_timestamp(value: &str) -> Option<i64> {
    let value = value.trim();
    if let Ok(date) = chrono::DateTime::parse_from_rfc3339(value) {
        return Some(date.timestamp_millis());
    }
    chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f").ok().map(|d| d.and_utc().timestamp_millis())
}

/// What an upload asks of the server for a saved position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProgressUpload {
    /// Keep this position, in seconds.
    Save(f64),
    /// Forget the position: the item was finished, marked unplayed, or barely started.
    Clear,
}

impl From<&PlaybackProgress> for ProgressUpload {
    fn from(progress: &PlaybackProgress) -> Self {
        progress.resume_position().map_or(Self::Clear, Self::Save)
    }
}

/// The positions of one library. `pending` names saved positions that wait for upload.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProgressArchive {
    #[serde(default)]
    pub saved: BTreeMap<String, PlaybackProgress>,
    #[serde(default)]
    pub pending: BTreeSet<String>,
}

impl ProgressArchive {
    pub fn decode(data: &[u8]) -> MusicResult<Self> {
        serde_json::from_slice(data).map_err(|_| MusicError::InvalidResponse)
    }

    pub fn encode(&self) -> MusicResult<Vec<u8>> {
        serde_json::to_vec(self).map_err(|_| MusicError::InvalidResponse)
    }

    pub fn get(&self, id: &str) -> Option<&PlaybackProgress> {
        self.saved.get(id)
    }

    pub fn resume_position(&self, id: &str) -> Option<f64> {
        self.saved.get(id).and_then(PlaybackProgress::resume_position)
    }

    pub fn is_pending(&self, id: &str) -> bool {
        self.pending.contains(id)
    }

    /// A position from the player or a played/unplayed mark: saved now and queued for upload.
    pub fn record(&mut self, id: &str, progress: PlaybackProgress) {
        self.saved.insert(id.to_string(), progress);
        self.pending.insert(id.to_string());
        self.trim();
    }

    /// Pending positions, as `(id, progress)`, in a stable order.
    pub fn pending_items(&self) -> Vec<(String, PlaybackProgress)> {
        self.pending.iter().filter_map(|id| self.saved.get(id).map(|p| (id.clone(), *p))).collect()
    }

    /// The upload of `sent` succeeded. A newer position recorded meanwhile stays pending.
    pub fn sent(&mut self, id: &str, sent: &PlaybackProgress) {
        if self.saved.get(id) == Some(sent) {
            self.pending.remove(id);
        }
    }

    /// Merges the complete list of server positions read at `now`.
    ///
    /// A strictly newer server position replaces the local one, even an unsent one. Synced positions the server
    /// no longer has were finished or cleared elsewhere and are forgotten; finished marks stay.
    /// Returns the identifiers whose resume position changed.
    pub fn merge(&mut self, remote: &[RemoteProgress]) -> Vec<String> {
        let mut changed = Vec::new();
        let mut present = BTreeSet::new();
        for item in remote {
            if item.id.is_empty() || !item.position.is_finite() {
                continue;
            }
            present.insert(item.id.clone());
            let local = self.saved.get(&item.id).copied();
            let pending = self.pending.contains(&item.id);
            let newer = match (local, item.updated) {
                (None, _) => true,
                (Some(local), Some(updated)) => if pending { updated > local.updated } else { updated >= local.updated },
                // Without a server time, the server is trusted only for positions already synced.
                (Some(_), None) => !pending,
            };
            if !newer {
                continue;
            }
            let duration = item.duration.filter(|d| d.is_finite() && *d > 0.0).or(local.map(|l| l.duration)).unwrap_or(0.0);
            let progress = PlaybackProgress::at(item.position, duration, item.updated.unwrap_or(0));
            if local.and_then(|l| l.resume_position()) != progress.resume_position() {
                changed.push(item.id.clone());
            }
            self.saved.insert(item.id.clone(), progress);
            self.pending.remove(&item.id);
        }
        let gone: Vec<String> = self
            .saved
            .iter()
            .filter(|(id, p)| !p.finished && !present.contains(*id) && !self.pending.contains(*id))
            .map(|(id, _)| id.clone())
            .collect();
        for id in gone {
            if let Some(progress) = self.saved.remove(&id)
                && progress.resume_position().is_some()
            {
                changed.push(id);
            }
        }
        self.trim();
        changed
    }

    /// Forgets every position (after clearing listening history).
    pub fn clear(&mut self) {
        self.saved.clear();
        self.pending.clear();
    }

    /// Keeps at most `ARCHIVE_LIMIT` positions, dropping the oldest synced ones first.
    fn trim(&mut self) {
        if self.saved.len() <= ARCHIVE_LIMIT {
            return;
        }
        let mut synced: Vec<(i64, String)> =
            self.saved.iter().filter(|(id, _)| !self.pending.contains(*id)).map(|(id, p)| (p.updated, id.clone())).collect();
        synced.sort();
        let excess = self.saved.len() - ARCHIVE_LIMIT;
        for (_, id) in synced.into_iter().take(excess) {
            self.saved.remove(&id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(duration: Option<f64>, podcast: bool) -> Song {
        Song { duration, is_podcast: podcast.then_some(true), ..Song::new("id", "Title") }
    }

    fn remote(id: &str, position: f64, updated: Option<i64>) -> RemoteProgress {
        RemoteProgress { id: id.into(), position, duration: Some(3_000.0), updated }
    }

    #[test]
    fn podcasts_and_long_tracks_remember_their_position() {
        assert!(remembers_position(&song(None, true)));
        assert!(remembers_position(&song(Some(60.0), true)));
        assert!(remembers_position(&song(Some(LONG_TRACK_SECONDS), false)));
        assert!(!remembers_position(&song(Some(LONG_TRACK_SECONDS - 1.0), false)));
        assert!(!remembers_position(&song(None, false)));
        assert!(!remembers_position(&song(Some(f64::INFINITY), false)));
    }

    #[test]
    fn positions_resume_between_the_start_and_the_end() {
        let start = PlaybackProgress::at(MINIMUM_RESUME_SECONDS - 1.0, 3_000.0, 1);
        assert_eq!(start.resume_position(), None);
        assert_eq!(start.fraction(), None);
        let middle = PlaybackProgress::at(1_500.0, 3_000.0, 1);
        assert_eq!(middle.resume_position(), Some(1_500.0));
        assert_eq!(middle.fraction(), Some(0.5));
        assert_eq!(middle.remaining(), Some(1_500.0));
        // The last 30 seconds of a long episode count as finished.
        let end = PlaybackProgress::at(2_975.0, 3_000.0, 1);
        assert!(end.finished);
        assert_eq!(end.resume_position(), None);
        assert_eq!(end.fraction(), Some(1.0));
        // Short episodes use 5% of their length.
        assert!(!PlaybackProgress::at(110.0, 120.0, 1).finished);
        assert!(PlaybackProgress::at(115.0, 120.0, 1).finished);
        // Unknown durations never finish by position, and bad values are sanitized.
        let unknown = PlaybackProgress::at(500.0, f64::NAN, 1);
        assert_eq!((unknown.duration, unknown.finished, unknown.resume_position()), (0.0, false, Some(500.0)));
        assert_eq!(unknown.fraction(), None);
        assert_eq!(PlaybackProgress::at(f64::NAN, 100.0, 1).position, 0.0);
        assert_eq!(PlaybackProgress::at(500.0, 100.0, 1).position, 100.0);
    }

    #[test]
    fn played_and_unplayed_marks() {
        let played = PlaybackProgress::played(600.0, 5);
        assert!(played.finished);
        assert_eq!(ProgressUpload::from(&played), ProgressUpload::Clear);
        let unplayed = PlaybackProgress::unplayed(600.0, 6);
        assert!(!unplayed.finished);
        assert_eq!(unplayed.resume_position(), None);
        assert_eq!(unplayed.fraction(), None);
        assert_eq!(ProgressUpload::from(&unplayed), ProgressUpload::Clear);
        assert_eq!(ProgressUpload::from(&PlaybackProgress::at(90.0, 600.0, 7)), ProgressUpload::Save(90.0));
    }

    #[test]
    fn recorded_positions_wait_for_upload_and_newer_ones_stay_pending() {
        let mut archive = ProgressArchive::default();
        let first = PlaybackProgress::at(100.0, 3_000.0, 10);
        archive.record("episode", first);
        assert_eq!(archive.pending_items(), vec![("episode".to_string(), first)]);
        let second = PlaybackProgress::at(130.0, 3_000.0, 20);
        archive.record("episode", second);
        archive.sent("episode", &first);
        assert!(archive.is_pending("episode"));
        archive.sent("episode", &second);
        assert!(!archive.is_pending("episode"));
        assert_eq!(archive.resume_position("episode"), Some(130.0));
        let decoded = ProgressArchive::decode(&archive.encode().unwrap()).unwrap();
        assert_eq!(decoded, archive);
    }

    #[test]
    fn newer_server_positions_win_and_unsent_local_positions_survive_older_ones() {
        let mut archive = ProgressArchive::default();
        archive.record("unsent", PlaybackProgress::at(200.0, 3_000.0, 1_000));
        archive.record("synced", PlaybackProgress::at(300.0, 3_000.0, 1_000));
        archive.sent("synced", &PlaybackProgress::at(300.0, 3_000.0, 1_000));
        let changed = archive.merge(&[
            remote("unsent", 50.0, Some(900)),
            remote("synced", 400.0, Some(1_000)),
            remote("new", 600.0, Some(500)),
        ]);
        assert_eq!(archive.resume_position("unsent"), Some(200.0));
        assert!(archive.is_pending("unsent"));
        assert_eq!(archive.resume_position("synced"), Some(400.0));
        assert_eq!(archive.resume_position("new"), Some(600.0));
        assert_eq!(changed, vec!["synced".to_string(), "new".to_string()]);

        // A strictly newer position from another device replaces an unsent one.
        archive.merge(&[remote("unsent", 900.0, Some(1_001)), remote("synced", 400.0, Some(1_000))]);
        assert_eq!(archive.resume_position("unsent"), Some(900.0));
        assert!(!archive.is_pending("unsent"));
        assert_eq!(archive.get("unsent").unwrap().updated, 1_001);
    }

    #[test]
    fn untimed_server_positions_only_replace_synced_ones() {
        let mut archive = ProgressArchive::default();
        archive.record("unsent", PlaybackProgress::at(200.0, 3_000.0, 1_000));
        archive.record("synced", PlaybackProgress::at(300.0, 3_000.0, 1_000));
        archive.sent("synced", &PlaybackProgress::at(300.0, 3_000.0, 1_000));
        archive.merge(&[remote("unsent", 50.0, None), remote("synced", 60.0, None)]);
        assert_eq!(archive.resume_position("unsent"), Some(200.0));
        assert_eq!(archive.resume_position("synced"), Some(60.0));
    }

    #[test]
    fn synced_positions_missing_on_the_server_are_forgotten_but_finished_marks_stay() {
        let mut archive = ProgressArchive::default();
        for (id, progress) in [
            ("cleared", PlaybackProgress::at(300.0, 3_000.0, 1)),
            ("finished", PlaybackProgress::played(3_000.0, 1)),
            ("unsent", PlaybackProgress::at(300.0, 3_000.0, 1)),
        ] {
            archive.record(id, progress);
            if id != "unsent" {
                archive.sent(id, &progress);
            }
        }
        let changed = archive.merge(&[]);
        assert_eq!(changed, vec!["cleared".to_string()]);
        assert!(archive.get("cleared").is_none());
        assert!(archive.get("finished").unwrap().finished);
        assert_eq!(archive.resume_position("unsent"), Some(300.0));
    }

    #[test]
    fn remote_durations_fall_back_to_local_ones_and_invalid_entries_are_ignored() {
        let mut archive = ProgressArchive::default();
        archive.record("episode", PlaybackProgress::at(10.0, 1_200.0, 1));
        archive.sent("episode", &PlaybackProgress::at(10.0, 1_200.0, 1));
        archive.merge(&[
            RemoteProgress { id: "episode".into(), position: 1_190.0, duration: None, updated: Some(2) },
            RemoteProgress { id: String::new(), position: 1.0, duration: None, updated: Some(2) },
            RemoteProgress { id: "nan".into(), position: f64::NAN, duration: None, updated: Some(2) },
        ]);
        let progress = archive.get("episode").unwrap();
        assert_eq!(progress.duration, 1_200.0);
        assert!(progress.finished);
        assert_eq!(archive.saved.len(), 1);
    }

    #[test]
    fn the_archive_drops_the_oldest_synced_positions_first() {
        let mut archive = ProgressArchive::default();
        archive.record("unsent", PlaybackProgress::at(100.0, 3_000.0, 0));
        for i in 0..ARCHIVE_LIMIT {
            let progress = PlaybackProgress::at(100.0, 3_000.0, i as i64 + 1);
            archive.record(&format!("item-{i}"), progress);
            archive.sent(&format!("item-{i}"), &progress);
        }
        assert_eq!(archive.saved.len(), ARCHIVE_LIMIT);
        assert!(archive.get("unsent").is_some());
        assert!(archive.get("item-0").is_none());
        assert!(archive.get(&format!("item-{}", ARCHIVE_LIMIT - 1)).is_some());
    }

    #[test]
    fn server_timestamps_with_and_without_time_zones() {
        assert_eq!(parse_timestamp("2024-05-01T10:00:00.000Z"), Some(1_714_557_600_000));
        assert_eq!(parse_timestamp("2024-05-01T12:00:00+02:00"), Some(1_714_557_600_000));
        assert_eq!(parse_timestamp("2024-05-01T10:00:00.1234567Z"), Some(1_714_557_600_123));
        assert_eq!(parse_timestamp("2024-05-01T10:00:00"), Some(1_714_557_600_000));
        assert_eq!(parse_timestamp("yesterday"), None);
    }
}
