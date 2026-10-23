//! Audiobookshelf client: sign-in with a local account, token renewal, book libraries, playback
//! sessions on the original files, and listening progress with "newest position wins" merging.
//!
//! The account (server, user, alias) and its tokens are separate types so the app can keep the
//! tokens in the OS keychain and the account with the other settings. Requests never carry music
//! credentials, cookies or redirects. Request building and response parsing are pure functions;
//! [`AudiobookshelfClient`] only adds the network and token renewal.

use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

use crate::error::{MusicError, MusicResult};
use crate::models::{Song, connection_alias, server_string, validate_server};

/// Media types offered to the server when starting a direct-play session.
pub const SUPPORTED_MIME_TYPES: [&str; 6] = ["audio/mpeg", "audio/mp4", "audio/aac", "audio/flac", "audio/x-m4b", "audio/x-m4a"];
/// Items requested per catalog page.
pub const PAGE_SIZE: usize = 50;
/// Maximum search results requested.
pub const SEARCH_LIMIT: usize = 100;
/// Cover width requested from the server.
pub const COVER_WIDTH: u32 = 600;
/// How long failed progress uploads wait before retrying automatically.
pub const SYNC_RETRY_DELAY_MS: i64 = 60_000;

// MARK: Account and tokens

/// A saved Audiobookshelf connection, without secrets. Safe to store with the other settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudiobookshelfAccount {
    pub server: Url,
    #[serde(rename = "userID")]
    pub user_id: String,
    pub username: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Identifies one sign-in; a token rotation keeps it, a new login replaces it. Persist callbacks
    /// compare it to refuse writing tokens for a connection that was replaced meanwhile.
    #[serde(default = "Uuid::new_v4", rename = "connectionID")]
    pub connection_id: Uuid,
}

impl AudiobookshelfAccount {
    pub fn new(server: Url, user_id: &str, username: &str, name: Option<&str>) -> Self {
        Self {
            server,
            user_id: user_id.to_string(),
            username: username.to_string(),
            name: name.map(|n| n.trim().to_string()),
            connection_id: Uuid::new_v4(),
        }
    }

    /// The namespace for progress, bookmarks and downloads. It contains no token.
    pub fn id(&self) -> String {
        crate::md5_hex(format!("{}|abs|{}", server_string(&self.server), self.user_id).as_bytes())
    }

    pub fn display_name(&self) -> String {
        connection_alias::resolve(self.name.as_deref(), "Audiobookshelf", &self.server, true)
    }

    pub fn subtitle(&self) -> String {
        format!("{} · {}", self.username, server_string(&self.server))
    }
}

/// The secret half of a connection. Store it in the OS keychain; `Debug` never prints it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudiobookshelfTokens {
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
}

