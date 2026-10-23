//! LRCLIB online lyrics. Requests are unauthenticated and carry only public song metadata;
//! server credentials never reach this client.
//!
//! The lookup is split into pure steps ([`get_url`], [`search_url`], [`interpret_reply`],
//! [`lookup`]) so it can be tested with fixtures; [`LrcLibClient`] adds HTTP and rate limiting.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use serde::Deserialize;
use url::Url;

use crate::error::MusicError;
use crate::listening_history::normalize_key;
use crate::lyrics::SongLyrics;
use crate::models::Song;
use crate::now_millis;

pub const API_BASE: &str = "https://lrclib.net/api/";
pub const USER_AGENT: &str = "Discophile/0.1.0 (https://github.com/debba/discophile)";
pub const MAX_RESPONSE_BYTES: usize = 2_000_000;
/// Minimum spacing between requests, and how often a waiting request checks again.
pub const REQUEST_SPACING_MILLIS: i64 = 300;
const POLL_MILLIS: u64 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum LyricsError {
    /// LRCLIB asked to wait until this Unix time in milliseconds. No request is sent before then.
    #[error("rate limited")]
    RateLimited { until: i64 },
    #[error(transparent)]
    Music(#[from] MusicError),
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LrcLibRecord {
    pub track_name: String,
    pub artist_name: String,
    #[serde(default)]
    pub album_name: Option<String>,
    pub duration: f64,
    pub instrumental: bool,
    #[serde(default)]
    pub plain_lyrics: Option<String>,
    #[serde(default)]
    pub synced_lyrics: Option<String>,
}

impl LrcLibRecord {
    /// Synced lyrics when they parse as fully timed, otherwise plain text. Instrumentals have none.
    pub fn lyrics(&self) -> Option<SongLyrics> {
        if self.instrumental {
            return None;
        }
        if let Some(text) = &self.synced_lyrics {
            let parsed = SongLyrics::from_text(text);
            if parsed.is_synced() && !parsed.is_empty() {
                return Some(parsed);
            }
        }
        let parsed = SongLyrics::from_text(self.plain_lyrics.as_deref()?);
        (!parsed.is_empty()).then_some(parsed)
    }
}

fn known_duration(song: &Song) -> Option<f64> {
    song.duration.filter(|d| d.is_finite() && *d > 0.0)
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.filter(|v| !v.trim().is_empty())
}

/// The first artist of a joined display artist such as `A; B` or `A, B`.
pub fn primary_artist(value: &str) -> String {
    value.split(';').next().unwrap_or_default().split(", ").next().unwrap_or_default().trim().to_string()
}

fn endpoint(name: &str, mut parameters: Vec<(&str, String)>) -> Url {
    parameters.sort_by(|a, b| a.0.cmp(b.0));
    let mut url = Url::parse(API_BASE).and_then(|base| base.join(name)).expect("static LRCLIB URL");
    url.query_pairs_mut().extend_pairs(parameters);
    url
}

/// The exact-match request, or `None` for podcasts and songs without a title or artist.
pub fn get_url(song: &Song) -> Option<Url> {
    if song.is_podcast() || song.title.trim().is_empty() {
        return None;
    }
    let artist = non_empty(song.artist.as_deref())?;
    let mut parameters = vec![("track_name", song.title.clone()), ("artist_name", artist.to_string())];
    if let Some(album) = song.album.as_deref().filter(|a| !a.is_empty()) {
        parameters.push(("album_name", album.to_string()));
    }
    if let Some(duration) = song.duration.filter(|d| d.is_finite() && (1.0..=3600.0).contains(d)) {
        parameters.push(("duration", duration.to_string()));
    }
    Some(endpoint("get", parameters))
}

/// The fallback search by title and primary artist, used across releases such as compilations.
pub fn search_url(song: &Song) -> Url {
    let artist = primary_artist(song.artist.as_deref().unwrap_or_default());
    endpoint("search", vec![("track_name", song.title.clone()), ("artist_name", artist)])
}

/// Title, artist and duration (within two seconds) must match. A joined display artist may match
/// its primary artist only for the same album and a known duration.
pub fn matches(record: &LrcLibRecord, song: &Song) -> bool {
    if normalize_key(&record.track_name) != normalize_key(&song.title) {
        return false;
    }
    let artist = song.artist.as_deref().unwrap_or_default();
    if normalize_key(&record.artist_name) != normalize_key(artist) {
        let primary = primary_artist(artist);
        let same_album = song.album.as_deref().filter(|a| !a.is_empty()).is_some_and(|album| {
            normalize_key(record.album_name.as_deref().unwrap_or_default()) == normalize_key(album)
        });
        if song.artist.is_none()
            || primary == artist
            || normalize_key(&record.artist_name) != normalize_key(&primary)
            || !same_album
            || known_duration(song).is_none()
        {
            return false;
        }
    }
    known_duration(song).is_none_or(|duration| (record.duration - duration).abs() <= 2.0)
}

/// Picks search results: matching records (the same album is required when the duration is unknown),
/// the same album first, then the closest duration. Synced lyrics win, then `fallback`.
pub fn select_search_result(
    song: &Song,
    records: Vec<LrcLibRecord>,
    fallback: Option<SongLyrics>,
) -> Option<SongLyrics> {
    let album = normalize_key(song.album.as_deref().unwrap_or_default());
    let same_album = |r: &LrcLibRecord| normalize_key(r.album_name.as_deref().unwrap_or_default()) == album;
    let mut candidates: Vec<LrcLibRecord> = records
        .into_iter()
        .filter(|r| {
            matches(r, song)
                && (known_duration(song).is_some()
                    || (!song.album.as_deref().unwrap_or_default().is_empty() && same_album(r)))
        })
        .collect();
    let distance = |r: &LrcLibRecord| (r.duration - song.duration.unwrap_or(r.duration)).abs();
    candidates.sort_by(|a, b| same_album(b).cmp(&same_album(a)).then(distance(a).total_cmp(&distance(b))));
    let lyrics: Vec<SongLyrics> = candidates.iter().filter_map(LrcLibRecord::lyrics).collect();
    if let Some(synced) = lyrics.iter().find(|l| l.is_synced()) {
        return Some(synced.clone());
    }
    fallback.or_else(|| lyrics.into_iter().next())
}

/// Parses `Retry-After` as seconds (at least one) or an HTTP date.
pub fn retry_after(value: &str, now: i64) -> Option<i64> {
    let value = value.trim();
    if let Ok(seconds) = value.parse::<f64>() {
        return seconds.is_finite().then(|| now + (seconds.max(1.0) * 1000.0) as i64);
    }
    chrono::DateTime::parse_from_rfc2822(value).ok().map(|date| date.timestamp_millis())
}

/// The result of one HTTP exchange: a body, `None` for 404, or an error. Also returns a new
/// cooldown (Unix ms) for 429 and 503 replies.
pub fn interpret_reply(
    status: u16,
    retry_after_header: Option<&str>,
    body: Vec<u8>,
    now: i64,
) -> (Option<i64>, Result<Option<Vec<u8>>, LyricsError>) {
    if status == 404 {
        return (None, Ok(None));
    }
    let mut cooldown = None;
    if status == 429 || status == 503 {
        let until = retry_after_header
            .and_then(|v| retry_after(v, now))
            .unwrap_or(now + if status == 429 { 60_000 } else { 1_000 });
        cooldown = Some(until);
        if status == 429 {
            return (cooldown, Err(LyricsError::RateLimited { until }));
        }
    }
    if status != 200 {
        return (cooldown, Err(MusicError::ServerUnavailable.into()));
    }
    if body.len() > MAX_RESPONSE_BYTES {
        return (cooldown, Err(MusicError::InvalidResponse.into()));
    }
    (cooldown, Ok(Some(body)))
}

/// The lookup flow over any transport. `fetch` returns a body, or `None` for 404.
/// An exact match with synced lyrics wins; otherwise a search may find a synced version
/// on another release, and plain text from the exact match is the fallback.
pub async fn lookup<F, Fut>(song: &Song, mut fetch: F) -> Result<Option<SongLyrics>, LyricsError>
where
    F: FnMut(Url) -> Fut,
    Fut: Future<Output = Result<Option<Vec<u8>>, LyricsError>>,
{
    let Some(get) = get_url(song) else { return Ok(None) };
    let mut fallback = None;
    if let Some(body) = fetch(get).await? {
        let record: LrcLibRecord = serde_json::from_slice(&body).map_err(|_| MusicError::InvalidResponse)?;
        if matches(&record, song) {
            fallback = record.lyrics();
            if fallback.as_ref().is_some_and(SongLyrics::is_synced) {
                return Ok(fallback);
            }
            if record.instrumental {
                return Ok(None);
            }
        }
    }
    let body = match fetch(search_url(song)).await {
        Ok(Some(body)) => body,
        Ok(None) => return Ok(fallback),
        Err(error) => return if fallback.is_some() { Ok(fallback) } else { Err(error) },
    };
    match serde_json::from_slice::<Vec<LrcLibRecord>>(&body) {
        Ok(records) => Ok(select_search_result(song, records, fallback)),
        Err(_) if fallback.is_some() => Ok(fallback),
        Err(_) => Err(MusicError::InvalidResponse.into()),
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub type SleepFuture = Pin<Box<dyn Future<Output = ()> + Send>>;
#[cfg(target_arch = "wasm32")]
pub type SleepFuture = Pin<Box<dyn Future<Output = ()>>>;
/// Sleeps for the given milliseconds, provided by the app (tokio on desktop, a timer in the browser).
pub type Sleeper = Arc<dyn Fn(u64) -> SleepFuture + Send + Sync>;

/// Serializes and spaces requests, and honors LRCLIB's cooldown.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RequestGate {
    pub requesting: bool,
    pub next_request: i64,
    pub cooldown: i64,
}

impl RequestGate {
    /// `Ok(true)` when a request may start now (and marks it started), `Ok(false)` to wait.
    pub fn try_start(&mut self, now: i64) -> Result<bool, LyricsError> {
        if now < self.cooldown {
            return Err(LyricsError::RateLimited { until: self.cooldown });
        }
        if self.requesting || now < self.next_request {
            return Ok(false);
        }
        self.requesting = true;
        Ok(true)
    }

    pub fn finish(&mut self, now: i64) {
        self.requesting = false;
        self.next_request = now + REQUEST_SPACING_MILLIS;
    }
}

pub struct LrcLibClient {
    http: reqwest::Client,
    gate: Arc<Mutex<RequestGate>>,
    sleeper: Option<Sleeper>,
}

impl Default for LrcLibClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Releases the gate even when the request future is dropped.
struct GateTicket(Arc<Mutex<RequestGate>>);

impl Drop for GateTicket {
    fn drop(&mut self) {
        if let Ok(mut gate) = self.0.lock() {
            gate.finish(now_millis());
        }
    }
}

impl LrcLibClient {
    pub fn new() -> Self {
        Self::with_client(crate::http::public_client())
    }

    pub fn with_client(http: reqwest::Client) -> Self {
        Self { http, gate: Arc::default(), sleeper: None }
    }

    /// Enables waiting between overlapping lookups. Without a sleeper, overlapping lookups are not delayed.
    pub fn with_sleeper(mut self, sleeper: Sleeper) -> Self {
        self.sleeper = Some(sleeper);
        self
    }

    /// The end of the current rate-limit cooldown, if any.
    pub fn cooldown_until(&self) -> Option<i64> {
        let cooldown = self.gate.lock().map(|g| g.cooldown).unwrap_or(0);
        (cooldown > now_millis()).then_some(cooldown)
    }

    pub async fn lyrics(&self, song: &Song) -> Result<Option<SongLyrics>, LyricsError> {
        lookup(song, |url| self.request(url)).await
    }

    fn try_start(&self) -> Result<bool, LyricsError> {
        let mut gate = self.gate.lock().map_err(|_| MusicError::ServerUnavailable)?;
        let started = gate.try_start(now_millis())?;
        if !started && self.sleeper.is_none() {
            gate.requesting = true;
            return Ok(true);
        }
        Ok(started)
    }

    async fn request(&self, url: Url) -> Result<Option<Vec<u8>>, LyricsError> {
        while !self.try_start()? {
            if let Some(sleep) = &self.sleeper {
                sleep(POLL_MILLIS).await;
            }
        }
        let _ticket = GateTicket(self.gate.clone());
        let request = self.http.get(url).header(reqwest::header::ACCEPT, "application/json");
        #[cfg(not(target_arch = "wasm32"))]
        let request =
            request.header(reqwest::header::USER_AGENT, USER_AGENT).timeout(std::time::Duration::from_secs(20));
        let response = request.send().await.map_err(|_| MusicError::ServerUnavailable)?;
        let status = response.status().as_u16();
        let retry =
            response.headers().get(reqwest::header::RETRY_AFTER).and_then(|v| v.to_str().ok()).map(str::to_string);
        let body = if status == 200 {
            response.bytes().await.map_err(|_| MusicError::ServerUnavailable)?.to_vec()
        } else {
            Vec::new()
        };
        let (cooldown, result) = interpret_reply(status, retry.as_deref(), body, now_millis());
        if let Some(until) = cooldown
            && let Ok(mut gate) = self.gate.lock()
        {
            gate.cooldown = until;
        }
        result
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::cell::RefCell;

    pub(crate) const RECORD: &str = r#"{"trackName":"Synthetic Song","artistName":"Test Artist","albumName":"Test Album","duration":180,"instrumental":false,"syncedLyrics":"[00:02]Synthetic first verse\n[00:05]Synthetic second verse","plainLyrics":"Synthetic plain verse"}"#;

    pub(crate) fn song() -> Song {
        Song {
            artist: Some("Test Artist".into()),
            album: Some("Test Album".into()),
            duration: Some(180.0),
            ..Song::new("fixture", "Synthetic Song")
        }
    }
    pub(crate) fn synced() -> SongLyrics {
        SongLyrics::from_text("[00:02]Synthetic first verse\n[00:05]Synthetic second verse")
    }
    pub(crate) fn plain() -> SongLyrics {
        SongLyrics::from_text("Synthetic plain verse")
    }

    /// Runs a lookup against a fixture handler `(url) -> (status, headers, body)`, logging requests.
    pub(crate) fn run(
        song: &Song,
        handler: impl Fn(&Url) -> (u16, Option<&'static str>, String),
    ) -> (Result<Option<SongLyrics>, LyricsError>, Vec<Url>) {
        let log = RefCell::new(Vec::new());
        let result = futures::executor::block_on(lookup(song, |url| {
            log.borrow_mut().push(url.clone());
            let (status, retry, body) = handler(&url);
            let (_, reply) = interpret_reply(status, retry, body.into_bytes(), 0);
            async move { reply }
        }));
        (result, log.into_inner())
    }

    fn query(url: &Url) -> Vec<(String, String)> {
        url.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect()
    }

    fn is_get(url: &Url) -> bool {
        url.path() == "/api/get"
    }

    #[test]
    fn lookup_sends_only_public_song_metadata() {
        let special = Song {
            artist: Some("Èva".into()),
            album: Some("Test / Album".into()),
            duration: Some(180.0),
            ..Song::new("private-server-id", "Città & mare")
        };
        let response = RECORD.replace("Synthetic Song", "Città & mare").replace("Test Artist", "Èva");
        let (result, log) = run(&special, |_| (200, None, response.clone()));
        assert_eq!(result.unwrap(), Some(synced()));
        let url = &log[0];
        assert_eq!((url.scheme(), url.host_str(), url.path()), ("https", Some("lrclib.net"), "/api/get"));
        let query = query(url);
        assert_eq!(
            query.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
            ["album_name", "artist_name", "duration", "track_name"]
        );
        assert_eq!(query[0].1, "Test / Album");
        assert_eq!(query[1].1, "Èva");
        assert_eq!(query[2].1.parse::<f64>().unwrap(), 180.0);
        assert_eq!(query[3].1, "Città & mare");
        assert!(!url.as_str().contains("private-server-id"));
    }

    #[test]
    fn search_finds_synced_compilation_track_and_rejects_wrong_artist_title_or_duration() {
        let wrong_artist = RECORD.replace("Test Artist", "Other Artist");
        let wrong_title = RECORD.replace("Synthetic Song", "Synthetic Song Live");
        let wrong_duration = RECORD.replace("\"duration\":180", "\"duration\":220");
        let right = RECORD.replace("Test Album", "Original Album");
        let list = format!("[{wrong_artist},{wrong_title},{wrong_duration},{right}]");
        let (result, _) =
            run(&song(), |url| if is_get(url) { (404, None, String::new()) } else { (200, None, list.clone()) });
        assert_eq!(result.unwrap(), Some(synced()));
    }

    #[test]
    fn mismatched_versions_do_not_display_unrelated_lyrics() {
        let wrong = RECORD.replace("\"duration\":180", "\"duration\":183");
        let (result, _) =
            run(&song(), |url| (200, None, if is_get(url) { wrong.clone() } else { format!("[{wrong}]") }));
        assert_eq!(result.unwrap(), None);
    }

    #[test]
    fn featured_artist_metadata_can_match_primary_artist_with_same_album_and_duration() {
        let featured = Song { artist: Some("Test Artist, Guest Singer".into()), ..song() };
        let (result, log) =
            run(
                &featured,
                |url| if is_get(url) { (404, None, String::new()) } else { (200, None, format!("[{RECORD}]")) },
            );
        assert_eq!(result.unwrap(), Some(synced()));
        assert!(query(&log[1]).contains(&("artist_name".into(), "Test Artist".into())));
    }

    #[test]
    fn featured_artist_fallback_rejects_different_album_and_unknown_duration() {
        let handler =
            |url: &Url| if is_get(url) { (404, None, String::new()) } else { (200, None, format!("[{RECORD}]")) };
        let other_album =
            Song { artist: Some("Test Artist, Guest Singer".into()), album: Some("Other Album".into()), ..song() };
        let unknown_duration = Song { artist: Some("Test Artist, Guest Singer".into()), duration: None, ..song() };
        assert_eq!(run(&other_album, handler).0.unwrap(), None);
        assert_eq!(run(&unknown_duration, handler).0.unwrap(), None);
    }

    #[test]
    fn search_without_duration_requires_matching_album() {
        let wrong = RECORD.replace("Test Album", "Different Album");
        let untimed = Song { duration: None, ..song() };
        let (result, _) =
            run(
                &untimed,
                |url| if is_get(url) { (404, None, String::new()) } else { (200, None, format!("[{wrong}]")) },
            );
        assert_eq!(result.unwrap(), None);
    }

    #[test]
    fn plain_text_survives_failure_to_find_timed_version() {
        let record = RECORD.replace(
            r#""syncedLyrics":"[00:02]Synthetic first verse\n[00:05]Synthetic second verse""#,
            r#""syncedLyrics":null"#,
        );
        let (result, _) =
            run(&song(), |url| if is_get(url) { (200, None, record.clone()) } else { (503, None, String::new()) });
        assert_eq!(result.unwrap(), Some(plain()));
    }

    #[test]
    fn instrumental_track_and_missing_artist_do_not_trigger_extra_searches() {
        let record = RECORD.replace("\"instrumental\":false", "\"instrumental\":true");
        let (instrumental, log) = run(&song(), |_| (200, None, record.clone()));
        assert_eq!(instrumental.unwrap(), None);
        assert_eq!(log.len(), 1);
        let (missing_artist, log) = run(&Song::new("unknown", "Unknown"), |_| (200, None, record.clone()));
        assert_eq!(missing_artist.unwrap(), None);
        assert!(log.is_empty());
        let podcast = Song { artist: Some("Show".into()), is_podcast: Some(true), ..Song::new("podcast", "Episode") };
        let (podcast, log) = run(&podcast, |_| (200, None, record.clone()));
        assert_eq!(podcast.unwrap(), None);
        assert!(log.is_empty());
    }

    #[test]
    fn rate_limit_honors_retry_after_seconds_and_http_dates() {
        let now = 1_790_000_000_000;
        let (cooldown, result) = interpret_reply(429, Some("120"), b"busy".to_vec(), now);
        assert_eq!(cooldown, Some(now + 120_000));
        assert_eq!(result, Err(LyricsError::RateLimited { until: now + 120_000 }));
        let date = chrono::DateTime::from_timestamp_millis(now + 120_000)
            .unwrap()
            .format("%a, %d %b %Y %H:%M:%S GMT")
            .to_string();
        let (cooldown, _) = interpret_reply(429, Some(&date), Vec::new(), now);
        assert_eq!(cooldown, Some(now + 120_000));
        assert_eq!(interpret_reply(429, None, Vec::new(), now).0, Some(now + 60_000));
        let (cooldown, result) = interpret_reply(503, None, Vec::new(), now);
        assert_eq!(cooldown, Some(now + 1_000));
        assert_eq!(result, Err(LyricsError::Music(MusicError::ServerUnavailable)));
        assert_eq!(
            interpret_reply(200, None, vec![0; MAX_RESPONSE_BYTES + 1], now).1,
            Err(MusicError::InvalidResponse.into())
        );
        // While cooling down, the gate refuses without sending anything.
        let mut gate = RequestGate { cooldown: now + 120_000, ..Default::default() };
        assert_eq!(gate.try_start(now), Err(LyricsError::RateLimited { until: now + 120_000 }));
    }

    #[test]
    fn malformed_responses_remain_errors() {
        let (result, _) = run(&song(), |_| (200, None, "<html>Busy</html>".into()));
        assert_eq!(result, Err(MusicError::InvalidResponse.into()));
    }

    #[test]
    fn overlapping_requests_are_spaced() {
        let mut gate = RequestGate::default();
        assert_eq!(gate.try_start(1_000), Ok(true));
        assert_eq!(gate.try_start(1_050), Ok(false));
        gate.finish(1_100);
        assert_eq!(gate.try_start(1_200), Ok(false));
        assert_eq!(gate.try_start(1_400), Ok(true));
    }

    #[test]
    fn client_futures_are_send() {
        fn assert_send<T: Send>(_: T) {}
        let client = LrcLibClient::new();
        let song = song();
        assert_send(client.lyrics(&song));
    }
}
