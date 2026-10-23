//! Audiobook reader logic: chapter and book timelines, time display, skip intervals, playback
//! speed, bookmarks, repeat, the "end of chapter" sleep timer and listening-time accounting.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::audiobookshelf::{AudiobookChapter, AudiobookPlaybackSession, AudiobookTrack};
use crate::error::{MusicError, MusicResult};

/// Pressing "previous" later than this into a chapter restarts it instead of going back.
pub const PREVIOUS_RESTART_THRESHOLD: f64 = 3.0;
/// Longest bookmark title, in characters.
pub const BOOKMARK_TITLE_LIMIT: usize = 200;

// MARK: Timeline

#[derive(Debug, Clone, PartialEq)]
pub struct TimelineChapter {
    /// Zero-based position in the timeline.
    pub id: usize,
    pub title: String,
    pub start: f64,
    pub end: f64,
}

impl TimelineChapter {
    pub fn duration(&self) -> f64 {
        self.end - self.start
    }

    /// The chapter title, or "Chapter N" for untitled chapters and file boundaries.
    pub fn display_title(&self) -> String {
        if self.title.is_empty() { crate::count("Chapter %lld", self.id as i64 + 1) } else { self.title.clone() }
    }
}

/// What the scrubber and the time labels measure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimelineScope {
    #[default]
    Chapter,
    Book,
}

impl TimelineScope {
    pub const ALL: [TimelineScope; 2] = [Self::Chapter, Self::Book];
    pub fn toggled(self) -> Self {
        match self {
            Self::Chapter => Self::Book,
            Self::Book => Self::Chapter,
        }
    }
}

/// A span of the book shown by the scrubber. Offsets are relative; positions are absolute.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimelineWindow {
    pub start: f64,
    pub end: f64,
}

impl TimelineWindow {
    pub fn duration(&self) -> f64 {
        self.end - self.start
    }
    pub fn elapsed(&self, position: f64) -> f64 {
        let offset = if position.is_finite() { position - self.start } else { 0.0 };
        offset.max(0.0).min(self.duration())
    }
    pub fn remaining(&self, position: f64) -> f64 {
        self.duration() - self.elapsed(position)
    }
    /// The absolute book position for an offset inside the window.
    pub fn position(&self, offset: f64) -> f64 {
        let offset = if offset.is_finite() { offset } else { 0.0 };
        self.start + offset.max(0.0).min(self.duration())
    }
}

/// Contiguous chapters covering a book. Invalid, duplicate and out-of-range chapters are
/// dropped; without chapters, file boundaries are used; a leading gap becomes an untitled chapter.
#[derive(Debug, Clone, PartialEq)]
pub struct AudiobookTimeline {
    pub duration: f64,
    pub chapters: Vec<TimelineChapter>,
}

impl AudiobookTimeline {
    pub fn new(duration: f64, chapters: &[AudiobookChapter], tracks: &[AudiobookTrack]) -> Self {
        let duration = if duration.is_finite() { duration.max(0.0) } else { 0.0 };
        let mut valid: Vec<&AudiobookChapter> = chapters
            .iter()
            .filter(|c| c.start.is_finite() && c.end.is_finite() && c.start >= 0.0 && c.start < duration && c.end > c.start)
            .collect();
        valid.sort_by(|a, b| a.start.total_cmp(&b.start));
        let mut boundaries: Vec<(f64, String)> = Vec::new();
        for chapter in valid {
            if boundaries.last().map(|b| b.0) != Some(chapter.start) {
                boundaries.push((chapter.start, chapter.title.clone()));
            }
        }
        if boundaries.is_empty() {
            for track in tracks {
                let start = track.start_offset;
                if start.is_finite() && start >= 0.0 && start < duration && track.duration > 0.0 && !boundaries.iter().any(|b| b.0 == start)
                {
                    boundaries.push((start, String::new()));
                }
            }
            boundaries.sort_by(|a, b| a.0.total_cmp(&b.0));
        }
        if boundaries.first().map(|b| b.0) != Some(0.0) && duration > 0.0 {
            boundaries.insert(0, (0.0, String::new()));
        }
        let chapters = boundaries
            .iter()
            .enumerate()
            .map(|(index, (start, title))| TimelineChapter {
                id: index,
                title: title.clone(),
                start: *start,
                end: boundaries.get(index + 1).map(|b| b.0).unwrap_or(duration),
            })
            .collect();
        Self { duration, chapters }
    }