impl fmt::Debug for AudiobookshelfTokens {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AudiobookshelfTokens")
            .field("access_token", &"<redacted>")
            .field("refresh_token", &self.refresh_token.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

/// An account with its tokens, as returned by sign-in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudiobookshelfLogin {
    pub account: AudiobookshelfAccount,
    pub tokens: AudiobookshelfTokens,
}

// MARK: Catalog models

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudiobookLibrary {
    pub id: String,
    pub name: String,
    pub media_type: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudiobookAuthor {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudiobookMetadata {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authors: Option<Vec<AudiobookAuthor>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub narrator_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub narrators: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudiobookMedia {
    pub metadata: AudiobookMetadata,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub num_audio_files: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chapters: Option<Vec<AudiobookChapter>>,
    /// Present on expanded items; needed for downloads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tracks: Option<Vec<AudiobookTrack>>,
}

/// A library item. Minified items (catalog pages) have no chapters or tracks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Audiobook {
    pub id: String,
    pub library_id: String,
    pub media_type: String,
    pub media: AudiobookMedia,
}

impl Audiobook {
    pub fn title(&self) -> &str {
        &self.media.metadata.title
    }

    pub fn author(&self) -> String {
        let metadata = &self.media.metadata;
        metadata
            .author_name
            .clone()
            .or_else(|| metadata.authors.as_ref().map(|a| a.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ")))
            .unwrap_or_default()
    }

    pub fn narrator(&self) -> String {
        let metadata = &self.media.metadata;
        metadata.narrator_name.clone().or_else(|| metadata.narrators.as_ref().map(|n| n.join(", "))).unwrap_or_default()
    }

    /// The single queue entry the player shows while this book plays.
    pub fn song(&self, account: &str, duration: f64) -> Song {
        Song {
            id: format!("audiobookshelf:{account}:{}", self.id),
            title: self.title().to_string(),
            artist: Some(self.author()),
            duration: Some(duration),
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudiobookChapter {
    pub id: i64,
    pub start: f64,
    pub end: f64,
    pub title: String,
}

/// One audio file of a book, placed on the book's timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudiobookTrack {
    pub start_offset: f64,
    pub duration: f64,
    /// A server-relative route; see [`media_url`].
    pub content_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudiobookPage {
    pub results: Vec<Audiobook>,
    pub total: i64,
}

impl AudiobookPage {
    /// Whether another page follows page `page` (zero-based).
    pub fn has_more(&self, page: usize) -> bool {
        ((page + 1) * PAGE_SIZE) < self.total.max(0) as usize && !self.results.is_empty()
    }
}

// MARK: Playback session

/// A file and the offset inside it for a position on the book timeline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackLocation {
    pub index: usize,
    pub offset: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudiobookPlaybackSession {
    pub id: String,
    pub library_item_id: String,
    pub duration: f64,
    pub current_time: f64,
    pub audio_tracks: Vec<AudiobookTrack>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chapters: Option<Vec<AudiobookChapter>>,
}

impl AudiobookPlaybackSession {
    /// A local session for a downloaded book, built from its expanded item.
    pub fn downloaded(book: &Audiobook) -> MusicResult<Self> {
        let tracks = book.media.tracks.clone().ok_or(MusicError::Unsupported)?;
        let duration = book.media.duration.unwrap_or_else(|| tracks.iter().map(|t| t.duration).sum());
        let session = Self {
            id: format!("local-{}", book.id),
            library_item_id: book.id.clone(),
            duration,
            current_time: 0.0,
            audio_tracks: tracks,
            chapters: book.media.chapters.clone(),
        };
        session.validated()?;
        Ok(session)
    }

    /// Rejects sessions whose files overlap, leave gaps, or disagree with the book duration.
    pub fn validated(&self) -> MusicResult<()> {
        if self.id.is_empty()
            || !self.duration.is_finite()
            || self.duration <= 0.0
            || !self.current_time.is_finite()
            || self.audio_tracks.is_empty()
        {
            return Err(MusicError::InvalidResponse);
        }
        let mut end = 0.0;
        for track in &self.audio_tracks {
            if !track.start_offset.is_finite()
                || !track.duration.is_finite()
                || track.duration <= 0.0
                || (track.start_offset - end).abs() >= 1.0
            {
                return Err(MusicError::InvalidResponse);
            }
            end = track.start_offset + track.duration;
        }
        if (end - self.duration).abs() >= 2.0 {
            return Err(MusicError::InvalidResponse);
        }
        Ok(())
    }

    /// Positions are on the complete book's timeline, including books split across files.
    pub fn location(&self, seconds: f64) -> Option<TrackLocation> {
        if !seconds.is_finite() || self.audio_tracks.is_empty() {
            return None;
        }
        let position = seconds.max(0.0).min(self.duration);
        let index = self.audio_tracks.iter().rposition(|t| t.start_offset <= position).unwrap_or(0);
        let track = &self.audio_tracks[index];
        Some(TrackLocation { index, offset: (position - track.start_offset).max(0.0).min(track.duration) })
    }

    /// Chapters from the session, falling back to the book's.
    pub fn chapters_or<'a>(&'a self, book: &'a Audiobook) -> &'a [AudiobookChapter] {
        self.chapters.as_deref().or(book.media.chapters.as_deref()).unwrap_or_default()
    }
}

// MARK: Progress

/// A listening position. `last_update` is Unix milliseconds, as the server reports it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ProgressWire", into = "ProgressWire")]
pub struct AudiobookProgress {
    pub current_time: f64,
    pub duration: f64,
    pub is_finished: bool,
    pub last_update: Option<f64>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProgressWire {
    current_time: f64,
    duration: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    is_finished: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    progress: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_update: Option<f64>,
}

impl TryFrom<ProgressWire> for AudiobookProgress {
    type Error = MusicError;
    fn try_from(wire: ProgressWire) -> Result<Self, MusicError> {
        let (time, duration) = (wire.current_time, wire.duration);
        if !time.is_finite() || !duration.is_finite() || duration < 0.0 {
            return Err(MusicError::InvalidResponse);
        }
        Ok(Self {
            current_time: time.max(0.0).min(duration),
            duration,
            is_finished: wire.is_finished.unwrap_or(duration > 0.0 && time >= duration),
            last_update: wire.last_update,
        })
    }
}

impl From<AudiobookProgress> for ProgressWire {
    fn from(p: AudiobookProgress) -> Self {
        Self {
            current_time: p.current_time,
            duration: p.duration,
            is_finished: Some(p.is_finished),
            progress: Some(p.progress()),
            last_update: p.last_update,
        }
    }
}

impl AudiobookProgress {
    /// A position recorded now.
    pub fn new(current_time: f64, duration: f64) -> Self {
        Self::at(current_time, duration, crate::now_millis() as f64)
    }

    /// A position recorded at `last_update` (Unix milliseconds). Non-finite values become zero.
    pub fn at(current_time: f64, duration: f64, last_update: f64) -> Self {
        let duration = if duration.is_finite() { duration.max(0.0) } else { 0.0 };
        let current_time = if current_time.is_finite() { current_time.max(0.0).min(duration) } else { 0.0 };
        Self { current_time, duration, is_finished: duration > 0.0 && current_time >= duration, last_update: Some(last_update) }
    }

    /// Completion fraction in 0...1.
    pub fn progress(&self) -> f64 {
        if self.duration > 0.0 { (self.current_time / self.duration).clamp(0.0, 1.0) } else { 0.0 }
    }

    /// The position to resume from: the local one, unless the server's is strictly newer.
    pub fn resuming(remote: Option<Self>, local: Option<Self>) -> Option<Self> {
        let Some(local) = local else { return remote };
        if let Some(remote) = remote
            && let (Some(remote_time), Some(local_time)) = (remote.last_update, local.last_update)
            && remote_time > local_time
        {
            return Some(remote);
        }
        Some(local)
    }
}

/// Waits a minute after a failed upload before trying again; connectivity changes reset it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SyncBackoff {
    retry_after_ms: i64,
}

impl SyncBackoff {
    pub fn can_attempt(&self, now_ms: i64) -> bool {
        now_ms >= self.retry_after_ms
    }
    pub fn failed(&mut self, now_ms: i64) {
        self.retry_after_ms = now_ms + SYNC_RETRY_DELAY_MS;
    }
    pub fn reset(&mut self) {
        self.retry_after_ms = 0;
    }
}

// MARK: Requests

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Patch,
}

/// A fully built request. `Debug` omits headers and body, which carry secrets.
#[derive(Clone, PartialEq)]
pub struct AbsRequest {
    pub method: HttpMethod,
    pub url: Url,
    pub headers: Vec<(&'static str, String)>,
    pub body: Option<Vec<u8>>,
}

impl fmt::Debug for AbsRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AbsRequest").field("method", &self.method).field("path", &self.url.path()).finish()
    }
}

impl AbsRequest {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
    pub fn json_body(&self) -> serde_json::Value {
        self.body.as_deref().and_then(|b| serde_json::from_slice(b).ok()).unwrap_or(serde_json::Value::Null)
    }
    pub fn query(&self, name: &str) -> Option<String> {
        self.url.query_pairs().find(|(k, _)| k == name).map(|(_, v)| v.into_owned())
    }
}

/// Appends path components to the server URL, refusing separators and dot segments.
fn endpoint(server: &Url, path: &[&str]) -> MusicResult<Url> {
    let mut url = server.clone();
    url.set_query(None);
    url.set_fragment(None);
    {
        let mut segments = url.path_segments_mut().map_err(|_| MusicError::InvalidServer)?;
        segments.pop_if_empty();
        for component in path {
            if component.is_empty() || component.contains('/') || *component == "." || *component == ".." {
                return Err(MusicError::InvalidResponse);
            }
            segments.push(component);
        }
    }
    Ok(url)
}

fn json<T: Serialize>(value: &T) -> Vec<u8> {
    serde_json::to_vec(value).unwrap_or_default()
}

/// `POST /login` with the mobile token flow; the password travels only in the JSON body.
pub fn login_request(server: &Url, username: &str, password: &str) -> MusicResult<AbsRequest> {
    Ok(AbsRequest {
        method: HttpMethod::Post,
        url: endpoint(server, &["login"])?,
        headers: vec![("x-return-tokens", "true".into()), ("Content-Type", "application/json".into())],
        body: Some(json(&serde_json::json!({ "username": username, "password": password }))),
    })
}

pub fn refresh_request(server: &Url, refresh_token: &str) -> MusicResult<AbsRequest> {
    Ok(AbsRequest {
        method: HttpMethod::Post,
        url: endpoint(server, &["auth", "refresh"])?,
        headers: vec![("x-refresh-token", refresh_token.to_string()), ("x-return-tokens", "true".into())],
        body: None,
    })
}

pub fn logout_request(server: &Url, tokens: &AudiobookshelfTokens) -> MusicResult<Option<AbsRequest>> {
    let Some(refresh) = &tokens.refresh_token else { return Ok(None) };
    Ok(Some(AbsRequest {
        method: HttpMethod::Post,
        url: endpoint(server, &["logout"])?,
        headers: vec![("x-refresh-token", refresh.clone()), ("Authorization", format!("Bearer {}", tokens.access_token))],
        body: None,
    }))
}

/// An authenticated API request. The bearer token is a header, never part of the URL.
pub fn api_request(
    server: &Url,
    access_token: &str,
    path: &[&str],
    query: &[(&str, String)],
    method: HttpMethod,
    body: Option<Vec<u8>>,
) -> MusicResult<AbsRequest> {
    let mut url = endpoint(server, path)?;
    if !query.is_empty() {
        let mut pairs = url.query_pairs_mut();
        for (name, value) in query {
            pairs.append_pair(name, value);
        }
    }
    let mut headers = vec![("Accept", "application/json".to_string())];
    if body.is_some() {
        headers.push(("Content-Type", "application/json".into()));
    }
    headers.push(("Authorization", format!("Bearer {access_token}")));
    Ok(AbsRequest { method, url, headers, body })
}

/// The JSON body of `POST /api/items/{id}/play`, asking for the original files.
pub fn play_body() -> Vec<u8> {
    json(&serde_json::json!({
        "deviceInfo": {
            "clientName": "Discophile",
            "clientVersion": env!("CARGO_PKG_VERSION"),
            "osName": os_name(),
        },
        "mediaPlayer": "Discophile",
        "forceDirectPlay": true,
        "supportedMimeTypes": SUPPORTED_MIME_TYPES,
    }))
}

fn os_name() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macOS",
        "linux" => "Linux",
        "windows" => "Windows",
        _ if cfg!(target_arch = "wasm32") => "Web",
        other => other,
    }
}

pub fn sync_body(progress: &AudiobookProgress, time_listened: f64) -> MusicResult<Vec<u8>> {
    if !time_listened.is_finite() || time_listened < 0.0 {
        return Err(MusicError::InvalidResponse);
    }
    Ok(json(&serde_json::json!({
        "currentTime": progress.current_time,
        "duration": progress.duration,
        "timeListened": time_listened,
    })))
}

/// Maps an HTTP status to the error the interface shows.
pub fn status_result(status: u16) -> MusicResult<()> {
    match status {
        200..=299 => Ok(()),
        401 => Err(MusicError::Authentication),
        403 => Err(MusicError::Forbidden),
        404 => Err(MusicError::NotFound),
        405 | 501 => Err(MusicError::Unsupported),
        500..=599 => Err(MusicError::ServerUnavailable),
        _ => Err(MusicError::InvalidResponse),
    }
}

// MARK: Responses

fn decode<'a, T: Deserialize<'a>>(data: &'a [u8]) -> MusicResult<T> {
    serde_json::from_slice(data).map_err(|_| MusicError::InvalidResponse)
}

/// Reads a login or refresh response. A missing token or user is a failed sign-in.
pub fn parse_login(data: &[u8], server: &Url) -> MusicResult<AudiobookshelfLogin> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct User {
        id: String,
        username: String,
        #[serde(default)]
        access_token: Option<String>,
        #[serde(default)]
        refresh_token: Option<String>,
        #[serde(default)]
        token: Option<String>,
    }
    #[derive(Deserialize)]
    struct Login {
        user: User,
    }
    let login: Login = decode(data)?;
    let user = login.user;
    let token = user.access_token.or(user.token).filter(|t| !t.is_empty()).ok_or(MusicError::Authentication)?;
    if user.id.is_empty() {
        return Err(MusicError::Authentication);
    }
    Ok(AudiobookshelfLogin {
        account: AudiobookshelfAccount::new(server.clone(), &user.id, &user.username, None),
        tokens: AudiobookshelfTokens { access_token: token, refresh_token: user.refresh_token },
    })
}

/// Book libraries only; podcast libraries are left to the podcast features.
pub fn parse_libraries(data: &[u8]) -> MusicResult<Vec<AudiobookLibrary>> {
    #[derive(Deserialize)]
    struct Response {
        libraries: Vec<AudiobookLibrary>,
    }
    let response: Response = decode(data)?;
    Ok(response.libraries.into_iter().filter(|l| l.media_type == "book").collect())
}

pub fn parse_page(data: &[u8]) -> MusicResult<AudiobookPage> {
    decode(data)
}

pub fn parse_search(data: &[u8]) -> MusicResult<Vec<Audiobook>> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Match {
        library_item: Audiobook,
    }
    #[derive(Deserialize)]
    struct Response {
        #[serde(default)]
        book: Option<Vec<Match>>,
    }
    let response: Response = decode(data)?;
    Ok(response.book.unwrap_or_default().into_iter().map(|m| m.library_item).collect())
}

pub fn parse_book(data: &[u8]) -> MusicResult<Audiobook> {
    decode(data)
}

pub fn parse_progress(data: &[u8]) -> MusicResult<AudiobookProgress> {
    decode(data)
}

/// A playback session for `item`, rejecting sessions for another book or with a broken timeline.
pub fn parse_playback(data: &[u8], item: &str) -> MusicResult<AudiobookPlaybackSession> {
    let session: AudiobookPlaybackSession = decode(data)?;
    if session.library_item_id != item {
        return Err(MusicError::InvalidResponse);
    }
    session.validated()?;
    Ok(session)
}

/// A playable URL for a server-relative media route, carrying the media token as a query
/// parameter (players cannot send headers). Only `/s/...` and `/api/items/{id}/file/{ino}` routes
/// are accepted, so a session cannot point the token at another origin or API. Any reverse-proxy
/// base path of `server` is kept. Never persist or log the result.
pub fn media_url(content_path: &str, server: &Url, token: &str) -> MusicResult<Url> {
    let invalid = Err(MusicError::InvalidResponse);
    if !content_path.starts_with('/') || content_path.starts_with("//") {
        return invalid;
    }
    let allowed = |b: u8| b.is_ascii_alphanumeric() || b"-._~!$&'()*+,;=:@/%".contains(&b);
    if !content_path.bytes().all(allowed) || !valid_percent_escapes(content_path) {
        return invalid;
    }
    let Ok(decoded) = percent_encoding::percent_decode_str(content_path).decode_utf8() else { return invalid };
    if decoded.contains('\\') {
        return invalid;
    }
    let segments: Vec<&str> = decoded.split('/').filter(|s| !s.is_empty()).collect();
    if segments.iter().any(|s| *s == "." || *s == "..") {
        return invalid;
    }
    let file_route = segments.len() == 5 && segments[0] == "api" && segments[1] == "items" && segments[3] == "file";
    let share_route = decoded.starts_with("/s/") && segments.len() >= 3;
    if !(file_route || share_route) {
        return invalid;
    }
    let base: String = server.path().split('/').filter(|s| !s.is_empty()).map(|s| format!("/{s}")).collect();
    let mut url = server.clone();
    url.set_path(&format!("{base}{content_path}"));
    url.set_fragment(None);
    url.set_query(None);
    url.query_pairs_mut().append_pair("token", token);
    Ok(url)
}

fn valid_percent_escapes(s: &str) -> bool {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if !(bytes.get(i + 1).is_some_and(u8::is_ascii_hexdigit) && bytes.get(i + 2).is_some_and(u8::is_ascii_hexdigit)) {
                return false;
            }
            i += 3;
        } else {
            i += 1;
        }
    }
    true
}

