//! Device-only listening history, discovery mixes and the listening clock.
//!
//! The history contains library metadata only, never authenticated URLs or credentials.
//! Everything here is pure: the app crate loads and saves [`DiscoveryArchive`] per account.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, NaiveDate, TimeZone};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::curated_mix::CuratedMix;
use crate::l10n::t;
use crate::listening_statistics::ListeningLog;
use crate::md5_hex;
use crate::models::Song;
use crate::queue::PlaybackQueue;

pub const DAY_MILLIS: i64 = 86_400_000;
/// Most recently played songs kept in the history.
pub const HISTORY_LIMIT: usize = 2_000;
/// Songs kept in a resume queue, and how many already played songs precede the current one.
pub const RESUME_LIMIT: usize = 200;
pub const RESUME_LEADING: usize = 20;
/// A song becomes "worth another listen" after this long without playing.
pub const REDISCOVER_AFTER_MILLIS: i64 = 30 * DAY_MILLIS;
/// Budget of the "Have 25 minutes?" mix.
pub const SHORT_MIX_SECONDS: f64 = 25.0 * 60.0;
/// Library candidates kept for discovery, and curated mixes kept per account.
pub const CANDIDATE_LIMIT: usize = 500;
pub const CURATED_LIMIT: usize = 12;
/// Random songs requested when refreshing candidates; albums sampled when that is unsupported.
pub const RANDOM_SAMPLE_SIZE: usize = 150;
pub const FALLBACK_SAMPLE_ALBUMS: usize = 5;
/// Candidates are refreshed at most hourly unless forced.
pub const REFRESH_INTERVAL_MILLIS: i64 = 3_600_000;

/// Listening counts as a play after 30 seconds, or half of a shorter track.
pub fn play_threshold(duration: f64) -> f64 {
    if duration > 0.0 { (duration / 2.0).min(30.0) } else { 30.0 }
}

/// Whether an hourly refresh is due. `last` and `now` are Unix milliseconds.
pub fn refresh_due(last: Option<i64>, now: i64, force: bool) -> bool {
    force || last.is_none_or(|last| now - last > REFRESH_INTERVAL_MILLIS)
}

/// Removes podcasts, empty identifiers and duplicates (by stream identifier), keeping the first.
pub fn unique<'a>(songs: impl IntoIterator<Item = &'a Song>) -> Vec<Song> {
    let mut seen = HashSet::new();
    songs
        .into_iter()
        .filter(|s| {
            !s.is_podcast() && !s.stream_identifier().is_empty() && seen.insert(s.stream_identifier().to_string())
        })
        .cloned()
        .collect()
}

/// Case-, diacritic- and width-insensitive text, as Foundation's `folding` produces it.
pub fn fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        let c = match c as u32 {
            // Fullwidth ASCII variants.
            0xFF01..=0xFF5E => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            0x3000 => ' ',
            // Combining diacritical marks in decomposed input.
            0x0300..=0x036F => continue,
            _ => c,
        };
        for lower in c.to_lowercase() {
            out.push(strip_diacritic(lower));
        }
    }
    out
}

fn strip_diacritic(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' | 'ǎ' => 'a',
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => 'c',
        'ď' => 'd',
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => 'e',
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => 'g',
        'ĥ' => 'h',
        'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ǐ' => 'i',
        'ĵ' => 'j',
        'ķ' => 'k',
        'ĺ' | 'ļ' | 'ľ' => 'l',
        'ñ' | 'ń' | 'ņ' | 'ň' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ō' | 'ŏ' | 'ő' | 'ǒ' => 'o',
        'ŕ' | 'ŗ' | 'ř' => 'r',
        'ś' | 'ŝ' | 'ş' | 'š' | 'ș' => 's',
        'ţ' | 'ť' | 'ț' => 't',
        'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' | 'ǔ' => 'u',
        'ŵ' => 'w',
        'ý' | 'ÿ' | 'ŷ' => 'y',
        'ź' | 'ż' | 'ž' => 'z',
        other => other,
    }
}