    /// The timeline of a playback session, using the book's chapters when the session has none.
    pub fn for_session(session: &AudiobookPlaybackSession, book_chapters: Option<&[AudiobookChapter]>) -> Self {
        let chapters = session.chapters.as_deref().or(book_chapters).unwrap_or_default();
        Self::new(session.duration, chapters, &session.audio_tracks)
    }

    pub fn chapter(&self, position: f64) -> Option<&TimelineChapter> {
        let position = if position.is_finite() { position.max(0.0).min(self.duration) } else { 0.0 };
        self.chapters.iter().rev().find(|c| c.start <= position)
    }

    pub fn window(&self, position: f64, scope: TimelineScope) -> TimelineWindow {
        if scope == TimelineScope::Chapter
            && let Some(chapter) = self.chapter(position)
        {
            return TimelineWindow { start: chapter.start, end: chapter.end };
        }
        TimelineWindow { start: 0.0, end: self.duration }
    }

    /// Restarts the current chapter, or goes to the previous one within the first three seconds.
    pub fn previous(&self, position: f64) -> Option<f64> {
        let chapter = self.chapter(position)?;
        if position - chapter.start > PREVIOUS_RESTART_THRESHOLD {
            return Some(chapter.start);
        }
        Some(self.chapters[chapter.id.saturating_sub(1)].start)
    }

    pub fn next(&self, position: f64) -> Option<f64> {
        let chapter = self.chapter(position)?;
        self.chapters.get(chapter.id + 1).map(|c| c.start)
    }

    /// `mm:ss`, or `hh:mm:ss` from one hour; non-finite and negative values show `00:00`.
    pub fn clock(seconds: f64) -> String {
        if !seconds.is_finite() || seconds <= 0.0 {
            return "00:00".into();
        }
        let seconds = seconds.min(315_360_000.0) as i64;
        if seconds >= 3600 {
            format!("{:02}:{:02}:{:02}", seconds / 3600, seconds / 60 % 60, seconds % 60)
        } else {
            format!("{:02}:{:02}", seconds / 60, seconds % 60)
        }
    }
}

// MARK: Preferences

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AudiobookReaderPreferences {
    pub scope: TimelineScope,
    pub show_remaining: bool,
    /// Show times as they will pass at the current playback speed.
    pub adjust_time_for_speed: bool,
    pub backward_seconds: u32,
    pub forward_seconds: u32,
    pub rate: f64,
}

impl Default for AudiobookReaderPreferences {
    fn default() -> Self {
        Self {
            scope: TimelineScope::Chapter,
            show_remaining: true,
            adjust_time_for_speed: false,
            backward_seconds: 30,
            forward_seconds: 30,
            rate: 1.0,
        }
    }
}

impl AudiobookReaderPreferences {
    pub const SKIP_INTERVALS: [u32; 10] = [5, 10, 15, 30, 45, 60, 90, 120, 180, 300];
    pub const MIN_RATE: f64 = 0.5;
    pub const MAX_RATE: f64 = 4.0;
    pub const RATE_STEP: f64 = 0.05;

    /// A playback rate between 0.5× and 4×, in 0.05 steps.
    pub fn rate(value: f64) -> f64 {
        if !value.is_finite() {
            return 1.0;
        }
        (value.clamp(Self::MIN_RATE, Self::MAX_RATE) * 20.0).round() / 20.0
    }

    pub fn validated(&self) -> Self {
        let mut value = self.clone();
        value.rate = Self::rate(self.rate);
        if !Self::SKIP_INTERVALS.contains(&self.backward_seconds) {
            value.backward_seconds = 30;
        }
        if !Self::SKIP_INTERVALS.contains(&self.forward_seconds) {
            value.forward_seconds = 30;
        }
        value
    }