// MARK: Client

/// Persists a renamed or token-rotated connection before the client starts using it. Return an
/// error to keep the previous state. Called with the client's state locked: do not call back into
/// the client. Check `account.connection_id` against the stored one so a late rotation cannot
/// overwrite a connection that was replaced by a new sign-in.
pub type PersistFn = Arc<dyn Fn(&AudiobookshelfAccount, &AudiobookshelfTokens) -> MusicResult<()> + Send + Sync>;

struct State {
    account: AudiobookshelfAccount,
    tokens: AudiobookshelfTokens,
}

struct Inner {
    http: reqwest::Client,
    state: Mutex<State>,
    /// Serializes token renewal so concurrent 401s refresh once.
    refreshing: futures::lock::Mutex<()>,
    persist: PersistFn,
}

/// A cheap-to-clone Audiobookshelf client. Requests are isolated from music credentials,
/// cookies and redirects. Dropping a future cancels its request.
#[derive(Clone)]
pub struct AudiobookshelfClient {
    inner: Arc<Inner>,
}

impl fmt::Debug for AudiobookshelfClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AudiobookshelfClient").field("account", &self.account()).finish()
    }
}

fn reqwest_method(method: HttpMethod) -> reqwest::Method {
    match method {
        HttpMethod::Get => reqwest::Method::GET,
        HttpMethod::Post => reqwest::Method::POST,
        HttpMethod::Patch => reqwest::Method::PATCH,
    }
}

/// Sends a request and returns the body of a successful response.
pub async fn perform(http: &reqwest::Client, request: &AbsRequest) -> MusicResult<Vec<u8>> {
    let mut builder = http.request(reqwest_method(request.method), request.url.clone());
    for (name, value) in &request.headers {
        builder = builder.header(*name, value);
    }
    if let Some(body) = &request.body {
        builder = builder.body(body.clone());
    }
    let response = builder.send().await.map_err(|_| MusicError::ServerUnavailable)?;
    status_result(response.status().as_u16())?;
    let bytes = response.bytes().await.map_err(|_| MusicError::ServerUnavailable)?;
    Ok(bytes.to_vec())
}

impl AudiobookshelfClient {
    /// A client for a saved connection. `persist` stores rotated tokens and alias changes.
    pub fn new(
        account: AudiobookshelfAccount,
        tokens: AudiobookshelfTokens,
        persist: impl Fn(&AudiobookshelfAccount, &AudiobookshelfTokens) -> MusicResult<()> + Send + Sync + 'static,
    ) -> Self {
        Self::with_http(crate::http::server_client(), account, tokens, Arc::new(persist))
    }

    /// A client that keeps renewed tokens in memory only (used while checking a new sign-in).
    pub fn unsaved(login: AudiobookshelfLogin) -> Self {
        Self::new(login.account, login.tokens, |_, _| Ok(()))
    }

