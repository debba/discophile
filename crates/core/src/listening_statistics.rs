//! Listening per calendar day, and the statistics computed from it.
//! Days are `yyyy-MM-dd` in the listener's time zone when the listening happened.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use chrono::{Datelike, Days, Months, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::l10n::t;
use crate::listening_history::ListeningHistory;
use crate::models::Song;

/// Days of daily detail kept in the log.
pub const RETAINED_DAYS: u64 = 400;

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Tally {
    #[serde(default)]
    pub seconds: f64,
    #[serde(default)]
    pub plays: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LogDay {
    /// `yyyy-MM-dd` in the listener's calendar.
    pub day: String,
    /// Tallies by stream identifier.
    #[serde(default)]
    pub songs: BTreeMap<String, Tally>,
}

/// Listening per calendar day, so statistics can cover a period. It keeps only library metadata.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ListeningLog {
    #[serde(default)]
    days: Vec<LogDay>,
    #[serde(default)]
    songs: BTreeMap<String, Song>,
}

impl ListeningLog {
    pub fn days(&self) -> &[LogDay] {
        &self.days
    }

    pub fn songs(&self) -> &BTreeMap<String, Song> {
        &self.songs
    }

    pub fn first_day(&self) -> Option<&str> {
        self.days.first().map(|d| d.day.as_str())
    }

    pub fn key(date: NaiveDate) -> String {
        date.format("%Y-%m-%d").to_string()
    }