    /// A duration as displayed: divided by the speed when "adjust times to speed" is on.
    pub fn displayed_time(&self, seconds: f64, rate: f64) -> f64 {
        let seconds = if seconds.is_finite() { seconds.max(0.0) } else { 0.0 };
        if self.adjust_time_for_speed { seconds / Self::rate(rate) } else { seconds }
    }

    /// The seek target for the skip buttons, before clamping to the book.
    pub fn skip_target(&self, elapsed: f64, forward: bool) -> f64 {
        if forward { elapsed + self.forward_seconds as f64 } else { elapsed - self.backward_seconds as f64 }
    }
}

// MARK: Bookmarks

/// A saved moment in a book, scoped to an Audiobookshelf account (`AudiobookshelfAccount::id`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudiobookBookmark {
    pub id: Uuid,
    pub account: String,
    pub book: String,
    pub position: f64,
    pub title: String,
    /// Unix milliseconds.
    pub created_at: i64,
}

fn bookmark_title(title: &str) -> String {
    title.chars().take(BOOKMARK_TITLE_LIMIT).collect()
}

impl AudiobookBookmark {
    pub fn new(account: &str, book: &str, position: f64, title: &str) -> MusicResult<Self> {
        if account.is_empty() || book.is_empty() || !position.is_finite() || position < 0.0 {
            return Err(MusicError::InvalidResponse);
        }
        Ok(Self {
            id: Uuid::new_v4(),
            account: account.to_string(),
            book: book.to_string(),
            position,
            title: bookmark_title(title),
            created_at: crate::now_millis(),
        })
    }

    fn is_valid(&self) -> bool {
        !self.account.is_empty()
            && !self.book.is_empty()
            && self.position.is_finite()
            && self.position >= 0.0
            && self.title.chars().count() <= BOOKMARK_TITLE_LIMIT
    }

    /// Reads saved bookmarks, rejecting the whole file if any entry is malformed or duplicated.
    pub fn decode(data: &[u8]) -> MusicResult<Vec<Self>> {
        let values: Vec<Self> = serde_json::from_slice(data).map_err(|_| MusicError::InvalidResponse)?;
        let ids: std::collections::HashSet<Uuid> = values.iter().map(|b| b.id).collect();
        if ids.len() != values.len() || !values.iter().all(Self::is_valid) {
            return Err(MusicError::InvalidResponse);
        }
        Ok(values)
    }

    pub fn encode(values: &[Self]) -> MusicResult<Vec<u8>> {
        serde_json::to_vec(values).map_err(|_| MusicError::InvalidResponse)
    }

    /// The list title: the bookmark's own title, or its position.
    pub fn display_title(&self) -> String {
        if self.title.is_empty() { AudiobookTimeline::clock(self.position) } else { self.title.clone() }
    }

    /// A book's bookmarks in timeline order.
    pub fn for_book<'a>(values: &'a [Self], account: &str, book: &str) -> Vec<&'a Self> {
        let mut matching: Vec<&Self> = values.iter().filter(|b| b.account == account && b.book == book).collect();
        matching.sort_by(|a, b| a.position.total_cmp(&b.position));
        matching
    }

    /// The bookmarks after renaming `id`; the title is trimmed and shortened. `None` if absent.
    pub fn renamed(values: &[Self], id: Uuid, title: &str) -> Option<Vec<Self>> {
        let mut updated = values.to_vec();
        let bookmark = updated.iter_mut().find(|b| b.id == id)?;
        bookmark.title = bookmark_title(title.trim());
        Some(updated)
    }

    pub fn removed(values: &[Self], id: Uuid) -> Vec<Self> {
        values.iter().filter(|b| b.id != id).cloned().collect()
    }

    /// The default title for a bookmark created from the player: the chapter, else the book.
    pub fn suggested_title(chapter: Option<&TimelineChapter>, book_title: &str) -> String {
        chapter.map(TimelineChapter::display_title).unwrap_or_else(|| book_title.to_string())
    }
}

