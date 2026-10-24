//! Whole-album listening: complete track lists, durations and three suggestions for tonight.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::l10n::t;
use crate::listening_history::{DAY_MILLIS, ListeningHistory, REDISCOVER_AFTER_MILLIS, normalize_name};
use crate::md5_hex;
use crate::models::{Album, Song};

/// Listening-time choices offered for an album (0 means any duration).
pub const MINUTE_OPTIONS: [i64; 3] = [25, 45, 60];
/// Suggestions shown at once.
pub const SUGGESTION_COUNT: usize = 3;
/// Keep tonight's picks to album-length releases rather than singles or long box sets.
const MIN_ALBUM_SECONDS: f64 = 20.0 * 60.0;
const MAX_ALBUM_SECONDS: f64 = 90.0 * 60.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AlbumReason {
    Unheard,
    Rediscover,
    Favorite,
    Collection,
}

impl AlbumReason {
    fn rank(self) -> u8 {
        match self {
            Self::Unheard => 0,
            Self::Rediscover => 1,
            Self::Favorite => 2,
            Self::Collection => 3,
        }
    }

    pub fn description(self) -> String {
        t(match self {
            Self::Unheard => "An album you have not played on this device.",
            Self::Rediscover => "An album you have not played for at least a month on this device.",
            Self::Favorite => "One of your favorite albums.",
            Self::Collection => "Make time for a whole album from your collection.",
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AlbumSuggestion {
    pub album: Album,
    pub reason: AlbumReason,
}

impl AlbumSuggestion {
    pub fn id(&self) -> &str {
        &self.album.id
    }
}

/// True when the album's track list is loaded and matches its song count.
pub fn is_complete(album: &Album) -> bool {
    match &album.song {
        Some(songs) if !songs.is_empty() => album.song_count.is_none_or(|count| count == songs.len() as i64),
        _ => false,
    }
}

/// The album's duration, or the sum of its tracks when every track's duration is known.
pub fn duration(album: &Album) -> Option<f64> {
    if is_complete(album) {
        let songs = album.songs();
        if songs.iter().all(|s| s.duration.is_some_and(|d| d > 0.0 && d.is_finite())) {
            return Some(songs.iter().filter_map(|s| s.duration).sum());
        }
    }
    if let Some(duration) = album.duration.filter(|d| d.is_finite() && *d > 0.0) {
        return Some(duration);
    }
    let songs = album.song.as_deref().filter(|s| !s.is_empty())?;
    if !songs.iter().all(|s| s.duration.is_some_and(|d| d > 0.0 && d.is_finite())) {
        return None;
    }
    Some(songs.iter().filter_map(|s| s.duration).sum())
}

/// Tracks in disc and track order; missing numbers fall back to disc 1 and list position.
pub fn tracks(album: &Album) -> Vec<Song> {
    let mut indexed: Vec<(usize, &Song)> = album.songs().iter().enumerate().collect();
    indexed.sort_by(|(ai, a), (bi, b)| {
        let disc = |s: &Song| s.disc_number.unwrap_or(1);
        let track = |s: &Song, i: usize| s.track.unwrap_or(i as i64 + 1);
        disc(a).cmp(&disc(b)).then(track(a, *ai).cmp(&track(b, *bi))).then(ai.cmp(bi))
    });
    indexed.into_iter().map(|(_, s)| s.clone()).collect()
}

/// Up to three albums, preferring unheard, then not played for a month, then favorites, with
/// different artists when possible. The order is stable for a day. `now` is Unix milliseconds.
pub fn suggest(
    albums: &[Album],
    history: &ListeningHistory,
    favorite_ids: &HashSet<String>,
    minutes: Option<i64>,
    now: i64,
) -> Vec<AlbumSuggestion> {
    let mut seen = HashSet::new();
    let albums = albums.iter().filter(|a| seen.insert(a.id.as_str())).filter(|album| {
        let count = album.song_count.or_else(|| album.song.as_ref().map(|songs| songs.len() as i64));
        count.is_none_or(|count| count > 1)
            && duration(album).is_some_and(|seconds| (MIN_ALBUM_SECONDS..=MAX_ALBUM_SECONDS).contains(&seconds))
    }).filter(|album| match minutes {
        None => true,
        Some(minutes) => duration(album).is_some_and(|d| {
            d >= ((minutes - 15) as f64 * 60.0).max(MIN_ALBUM_SECONDS) && d <= minutes as f64 * 60.0
        }),
    });
    let mut latest: HashMap<&str, i64> = HashMap::new();
    for entry in history.entries() {
        if let Some(id) = entry.song.album_id.as_deref() {
            let value = latest.entry(id).or_insert(i64::MIN);
            *value = (*value).max(entry.last_played);
        }
    }
    let day = ((now as f64) / DAY_MILLIS as f64) as i64;
    let salt = day.to_string();
    let mut ranked: Vec<(String, AlbumSuggestion)> = albums
        .map(|album| {
            let reason = match latest.get(album.id.as_str()) {
                None => AlbumReason::Unheard,
                Some(last) if now - last >= REDISCOVER_AFTER_MILLIS => AlbumReason::Rediscover,
                _ if favorite_ids.contains(&album.id) => AlbumReason::Favorite,
                _ => AlbumReason::Collection,
            };
            (md5_hex(format!("{}{salt}", album.id).as_bytes()), AlbumSuggestion { album: album.clone(), reason })
        })
        .collect();
    ranked.sort_by(|(ak, a), (bk, b)| a.reason.rank().cmp(&b.reason.rank()).then(ak.cmp(bk)));
    let ranked: Vec<AlbumSuggestion> = ranked.into_iter().map(|(_, s)| s).collect();

    let mut result: Vec<AlbumSuggestion> = Vec::new();
    let mut artists = HashSet::new();
    for suggestion in &ranked {
        let artist = normalize_name(suggestion.album.artist.as_deref().unwrap_or(&suggestion.album.id));
        if artists.insert(artist) {
            result.push(suggestion.clone());
        }
        if result.len() == SUGGESTION_COUNT {
            return result;
        }
    }
    for suggestion in &ranked {
        if result.len() == SUGGESTION_COUNT {
            break;
        }
        if !result.iter().any(|r| r.album.id == suggestion.album.id) {
            result.push(suggestion.clone());
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::listening_history::tests::{NOW, at};
    use uuid::Uuid;

    fn album(json: &str) -> Album {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn album_tracks_follow_disc_and_track_order_and_duration_uses_all_tracks() {
        let full = album(
            r#"{"id":"album","name":"Album","song":[{"id":"three","title":"Three","track":1,"discNumber":2,"duration":180},{"id":"two","title":"Two","track":2,"discNumber":1,"duration":120},{"id":"one","title":"One","track":1,"discNumber":1,"duration":60}]}"#,
        );
        assert_eq!(tracks(&full).iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["one", "two", "three"]);
        assert_eq!(duration(&full), Some(360.0));
        let unknown = album(r#"{"id":"unknown","name":"Unknown","song":[{"id":"song","title":"Song"}]}"#);
        assert_eq!(duration(&unknown), None);
    }

    #[test]
    fn timed_album_suggestions_exclude_unknown_and_long_albums_and_avoid_duplicates() {
        let short = album(r#"{"id":"short","name":"Short","artist":"A","duration":1200}"#);
        let long = album(r#"{"id":"long","name":"Long","artist":"B","duration":3000}"#);
        let unknown = Album::new("unknown", "Unknown", "C");
        let result =
            suggest(&[short.clone(), short, long, unknown], &ListeningHistory::new(), &HashSet::new(), Some(25), NOW);
        assert_eq!(result.iter().map(|s| s.id()).collect::<Vec<_>>(), ["short"]);
        assert_eq!(result[0].reason, AlbumReason::Unheard);
    }

    #[test]
    fn forty_five_minutes_rejects_short_releases_and_uses_complete_track_durations() {
        let short = album(r#"{"id":"short","name":"Ghost Trees","duration":1087,"songCount":5}"#);
        let fitting = album(r#"{"id":"fit","name":"Album","duration":2370,"songCount":10}"#);
        let result = suggest(&[short, fitting], &ListeningHistory::new(), &HashSet::new(), Some(45), NOW);
        assert_eq!(result.iter().map(|s| s.id()).collect::<Vec<_>>(), ["fit"]);
        let full = album(r#"{"id":"full","name":"Full","duration":1087,"songCount":2,"song":[{"id":"a","title":"A","duration":1200},{"id":"b","title":"B","duration":1200}]}"#);
        assert_eq!(duration(&full), Some(2400.0));
        assert_eq!(suggest(&[full], &ListeningHistory::new(), &HashSet::new(), Some(45), NOW).len(), 1);
    }

    #[test]
    fn suggestions_exclude_singles_and_box_sets_even_without_a_time_limit() {
        let albums = [
            album(r#"{"id":"box","name":"Box set","duration":16720,"songCount":80}"#),
            album(r#"{"id":"single","name":"Single","duration":187,"songCount":1}"#),
            album(r#"{"id":"ep","name":"Short EP","duration":600,"songCount":4}"#),
            album(r#"{"id":"one","name":"One track","duration":1800,"songCount":1}"#),
            album(r#"{"id":"album","name":"Album","duration":2400,"songCount":10}"#),
        ];
        let result = suggest(&albums, &ListeningHistory::new(), &HashSet::new(), None, NOW);
        assert_eq!(result.iter().map(|s| s.id()).collect::<Vec<_>>(), ["album"]);
    }

    #[test]
    fn album_listening_rejects_missing_and_partial_track_lists() {
        let partial = album(r#"{"id":"partial","name":"Partial","songCount":2,"song":[{"id":"one","title":"One"}]}"#);
        assert!(!is_complete(&partial));
        assert!(!is_complete(&Album::new("list", "List metadata", "A")));
        let full = album(r#"{"id":"full","name":"Full","songCount":1,"song":[{"id":"one","title":"One"}]}"#);
        assert!(is_complete(&full));
    }

    #[test]
    fn album_reasons_use_latest_play_and_suggestions_vary_artists() {
        let mut history = ListeningHistory::new();
        let old_song = Song { album_id: Some("old".into()), ..Song::new("song", "Song") };
        let favorite_song = Song { album_id: Some("favorite".into()), ..Song::new("favoriteSong", "Favorite") };
        history.record(&old_song, Uuid::new_v4(), 40.0, 40.0, 180.0, false, &at(NOW - 40 * DAY_MILLIS));
        history.record(&favorite_song, Uuid::new_v4(), 40.0, 40.0, 180.0, false, &at(NOW));
        let albums = [
            album(r#"{"id":"new","name":"New","artist":"A","duration":2400}"#),
            album(r#"{"id":"same","name":"Same","artist":"A","duration":2400}"#),
            album(r#"{"id":"old","name":"Old","artist":"B","duration":2400}"#),
            album(r#"{"id":"favorite","name":"Favorite","artist":"C","duration":2400}"#),
        ];
        let favorites: HashSet<String> = ["favorite".to_string()].into();
        let result = suggest(&albums, &history, &favorites, None, NOW);
        assert_eq!(result.len(), 3);
        assert_eq!(result.iter().filter_map(|s| s.album.artist.clone()).collect::<HashSet<_>>().len(), 3);
        assert_eq!(result.iter().find(|s| s.id() == "old").unwrap().reason, AlbumReason::Rediscover);
        assert_eq!(result.iter().find(|s| s.id() == "favorite").unwrap().reason, AlbumReason::Favorite);
    }

    #[test]
    fn completion_keeps_last_position_without_counting_seek_as_a_play() {
        let mut history = ListeningHistory::new();
        let song = Song { duration: Some(180.0), ..Song::new("song", "Song") };
        history.record(&song, Uuid::new_v4(), 2.0, 180.0, 180.0, true, &at(NOW));
        assert_eq!(history.entries()[0].play_count, 0);
        assert!(history.entries()[0].completed);
    }
}
