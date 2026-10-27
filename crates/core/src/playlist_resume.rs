//! Where listening stopped in each playlist: the track, by its index and stream identifier, and the position in it.
//! Kept per library on the device. A queue counts as a playlist only while it holds that playlist's tracks in order.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::models::Song;
use crate::playback_progress::MINIMUM_RESUME_SECONDS;

/// Playlists remembered per library; the least recently played go first.
pub const LIMIT: usize = 200;

/// `updated` is Unix milliseconds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistResume {
    pub index: usize,
    pub id: String,
    pub position: f64,
    pub updated: i64,
}

impl PlaylistResume {
    /// The track to continue from in `songs`, the playlist as listed now: the saved index while it still holds the
    /// same track, otherwise the nearest occurrence of that track (the later one on a tie, as playlists mostly grow).
    /// `None` once the track left the playlist.
    pub fn locate(&self, songs: &[Song]) -> Option<usize> {
        if songs.get(self.index).is_some_and(|s| s.stream_identifier() == self.id) {
            return Some(self.index);
        }
        songs
            .iter()
            .enumerate()
            .filter(|(_, s)| s.stream_identifier() == self.id)
            .min_by_key(|(i, _)| (i.abs_diff(self.index), std::cmp::Reverse(*i)))
            .map(|(i, _)| i)
    }

    /// Where playback continues within the track, or `None` to start it from the beginning.
    pub fn resume_position(&self) -> Option<f64> {
        (self.position.is_finite() && self.position >= MINIMUM_RESUME_SECONDS).then_some(self.position)
    }

    /// Whether continuing differs from playing the playlist from the start.
    pub fn is_useful(&self, index: usize) -> bool {
        index > 0 || self.resume_position().is_some()
    }
}

/// Whether `queue` plays exactly the tracks of `playlist`, in order.
pub fn plays(queue: &[Song], playlist: &[Song]) -> bool {
    !playlist.is_empty() && queue.iter().map(Song::stream_identifier).eq(playlist.iter().map(Song::stream_identifier))
}

/// The resume points of one library, by playlist identifier.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PlaylistResumes {
    #[serde(default)]
    pub playlists: BTreeMap<String, PlaylistResume>,
}

impl PlaylistResumes {
    pub fn get(&self, playlist: &str) -> Option<&PlaylistResume> {
        self.playlists.get(playlist)
    }

    pub fn record(&mut self, playlist: &str, resume: PlaylistResume) {
        self.playlists.insert(playlist.to_string(), resume);
        while self.playlists.len() > LIMIT {
            let oldest = self.playlists.iter().min_by_key(|(id, r)| (r.updated, (*id).clone())).map(|(id, _)| id.clone());
            let Some(oldest) = oldest else { break };
            self.playlists.remove(&oldest);
        }
    }

    /// The playlist played to its end: the next playback starts from the beginning.
    pub fn finish(&mut self, playlist: &str) {
        self.playlists.remove(playlist);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn songs(ids: &[&str]) -> Vec<Song> {
        ids.iter().map(|id| Song::new(*id, *id)).collect()
    }

    fn resume(index: usize, id: &str, position: f64) -> PlaylistResume {
        PlaylistResume { index, id: id.into(), position, updated: 1 }
    }

    #[test]
    fn the_saved_track_is_found_after_the_playlist_changes() {
        let list = songs(&["a", "b", "c", "b", "d"]);
        assert_eq!(resume(3, "b", 0.0).locate(&list), Some(3));
        // A track added at the top moves the saved one down; the nearest occurrence wins.
        let moved = songs(&["new", "a", "b", "c", "b", "d"]);
        assert_eq!(resume(3, "b", 0.0).locate(&moved), Some(4));
        assert_eq!(resume(1, "b", 0.0).locate(&moved), Some(2));
        assert_eq!(resume(9, "d", 0.0).locate(&list), Some(4));
        assert_eq!(resume(2, "gone", 0.0).locate(&list), None);
        assert_eq!(resume(0, "a", 0.0).locate(&[]), None);
    }

    #[test]
    fn continuing_is_offered_after_the_first_track_or_into_it() {
        assert!(!resume(0, "a", MINIMUM_RESUME_SECONDS - 1.0).is_useful(0));
        assert!(resume(0, "a", MINIMUM_RESUME_SECONDS).is_useful(0));
        assert!(resume(1, "b", 0.0).is_useful(1));
        assert_eq!(resume(1, "b", 5.0).resume_position(), None);
        assert_eq!(resume(1, "b", 95.0).resume_position(), Some(95.0));
        assert_eq!(resume(1, "b", f64::NAN).resume_position(), None);
    }

    #[test]
    fn only_the_playlist_in_order_counts_as_playing_it() {
        let list = songs(&["a", "b", "c"]);
        assert!(plays(&songs(&["a", "b", "c"]), &list));
        assert!(!plays(&songs(&["b", "a", "c"]), &list));
        assert!(!plays(&songs(&["a", "b", "c", "x"]), &list));
        assert!(!plays(&[], &[]));
        let episode = Song { stream_id: Some("file".into()), ..Song::new("podcast:c:e", "Episode") };
        assert!(plays(&[episode], &[Song::new("file", "Episode")]));
    }

    #[test]
    fn finished_playlists_start_over_and_the_oldest_are_dropped() {
        let mut resumes = PlaylistResumes::default();
        resumes.record("list", resume(2, "c", 30.0));
        resumes.finish("list");
        assert!(resumes.get("list").is_none());
        for i in 0..=LIMIT {
            resumes.record(&format!("list-{i}"), PlaylistResume { updated: i as i64, ..resume(0, "a", 0.0) });
        }
        assert_eq!(resumes.playlists.len(), LIMIT);
        assert!(resumes.get("list-0").is_none());
        assert!(resumes.get(&format!("list-{LIMIT}")).is_some());
        let decoded: PlaylistResumes = serde_json::from_str(&serde_json::to_string(&resumes).unwrap()).unwrap();
        assert_eq!(decoded, resumes);
    }
}
