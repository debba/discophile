//! Personal radio: an endless queue from the user's library, related to a starting song,
//! with an explanation for every recommendation.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::l10n::t;
use crate::listening_history::{
    DAY_MILLIS, ListeningHistory, REDISCOVER_AFTER_MILLIS, normalize_name, same_artist, unique,
};
use crate::models::Song;
use crate::queue::PlaybackQueue;

/// Recommendations per refill by default, and at most.
pub const DEFAULT_LIMIT: usize = 25;
pub const MAX_LIMIT: usize = 100;
/// The radio refills when this many songs or fewer are left in the queue.
pub const REFILL_THRESHOLD: usize = 3;
/// Search and random sample sizes used to gather candidates from the server.
pub const ARTIST_SEARCH_SIZE: usize = 100;
pub const RANDOM_SAMPLE_SIZE: usize = 150;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PersonalRadioMode {
    #[default]
    Familiar,
    Explore,
}

impl PersonalRadioMode {
    pub const ALL: [PersonalRadioMode; 2] = [Self::Familiar, Self::Explore];

    pub fn title(self) -> String {
        t(match self {
            Self::Familiar => "Stay with this style",
            Self::Explore => "Let me explore",
        })
    }

    pub fn explanation(self) -> String {
        t(match self {
            Self::Familiar => "Songs close to this track's artist, genre and years.",
            Self::Explore => "Related songs alternate with discoveries from your library.",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RadioReason {
    Server,
    Artist,
    Genre,
    Era,
    Unheard,
    Rediscover,
    Collection,
}

impl RadioReason {
    /// A short chip label.
    pub fn label(self) -> String {
        t(match self {
            Self::Server => "Server pick",
            Self::Artist => "Same artist",
            Self::Genre => "Same genre",
            Self::Era => "Same era",
            Self::Unheard => "New to you",
            Self::Rediscover => "Rediscover",
            Self::Collection => "Your collection",
        })
    }

    /// The full sentence shown under "Why this song?".
    pub fn description(self) -> String {
        t(match self {
            Self::Server => "Suggested by your music server.",
            Self::Artist => "The same artist as your starting track.",
            Self::Genre => "The same genre in your library tags.",
            Self::Era => "Released within five years, according to library tags.",
            Self::Unheard => "You have not played this song on this device.",
            Self::Rediscover => "You last played this song over a month ago on this device.",
            Self::Collection => "Another song from your collection.",
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RadioRecommendation {
    pub song: Song,
    pub reasons: Vec<RadioReason>,
}

impl RadioRecommendation {
    pub fn id(&self) -> &str {
        self.song.stream_identifier()
    }
}

/// Whether the queue is short enough to refill.
pub fn needs_refill(queue: &PlaybackQueue) -> bool {
    queue.upcoming().len() <= REFILL_THRESHOLD
}

/// Songs already queued are excluded from refills; a new radio excludes nothing but the seed.
pub fn excluded_ids(queue: &PlaybackQueue, starting: bool) -> HashSet<String> {
    if starting { HashSet::new() } else { queue.songs().iter().map(|s| s.stream_identifier().to_string()).collect() }
}

fn year(song: &Song) -> Option<i64> {
    song.album_original_year.or(song.year).or(song.album_year)
}

/// Ranks candidates by affinity to `seed` (server suggestion, artist, genre, era) and discovery
/// (unheard, not played for a month), penalizing songs played in the last day. Familiar mode keeps only
/// related songs when there are any; Explore makes every third song a discovery. Consecutive songs by
/// the same artist are avoided when possible. `now` is Unix milliseconds.
#[allow(clippy::too_many_arguments)]
pub fn recommend(
    seed: &Song,
    candidates: &[Song],
    similar_ids: &HashSet<String>,
    history: &ListeningHistory,
    mode: PersonalRadioMode,
    excluding: &HashSet<String>,
    limit: usize,
    now: i64,
) -> Vec<RadioRecommendation> {
    struct Ranked {
        recommendation: RadioRecommendation,
        affinity: i64,
        score: i64,
    }
    let entries = history.entries_by_id();
    let mut ranked: Vec<Ranked> = Vec::new();
    for song in unique(candidates) {
        let id = song.stream_identifier();
        if id == seed.stream_identifier() || excluding.contains(id) {
            continue;
        }
        let mut reasons = Vec::new();
        let mut affinity = 0;
        if similar_ids.contains(id) {
            affinity += 6;
            reasons.push(RadioReason::Server);
        }
        if let (Some(artist), Some(other)) = (seed.artist.as_deref().filter(|a| !a.is_empty()), song.artist.as_deref())
            && same_artist(artist, other)
        {
            affinity += 5;
            reasons.push(RadioReason::Artist);
        }
        if let (Some(genre), Some(other)) = (seed.genre.as_deref().filter(|g| !g.is_empty()), song.genre.as_deref())
            && normalize_name(genre) == normalize_name(other)
        {
            affinity += 4;
            reasons.push(RadioReason::Genre);
        }
        if let (Some(a), Some(b)) = (year(seed), year(&song))
            && (a - b).abs() <= 5
        {
            affinity += 1;
            reasons.push(RadioReason::Era);
        }
        let entry = entries.get(id);
        let discovery = match entry {
            None => {
                reasons.push(RadioReason::Unheard);
                3
            }
            Some(e) if now - e.last_played >= REDISCOVER_AFTER_MILLIS => {
                reasons.push(RadioReason::Rediscover);
                2
            }
            Some(_) => 0,
        };
        if reasons.is_empty() {
            reasons.push(RadioReason::Collection);
        }
        let recent_penalty = if entry.is_some_and(|e| now - e.last_played < DAY_MILLIS) { 8 } else { 0 };
        ranked.push(Ranked {
            score: affinity + discovery - recent_penalty,
            affinity,
            recommendation: RadioRecommendation { song, reasons },
        });
    }
    ranked.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.recommendation.id().cmp(b.recommendation.id())));
    if mode == PersonalRadioMode::Familiar && ranked.iter().any(|r| r.affinity > 0) {
        ranked.retain(|r| r.affinity > 0);
    }
    // Alternate related tracks and discoveries. Avoid consecutive songs by the same artist when possible.
    let mut result: Vec<RadioRecommendation> = Vec::new();
    let limit = limit.min(MAX_LIMIT);
    while !ranked.is_empty() && result.len() < limit {
        let explore = mode == PersonalRadioMode::Explore && result.len() % 3 == 2;
        let previous = result.last().and_then(|r| r.song.artist.as_deref()).or(seed.artist.as_deref());
        let index = ranked
            .iter()
            .position(|r| {
                let varied = match (previous, r.recommendation.song.artist.as_deref()) {
                    (Some(a), Some(b)) => !same_artist(a, b),
                    _ => true,
                };
                varied && (!explore || r.affinity < 4)
            })
            .unwrap_or(0);
        result.push(ranked.remove(index).recommendation);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::listening_history::tests::NOW;

    fn ids(items: &[RadioRecommendation]) -> Vec<&str> {
        items.iter().map(|r| r.id()).collect()
    }

    #[test]
    fn radio_stays_related_and_explore_includes_discoveries_without_repeating_queue() {
        let tagged = |id: &str, artist: &str, year: i64, genre: &str| Song {
            artist: Some(artist.into()),
            year: Some(year),
            genre: Some(genre.into()),
            ..Song::new(id, id)
        };
        let seed = tagged("seed", "A", 1990, "Jazz");
        let related: Vec<Song> = (0..6).map(|i| tagged(&format!("jazz{i}"), &format!("B{i}"), 1992, "Jazz")).collect();
        let discovery = tagged("different", "C", 2025, "Rock");
        let mut candidates = vec![seed.clone(), discovery.clone(), discovery];
        candidates.extend(related);
        let history = ListeningHistory::new();
        let excluded: HashSet<String> = ["jazz0".to_string()].into();
        let familiar = recommend(
            &seed,
            &candidates,
            &HashSet::new(),
            &history,
            PersonalRadioMode::Familiar,
            &excluded,
            DEFAULT_LIMIT,
            NOW,
        );
        assert!(!familiar.iter().any(|r| ["seed", "different", "jazz0"].contains(&r.id())));
        assert!(
            familiar.iter().all(|r| r.reasons.contains(&RadioReason::Genre) && r.reasons.contains(&RadioReason::Era))
        );
        let explore = recommend(
            &seed,
            &candidates,
            &HashSet::new(),
            &history,
            PersonalRadioMode::Explore,
            &HashSet::new(),
            DEFAULT_LIMIT,
            NOW,
        );
        assert_eq!(explore[2].id(), "different");
        assert_eq!(ids(&explore).into_iter().collect::<HashSet<_>>().len(), explore.len());
        assert!(explore[2].reasons.contains(&RadioReason::Unheard));
    }

    #[test]
    fn radio_uses_server_recommendations_and_falls_back_honestly_with_missing_tags() {
        let seed = Song { artist: Some("Artist".into()), duration: Some(180.0), ..Song::new("seed", "seed") };
        let suggested = Song::new("suggested", "Suggested");
        let other = Song::new("other", "Other");
        let similar: HashSet<String> = ["suggested".to_string()].into();
        let history = ListeningHistory::new();
        let none = HashSet::new();
        let result = recommend(
            &seed,
            &[other.clone(), suggested],
            &similar,
            &history,
            PersonalRadioMode::Familiar,
            &none,
            DEFAULT_LIMIT,
            NOW,
        );
        assert_eq!(ids(&result), ["suggested"]);
        assert!(result[0].reasons.contains(&RadioReason::Server));
        let fallback =
            recommend(&seed, &[other], &none, &history, PersonalRadioMode::Familiar, &none, DEFAULT_LIMIT, NOW);
        assert_eq!(fallback[0].id(), "other");
        assert!(!fallback[0].reasons.contains(&RadioReason::Genre));
    }

    #[test]
    fn refills_exclude_queued_songs() {
        let mut queue = PlaybackQueue::new();
        queue.replace((0..5).map(|i| Song::new(i.to_string(), "x")).collect(), 0, false);
        assert!(!needs_refill(&queue));
        queue.select(1);
        assert!(needs_refill(&queue));
        assert_eq!(excluded_ids(&queue, false).len(), 5);
        assert!(excluded_ids(&queue, true).is_empty());
    }
}