    pub(crate) fn add(&mut self, song: &Song, seconds: f64, play: bool, today: NaiveDate) {
        if !seconds.is_finite() || !(seconds > 0.0 || play) {
            return;
        }
        let key = Self::key(today);
        let id = song.stream_identifier().to_string();
        // A time zone change can move "today" earlier than the last saved day.
        let index = match self.days.iter().rposition(|d| d.day == key) {
            Some(index) => index,
            None => {
                self.days.push(LogDay { day: key.clone(), songs: BTreeMap::new() });
                self.days.sort_by(|a, b| a.day.cmp(&b.day));
                self.days.iter().position(|d| d.day == key).unwrap_or(0)
            }
        };
        let tally = self.days[index].songs.entry(id.clone()).or_default();
        tally.seconds += seconds.max(0.0);
        if play {
            tally.plays += 1;
        }
        self.songs.insert(id, song.clone());
        let cutoff = today.checked_sub_days(Days::new(RETAINED_DAYS)).map(Self::key).unwrap_or(key);
        if self.days.first().is_some_and(|d| d.day <= cutoff) {
            self.days.retain(|d| d.day > cutoff);
            let kept: BTreeSet<&String> = self.days.iter().flat_map(|d| d.songs.keys()).collect();
            let songs = std::mem::take(&mut self.songs);
            self.songs = songs.into_iter().filter(|(id, _)| kept.contains(id)).collect();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ListeningPeriod {
    Week,
    #[default]
    Month,
    Year,
    All,
}

impl ListeningPeriod {
    pub const ALL: [ListeningPeriod; 4] = [Self::Week, Self::Month, Self::Year, Self::All];

    pub fn title(self) -> String {
        t(match self {
            Self::Week => "Last 7 days",
            Self::Month => "Last 30 days",
            Self::Year => "Last 12 months",
            Self::All => "All time",
        })
    }

    /// Whether the timeline has one bucket per month rather than per day.
    pub fn is_monthly(self) -> bool {
        matches!(self, Self::Year | Self::All)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rank {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub seconds: f64,
    pub plays: i64,
    /// The most listened song in this group, used for artwork and playback.
    pub song: Song,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bucket {
    /// The first day of the bucket (a day, or the first of a month).
    pub start: NaiveDate,
    pub seconds: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ListeningSummary {
    pub period: ListeningPeriod,
    pub seconds: f64,
    pub plays: i64,
    pub song_count: usize,
    pub artist_count: usize,
    pub active_days: usize,
    pub streak: usize,
    pub songs: Vec<Rank>,
    pub artists: Vec<Rank>,
    pub albums: Vec<Rank>,
    pub genres: Vec<Rank>,
    /// Daily buckets for a week or month, monthly buckets for a year or all time.
    pub timeline: Vec<Bucket>,
}

/// A ranking group: identifier, title and subtitle.
type Group = Option<(String, String, Option<String>)>;

fn name(value: Option<&str>) -> Option<String> {
    value.map(str::trim).filter(|v| !v.is_empty()).map(str::to_string)
}

impl ListeningSummary {
    /// The summary for the device's current local day, with the top five of each ranking.
    pub fn now(history: &ListeningHistory, period: ListeningPeriod) -> Self {
        Self::new(history, period, chrono::Local::now().date_naive(), 5)
    }

    /// `today` is the listener's current calendar day.
    pub fn new(history: &ListeningHistory, period: ListeningPeriod, today: NaiveDate, limit: usize) -> Self {
        let log = history.log();
        let day = |offset: u64| today.checked_sub_days(Days::new(offset)).unwrap_or(today);
        let this_month = today.with_day(1).unwrap_or(today);
        let month = |offset: u32| this_month.checked_sub_months(Months::new(offset)).unwrap_or(this_month);
        let day_count: u64 = if period == ListeningPeriod::Week { 7 } else { 30 };
        let earliest = match period {
            ListeningPeriod::All => String::new(),
            ListeningPeriod::Year => ListeningLog::key(month(11)),
            _ => ListeningLog::key(day(day_count - 1)),
        };

        let mut tallies: BTreeMap<String, Tally> = BTreeMap::new();
        let mut catalog: HashMap<String, Song> = log.songs().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        if period == ListeningPeriod::All {
            // Totals saved before daily tracking began are only available per song.
            for entry in history.entries() {
                tallies
                    .insert(entry.id().to_string(), Tally { seconds: entry.listened_seconds, plays: entry.play_count });
                catalog.insert(entry.id().to_string(), entry.song.clone());
            }
        } else {
            for d in log.days().iter().filter(|d| d.day >= earliest) {
                for (id, tally) in &d.songs {
                    let total = tallies.entry(id.clone()).or_default();
                    total.seconds += tally.seconds;
                    total.plays += tally.plays;
                }
            }
        }
        let listened: Vec<(&Song, Tally)> = tallies
            .iter()
            .filter_map(|(id, tally)| catalog.get(id).map(|song| (song, *tally)))
            .filter(|(_, tally)| tally.seconds >= 1.0 || tally.plays > 0)
            .collect();

        let rank = |group: &dyn Fn(&Song) -> Group| -> Vec<Rank> {
            struct Group {
                title: String,
                subtitle: Option<String>,
                seconds: f64,
                plays: i64,
                song: Song,
                best: f64,
            }
            let mut groups: HashMap<String, Group> = HashMap::new();
            for (song, tally) in &listened {
                let Some((id, title, subtitle)) = group(song) else { continue };
                let value = groups.entry(id).or_insert_with(|| Group {
                    title,
                    subtitle,
                    seconds: 0.0,
                    plays: 0,
                    song: (*song).clone(),
                    best: -1.0,
                });
                value.seconds += tally.seconds;
                value.plays += tally.plays;
                if tally.seconds > value.best {
                    value.best = tally.seconds;
                    value.song = (*song).clone();
                }
            }
            let mut ranks: Vec<Rank> = groups
                .into_iter()
                .map(|(id, g)| Rank {
                    id,
                    title: g.title,
                    subtitle: g.subtitle,
                    seconds: g.seconds,
                    plays: g.plays,
                    song: g.song,
                })
                .collect();
            ranks.sort_by(|a, b| {
                b.seconds
                    .total_cmp(&a.seconds)
                    .then(b.plays.cmp(&a.plays))
                    .then(a.title.cmp(&b.title))
                    .then(a.id.cmp(&b.id))
            });
            ranks
        };
        let top = |mut ranks: Vec<Rank>| {
            ranks.truncate(limit);
            ranks
        };

        let artists = rank(&|song| {
            name(song.artist.as_deref())
                .map(|n| (format!("artist:{}", song.artist_id.clone().unwrap_or_else(|| n.to_lowercase())), n, None))
        });
        let artist_count = artists.len();
        let songs = top(rank(&|song| {
            Some((song.stream_identifier().to_string(), song.title.clone(), name(song.artist.as_deref())))
        }));
        let albums = top(rank(&|song| {
            name(song.album.as_deref()).map(|n| {
                (
                    format!("album:{}", song.album_id.clone().unwrap_or_else(|| n.to_lowercase())),
                    n,
                    name(song.artist.as_deref()),
                )
            })
        }));
        let genres =
            top(rank(&|song| name(song.genre.as_deref()).map(|n| (format!("genre:{}", n.to_lowercase()), n, None))));

        let active: BTreeSet<&str> = log
            .days()
            .iter()
            .filter(|d| d.songs.values().any(|t| t.seconds >= 1.0 || t.plays > 0))
            .map(|d| d.day.as_str())
            .collect();
        let active_days = active.iter().filter(|d| **d >= earliest.as_str()).count();
        let mut streak = 0;
        let mut offset = if active.contains(ListeningLog::key(today).as_str()) { 0 } else { 1 };
        while active.contains(ListeningLog::key(day(offset)).as_str()) {
            streak += 1;
            offset += 1;
        }

        let totals: HashMap<&str, f64> =
            log.days().iter().map(|d| (d.day.as_str(), d.songs.values().map(|t| t.seconds).sum())).collect();
        let timeline = if period.is_monthly() {
            let mut months: HashMap<&str, f64> = HashMap::new();
            for (key, seconds) in &totals {
                *months.entry(&key[..7.min(key.len())]).or_default() += seconds;
            }
            let count = match (period, log.first_day()) {
                (ListeningPeriod::Year, _) => 12,
                (_, None) => 0,
                (_, Some(first)) => {
                    let parts: Vec<i32> = first.split('-').filter_map(|p| p.parse().ok()).collect();
                    if parts.len() == 3 {
                        let elapsed = (this_month.year() - parts[0]) * 12 + this_month.month() as i32 - parts[1];
                        (elapsed + 1).max(1) as u32
                    } else {
                        1
                    }
                }
            };
            (0..count)
                .rev()
                .map(|offset| {
                    let start = month(offset);
                    let key = ListeningLog::key(start);
                    Bucket { start, seconds: months.get(&key[..7]).copied().unwrap_or(0.0) }
                })
                .collect()
        } else {
            (0..day_count)
                .rev()
                .map(|offset| {
                    let start = day(offset);
                    Bucket { start, seconds: totals.get(ListeningLog::key(start).as_str()).copied().unwrap_or(0.0) }
                })
                .collect()
        };

        Self {
            period,
            seconds: listened.iter().map(|(_, t)| t.seconds).sum(),
            plays: listened.iter().map(|(_, t)| t.plays).sum(),
            song_count: listened.len(),
            artist_count,
            active_days,
            streak,
            songs,
            artists: top(artists),
            albums,
            genres,
            timeline,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.seconds < 1.0 && self.plays == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Duration, FixedOffset, TimeZone};
    use uuid::Uuid;

    // Europe/Rome in October 2026, before the DST change.
    fn tz() -> FixedOffset {
        FixedOffset::east_opt(2 * 3600).unwrap()
    }
    fn now() -> DateTime<FixedOffset> {
        tz().with_ymd_and_hms(2026, 10, 21, 21, 0, 0).unwrap()
    }
    fn today() -> NaiveDate {
        now().date_naive()
    }
    fn days_ago(days: i64) -> DateTime<FixedOffset> {
        now() - Duration::days(days)
    }
    fn song(id: &str, artist: &str, album: &str, genre: Option<&str>) -> Song {
        Song {
            artist: Some(artist.into()),
            album: Some(album.into()),
            album_id: Some(album.to_lowercase()),
            duration: Some(200.0),
            genre: genre.map(str::to_string),
            ..Song::new(id, id)
        }
    }
    fn simple(id: &str) -> Song {
        song(id, "Artist", "Album", Some("Rock"))
    }
    fn listen(history: &mut ListeningHistory, song: &Song, seconds: f64, at: DateTime<FixedOffset>) {
        history.record(song, Uuid::new_v4(), seconds, seconds, 200.0, false, &at);
    }

    #[test]
    fn log_splits_listening_by_local_day_and_counts_plays_once() {
        let mut history = ListeningHistory::new();
        let session = Uuid::new_v4();
        let track = simple("one");
        // 23:30 and 00:30 local time fall on different days even though they are an hour apart.
        let late = tz().with_ymd_and_hms(2026, 10, 20, 23, 30, 0).unwrap();
        history.record(&track, session, 40.0, 40.0, 200.0, false, &late);
        history.record(&track, session, 100.0, 100.0, 200.0, false, &(late + Duration::hours(1)));
        let days = history.log().days();
        assert_eq!(days.iter().map(|d| d.day.as_str()).collect::<Vec<_>>(), ["2026-10-20", "2026-10-21"]);
        assert_eq!(days.iter().map(|d| d.songs["one"].seconds).collect::<Vec<_>>(), [40.0, 60.0]);
        assert_eq!(days.iter().map(|d| d.songs["one"].plays).collect::<Vec<_>>(), [1, 0]);
    }

    #[test]
    fn periods_rank_songs_artists_albums_and_genres_by_listening_time() {
        let mut history = ListeningHistory::new();
        listen(&mut history, &song("a", "Alpha", "First", Some("Rock")), 120.0, days_ago(1));
        listen(&mut history, &song("b", "Beta", "Second", Some("Jazz")), 100.0, days_ago(2));
        listen(&mut history, &song("c", "Beta", "Second", Some("Jazz")), 90.0, days_ago(3));
        listen(&mut history, &song("old", "Gamma", "Old", Some("Rock")), 150.0, days_ago(20));
        let week = ListeningSummary::new(&history, ListeningPeriod::Week, today(), 5);
        let titles = |ranks: &[Rank]| ranks.iter().map(|r| r.title.clone()).collect::<Vec<_>>();
        assert_eq!(week.seconds, 310.0);
        assert_eq!(week.plays, 3);
        assert_eq!(week.song_count, 3);
        assert_eq!(week.songs.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["a", "b", "c"]);
        assert_eq!(titles(&week.artists), ["Beta", "Alpha"]);
        assert_eq!(week.artists[0].song.id, "b");
        assert_eq!(titles(&week.albums), ["Second", "First"]);
        assert_eq!(titles(&week.genres), ["Jazz", "Rock"]);
        assert_eq!(week.timeline.len(), 7);
        assert_eq!(week.timeline.last().unwrap().start, today());
        assert_eq!(
            week.timeline.iter().map(|b| b.seconds).collect::<Vec<_>>(),
            [0.0, 0.0, 0.0, 90.0, 100.0, 120.0, 0.0]
        );
        let month = ListeningSummary::new(&history, ListeningPeriod::Month, today(), 5);
        assert_eq!(month.seconds, 460.0);
        assert_eq!(titles(&month.artists), ["Beta", "Gamma", "Alpha"]);
        assert_eq!(month.timeline.len(), 30);
    }

    #[test]
    fn all_time_includes_totals_saved_before_daily_tracking() {
        let mut history = ListeningHistory::new();
        listen(&mut history, &simple("tracked"), 60.0, days_ago(1));
        // Simulate an archive written before the log existed.
        let mut json: serde_json::Value = serde_json::to_value(&history).unwrap();
        json.as_object_mut().unwrap().remove("log");
        let mut legacy: ListeningHistory = serde_json::from_value(json).unwrap();
        assert!(legacy.log().days().is_empty());
        listen(&mut legacy, &simple("new"), 30.0, days_ago(0));
        let all = ListeningSummary::new(&legacy, ListeningPeriod::All, today(), 5);
        assert_eq!(all.seconds, 90.0);
        assert_eq!(all.songs.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["tracked", "new"]);
        assert_eq!(all.timeline.len(), 1);
        assert_eq!(ListeningSummary::new(&legacy, ListeningPeriod::Week, today(), 5).seconds, 30.0);
        let restored: ListeningHistory = serde_json::from_str(&serde_json::to_string(&legacy).unwrap()).unwrap();
        assert_eq!(restored, legacy);
    }

    #[test]
    fn year_uses_twelve_monthly_buckets_and_streak_counts_consecutive_days() {
        let mut history = ListeningHistory::new();
        for day in [1, 2, 3, 5] {
            listen(&mut history, &simple(&day.to_string()), 60.0, days_ago(day));
        }
        listen(&mut history, &simple("spring"), 600.0, tz().with_ymd_and_hms(2026, 3, 2, 12, 0, 0).unwrap());
        listen(&mut history, &simple("ancient"), 600.0, tz().with_ymd_and_hms(2025, 10, 2, 12, 0, 0).unwrap());
        let year = ListeningSummary::new(&history, ListeningPeriod::Year, today(), 5);
        assert_eq!(year.timeline.len(), 12);
        assert_eq!(year.timeline[0].start, NaiveDate::from_ymd_opt(2025, 11, 1).unwrap());
        assert_eq!(year.timeline.last().unwrap().seconds, 240.0);
        assert_eq!(year.seconds, 840.0);
        assert_eq!(year.active_days, 5);
        // No listening yet today, so the streak runs from yesterday back to three days ago.
        assert_eq!(year.streak, 3);
        assert_eq!(ListeningSummary::new(&history, ListeningPeriod::All, today(), 5).timeline.len(), 13);
    }

    #[test]
    fn log_drops_days_outside_retention_and_their_metadata() {
        let mut history = ListeningHistory::new();
        listen(&mut history, &simple("forgotten"), 60.0, days_ago(RETAINED_DAYS as i64 + 5));
        listen(&mut history, &simple("recent"), 60.0, days_ago(0));
        assert_eq!(history.log().days().len(), 1);
        assert_eq!(history.log().songs().keys().collect::<Vec<_>>(), ["recent"]);
    }

    #[test]
    fn empty_history_has_no_statistics() {
        let summary = ListeningSummary::new(&ListeningHistory::new(), ListeningPeriod::All, today(), 5);
        assert!(summary.is_empty());
        assert!(summary.timeline.is_empty());
        assert_eq!(summary.streak, 0);
    }
}