/// Folded text with whitespace runs collapsed, for comparing names.
pub fn normalize_name(text: &str) -> String {
    fold(text).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Folded letters and digits only, for matching titles across services.
pub fn normalize_key(text: &str) -> String {
    fold(text).chars().filter(|c| c.is_alphanumeric()).collect()
}

/// Same artist, ignoring case, accents and a leading "The".
pub fn same_artist(lhs: &str, rhs: &str) -> bool {
    fn name(text: &str) -> String {
        let value = normalize_name(text);
        value.strip_prefix("the ").map(str::to_string).unwrap_or(value)
    }
    let left = name(lhs);
    !left.is_empty() && left == name(rhs)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListeningEntry {
    pub song: Song,
    /// Unix milliseconds of the last listening that advanced.
    pub last_played: i64,
    pub position: f64,
    pub duration: f64,
    #[serde(default)]
    pub play_count: i64,
    #[serde(default)]
    pub listened_seconds: f64,
    #[serde(default)]
    pub completed: bool,
    #[serde(rename = "sessionID")]
    session_id: Uuid,
    #[serde(default)]
    session_seconds: f64,
    #[serde(default)]
    session_counted: bool,
}

impl ListeningEntry {
    pub fn id(&self) -> &str {
        self.song.stream_identifier()
    }
}

/// The queue to restore on the next launch: at most 200 songs, starting 20 before the current one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ListeningResume {
    pub songs: Vec<Song>,
    pub index: usize,
    pub position: f64,
}

impl ListeningResume {
    pub fn new(queue: &PlaybackQueue, position: f64) -> Option<Self> {
        let current = queue.current()?;
        if current.is_podcast() || !position.is_finite() {
            return None;
        }
        let start = queue.index().saturating_sub(RESUME_LEADING);
        let songs = queue.songs()[start..].iter().take(RESUME_LIMIT).cloned().collect();
        Some(Self { songs, index: queue.index() - start, position: position.max(0.0) })
    }

    pub fn current(&self) -> Option<&Song> {
        self.songs.get(self.index)
    }

    /// A queue positioned on the resumed song.
    pub fn queue(&self) -> PlaybackQueue {
        let mut queue = PlaybackQueue::new();
        queue.replace(self.songs.clone(), self.index, false);
        queue
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ListeningHistory {
    #[serde(default)]
    entries: Vec<ListeningEntry>,
    #[serde(default)]
    pub resume: Option<ListeningResume>,
    /// Histories saved before daily statistics have no log.
    #[serde(default)]
    log: ListeningLog,
}

impl ListeningHistory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Most recently played first.
    pub fn entries(&self) -> &[ListeningEntry] {
        &self.entries
    }

    pub fn log(&self) -> &ListeningLog {
        &self.log
    }

    pub(crate) fn entries_by_id(&self) -> HashMap<&str, &ListeningEntry> {
        self.entries.iter().map(|e| (e.id(), e)).collect()
    }

    /// Records listening progress for a playback session. `listened` is the session's cumulative
    /// listening time; only the increase since the last call is added. A session counts as one play
    /// once it passes [`play_threshold`]. The calendar day comes from `now`'s time zone.
    #[allow(clippy::too_many_arguments)]
    pub fn record<Tz: TimeZone>(
        &mut self,
        song: &Song,
        session_id: Uuid,
        listened: f64,
        position: f64,
        duration: f64,
        completed: bool,
        now: &DateTime<Tz>,
    ) {
        self.record_at(
            song,
            session_id,
            listened,
            position,
            duration,
            completed,
            now.timestamp_millis(),
            now.date_naive(),
        );
    }

    /// [`record`](Self::record) with the device's local time.
    pub fn record_now(
        &mut self,
        song: &Song,
        session_id: Uuid,
        listened: f64,
        position: f64,
        duration: f64,
        completed: bool,
    ) {
        self.record(song, session_id, listened, position, duration, completed, &chrono::Local::now());
    }

    #[allow(clippy::too_many_arguments)]
    fn record_at(
        &mut self,
        song: &Song,
        session_id: Uuid,
        listened: f64,
        position: f64,
        duration: f64,
        completed: bool,
        now: i64,
        today: NaiveDate,
    ) {
        if song.is_podcast()
            || !listened.is_finite()
            || listened < 1.0
            || !position.is_finite()
            || !duration.is_finite()
        {
            return;
        }
        let id = song.stream_identifier().to_string();
        let mut entry = match self.entries.iter().position(|e| e.id() == id) {
            Some(index) => self.entries.remove(index),
            None => ListeningEntry {
                song: song.clone(),
                last_played: now,
                position: 0.0,
                duration: 0.0,
                play_count: 0,
                listened_seconds: 0.0,
                completed: false,
                session_id,
                session_seconds: 0.0,
                session_counted: false,
            },
        };
        if entry.session_id != session_id {
            entry.session_id = session_id;
            entry.session_seconds = 0.0;
            entry.session_counted = false;
        }
        let delta = (listened - entry.session_seconds).max(0.0);
        entry.song = song.clone();
        entry.position = position.max(0.0);
        entry.duration = duration.max(0.0);
        entry.completed = completed || (duration > 0.0 && position >= duration - 2.0);
        entry.listened_seconds += delta;
        entry.session_seconds = listened.max(entry.session_seconds);
        if delta > 0.0 {
            entry.last_played = now;
        }
        let counted = !entry.session_counted && listened >= play_threshold(duration);
        if counted {
            entry.play_count += 1;
            entry.session_counted = true;
        }
        self.log.add(song, delta, counted, today);
        self.entries.push(entry);
        self.entries.sort_by_key(|e| std::cmp::Reverse(e.last_played));
        self.entries.truncate(HISTORY_LIMIT);
    }

    /// Updates the resume point after recording. A completed song resumes on the next one;
    /// a finished queue clears the resume point.
    pub fn update_resume(&mut self, queue: &PlaybackQueue, position: f64, completed: bool, finished_queue: bool) {
        if finished_queue {
            self.resume = None;
            return;
        }
        let mut next = queue.clone();
        if completed && !queue.upcoming().is_empty() {
            next.advance(false);
        }
        self.resume = ListeningResume::new(&next, if completed { 0.0 } else { position });
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiscoveryMix {
    Rediscover,
    Unheard,
    Short,
}

impl DiscoveryMix {
    pub const ALL: [DiscoveryMix; 3] = [Self::Rediscover, Self::Unheard, Self::Short];

    pub fn id(self) -> &'static str {
        match self {
            Self::Rediscover => "rediscover",
            Self::Unheard => "unheard",
            Self::Short => "short",
        }
    }

    pub fn title(self) -> String {
        t(match self {
            Self::Rediscover => "Worth another listen",
            Self::Unheard => "Still to discover",
            Self::Short => "Have 25 minutes?",
        })
    }

    pub fn explanation(self) -> String {
        t(match self {
            Self::Rediscover => "Songs you last played at least a month ago on this device.",
            Self::Unheard => "Songs from your library sample with no listening history on this device.",
            Self::Short => "A selection that fits within 25 minutes, using known track durations.",
        })
    }

    /// A daily-stable selection: the order changes once per day and per mix.
    pub fn songs(self, candidates: &[Song], history: &ListeningHistory, now: i64) -> Vec<Song> {
        let entries = history.entries_by_id();
        let day = ((now as f64) / DAY_MILLIS as f64) as i64;
        let salt = format!("{day}{}", self.id());
        let mut songs: Vec<(String, Song)> = unique(candidates)
            .into_iter()
            .filter(|song| match self {
                Self::Rediscover => entries
                    .get(song.stream_identifier())
                    .is_some_and(|e| now - e.last_played >= REDISCOVER_AFTER_MILLIS),
                Self::Unheard => !entries.contains_key(song.stream_identifier()),
                Self::Short => song.duration.is_some_and(|d| d > 0.0 && d.is_finite()),
            })
            .map(|song| (md5_hex(format!("{}{salt}", song.stream_identifier()).as_bytes()), song))
            .collect();
        songs.sort_by(|a, b| a.0.cmp(&b.0));
        let mut songs: Vec<Song> = songs.into_iter().map(|(_, s)| s).collect();
        if self == Self::Short {
            let mut remaining = SHORT_MIX_SECONDS;
            songs.retain(|song| match song.duration {
                Some(duration) if duration <= remaining => {
                    remaining -= duration;
                    true
                }
                _ => false,
            });
        }
        songs.truncate(if self == Self::Short { 100 } else { 30 });
        songs
    }
}

/// Uses both advancing media time and a monotonic clock, so seeks and buffering do not count as listening.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MusicListeningClock {
    position: Option<f64>,
    instant: Option<f64>,
}

impl MusicListeningClock {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Returns the seconds listened since the previous observation. `instant` is a monotonic time in seconds.
    pub fn observe(&mut self, position: f64, playing: bool, instant: f64) -> f64 {
        let previous = (self.position, self.instant);
        self.position = position.is_finite().then_some(position);
        self.instant = Some(instant);
        match previous {
            (Some(previous_position), Some(previous_instant))
                if playing && position.is_finite() && instant.is_finite() =>
            {
                (position - previous_position).min((instant - previous_instant).max(0.0)).max(0.0)
            }
            _ => 0.0,
        }
    }
}

/// Everything the discovery features persist per account (`Listening-<account>.json` on iOS).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DiscoveryArchive {
    #[serde(default)]
    pub history: ListeningHistory,
    #[serde(default)]
    pub candidates: Vec<Song>,
    #[serde(default)]
    pub curated: Vec<CuratedMix>,
}

impl DiscoveryArchive {
    /// Cleans an archive after loading.
    pub fn normalized(mut self) -> Self {
        self.candidates = unique(&self.candidates);
        self
    }

    /// Replaces the library sample after a refresh: random songs and favorites, at most 500.
    pub fn set_candidates(&mut self, sample: &[Song], favorites: &[Song]) {
        let mut songs = unique(sample.iter().chain(favorites));
        songs.truncate(CANDIDATE_LIMIT);
        self.candidates = songs;
    }

    /// The songs discovery features choose from.
    pub fn pool(&self, downloaded: &[Song], favorites: &[Song]) -> Vec<Song> {
        unique(
            self.candidates
                .iter()
                .chain(downloaded)
                .chain(self.history.entries.iter().map(|e| &e.song))
                .chain(favorites),
        )
    }

    /// Records listening and updates the resume point. Returns whether anything changed.
    #[allow(clippy::too_many_arguments)]
    pub fn record_playback<Tz: TimeZone>(
        &mut self,
        song: &Song,
        session_id: Uuid,
        listened: f64,
        position: f64,
        duration: f64,
        queue: &PlaybackQueue,
        completed: bool,
        finished_queue: bool,
        now: &DateTime<Tz>,
    ) -> bool {
        if song.is_podcast() || listened.is_nan() || listened < 1.0 {
            return false;
        }
        self.history.record(song, session_id, listened, position, duration, completed, now);
        self.history.update_resume(queue, position, completed, finished_queue);
        true
    }

    pub fn clear_history(&mut self) {
        self.history = ListeningHistory::new();
    }

    /// Saves a mix first, keeping the 12 most recent. Empty new mixes are rejected.
    pub fn save_mix(&mut self, mix: CuratedMix) -> bool {
        if mix.songs.is_empty() && !self.curated.iter().any(|m| m.id == mix.id) {
            return false;
        }
        self.curated.retain(|m| m.id != mix.id);
        self.curated.insert(0, mix);
        self.curated.truncate(CURATED_LIMIT);
        true
    }

    pub fn remove_mix(&mut self, id: Uuid) -> bool {
        let before = self.curated.len();
        self.curated.retain(|m| m.id != id);
        self.curated.len() != before
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use chrono::{FixedOffset, Utc};

    pub(crate) fn at(millis: i64) -> DateTime<Utc> {
        DateTime::from_timestamp_millis(millis).unwrap()
    }

    pub(crate) const NOW: i64 = 1_790_000_000_000;

    fn song(id: &str, duration: f64) -> Song {
        Song {
            artist: Some("Artist".into()),
            album: Some("Album".into()),
            album_id: Some("album".into()),
            duration: Some(duration),
            ..Song::new(id, id)
        }
    }

    #[test]
    fn history_counts_each_qualified_session_once_and_accumulates_only_new_listening() {
        let mut history = ListeningHistory::new();
        let session = Uuid::new_v4();
        let track = song("one", 180.0);
        history.record(&track, session, 15.0, 150.0, 180.0, false, &at(NOW));
        assert_eq!(history.entries()[0].play_count, 0);
        history.record(&track, session, 30.0, 165.0, 180.0, false, &at(NOW));
        history.record(&track, session, 30.0, 5.0, 180.0, false, &at(NOW));
        assert_eq!(history.entries()[0].play_count, 1);
        assert_eq!(history.entries()[0].listened_seconds, 30.0);
        history.record(&track, Uuid::new_v4(), 35.0, 35.0, 180.0, false, &at(NOW));
        assert_eq!(history.entries()[0].play_count, 2);
        assert_eq!(history.entries()[0].listened_seconds, 65.0);
        let restored: ListeningHistory = serde_json::from_str(&serde_json::to_string(&history).unwrap()).unwrap();
        assert_eq!(restored, history);
    }

    #[test]
    fn history_rejects_podcasts_and_invalid_progress() {
        let mut history = ListeningHistory::new();
        let podcast = Song { is_podcast: Some(true), ..Song::new("podcast", "Episode") };
        history.record(&podcast, Uuid::new_v4(), 60.0, 60.0, 180.0, false, &at(NOW));
        history.record(&song("one", 180.0), Uuid::new_v4(), f64::NAN, 60.0, 180.0, false, &at(NOW));
        assert!(history.entries().is_empty());
    }

    #[test]
    fn play_threshold_is_thirty_seconds_or_half_a_shorter_track() {
        assert_eq!(play_threshold(180.0), 30.0);
        assert_eq!(play_threshold(40.0), 20.0);
        assert_eq!(play_threshold(0.0), 30.0);
    }

    #[test]
    fn discovery_separates_unheard_old_and_recent_songs_and_deduplicates() {
        let old = song("old", 180.0);
        let recent = song("recent", 180.0);
        let unheard = song("unheard", 180.0);
        let mut history = ListeningHistory::new();
        history.record(&old, Uuid::new_v4(), 40.0, 40.0, 180.0, false, &at(NOW - 31 * DAY_MILLIS));
        history.record(&recent, Uuid::new_v4(), 40.0, 40.0, 180.0, false, &at(NOW));
        let podcast = Song { is_podcast: Some(true), ..Song::new("podcast", "Episode") };
        let candidates = [old, recent, unheard.clone(), unheard, podcast];
        let ids =
            |mix: DiscoveryMix| mix.songs(&candidates, &history, NOW).into_iter().map(|s| s.id).collect::<Vec<_>>();
        assert_eq!(ids(DiscoveryMix::Rediscover), ["old"]);
        assert_eq!(ids(DiscoveryMix::Unheard), ["unheard"]);
    }

    #[test]
    fn short_mix_never_exceeds_budget_and_ignores_unknown_durations() {
        let mut candidates: Vec<Song> = (0..30).map(|i| song(&i.to_string(), 181.0)).collect();
        candidates.push(Song::new("unknown", "Unknown"));
        candidates.push(song("long", 1_800.0));
        let selection = DiscoveryMix::Short.songs(&candidates, &ListeningHistory::new(), NOW);
        assert_eq!(selection.len(), 8);
        assert!(selection.iter().map(|s| s.duration.unwrap()).sum::<f64>() <= 1_500.0);
        assert_eq!(selection, DiscoveryMix::Short.songs(&candidates, &ListeningHistory::new(), NOW));
    }

    #[test]
    fn resume_retains_current_track_when_bounding_large_queues() {
        let mut queue = PlaybackQueue::new();
        queue.replace((0..1_000).map(|i| song(&i.to_string(), 180.0)).collect(), 800, false);
        let resume = ListeningResume::new(&queue, 45.0).unwrap();
        assert_eq!(resume.current().unwrap().id, "800");
        assert_eq!(resume.songs.len(), 200);
        assert_eq!(resume.position, 45.0);
        assert_eq!(resume.queue().current().unwrap().id, "800");
    }

    #[test]
    fn completed_song_resumes_on_the_next_and_finished_queue_clears_resume() {
        let mut archive = DiscoveryArchive::default();
        let mut queue = PlaybackQueue::new();
        queue.replace(vec![song("a", 180.0), song("b", 180.0)], 0, false);
        let tz = FixedOffset::east_opt(3600).unwrap();
        let now = tz.timestamp_millis_opt(NOW).unwrap();
        assert!(archive.record_playback(
            &song("a", 180.0),
            Uuid::new_v4(),
            180.0,
            180.0,
            180.0,
            &queue,
            true,
            false,
            &now
        ));
        let resume = archive.history.resume.clone().unwrap();
        assert_eq!(resume.current().unwrap().id, "b");
        assert_eq!(resume.position, 0.0);
        archive.record_playback(&song("a", 180.0), Uuid::new_v4(), 30.0, 30.0, 180.0, &queue, false, true, &now);
        assert!(archive.history.resume.is_none());
        assert!(!archive.record_playback(
            &song("a", 180.0),
            Uuid::new_v4(),
            0.5,
            1.0,
            180.0,
            &queue,
            false,
            false,
            &now
        ));
    }

    #[test]
    fn listening_clock_excludes_pause_buffering_and_seeks() {
        let mut clock = MusicListeningClock::new();
        assert_eq!(clock.observe(0.0, true, 0.0), 0.0);
        assert_eq!(clock.observe(1.0, true, 1.0), 1.0);
        assert_eq!(clock.observe(1.0, true, 4.0), 0.0);
        assert_eq!(clock.observe(2.0, false, 5.0), 0.0);
        clock.reset();
        assert_eq!(clock.observe(100.0, true, 6.0), 0.0);
        assert_eq!(clock.observe(101.0, true, 7.0), 1.0);
        assert_eq!(clock.observe(80.0, true, 8.0), 0.0);
    }

    #[test]
    fn archive_keeps_twelve_mixes_and_bounded_candidates() {
        use crate::curated_mix::{CuratedMix, CuratorPreset};
        let mut archive = DiscoveryArchive::default();
        let empty = CuratedMix::new(CuratorPreset::Focus, "Empty", "", &[], "Local", NOW);
        assert!(!archive.save_mix(empty));
        for i in 0..14 {
            archive.save_mix(CuratedMix::new(
                CuratorPreset::Focus,
                &format!("Mix {i}"),
                "",
                &[song("a", 180.0)],
                "Local",
                NOW,
            ));
        }
        assert_eq!(archive.curated.len(), CURATED_LIMIT);
        assert_eq!(archive.curated[0].title, "Mix 13");
        let id = archive.curated[0].id;
        assert!(archive.remove_mix(id));
        assert!(!archive.remove_mix(id));
        let sample: Vec<Song> = (0..600).map(|i| song(&i.to_string(), 180.0)).collect();
        archive.set_candidates(&sample, &[]);
        assert_eq!(archive.candidates.len(), CANDIDATE_LIMIT);
        assert!(refresh_due(None, NOW, false));
        assert!(!refresh_due(Some(NOW - 1_000), NOW, false));
        assert!(refresh_due(Some(NOW - 3_600_001), NOW, false));
    }

    #[test]
    fn discovery_strings_are_translated() {
        for key in [
            "Worth another listen",
            "Still to discover",
            "Have 25 minutes?",
            "Songs you last played at least a month ago on this device.",
            "Songs from your library sample with no listening history on this device.",
            "A selection that fits within 25 minutes, using known track durations.",
            "An album you have not played on this device.",
            "An album you have not played for at least a month on this device.",
            "One of your favorite albums.",
            "Make time for a whole album from your collection.",
            "You have not played this song on this device.",
            "You last played this song over a month ago on this device.",
            "Suggested by your music server.",
            "Server pick",
            "Your collection",
            "Stay with this style",
            "Let me explore",
            "A quiet evening",
            "Time to focus",
            "Last 12 months",
            "All time",
            "Lyrics from LRCLIB",
            "No public feed was found for this podcast. It may be exclusive to Spotify.",
        ] {
            assert!(crate::l10n::has_translation(key), "{key}");
        }
    }

    #[test]
    fn folding_ignores_case_accents_width_and_the() {
        assert_eq!(fold("Città ÉVA Ｂｊö"), "citta eva bjo");
        assert_eq!(normalize_key("Synthetic & Show!"), "syntheticshow");
        assert!(same_artist("The Beatles", "beatles"));
        assert!(!same_artist("", ""));
        assert_eq!(normalize_name("  A   b "), "a b");
    }
}