// MARK: Repeat

/// Books that restart when they end, keyed by account and book.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RepeatingAudiobooks(pub BTreeSet<String>);

impl RepeatingAudiobooks {
    fn key(account: &str, book: &str) -> String {
        format!("{account}:{book}")
    }
    pub fn contains(&self, account: &str, book: &str) -> bool {
        self.0.contains(&Self::key(account, book))
    }
    pub fn set(&mut self, account: &str, book: &str, enabled: bool) {
        let key = Self::key(account, book);
        if enabled {
            self.0.insert(key);
        } else {
            self.0.remove(&key);
        }
    }
}

// MARK: Playback helpers

/// The book position where an "end of current chapter" sleep timer pauses, if the chapter has not ended.
pub fn end_of_chapter_stop(timeline: &AudiobookTimeline, elapsed: f64) -> Option<f64> {
    timeline.chapter(elapsed).filter(|c| c.end > elapsed).map(|c| c.end)
}

/// Where playback of file `track` should stop early for a sleep position, as an offset in that file.
pub fn sleep_stop_offset(session: &AudiobookPlaybackSession, track: usize, stop: Option<f64>) -> Option<f64> {
    let track = session.audio_tracks.get(track)?;
    let stop = stop?;
    (stop > track.start_offset && stop <= track.start_offset + track.duration).then_some(stop - track.start_offset)
}

/// Whether seeking to `target` cancels an end-of-chapter sleep position.
pub fn seek_clears_sleep_stop(target: f64, stop: Option<f64>) -> bool {
    stop.is_some_and(|stop| target >= stop)
}

/// What to do when a file finishes playing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrackEndAction {
    /// The sleep timer ends here: set the position, clear the stop and pause.
    PauseAt(f64),
    /// Load the file starting at this book position.
    Continue(f64),
    /// The book ended and repeats: start again at zero.
    Restart,
    /// The book ended: set the position to the duration and pause.
    Finish,
}

pub fn track_end_action(session: &AudiobookPlaybackSession, track: usize, sleep_stop: Option<f64>, repeating: bool) -> TrackEndAction {
    if let (Some(stop), Some(current)) = (sleep_stop, session.audio_tracks.get(track))
        && stop <= current.start_offset + current.duration + 0.1
    {
        return TrackEndAction::PauseAt(stop);
    }
    if let Some(next) = session.audio_tracks.get(track + 1) {
        TrackEndAction::Continue(next.start_offset)
    } else if repeating {
        TrackEndAction::Restart
    } else {
        TrackEndAction::Finish
    }
}

/// Counts real listening time between progress reports. Ticks further apart than two seconds
/// (sleep, suspended tabs) count as two seconds.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ListeningMeter {
    listened: f64,
    last_tick_ms: i64,
    last_report_ms: i64,
}

impl ListeningMeter {
    /// Seconds between periodic progress reports while playing.
    pub const REPORT_INTERVAL_MS: i64 = 15_000;
    const MAX_TICK_SECONDS: f64 = 2.0;

    pub fn start(now_ms: i64) -> Self {
        Self { listened: 0.0, last_tick_ms: now_ms, last_report_ms: now_ms }
    }

    /// Records a player tick. Returns `true` when a periodic report is due.
    pub fn tick(&mut self, now_ms: i64, playing: bool) -> bool {
        if playing {
            let seconds = (now_ms - self.last_tick_ms) as f64 / 1000.0;
            self.listened += seconds.clamp(0.0, Self::MAX_TICK_SECONDS);
        }
        self.last_tick_ms = now_ms;
        playing && now_ms - self.last_report_ms >= Self::REPORT_INTERVAL_MS
    }

    /// Restarts tick measurement without counting the gap (after loading or seeking).
    pub fn resume(&mut self, now_ms: i64) {
        self.last_tick_ms = now_ms;
    }

    pub fn listened(&self) -> f64 {
        self.listened
    }