    pub fn with_http(http: reqwest::Client, account: AudiobookshelfAccount, tokens: AudiobookshelfTokens, persist: PersistFn) -> Self {
        Self {
            inner: Arc::new(Inner {
                http,
                state: Mutex::new(State { account, tokens }),
                refreshing: futures::lock::Mutex::new(()),
                persist,
            }),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.inner.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The current account, including an alias set with [`rename`](Self::rename).
    pub fn account(&self) -> AudiobookshelfAccount {
        self.state().account.clone()
    }

    /// The current tokens, after any renewal.
    pub fn tokens(&self) -> AudiobookshelfTokens {
        self.state().tokens.clone()
    }

    pub fn login_state(&self) -> AudiobookshelfLogin {
        let state = self.state();
        AudiobookshelfLogin { account: state.account.clone(), tokens: state.tokens.clone() }
    }

    /// Changes the alias. Nothing changes if persisting fails.
    pub fn rename(&self, name: &str) -> MusicResult<AudiobookshelfAccount> {
        let mut state = self.state();
        let mut next = state.account.clone();
        next.name = Some(name.trim().to_string());
        (self.inner.persist)(&next, &state.tokens)?;
        state.account = next.clone();
        Ok(next)
    }

    /// Signs in with a local Audiobookshelf user. Validates the address like music servers.
    pub async fn login(server: &str, username: &str, password: &str, allow_http: bool) -> MusicResult<AudiobookshelfLogin> {
        Self::login_with(&crate::http::server_client(), server, username, password, allow_http).await
    }

    pub async fn login_with(
        http: &reqwest::Client,
        server: &str,
        username: &str,
        password: &str,
        allow_http: bool,
    ) -> MusicResult<AudiobookshelfLogin> {
        let server = validate_server(server, allow_http)?;
        let username = username.trim();
        if username.is_empty() || password.is_empty() {
            return Err(MusicError::MissingCredentials);
        }
        let data = perform(http, &login_request(&server, username, password)?).await?;
        parse_login(&data, &server)
    }

    /// Signs in and checks library access before the caller replaces a working connection.
    /// The returned login carries the alias and is ready to persist.
    pub async fn connect(server: &str, username: &str, password: &str, allow_http: bool, name: &str) -> MusicResult<AudiobookshelfLogin> {
        let login = Self::login(server, username, password, allow_http).await?;
        let checking = Self::unsaved(login);
        checking.libraries().await?;
        let mut login = checking.login_state();
        login.account.name = Some(name.trim().to_string());
        Ok(login)
    }

    async fn renew(&self, rejected_token: &str) -> MusicResult<()> {
        let _refreshing = self.inner.refreshing.lock().await;
        let (previous, refresh) = {
            let state = self.state();
            if state.tokens.access_token != rejected_token {
                // Another request already rotated the token.
                return Ok(());
            }
            let refresh = state.tokens.refresh_token.clone().filter(|r| !r.is_empty()).ok_or(MusicError::Authentication)?;
            (state.account.clone(), refresh)
        };
        let data = perform(&self.inner.http, &refresh_request(&previous.server, &refresh)?).await?;
        let next = parse_login(&data, &previous.server)?;
        if next.account.user_id != previous.user_id || next.tokens.refresh_token.as_deref().is_none_or(str::is_empty) {
            return Err(MusicError::Authentication);
        }
        let mut state = self.state();
        if state.tokens.access_token == rejected_token {
            let account = AudiobookshelfAccount {
                // An alias can change while renewal waits for the network.
                name: state.account.name.clone(),
                connection_id: previous.connection_id,
                ..next.account
            };
            (self.inner.persist)(&account, &next.tokens)?;
            state.account = account;
            state.tokens = next.tokens;
        }
        Ok(())
    }

    async fn request(&self, path: &[&str], query: &[(&str, String)], method: HttpMethod, body: Option<Vec<u8>>) -> MusicResult<Vec<u8>> {
        let (server, token) = {
            let state = self.state();
            (state.account.server.clone(), state.tokens.access_token.clone())
        };
        let request = api_request(&server, &token, path, query, method, body.clone())?;
        match perform(&self.inner.http, &request).await {
            Err(MusicError::Authentication) => {
                self.renew(&token).await?;
                let token = self.state().tokens.access_token.clone();
                perform(&self.inner.http, &api_request(&server, &token, path, query, method, body)?).await
            }
            other => other,
        }
    }

    pub async fn libraries(&self) -> MusicResult<Vec<AudiobookLibrary>> {
        parse_libraries(&self.request(&["api", "libraries"], &[], HttpMethod::Get, None).await?)
    }

    /// One page (zero-based) of a library sorted by title, [`PAGE_SIZE`] items per page.
    pub async fn books(&self, library: &str, page: usize) -> MusicResult<AudiobookPage> {
        let query = [
            ("limit", PAGE_SIZE.to_string()),
            ("page", page.to_string()),
            ("sort", "media.metadata.title".to_string()),
            ("minified", "1".to_string()),
        ];
        parse_page(&self.request(&["api", "libraries", library, "items"], &query, HttpMethod::Get, None).await?)
    }

    pub async fn search(&self, query: &str, library: &str) -> MusicResult<Vec<Audiobook>> {
        let params = [("q", query.to_string()), ("limit", SEARCH_LIMIT.to_string())];
        parse_search(&self.request(&["api", "libraries", library, "search"], &params, HttpMethod::Get, None).await?)
    }

    /// The expanded item with chapters and tracks.
    pub async fn book(&self, id: &str) -> MusicResult<Audiobook> {
        parse_book(&self.request(&["api", "items", id], &[("expanded", "1".into())], HttpMethod::Get, None).await?)
    }

    /// The server position, or `None` if the book was never started.
    pub async fn progress(&self, id: &str) -> MusicResult<Option<AudiobookProgress>> {
        match self.request(&["api", "me", "progress", id], &[], HttpMethod::Get, None).await {
            Ok(data) => parse_progress(&data).map(Some),
            Err(MusicError::NotFound) => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub async fn update_progress(&self, id: &str, progress: &AudiobookProgress) -> MusicResult<()> {
        self.request(&["api", "me", "progress", id], &[], HttpMethod::Patch, Some(json(progress))).await.map(drop)
    }

    /// Starts a direct-play session on the original files.
    pub async fn play(&self, id: &str) -> MusicResult<AudiobookPlaybackSession> {
        parse_playback(&self.request(&["api", "items", id, "play"], &[], HttpMethod::Post, Some(play_body())).await?, id)
    }

    /// Reports a session position and the seconds listened since the previous report.
    pub async fn sync(&self, session: &str, progress: &AudiobookProgress, time_listened: f64, close: bool) -> MusicResult<()> {
        let body = sync_body(progress, time_listened)?;
        let route = if close { "close" } else { "sync" };
        self.request(&["api", "session", session, route], &[], HttpMethod::Post, Some(body)).await.map(drop)
    }

    /// Cover image bytes, [`COVER_WIDTH`] pixels wide.
    pub async fn cover(&self, id: &str) -> MusicResult<Vec<u8>> {
        self.request(&["api", "items", id, "cover"], &[("width", COVER_WIDTH.to_string())], HttpMethod::Get, None).await
    }

    /// Confirms the access token works (renewing it if needed), e.g. before handing URLs to a player.
    pub async fn verify(&self) -> MusicResult<()> {
        self.request(&["api", "me"], &[], HttpMethod::Get, None).await.map(drop)
    }

    /// A playable URL for a track, after verifying the token, including after a long pause.
    pub async fn media_url(&self, content_path: &str) -> MusicResult<Url> {
        Ok(self.media_urls(&[content_path]).await?.remove(0))
    }

    /// Playable URLs for several tracks with a single token check.
    pub async fn media_urls(&self, content_paths: &[&str]) -> MusicResult<Vec<Url>> {
        self.verify().await?;
        let state = self.state();
        content_paths.iter().map(|p| media_url(p, &state.account.server, &state.tokens.access_token)).collect()
    }

    /// Revokes the refresh token. Local removal should not wait for or depend on this.
    pub async fn logout(&self) -> MusicResult<()> {
        let request = {
            let state = self.state();
            logout_request(&state.account.server, &state.tokens)?
        };
        match request {
            Some(request) => perform(&self.inner.http, &request).await.map(drop),
            None => Ok(()),
        }
    }

    /// Uploads a playback position. Streaming sessions use the session route (which also records
    /// listening time); downloaded books and expired sessions fall back to the progress route.
    pub async fn push_progress(
        &self,
        item: &str,
        session: &str,
        progress: &AudiobookProgress,
        time_listened: f64,
        close: bool,
        offline: bool,
    ) -> MusicResult<()> {
        let result =
            if offline { self.update_progress(item, progress).await } else { self.sync(session, progress, time_listened, close).await };
        match result {
            Err(MusicError::NotFound) => self.update_progress(item, progress).await,
            other => other,
        }
    }

    /// Merges an unsent local position with the server's: uploads `local` when it is at least as
    /// new, and returns the position to resume from.
    pub async fn reconcile_progress(&self, item: &str, local: Option<&AudiobookProgress>) -> MusicResult<Option<AudiobookProgress>> {
        let remote = self.progress(item).await?;
        let Some(local) = local else { return Ok(remote) };
        let latest = AudiobookProgress::resuming(remote, Some(*local));
        if latest.as_ref() == Some(local) {
            self.update_progress(item, local).await?;
        }
        Ok(latest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOOK_JSON: &str = r#"{"id":"book","libraryId":"library","mediaType":"book","media":{"metadata":{"title":"Synthetic book","authorName":"Synthetic author"},"duration":180,"numAudioFiles":2}}"#;
    const PLAYBACK_JSON: &str = r#"{"id":"session","libraryItemId":"book","duration":180,"currentTime":75,"chapters":[{"id":0,"start":0,"end":100,"title":"Beginning"},{"id":1,"start":100,"end":180,"title":"Ending"}],"audioTracks":[{"startOffset":0,"duration":60,"contentUrl":"/s/item/book/first.mp3","mimeType":"audio/mpeg"},{"startOffset":60,"duration":120,"contentUrl":"/s/item/book/second.m4b","mimeType":"audio/mp4"}]}"#;
    const ROTATED: &str =
        r#"{"user":{"id":"fixture-user","username":"fixture","accessToken":"rotated-access","refreshToken":"rotated-refresh"}}"#;

    fn base() -> Url {
        Url::parse("https://example.test/books").unwrap()
    }

    fn account(server: Url) -> AudiobookshelfAccount {
        AudiobookshelfAccount::new(server, "fixture-user", "fixture", None)
    }

    fn tokens() -> AudiobookshelfTokens {
        AudiobookshelfTokens { access_token: "access-fixture".into(), refresh_token: Some("refresh-fixture".into()) }
    }

    fn decoded_path(url: &Url) -> String {
        percent_encoding::percent_decode_str(url.path()).decode_utf8().unwrap().into_owned()
    }

    // MARK: Pure request building and parsing

    #[test]
    fn login_request_keeps_password_in_body() {
        let request = login_request(&base(), "fixture", "synthetic + & password").unwrap();
        assert_eq!(request.url.as_str(), "https://example.test/books/login");
        assert_eq!(request.method, HttpMethod::Post);
        assert_eq!(request.header("x-return-tokens"), Some("true"));
        assert_eq!(request.json_body()["password"], "synthetic + & password");
        assert!(!format!("{request:?}").contains("password"));
        let root = login_request(&validate_server("https://example.test/", false).unwrap(), "a", "b").unwrap();
        assert_eq!(root.url.as_str(), "https://example.test/login");
    }

    #[test]
    fn api_requests_keep_bearer_out_of_url_and_encode_components() {
        let request = api_request(&base(), "access-fixture", &["api", "libraries"], &[], HttpMethod::Get, None).unwrap();
        assert_eq!(request.url.path(), "/books/api/libraries");
        assert_eq!(request.url.query(), None);
        assert_eq!(request.header("Authorization"), Some("Bearer access-fixture"));
        assert_eq!(request.header("Content-Type"), None);
        let search = api_request(
            &base(),
            "t",
            &["api", "libraries", "lib id", "search"],
            &[("q", "author & title +".into())],
            HttpMethod::Get,
            None,
        )
        .unwrap();
        assert_eq!(search.url.path(), "/books/api/libraries/lib%20id/search");
        assert_eq!(search.query("q").as_deref(), Some("author & title +"));
        for bad in ["", ".", "..", "a/b"] {
            assert_eq!(api_request(&base(), "t", &["api", "items", bad], &[], HttpMethod::Get, None), Err(MusicError::InvalidResponse));
        }
        let patch = api_request(&base(), "t", &["api", "me"], &[], HttpMethod::Patch, Some(b"{}".to_vec())).unwrap();
        assert_eq!(patch.header("content-type"), Some("application/json"));
    }

    #[test]
    fn login_parsing_requires_token_and_user() {
        let login = parse_login(ROTATED.as_bytes(), &base()).unwrap();
        assert_eq!(login.account.server, base());
        assert_eq!(login.tokens.access_token, "rotated-access");
        assert_eq!(login.tokens.refresh_token.as_deref(), Some("rotated-refresh"));
        let legacy = parse_login(br#"{"user":{"id":"u","username":"n","token":"legacy"}}"#, &base()).unwrap();
        assert_eq!(legacy.tokens.access_token, "legacy");
        assert_eq!(legacy.tokens.refresh_token, None);
        assert_eq!(parse_login(br#"{"user":{"id":"u","username":"n"}}"#, &base()), Err(MusicError::Authentication));
        assert_eq!(parse_login(br#"{"user":{"id":"","username":"n","token":"x"}}"#, &base()), Err(MusicError::Authentication));
        assert_eq!(parse_login(b"<html>", &base()), Err(MusicError::InvalidResponse));
    }

    #[test]
    fn libraries_exclude_podcasts_and_catalog_decodes_items() {
        let libraries = parse_libraries(
            br#"{"libraries":[{"id":"library","name":"Books","mediaType":"book"},{"id":"podcast","name":"Podcasts","mediaType":"podcast"}]}"#,
        )
        .unwrap();
        assert_eq!(libraries.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["Books"]);
        let page = parse_page(format!(r#"{{"results":[{BOOK_JSON}],"total":151}}"#).as_bytes()).unwrap();
        assert_eq!(page.total, 151);
        assert_eq!(page.results[0].author(), "Synthetic author");
        assert!(page.has_more(2));
        assert!(!page.has_more(3));
        let search = parse_search(format!(r#"{{"book":[{{"libraryItem":{BOOK_JSON}}}]}}"#).as_bytes()).unwrap();
        assert_eq!(search[0].id, "book");
        assert!(parse_search(b"{}").unwrap().is_empty());
        assert_eq!(parse_libraries(b"<html>Sign in</html>"), Err(MusicError::InvalidResponse));
        let song = search[0].song("acc", 180.0);
        assert_eq!(song.id, "audiobookshelf:acc:book");
        assert_eq!(song.artist.as_deref(), Some("Synthetic author"));
    }

    #[test]
    fn authors_and_narrators_fall_back_to_lists() {
        let book: Audiobook = serde_json::from_str(
            r#"{"id":"b","libraryId":"l","mediaType":"book","media":{"metadata":{"title":"T","authors":[{"name":"A"},{"name":"B"}],"narrators":["N","M"]}}}"#,
        )
        .unwrap();
        assert_eq!(book.author(), "A, B");
        assert_eq!(book.narrator(), "N, M");
    }

    #[test]
    fn status_codes_map_to_errors() {
        assert_eq!(status_result(204), Ok(()));
        assert_eq!(status_result(401), Err(MusicError::Authentication));
        assert_eq!(status_result(403), Err(MusicError::Forbidden));
        assert_eq!(status_result(404), Err(MusicError::NotFound));
        assert_eq!(status_result(405), Err(MusicError::Unsupported));
        assert_eq!(status_result(501), Err(MusicError::Unsupported));
        assert_eq!(status_result(503), Err(MusicError::ServerUnavailable));
        assert_eq!(status_result(302), Err(MusicError::InvalidResponse));
    }

    #[test]
    fn multi_file_timeline_resumes_and_seeks_across_file_boundaries() {
        let session = parse_playback(PLAYBACK_JSON.as_bytes(), "book").unwrap();
        let at = |s: f64| session.location(s).map(|l| (l.index, l.offset));
        assert_eq!(at(75.0), Some((1, 15.0)));
        assert_eq!(at(60.0).unwrap().0, 1);
        assert_eq!(at(59.0).unwrap().0, 0);
        assert_eq!(at(-10.0).unwrap().1, 0.0);
        assert_eq!(at(500.0).unwrap().1, 120.0);
        assert_eq!(at(f64::NAN), None);
        assert_eq!(session.chapters.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn invalid_track_offsets_and_wrong_book_sessions_are_rejected() {
        for json in [
            PLAYBACK_JSON.replace("\"startOffset\":60", "\"startOffset\":10"),
            PLAYBACK_JSON.replace("\"libraryItemId\":\"book\"", "\"libraryItemId\":\"other\""),
            PLAYBACK_JSON.replace("\"duration\":180", "\"duration\":100"),
            PLAYBACK_JSON.replace("\"id\":\"session\"", "\"id\":\"\""),
        ] {
            assert_eq!(parse_playback(json.as_bytes(), "book"), Err(MusicError::InvalidResponse));
        }
    }

    #[test]
    fn media_urls_keep_proxy_prefix_and_encode_token_and_filename() {
        let url = media_url("/s/item/book/A%20book%20%231.mp3", &base(), "fixture+&").unwrap();
        assert_eq!(decoded_path(&url), "/books/s/item/book/A book #1.mp3");
        assert_eq!(url.query_pairs().collect::<Vec<_>>(), [("token".into(), "fixture+&".into())]);
        let current = media_url("/api/items/book/file/12345", &base(), "fixture").unwrap();
        assert_eq!(current.path(), "/books/api/items/book/file/12345");
        let root = media_url("/api/items/book/file/1", &validate_server("https://example.test", false).unwrap(), "t").unwrap();
        assert_eq!(root.as_str(), "https://example.test/api/items/book/file/1?token=t");
    }

    #[test]
    fn media_urls_reject_foreign_origins_traversal_and_injected_query() {
        for path in [
            "https://other.test/s/item/book.mp3",
            "//other.test/s/item/book.mp3",
            "/s/../api/users",
            "/s/%2e%2e/api/users",
            "/s/item/book?token=other",
            "/api/users",
            "/s/item/book#fragment",
            "/s/item/a%5Cb",
            "/s/item/a b",
            "/s/item/%zz",
            "s/item/book.mp3",
        ] {
            assert!(media_url(path, &base(), "fixture").is_err(), "{path}");
        }
    }

    #[test]
    fn progress_keeps_completion_fraction_and_clamps() {
        let progress = AudiobookProgress::at(75.0, 180.0, 1000.0);
        let body: serde_json::Value = serde_json::to_value(progress).unwrap();
        assert_eq!(body["currentTime"], 75.0);
        assert_eq!(body["duration"], 180.0);
        assert_eq!(body["progress"], 75.0 / 180.0);
        assert_eq!(body["isFinished"], false);
        assert_eq!(body["lastUpdate"], 1000.0);
        let sync: serde_json::Value = serde_json::from_slice(&sync_body(&progress, 5.0).unwrap()).unwrap();
        assert_eq!(sync["timeListened"], 5.0);
        assert_eq!(sync_body(&progress, f64::NAN), Err(MusicError::InvalidResponse));
        assert_eq!(sync_body(&progress, -1.0), Err(MusicError::InvalidResponse));
        assert!(AudiobookProgress::new(200.0, 180.0).is_finished);
        assert_eq!(AudiobookProgress::new(f64::NAN, 180.0).current_time, 0.0);
        assert!(AudiobookProgress::new(1.0, 2.0).last_update.is_some());
        // Server payloads: clamped, derived completion, rejected when non-finite or negative.
        let server: AudiobookProgress = serde_json::from_str(r#"{"currentTime":200,"duration":180,"lastUpdate":5}"#).unwrap();
        assert_eq!((server.current_time, server.is_finished, server.last_update), (180.0, true, Some(5.0)));
        assert!(serde_json::from_str::<AudiobookProgress>(r#"{"currentTime":1,"duration":-1}"#).is_err());
        assert_eq!(serde_json::from_slice::<AudiobookProgress>(&serde_json::to_vec(&progress).unwrap()).unwrap(), progress);
    }

    #[test]
    fn account_and_pending_progress_round_trip_keep_server_and_user_isolation() {
        let first = account(base());
        let other = AudiobookshelfAccount::new(base(), "another", "another", None);
        assert_ne!(first.id(), other.id());
        assert_eq!(first.id(), crate::md5_hex(b"https://example.test/books|abs|fixture-user"));
        let json = serde_json::to_string(&first).unwrap();
        assert!(!json.contains("access") && !json.contains("refresh"));
        assert_eq!(serde_json::from_str::<AudiobookshelfAccount>(&json).unwrap(), first);
        let secret = serde_json::to_string(&tokens()).unwrap();
        assert_eq!(serde_json::from_str::<AudiobookshelfTokens>(&secret).unwrap(), tokens());
        assert!(!format!("{:?}", tokens()).contains("access-fixture"));
        let pending: std::collections::BTreeMap<String, AudiobookProgress> = [
            (format!("{}:book", first.id()), AudiobookProgress::new(75.0, 180.0)),
            (format!("{}:book", other.id()), AudiobookProgress::new(10.0, 180.0)),
        ]
        .into();
        let restored: std::collections::BTreeMap<String, AudiobookProgress> =
            serde_json::from_slice(&serde_json::to_vec(&pending).unwrap()).unwrap();
        assert_eq!(restored, pending);
        assert_eq!(first.display_name(), "Audiobookshelf · example.test/books");
    }

    #[test]
    fn newer_remote_progress_wins_over_older_unsent_local_position() {
        let old_local = AudiobookProgress::at(75.0, 180.0, 1000.0);
        let newer_remote = AudiobookProgress::at(20.0, 180.0, 2000.0);
        assert_eq!(AudiobookProgress::resuming(Some(newer_remote), Some(old_local)), Some(newer_remote));
        assert_eq!(AudiobookProgress::resuming(Some(old_local), Some(newer_remote)), Some(newer_remote));
        assert_eq!(AudiobookProgress::resuming(None, Some(old_local)), Some(old_local));
        assert_eq!(AudiobookProgress::resuming(Some(newer_remote), None), Some(newer_remote));
        // Equal timestamps keep the local position.
        let same = AudiobookProgress::at(30.0, 180.0, 1000.0);
        assert_eq!(AudiobookProgress::resuming(Some(same), Some(old_local)), Some(old_local));
    }

    #[test]
    fn sync_backoff_waits_a_minute() {
        let mut backoff = SyncBackoff::default();
        assert!(backoff.can_attempt(0));
        backoff.failed(1_000);
        assert!(!backoff.can_attempt(60_999));
        assert!(backoff.can_attempt(61_000));
        backoff.failed(1_000);
        backoff.reset();
        assert!(backoff.can_attempt(1_000));
    }

    #[test]
    fn failed_alias_persistence_leaves_the_working_account_unchanged() {
        let original = account(base());
        let client = AudiobookshelfClient::new(original.clone(), tokens(), |_, _| Err(MusicError::Forbidden));
        assert_eq!(client.rename("Not saved"), Err(MusicError::Forbidden));
        assert_eq!(client.account(), original);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn client_futures_are_send() {
        fn assert_send<T: Send>(_: T) {}
        let client = AudiobookshelfClient::new(account(base()), tokens(), |_, _| Ok(()));
        let progress = AudiobookProgress::new(1.0, 2.0);
        assert_send(client.libraries());
        assert_send(client.media_urls(&["/s/a/b"]));
        assert_send(client.push_progress("b", "s", &progress, 1.0, false, false));
        assert_send(client.reconcile_progress("b", Some(&progress)));
        assert_send(AudiobookshelfClient::connect("https://x", "a", "b", false, ""));
        assert_send(crate::offline_audiobook::prepare_download(&client, "a", "b", None));
    }

    // MARK: End to end against a local server

    mod server {
        use std::sync::{Arc, Mutex};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use url::Url;

        #[derive(Debug, Clone)]
        pub struct Seen {
            pub method: String,
            pub target: String,
            pub headers: Vec<(String, String)>,
            pub body: Vec<u8>,
        }

        impl Seen {
            pub fn header(&self, name: &str) -> Option<&str> {
                self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
            }
            pub fn path(&self) -> &str {
                self.target.split('?').next().unwrap_or_default()
            }
            pub fn url(&self) -> Url {
                Url::parse(&format!("http://local{}", self.target)).unwrap()
            }
            pub fn json(&self) -> serde_json::Value {
                serde_json::from_slice(&self.body).unwrap_or(serde_json::Value::Null)
            }
        }

        pub type Handler = Arc<dyn Fn(&Seen) -> (u16, String) + Send + Sync>;

        /// Serves one response per connection. Handlers run on a blocking thread so they may wait.
        pub async fn serve(handler: impl Fn(&Seen) -> (u16, String) + Send + Sync + 'static) -> (Url, Arc<Mutex<Vec<Seen>>>) {
            let handler: Handler = Arc::new(handler);
            let seen = Arc::new(Mutex::new(Vec::new()));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let log = seen.clone();
            tokio::spawn(async move {
                while let Ok((mut socket, _)) = listener.accept().await {
                    let (handler, log) = (handler.clone(), log.clone());
                    tokio::spawn(async move {
                        let mut data = Vec::new();
                        let mut chunk = [0u8; 4096];
                        let head_end = loop {
                            let n = socket.read(&mut chunk).await.unwrap_or(0);
                            if n == 0 {
                                return;
                            }
                            data.extend_from_slice(&chunk[..n]);
                            if let Some(i) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                                break i + 4;
                            }
                        };
                        let head = String::from_utf8_lossy(&data[..head_end]).to_string();
                        let mut lines = head.split("\r\n");
                        let mut first = lines.next().unwrap_or_default().split(' ');
                        let (method, target) = (first.next().unwrap_or_default().to_string(), first.next().unwrap_or_default().to_string());
                        let headers: Vec<(String, String)> =
                            lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_string(), v.trim().to_string())).collect();
                        let length: usize = headers
                            .iter()
                            .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                            .and_then(|(_, v)| v.parse().ok())
                            .unwrap_or(0);
                        let mut body = data[head_end..].to_vec();
                        while body.len() < length {
                            let n = socket.read(&mut chunk).await.unwrap_or(0);
                            if n == 0 {
                                break;
                            }
                            body.extend_from_slice(&chunk[..n]);
                        }
                        let request = Seen { method, target, headers, body };
                        log.lock().unwrap().push(request.clone());
                        let (status, body) = tokio::task::spawn_blocking(move || handler(&request)).await.unwrap_or((599, String::new()));
                        let response = format!(
                            "HTTP/1.1 {status} Status\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        let _ = socket.write_all(response.as_bytes()).await;
                        let _ = socket.shutdown().await;
                    });
                }
            });
            (Url::parse(&format!("http://{address}/books")).unwrap(), seen)
        }
    }

    type Saved = Arc<Mutex<Vec<(AudiobookshelfAccount, AudiobookshelfTokens)>>>;

    fn http() -> reqwest::Client {
        reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).build().unwrap()
    }

    fn client(server: &Url, saved: &Saved) -> AudiobookshelfClient {
        let saved = saved.clone();
        AudiobookshelfClient::with_http(
            http(),
            account(server.clone()),
            tokens(),
            Arc::new(move |a: &AudiobookshelfAccount, t: &AudiobookshelfTokens| {
                saved.lock().unwrap().push((a.clone(), t.clone()));
                Ok(())
            }),
        )
    }

    fn bearer(seen: &server::Seen) -> Option<&str> {
        seen.header("Authorization")
    }

    #[tokio::test]
    async fn login_uses_mobile_token_flow_without_putting_password_in_url() {
        let (url, seen) = server::serve(|_| {
            (200, r#"{"user":{"id":"fixture-user","username":"fixture","accessToken":"new-access","refreshToken":"new-refresh"}}"#.into())
        })
        .await;
        let login =
            AudiobookshelfClient::login_with(&http(), &format!("{url}/"), " fixture ", "synthetic + & password", true).await.unwrap();
        assert_eq!(login.account.server, url);
        assert_eq!(login.tokens.access_token, "new-access");
        assert_eq!(login.tokens.refresh_token.as_deref(), Some("new-refresh"));
        let request = seen.lock().unwrap()[0].clone();
        assert_eq!((request.method.as_str(), request.target.as_str()), ("POST", "/books/login"));
        assert_eq!(request.header("x-return-tokens"), Some("true"));
        assert_eq!(request.json()["password"], "synthetic + & password");
        assert_eq!(request.json()["username"], "fixture");
    }

    #[tokio::test]
    async fn login_rejects_http_without_explicit_opt_in_and_invalid_server_urls() {
        for server in ["http://example.test", "https://user:password@example.test", "https://example.test?token=x", "file:///tmp/books"] {
            let error = AudiobookshelfClient::login_with(&http(), server, "fixture", "fixture", false).await.unwrap_err();
            assert!(matches!(error, MusicError::InvalidServer | MusicError::InsecureServer), "{server}");
        }
        let missing = AudiobookshelfClient::login_with(&http(), "https://example.test", " ", "x", false).await;
        assert_eq!(missing, Err(MusicError::MissingCredentials));
    }

    #[tokio::test]
    async fn catalog_pagination_search_and_playback_requests() {
        let (url, seen) = server::serve(|request| {
            let path = request.path().to_string();
            if path.ends_with("/search") {
                (200, format!(r#"{{"book":[{{"libraryItem":{BOOK_JSON}}}]}}"#))
            } else if path.ends_with("/items") {
                (200, format!(r#"{{"results":[{BOOK_JSON}],"total":151}}"#))
            } else if path.ends_with("/play") {
                (200, PLAYBACK_JSON.into())
            } else {
                (
                    200,
                    r#"{"libraries":[{"id":"library","name":"Books","mediaType":"book"},{"id":"p","name":"P","mediaType":"podcast"}]}"#
                        .into(),
                )
            }
        })
        .await;
        let saved = Saved::default();
        let client = client(&url, &saved);
        assert_eq!(client.libraries().await.unwrap().len(), 1);
        let page = client.books("library", 3).await.unwrap();
        assert_eq!(page.total, 151);
        let search = client.search("author & title +", "library").await.unwrap();
        assert_eq!(search[0].id, "book");
        let session = client.play("book").await.unwrap();
        assert_eq!(session.location(75.0).unwrap().index, 1);
        let seen = seen.lock().unwrap().clone();
        assert_eq!(seen[0].path(), "/books/api/libraries");
        assert_eq!(seen[0].url().query(), None);
        assert_eq!(bearer(&seen[0]), Some("Bearer access-fixture"));
        let query = |i: usize, name: &str| seen[i].url().query_pairs().find(|(k, _)| k == name).map(|(_, v)| v.into_owned());
        assert_eq!(query(1, "page").as_deref(), Some("3"));
        assert_eq!(query(1, "limit").as_deref(), Some("50"));
        assert_eq!(query(2, "q").as_deref(), Some("author & title +"));
        assert_eq!(seen[3].path(), "/books/api/items/book/play");
        assert_eq!(seen[3].json()["forceDirectPlay"], true);
        assert!(saved.lock().unwrap().is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn expired_access_token_rotates_and_persists_before_retrying() {
        let saved = Saved::default();
        let persisted = saved.clone();
        let (url, seen) = server::serve(move |request| {
            if request.path() == "/books/auth/refresh" {
                return if request.header("x-refresh-token") == Some("refresh-fixture") {
                    (200, ROTATED.into())
                } else {
                    (400, String::new())
                };
            }
            if request.header("Authorization") == Some("Bearer access-fixture") {
                return (401, String::new());
            }
            // The retried request must only happen after the rotated tokens were saved.
            if persisted.lock().unwrap().first().map(|(_, t)| t.access_token.as_str()) != Some("rotated-access") {
                return (500, String::new());
            }
            (200, r#"{"libraries":[]}"#.into())
        })
        .await;
        let original = account(url.clone());
        let client = AudiobookshelfClient::with_http(http(), original.clone(), tokens(), {
            let saved = saved.clone();
            Arc::new(move |a: &AudiobookshelfAccount, t: &AudiobookshelfTokens| {
                saved.lock().unwrap().push((a.clone(), t.clone()));
                Ok(())
            })
        });
        let (first, second) = futures::join!(client.libraries(), client.libraries());
        first.unwrap();
        second.unwrap();
        let saved = saved.lock().unwrap().clone();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].0.connection_id, original.connection_id);
        assert_eq!(client.tokens().refresh_token.as_deref(), Some("rotated-refresh"));
        let refreshes = seen.lock().unwrap().iter().filter(|s| s.path().ends_with("/auth/refresh")).count();
        assert_eq!(refreshes, 1);
        assert!(seen.lock().unwrap().iter().filter(|s| s.path().ends_with("/auth/refresh")).all(|s| s.url().query().is_none()));
    }

    #[tokio::test]
    async fn alias_changes_use_latest_rotated_tokens_without_another_login() {
        let (url, _) = server::serve(|request| {
            if request.path() == "/books/auth/refresh" {
                return (200, ROTATED.into());
            }
            if request.header("Authorization") == Some("Bearer access-fixture") {
                return (401, String::new());
            }
            (200, r#"{"libraries":[]}"#.into())
        })
        .await;
        let saved = Saved::default();
        let client = client(&url, &saved);
        client.rename("Before refresh").unwrap();
        client.libraries().await.unwrap();
        assert_eq!(saved.lock().unwrap().last().unwrap().0.name.as_deref(), Some("Before refresh"));
        let renamed = client.rename("  My books  ").unwrap();
        assert_eq!(renamed.name.as_deref(), Some("My books"));
        let last = saved.lock().unwrap().last().unwrap().clone();
        assert_eq!(last.1.access_token, "rotated-access");
        assert_eq!(last.1.refresh_token.as_deref(), Some("rotated-refresh"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn alias_changed_during_refresh_survives_refresh_completion() {
        let (started_tx, started_rx) = std::sync::mpsc::channel::<()>();
        let (gate_tx, gate_rx) = std::sync::mpsc::channel::<()>();
        let started_tx = Mutex::new(started_tx);
        let gate_rx = Mutex::new(gate_rx);
        let (url, _) = server::serve(move |request| {
            if request.path() == "/books/auth/refresh" {
                started_tx.lock().unwrap().send(()).unwrap();
                gate_rx.lock().unwrap().recv_timeout(std::time::Duration::from_secs(5)).unwrap();
                return (200, ROTATED.into());
            }
            if request.header("Authorization") == Some("Bearer access-fixture") {
                return (401, String::new());
            }
            (200, r#"{"libraries":[]}"#.into())
        })
        .await;
        let saved = Saved::default();
        let client = client(&url, &saved);
        let loading = tokio::spawn({
            let client = client.clone();
            async move { client.libraries().await }
        });
        tokio::task::spawn_blocking(move || started_rx.recv_timeout(std::time::Duration::from_secs(5))).await.unwrap().unwrap();
        let renamed = client.rename("New alias");
        gate_tx.send(()).unwrap();
        renamed.unwrap();
        loading.await.unwrap().unwrap();
        let last = saved.lock().unwrap().last().unwrap().clone();
        assert_eq!(last.0.name.as_deref(), Some("New alias"));
        assert_eq!(last.1.access_token, "rotated-access");
        assert_eq!(client.account().name.as_deref(), Some("New alias"));
    }

    #[tokio::test]
    async fn revoked_refresh_does_not_retry_login_or_hide_authentication_failure() {
        let (url, seen) = server::serve(|_| (401, String::new())).await;
        let saved = Saved::default();
        assert_eq!(client(&url, &saved).libraries().await, Err(MusicError::Authentication));
        assert!(seen.lock().unwrap().iter().all(|s| !s.path().ends_with("/login")));
        assert!(saved.lock().unwrap().is_empty());
        // Without a refresh token the failure is immediate.
        let no_refresh = AudiobookshelfClient::with_http(
            http(),
            account(url.clone()),
            AudiobookshelfTokens { access_token: "x".into(), refresh_token: None },
            Arc::new(|_: &AudiobookshelfAccount, _: &AudiobookshelfTokens| Ok(())),
        );
        let before = seen.lock().unwrap().len();
        assert_eq!(no_refresh.libraries().await, Err(MusicError::Authentication));
        assert_eq!(seen.lock().unwrap().len(), before + 1);
    }

    #[tokio::test]
    async fn refresh_cannot_switch_user_identity() {
        let (url, _) = server::serve(|request| {
            if request.path().ends_with("/refresh") {
                return (
                    200,
                    r#"{"user":{"id":"other","username":"other","accessToken":"other-access","refreshToken":"other-refresh"}}"#.into(),
                );
            }
            (401, String::new())
        })
        .await;
        let saved = Saved::default();
        let client = client(&url, &saved);
        assert_eq!(client.libraries().await, Err(MusicError::Authentication));
        assert!(saved.lock().unwrap().is_empty());
        assert_eq!(client.tokens(), tokens());
    }

    #[tokio::test]
    async fn http_failures_and_malformed_catalog_do_not_become_an_empty_library() {
        for (status, body, expected) in [
            (403, "", MusicError::Forbidden),
            (503, "", MusicError::ServerUnavailable),
            (302, "", MusicError::InvalidResponse),
            (200, "<html>Sign in</html>", MusicError::InvalidResponse),
        ] {
            let (url, _) = server::serve(move |_| (status, body.to_string())).await;
            assert_eq!(client(&url, &Saved::default()).libraries().await, Err(expected), "{status}");
        }
        let closed = Url::parse("http://127.0.0.1:9/books").unwrap();
        assert_eq!(client(&closed, &Saved::default()).libraries().await, Err(MusicError::ServerUnavailable));
    }

    #[tokio::test]
    async fn progress_upload_keeps_completion_fraction_and_separate_listening_time() {
        let (url, seen) = server::serve(|_| (200, String::new())).await;
        let client = client(&url, &Saved::default());
        let progress = AudiobookProgress::new(75.0, 180.0);
        client.update_progress("book", &progress).await.unwrap();
        client.sync("session", &progress, 5.0, true).await.unwrap();
        let seen = seen.lock().unwrap().clone();
        assert_eq!((seen[0].method.as_str(), seen[0].path()), ("PATCH", "/books/api/me/progress/book"));
        assert_eq!(seen[0].json()["progress"], 75.0 / 180.0);
        assert_eq!(seen[0].json()["isFinished"], false);
        assert_eq!(seen[0].json()["currentTime"], 75.0);
        assert_eq!(seen[1].path(), "/books/api/session/session/close");
        assert_eq!(seen[1].json()["timeListened"], 5.0);
        assert_eq!(seen[1].json()["duration"], 180.0);
    }

    #[tokio::test]
    async fn missing_progress_is_distinct_from_offline_and_authentication_failure() {
        let (missing, _) = server::serve(|_| (404, String::new())).await;
        assert_eq!(client(&missing, &Saved::default()).progress("book").await, Ok(None));
        let (offline, _) = server::serve(|_| (503, String::new())).await;
        assert_eq!(client(&offline, &Saved::default()).progress("book").await, Err(MusicError::ServerUnavailable));
    }

    #[tokio::test]
    async fn push_falls_back_to_progress_route_and_reconcile_uploads_only_newer_local_positions() {
        let remote = AudiobookProgress::at(20.0, 180.0, 2000.0);
        let (url, seen) = server::serve(move |request| match (request.method.as_str(), request.path()) {
            ("POST", p) if p.ends_with("/sync") => (404, String::new()),
            ("GET", p) if p.ends_with("/progress/book") => (200, serde_json::to_string(&remote).unwrap()),
            _ => (200, String::new()),
        })
        .await;
        let client = client(&url, &Saved::default());
        let local = AudiobookProgress::at(75.0, 180.0, 1000.0);
        client.push_progress("book", "expired", &local, 3.0, false, false).await.unwrap();
        client.push_progress("book", "unused", &local, 3.0, false, true).await.unwrap();
        assert_eq!(client.reconcile_progress("book", Some(&local)).await, Ok(Some(remote)));
        let newer = AudiobookProgress::at(90.0, 180.0, 3000.0);
        assert_eq!(client.reconcile_progress("book", Some(&newer)).await, Ok(Some(newer)));
        assert_eq!(client.reconcile_progress("book", None).await, Ok(Some(remote)));
        let routes: Vec<String> = seen.lock().unwrap().iter().map(|s| format!("{} {}", s.method, s.path())).collect();
        assert_eq!(
            routes,
            [
                "POST /books/api/session/expired/sync",
                "PATCH /books/api/me/progress/book",
                "PATCH /books/api/me/progress/book",
                "GET /books/api/me/progress/book",
                "GET /books/api/me/progress/book",
                "PATCH /books/api/me/progress/book",
                "GET /books/api/me/progress/book",
            ]
        );
    }

    #[tokio::test]
    async fn media_urls_verify_the_token_first_and_downloads_are_planned() {
        let detail = r#"{"id":"book","libraryId":"library","mediaType":"book","media":{"metadata":{"title":"Synthetic book"},"duration":180,"tracks":[{"startOffset":0,"duration":60,"contentUrl":"/api/items/book/file/1","mimeType":"audio/mpeg"},{"startOffset":60,"duration":120,"contentUrl":"/api/items/book/file/2"}]}}"#;
        let (url, seen) = server::serve(move |request| match request.path() {
            "/books/api/items/book" => (200, detail.into()),
            "/books/api/me/progress/book" => (404, String::new()),
            "/books/api/items/book/cover" => (200, "jpeg".into()),
            _ => (200, "{}".into()),
        })
        .await;
        let client = client(&url, &Saved::default());
        let media = client.media_url("/api/items/book/file/1").await.unwrap();
        assert_eq!(media.query(), Some("token=access-fixture"));
        assert_eq!(seen.lock().unwrap()[0].path(), "/books/api/me");
        let plan = crate::offline_audiobook::prepare_download(&client, "account", "book", None).await.unwrap();
        assert_eq!(plan.urls.iter().map(|(i, _)| *i).collect::<Vec<_>>(), [0, 1]);
        assert!(plan.urls[1].1.path().ends_with("/api/items/book/file/2"));
        assert_eq!(plan.cover.as_deref(), Some(&b"jpeg"[..]));
        assert_eq!(plan.server_progress, None);
        assert!(plan.record.queued());
        assert!(!serde_json::to_string(&plan.record).unwrap().contains("access-fixture"));
    }
}
