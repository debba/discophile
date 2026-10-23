//! Half-hour selections chosen by the music assistant from the user's library.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::l10n::t;
use crate::listening_history::{ListeningHistory, unique};
use crate::models::Song;

/// A curated mix lasts at most half an hour.
pub const CURATED_MIX_SECONDS: f64 = 30.0 * 60.0;
/// Songs offered to the curator, at most.
pub const CURATOR_CANDIDATE_LIMIT: usize = 200;
const TITLE_LIMIT: usize = 120;
const EXPLANATION_LIMIT: usize = 2_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CuratorPreset {
    Evening,
    Italian,
    Focus,
}

impl CuratorPreset {
    pub const ALL: [CuratorPreset; 3] = [Self::Evening, Self::Italian, Self::Focus];

    pub fn id(self) -> &'static str {
        match self {
            Self::Evening => "evening",
            Self::Italian => "italian",
            Self::Focus => "focus",
        }
    }

    pub fn title(self) -> String {
        t(match self {
            Self::Evening => "A quiet evening",
            Self::Italian => "Italian songwriters",
            Self::Focus => "Time to focus",
        })
    }

    /// The request sent to the assistant, in the interface language.
    pub fn request(self) -> String {
        t(match self {
            Self::Evening => {
                "Choose up to thirty minutes for a relaxed evening from my library, favoring songs I play less."
            }
            Self::Italian => {
                "Choose up to thirty minutes of Italian singer-songwriters from my library, favoring songs I play less."
            }
            Self::Focus => "Choose up to thirty minutes for concentrating from my library, favoring songs I play less.",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CuratedMix {
    pub id: Uuid,
    pub preset: CuratorPreset,
    pub title: String,
    pub explanation: String,
    pub songs: Vec<Song>,
    /// Who chose the songs, for example the assistant provider's name.
    pub source: String,
    /// Unix milliseconds.
    pub created: i64,
    #[serde(default, rename = "playlistID")]
    pub playlist_id: Option<String>,
    #[serde(default)]
    pub playlist_saved: Option<bool>,
}

impl CuratedMix {
    /// Bounds the texts and keeps the songs, in order, that fit in half an hour.
    pub fn new(
        preset: CuratorPreset,
        title: &str,
        explanation: &str,
        songs: &[Song],
        source: &str,
        created: i64,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            preset,
            title: title.chars().take(TITLE_LIMIT).collect(),
            explanation: explanation.chars().take(EXPLANATION_LIMIT).collect(),
            songs: fitting(songs, CURATED_MIX_SECONDS),
            source: source.to_string(),
            created,
            playlist_id: None,
            playlist_saved: None,
        }
    }

    pub fn duration(&self) -> f64 {
        self.songs.iter().filter_map(|s| s.duration).sum()
    }
}

/// Unique songs with known durations, in order, that fit within `seconds`.
pub fn fitting(candidates: &[Song], seconds: f64) -> Vec<Song> {
    if !seconds.is_finite() || seconds <= 0.0 {
        return Vec::new();
    }
    let mut remaining = seconds;
    unique(candidates)
        .into_iter()
        .filter(|song| match song.duration {
            Some(duration) if duration.is_finite() && duration > 0.0 && duration <= remaining => {
                remaining -= duration;
                true
            }
            _ => false,
        })
        .collect()
}

/// Songs to offer the curator: least played first, then least recently played, then library order.
pub fn curator_candidates(candidates: &[Song], history: &ListeningHistory, limit: usize) -> Vec<Song> {
    let entries: HashMap<&str, (i64, i64)> =
        history.entries().iter().map(|e| (e.id(), (e.play_count, e.last_played))).collect();
    let mut ranked: Vec<(usize, Song)> = unique(candidates).into_iter().enumerate().collect();
    ranked.sort_by(|(left_index, left), (right_index, right)| {
        let a = entries.get(left.stream_identifier()).copied().unwrap_or((0, i64::MIN));
        let b = entries.get(right.stream_identifier()).copied().unwrap_or((0, i64::MIN));
        a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(left_index.cmp(right_index))
    });
    ranked.into_iter().take(limit.min(CURATOR_CANDIDATE_LIMIT)).map(|(_, s)| s).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::listening_history::DAY_MILLIS;
    use crate::listening_history::tests::{NOW, at};

    fn song(id: &str, duration: f64) -> Song {
        Song {
            artist: Some("Artist".into()),
            album_id: Some("album".into()),
            duration: Some(duration),
            ..Song::new(id, id)
        }
    }

    #[test]
    fn curator_prioritizes_less_played_songs_and_preserves_validated_selection_order() {
        let mut history = ListeningHistory::new();
        let favorite = song("favorite", 180.0);
        let unknown = song("new", 180.0);
        let old = song("old", 180.0);
        history.record(&favorite, Uuid::new_v4(), 40.0, 40.0, 180.0, false, &at(NOW));
        history.record(&old, Uuid::new_v4(), 10.0, 10.0, 180.0, false, &at(NOW - 40 * DAY_MILLIS));
        let candidates =
            curator_candidates(&[favorite.clone(), old.clone(), unknown.clone(), unknown.clone()], &history, 200);
        assert_eq!(candidates.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["new", "old", "favorite"]);
        let mix =
            CuratedMix::new(CuratorPreset::Evening, "Evening", "A selection", &[unknown, old, favorite], "Local", NOW);
        assert_eq!(mix.songs.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["new", "old", "favorite"]);
        let restored: CuratedMix = serde_json::from_str(&serde_json::to_string(&mix).unwrap()).unwrap();
        assert_eq!(restored, mix);
    }

    #[test]
    fn curator_excludes_unknown_and_oversized_durations_without_exceeding_half_hour() {
        let mut songs = vec![song("long", 2_000.0), Song::new("unknown", "Unknown")];
        songs.extend((0..20).map(|i| song(&i.to_string(), 180.0)));
        let mix = CuratedMix::new(CuratorPreset::Focus, "Focus", "", &songs, "Local", NOW);
        assert_eq!(mix.songs.len(), 10);
        assert_eq!(mix.duration(), 1_800.0);
    }

    #[test]
    fn texts_are_bounded() {
        let mix = CuratedMix::new(CuratorPreset::Italian, &"x".repeat(500), &"y".repeat(5_000), &[], "Local", NOW);
        assert_eq!(mix.title.chars().count(), 120);
        assert_eq!(mix.explanation.chars().count(), 2_000);
    }
}