    /// Returns the seconds listened since the last report and starts a new period.
    pub fn take(&mut self, now_ms: i64) -> f64 {
        let listened = self.listened;
        self.listened = 0.0;
        self.last_report_ms = now_ms;
        listened
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chapter(id: i64, start: f64, end: f64, title: &str) -> AudiobookChapter {
        AudiobookChapter { id, start, end, title: title.into() }
    }

    fn chapters() -> Vec<AudiobookChapter> {
        vec![chapter(3, 0.0, 3600.0, "First"), chapter(9, 3600.0, 7205.0, "Second")]
    }

    fn track(start: f64, duration: f64, url: &str) -> AudiobookTrack {
        AudiobookTrack { start_offset: start, duration, content_url: url.into(), mime_type: None }
    }

    #[test]
    fn chapter_and_book_windows_keep_absolute_seek_positions() {
        let timeline = AudiobookTimeline::new(7205.0, &chapters(), &[]);
        let window = timeline.window(3665.0, TimelineScope::Chapter);
        assert_eq!(window.elapsed(3665.0), 65.0);
        assert_eq!(window.remaining(3665.0), 3540.0);
        assert_eq!(window.position(90.0), 3690.0);
        assert_eq!(window.position(9999.0), 7205.0);
        assert_eq!(window.position(f64::NEG_INFINITY), 3600.0);
        assert_eq!(timeline.window(3665.0, TimelineScope::Book).elapsed(3665.0), 3665.0);
        assert_eq!(timeline.chapter(3600.0).unwrap().title, "Second");
        assert_eq!(timeline.chapter(7205.0).unwrap().id, 1);
    }

    #[test]
    fn previous_restarts_current_chapter_then_moves_to_previous_within_three_seconds() {
        let timeline = AudiobookTimeline::new(7205.0, &chapters(), &[]);
        assert_eq!(timeline.previous(3610.0), Some(3600.0));
        assert_eq!(timeline.previous(3603.0), Some(0.0));
        assert_eq!(timeline.previous(0.0), Some(0.0));
        assert_eq!(timeline.next(100.0), Some(3600.0));
        assert_eq!(timeline.next(3600.0), None);
    }

    #[test]
    fn unordered_invalid_and_duplicate_chapters_produce_contiguous_windows() {
        let input = vec![
            chapter(0, 90.0, 110.0, "Last"),
            chapter(1, 20.0, 30.0, "Middle"),
            chapter(2, 20.0, 50.0, "Duplicate"),
            chapter(3, -1.0, 5.0, "Invalid"),
            chapter(4, f64::NAN, 10.0, "Invalid"),
            chapter(5, 120.0, 200.0, "Outside"),
        ];
        let timeline = AudiobookTimeline::new(120.0, &input, &[]);
        assert_eq!(timeline.chapters.iter().map(|c| c.start).collect::<Vec<_>>(), [0.0, 20.0, 90.0]);
        assert_eq!(timeline.chapters.iter().map(|c| c.end).collect::<Vec<_>>(), [20.0, 90.0, 120.0]);
        assert_eq!(timeline.chapters[1].title, "Middle");
        assert_eq!(timeline.chapter(f64::NAN).unwrap().start, 0.0);
        assert_eq!(timeline.chapter(-100.0).unwrap().start, 0.0);
    }

    #[test]
    fn track_boundaries_provide_chapter_fallback_and_empty_books_stay_safe() {
        let tracks = [track(60.0, 60.0, "/two"), track(0.0, 60.0, "/one")];
        let timeline = AudiobookTimeline::new(120.0, &[], &tracks);
        assert_eq!(timeline.chapters.iter().map(|c| c.start).collect::<Vec<_>>(), [0.0, 60.0]);
        assert_eq!(timeline.chapter(75.0).unwrap().duration(), 60.0);
        assert_eq!(AudiobookTimeline::new(120.0, &[], &[]).chapters.len(), 1);
        let empty = AudiobookTimeline::new(f64::INFINITY, &chapters(), &[]);
        assert!(empty.chapters.is_empty());
        assert_eq!(empty.previous(0.0), None);
        assert_eq!(empty.next(0.0), None);
        assert_eq!(empty.window(f64::NAN, TimelineScope::Chapter).remaining(0.0), 0.0);
    }

    #[test]
    fn clock_shows_hours_and_handles_nonfinite_values() {
        assert_eq!(AudiobookTimeline::clock(65.9), "01:05");
        assert_eq!(AudiobookTimeline::clock(3600.0), "01:00:00");
        assert_eq!(AudiobookTimeline::clock(39006.0), "10:50:06");
        for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0] {
            assert_eq!(AudiobookTimeline::clock(invalid), "00:00");
        }
    }

    #[test]
    fn preferences_validate_rates_and_skip_intervals_without_changing_timeline() {
        let mut preferences = AudiobookReaderPreferences {
            scope: TimelineScope::Book,
            show_remaining: false,
            backward_seconds: 7,
            forward_seconds: 90,
            rate: 1.234,
            ..Default::default()
        };
        let saved = preferences.validated();
        assert_eq!(saved.backward_seconds, 30);
        assert_eq!(saved.forward_seconds, 90);
        assert_eq!(saved.rate, 1.25);
        let json = serde_json::to_vec(&saved).unwrap();
        assert_eq!(serde_json::from_slice::<AudiobookReaderPreferences>(&json).unwrap(), saved);
        assert_eq!(AudiobookReaderPreferences::rate(f64::NAN), 1.0);
        assert_eq!(AudiobookReaderPreferences::rate(0.1), 0.5);
        assert_eq!(AudiobookReaderPreferences::rate(10.0), 4.0);
        assert_eq!(preferences.displayed_time(3600.0, 2.0), 3600.0);
        preferences.adjust_time_for_speed = true;
        assert_eq!(preferences.displayed_time(3600.0, 2.0), 1800.0);
        assert_eq!(preferences.displayed_time(f64::NAN, 2.0), 0.0);
        assert_eq!(saved.skip_target(100.0, false), 70.0);
        assert_eq!(saved.skip_target(100.0, true), 190.0);
        // Missing keys fall back to defaults.
        let partial: AudiobookReaderPreferences = serde_json::from_str(r#"{"scope":"book"}"#).unwrap();
        assert_eq!(partial.scope, TimelineScope::Book);
        assert_eq!(partial.forward_seconds, 30);
    }

    #[test]
    fn bookmarks_round_trip_preserves_account_book_and_exact_media_time() {
        let first = AudiobookBookmark::new("first", "book", 3665.25, "Moment").unwrap();
        let other = AudiobookBookmark::new("other", "book", 5.0, &"a".repeat(250)).unwrap();
        let decoded = AudiobookBookmark::decode(&AudiobookBookmark::encode(&[first.clone(), other.clone()]).unwrap()).unwrap();
        assert_eq!(decoded, vec![first, other]);
        assert_eq!(decoded[1].title.chars().count(), 200);
        assert_ne!(decoded[0].account, decoded[1].account);
        assert_eq!(decoded[0].position, 3665.25);
    }

    #[test]
    fn malformed_and_duplicate_bookmarks_are_rejected() {
        for position in [f64::NAN, f64::INFINITY, -1.0] {
            assert!(AudiobookBookmark::new("account", "book", position, "").is_err());
        }
        assert!(AudiobookBookmark::new("", "book", 0.0, "").is_err());
        let bookmark = AudiobookBookmark::new("account", "book", 20.0, "Saved").unwrap();
        assert!(AudiobookBookmark::decode(&AudiobookBookmark::encode(&[bookmark.clone(), bookmark.clone()]).unwrap()).is_err());
        let text = String::from_utf8(AudiobookBookmark::encode(&[bookmark]).unwrap()).unwrap();
        let invalid = text.replace("\"position\":20.0", "\"position\":-20.0");
        assert_ne!(invalid, text);
        assert!(AudiobookBookmark::decode(invalid.as_bytes()).is_err());
    }

    #[test]
    fn bookmark_lists_rename_and_titles() {
        let late = AudiobookBookmark::new("a", "book", 90.0, "").unwrap();
        let early = AudiobookBookmark::new("a", "book", 10.0, "Early").unwrap();
        let elsewhere = AudiobookBookmark::new("b", "book", 5.0, "Other").unwrap();
        let all = vec![late.clone(), early.clone(), elsewhere];
        let ids: Vec<Uuid> = AudiobookBookmark::for_book(&all, "a", "book").iter().map(|b| b.id).collect();
        assert_eq!(ids, [early.id, late.id]);
        assert_eq!(late.display_title(), "01:30");
        let renamed = AudiobookBookmark::renamed(&all, late.id, "  Renamed  ").unwrap();
        assert_eq!(renamed[0].title, "Renamed");
        assert!(AudiobookBookmark::renamed(&all, Uuid::new_v4(), "x").is_none());
        assert_eq!(AudiobookBookmark::removed(&all, early.id).len(), 2);
        crate::l10n::set_language(crate::l10n::Language::English);
        let timeline = AudiobookTimeline::new(120.0, &[], &[track(0.0, 60.0, "/a"), track(60.0, 60.0, "/b")]);
        assert_eq!(AudiobookBookmark::suggested_title(timeline.chapter(70.0), "Book"), "Chapter 2");
        assert_eq!(AudiobookBookmark::suggested_title(None, "Book"), "Book");
    }

    #[test]
    fn repeat_is_scoped_to_account_and_book() {
        let mut repeating = RepeatingAudiobooks::default();
        repeating.set("a", "book", true);
        assert!(repeating.contains("a", "book"));
        assert!(!repeating.contains("b", "book"));
        let json = serde_json::to_string(&repeating).unwrap();
        assert_eq!(json, r#"["a:book"]"#);
        repeating.set("a", "book", false);
        assert!(!repeating.contains("a", "book"));
    }

    fn session() -> AudiobookPlaybackSession {
        AudiobookPlaybackSession {
            id: "session".into(),
            library_item_id: "book".into(),
            duration: 180.0,
            current_time: 0.0,
            audio_tracks: vec![track(0.0, 60.0, "/s/a/b/1.mp3"), track(60.0, 120.0, "/s/a/b/2.mp3")],
            chapters: Some(vec![chapter(0, 0.0, 100.0, "Beginning"), chapter(1, 100.0, 180.0, "Ending")]),
        }
    }

    #[test]
    fn end_of_chapter_sleep_timer_stops_inside_the_right_file() {
        let session = session();
        let timeline = AudiobookTimeline::for_session(&session, None);
        let stop = end_of_chapter_stop(&timeline, 30.0);
        assert_eq!(stop, Some(100.0));
        assert_eq!(end_of_chapter_stop(&timeline, 180.0), None);
        assert_eq!(sleep_stop_offset(&session, 0, stop), None);
        assert_eq!(sleep_stop_offset(&session, 1, stop), Some(40.0));
        assert_eq!(track_end_action(&session, 0, stop, false), TrackEndAction::Continue(60.0));
        assert_eq!(track_end_action(&session, 1, stop, false), TrackEndAction::PauseAt(100.0));
        assert_eq!(track_end_action(&session, 1, None, false), TrackEndAction::Finish);
        assert_eq!(track_end_action(&session, 1, None, true), TrackEndAction::Restart);
        assert!(seek_clears_sleep_stop(120.0, stop));
        assert!(!seek_clears_sleep_stop(20.0, stop));
    }

    #[test]
    fn listening_meter_caps_gaps_and_reports_every_fifteen_seconds() {
        let mut meter = ListeningMeter::start(0);
        assert!(!meter.tick(1_000, true));
        assert!(!meter.tick(11_000, true)); // a 10 s gap counts as 2 s
        assert!(!meter.tick(12_000, false));
        assert_eq!(meter.listened(), 3.0);
        assert!(meter.tick(15_000, true));
        assert_eq!(meter.take(15_000), 5.0);
        assert!(!meter.tick(16_000, true));
    }
}
