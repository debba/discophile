//! Library models shared by every provider. Field names follow the Subsonic JSON wire format.

use serde::{Deserialize, Deserializer, Serialize};
use url::Url;

use crate::error::MusicError;
use crate::md5_hex;

/// Accepts JSON strings or numbers for identifiers that some servers encode numerically.
pub(crate) fn lenient_string<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Value {
        S(String),
        I(i64),
        U(u64),
        F(f64),
    }
    Ok(match Value::deserialize(d)? {
        Value::S(s) => s,
        Value::I(i) => i.to_string(),
        Value::U(u) => u.to_string(),
        Value::F(f) => f.to_string(),
    })
}

pub(crate) fn lenient_opt_string<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Value {
        S(String),
        I(i64),
        U(u64),
        F(f64),
    }
    Ok(Option::<Value>::deserialize(d)?.map(|v| match v {
        Value::S(s) => s,
        Value::I(i) => i.to_string(),
        Value::U(u) => u.to_string(),
        Value::F(f) => f.to_string(),
    }))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum MusicProvider {
    #[default]
    Subsonic,
    Navidrome,
    Jellyfin,
}

impl MusicProvider {
    pub const ALL: [MusicProvider; 3] = [Self::Subsonic, Self::Navidrome, Self::Jellyfin];
    pub fn title(self) -> &'static str {
        match self {
            Self::Subsonic => "Subsonic / OpenSubsonic",
            Self::Navidrome => "Navidrome",
            Self::Jellyfin => "Jellyfin",
        }
    }
    pub fn supports_podcasts(self) -> bool {
        self == Self::Subsonic
    }
    pub fn alias_title(self) -> &'static str {
        if self == Self::Subsonic { "Subsonic" } else { self.title() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Authentication {
    #[default]
    Token,
    Password,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Credentials {
    pub server: Url,
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub authentication: Authentication,
    #[serde(default)]
    pub provider: MusicProvider,
    /// Jellyfin's user ID. Its access token is stored in `password` after authentication.
    #[serde(default, rename = "userID")]
    pub user_id: Option<String>,
    #[serde(default, rename = "deviceID")]
    pub device_id: Option<String>,
}

/// Validates a server address: http(s), a host, no userinfo, query or fragment; trailing slashes removed.
pub fn validate_server(server: &str, allow_http: bool) -> Result<Url, MusicError> {
    let mut url = Url::parse(server.trim()).map_err(|_| MusicError::InvalidServer)?;
    let scheme = url.scheme().to_ascii_lowercase();
    if !(scheme == "https" || scheme == "http") {
        return Err(MusicError::InvalidServer);
    }
    if url.host_str().is_none_or(str::is_empty)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(MusicError::InvalidServer);
    }
    if scheme != "https" && !allow_http {
        return Err(MusicError::InsecureServer);
    }
    let path = url.path().trim_end_matches('/').to_string();
    url.set_path(&path);
    Ok(url)
}

/// The server URL without a trailing slash, as Swift's `URL.absoluteString` renders it.
pub fn server_string(url: &Url) -> String {
    let s = url.as_str();
    s.strip_suffix('/').unwrap_or(s).to_string()
}

impl Credentials {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        server: &str,
        username: &str,
        password: &str,
        allow_http: bool,
        authentication: Authentication,
        provider: MusicProvider,
        user_id: Option<String>,
        device_id: Option<String>,
    ) -> Result<Self, MusicError> {
        let server = validate_server(server, allow_http)?;
        let user = username.trim();
        if user.is_empty() || password.is_empty() {
            return Err(MusicError::MissingCredentials);
        }
        let device_id = if provider == MusicProvider::Jellyfin {
            Some(device_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string().to_uppercase()))
        } else {
            None
        };
        Ok(Self {
            server,
            username: user.to_string(),
            password: password.to_string(),
            authentication,
            provider,
            user_id,
            device_id,
        })
    }

    pub fn simple(server: &str, username: &str, password: &str, allow_http: bool, provider: MusicProvider) -> Result<Self, MusicError> {
        Self::new(server, username, password, allow_http, Authentication::Token, provider, None, None)
    }

    /// The server base as a string without a trailing slash.
    pub fn server_string(&self) -> String {
        server_string(&self.server)
    }

    /// A namespace for caches and downloads. No password, token, or authenticated URL is part of it.
    pub fn account_key(&self) -> String {
        let namespace = if self.provider == MusicProvider::Jellyfin { "jellyfin\n" } else { "" };
        md5_hex(format!("{namespace}{}\n{}", self.server_string(), self.username).as_bytes())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayGain {
    #[serde(default)]
    pub track_gain: Option<f64>,
    #[serde(default)]
    pub album_gain: Option<f64>,
    #[serde(default)]
    pub track_peak: Option<f64>,
    #[serde(default)]
    pub album_peak: Option<f64>,
    #[serde(default)]
    pub base_gain: Option<f64>,
    #[serde(default)]
    pub fallback_gain: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Song {
    #[serde(deserialize_with = "lenient_string")]
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub artist: Option<String>,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default, deserialize_with = "lenient_opt_string")]
    pub album_id: Option<String>,
    #[serde(default, deserialize_with = "lenient_opt_string")]
    pub artist_id: Option<String>,
    #[serde(default, deserialize_with = "lenient_opt_string")]
    pub cover_art: Option<String>,
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default)]
    pub track: Option<i64>,
    #[serde(default)]
    pub disc_number: Option<i64>,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub suffix: Option<String>,
    #[serde(default)]
    pub bit_rate: Option<i64>,
    #[serde(default)]
    pub starred: Option<String>,
    #[serde(default, deserialize_with = "lenient_opt_string")]
    pub stream_id: Option<String>,
    #[serde(default)]
    pub is_podcast: Option<bool>,
    #[serde(default)]
    pub year: Option<i64>,
    #[serde(default)]
    pub genre: Option<String>,
    /// Album metadata kept separately: an edition's year can differ from its original release.
    #[serde(default)]
    pub album_year: Option<i64>,
    #[serde(default)]
    pub album_original_year: Option<i64>,
    /// OpenSubsonic audio details and loudness tags, when the server provides them.
    #[serde(default)]
    pub sampling_rate: Option<i64>,
    #[serde(default)]
    pub bit_depth: Option<i64>,
    #[serde(default)]
    pub channel_count: Option<i64>,
    #[serde(default)]
    pub replay_gain: Option<ReplayGain>,
}

impl Song {
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self { id: id.into(), title: title.into(), ..Default::default() }
    }
    pub fn stream_identifier(&self) -> &str {
        self.stream_id.as_deref().unwrap_or(&self.id)
    }
    pub fn is_podcast(&self) -> bool {
        self.is_podcast == Some(true)
    }
    pub fn is_starred(&self) -> bool {
        self.starred.is_some()
    }
    pub fn subtitle(&self) -> String {
        match (&self.artist, &self.album) {
            (Some(a), Some(b)) if !b.is_empty() => format!("{a} · {b}"),
            (Some(a), _) => a.clone(),
            (None, Some(b)) => b.clone(),
            _ => String::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ReleaseDate {
    #[serde(default)]
    pub year: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Album {
    #[serde(deserialize_with = "lenient_string")]
    pub id: String,
    #[serde(default, alias = "title")]
    pub name: String,
    #[serde(default)]
    pub artist: Option<String>,
    #[serde(default, deserialize_with = "lenient_opt_string")]
    pub artist_id: Option<String>,
    #[serde(default, deserialize_with = "lenient_opt_string")]
    pub cover_art: Option<String>,
    #[serde(default)]
    pub song_count: Option<i64>,
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default)]
    pub year: Option<i64>,
    #[serde(default)]
    pub genre: Option<String>,
    #[serde(default)]
    pub starred: Option<String>,
    #[serde(default)]
    pub song: Option<Vec<Song>>,
    #[serde(default)]
    pub original_release_date: Option<ReleaseDate>,
    #[serde(default)]
    pub is_compilation: Option<bool>,
}

impl Album {
    pub fn new(id: impl Into<String>, name: impl Into<String>, artist: impl Into<String>) -> Self {
        Self { id: id.into(), name: name.into(), artist: Some(artist.into()), ..Default::default() }
    }
    pub fn songs(&self) -> &[Song] {
        self.song.as_deref().unwrap_or_default()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artist {
    #[serde(deserialize_with = "lenient_string")]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default, deserialize_with = "lenient_opt_string")]
    pub cover_art: Option<String>,
    #[serde(default)]
    pub album_count: Option<i64>,
    #[serde(default)]
    pub album: Option<Vec<Album>>,
}

impl Artist {
    pub fn albums(&self) -> &[Album] {
        self.album.as_deref().unwrap_or_default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PlaylistAdditionResult {
    pub added_count: usize,
    pub skipped_count: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Playlist {
    #[serde(deserialize_with = "lenient_string")]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default, deserialize_with = "lenient_opt_string")]
    pub cover_art: Option<String>,
    #[serde(default)]
    pub song_count: Option<i64>,
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default)]
    pub comment: Option<String>,
    #[serde(default)]
    pub entry: Option<Vec<Song>>,
}

impl Playlist {
    pub fn songs(&self) -> &[Song] {
        self.entry.as_deref().unwrap_or_default()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SearchResults {
    #[serde(default)]
    pub artist: Option<Vec<Artist>>,
    #[serde(default)]
    pub album: Option<Vec<Album>>,
    #[serde(default)]
    pub song: Option<Vec<Song>>,
}

impl SearchResults {
    pub fn new(artist: Vec<Artist>, album: Vec<Album>, song: Vec<Song>) -> Self {
        Self { artist: Some(artist), album: Some(album), song: Some(song) }
    }
    pub fn artists(&self) -> &[Artist] {
        self.artist.as_deref().unwrap_or_default()
    }
    pub fn albums(&self) -> &[Album] {
        self.album.as_deref().unwrap_or_default()
    }
    pub fn songs(&self) -> &[Song] {
        self.song.as_deref().unwrap_or_default()
    }
    pub fn is_empty(&self) -> bool {
        self.artists().is_empty() && self.albums().is_empty() && self.songs().is_empty()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PodcastChannel {
    #[serde(deserialize_with = "lenient_string")]
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, deserialize_with = "lenient_opt_string")]
    pub cover_art: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub episode: Option<Vec<PodcastEpisode>>,
}

impl PodcastChannel {
    pub fn episodes(&self) -> &[PodcastEpisode] {
        self.episode.as_deref().unwrap_or_default()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PodcastEpisode {
    #[serde(deserialize_with = "lenient_string")]
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default, deserialize_with = "podcast_stream_id")]
    pub stream_id: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default)]
    pub publish_date: Option<String>,
    #[serde(default, deserialize_with = "lenient_opt_string")]
    pub cover_art: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

fn podcast_stream_id<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Ok(lenient_opt_string(d)?.filter(|s| !s.is_empty()))
}

impl PodcastEpisode {
    pub fn is_playable(&self) -> bool {
        self.stream_id.is_some() && self.status.as_deref().is_none_or(|s| s == "completed")
    }
    pub fn song(&self, channel: &PodcastChannel) -> Song {
        Song {
            id: format!("podcast:{}:{}", channel.id, self.id),
            title: self.title.clone(),
            artist: Some(channel.title.clone()),
            album: Some(channel.title.clone()),
            album_id: Some(format!("podcast:{}", channel.id)),
            cover_art: self.cover_art.clone().or_else(|| channel.cover_art.clone()),
            duration: self.duration,
            stream_id: self.stream_id.clone(),
            is_podcast: Some(true),
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum StreamingQuality {
    Original,
    #[default]
    High,
    Compact,
}

impl StreamingQuality {
    pub const ALL: [StreamingQuality; 3] = [Self::Original, Self::High, Self::Compact];
    pub fn parameters(self) -> Vec<(&'static str, &'static str)> {
        match self {
            Self::Original => vec![("format", "raw")],
            Self::High => vec![("format", "mp3"), ("maxBitRate", "320")],
            Self::Compact => vec![("format", "mp3"), ("maxBitRate", "128")],
        }
    }
    /// The English localization key for this quality.
    pub fn title(self) -> &'static str {
        match self {
            Self::Original => "Original",
            Self::High => "High quality",
            Self::Compact => "Data saver",
        }
    }
}

pub mod podcast_order {
    use super::PodcastEpisode;
    use chrono::DateTime;

    /// Newest first; undated episodes keep their server order after dated ones.
    pub fn newest_first(episodes: &[PodcastEpisode]) -> Vec<PodcastEpisode> {
        let mut indexed: Vec<(usize, &PodcastEpisode, Option<i64>)> = episodes
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let date = e
                    .publish_date
                    .as_deref()
                    .and_then(|d| DateTime::parse_from_rfc3339(d).ok())
                    .map(|d| d.timestamp_millis());
                (i, e, date)
            })
            .collect();
        indexed.sort_by(|l, r| match (l.2, r.2) {
            (Some(a), Some(b)) if a != b => b.cmp(&a),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            _ => l.0.cmp(&r.0),
        });
        indexed.into_iter().map(|(_, e, _)| e.clone()).collect()
    }
}

/// A saved music library connection, with an optional user alias.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServerAccount {
    pub credentials: Credentials,
    #[serde(default)]
    pub name: String,
}

impl ServerAccount {
    pub fn new(credentials: Credentials, name: &str) -> Self {
        Self { credentials, name: name.trim().to_string() }
    }
    pub fn id(&self) -> String {
        self.credentials.account_key()
    }
    pub fn display_name(&self) -> String {
        connection_alias::resolve(Some(&self.name), self.credentials.provider.alias_title(), &self.credentials.server, true)
    }
    pub fn subtitle(&self) -> String {
        format!("{} · {}", self.credentials.username, self.credentials.server_string())
    }
}

pub mod connection_alias {
    use url::Url;

    pub fn suggested(provider: &str, server: Option<&Url>, include_path: bool) -> String {
        let Some(server) = server else { return provider.to_string() };
        let Some(host) = server.host_str().filter(|h| !h.is_empty()) else { return provider.to_string() };
        let port = server.port().map(|p| format!(":{p}")).unwrap_or_default();
        let path = if include_path { server.path().trim_end_matches('/') } else { "" };
        format!("{provider} · {host}{port}{path}")
    }

    pub fn resolve(alias: Option<&str>, provider: &str, server: &Url, include_path: bool) -> String {
        let name = alias.map(str::trim).unwrap_or_default();
        if name.is_empty() { suggested(provider, Some(server), include_path) } else { name.to_string() }
    }
}

/// A saved direct HTTP(S) audio or HLS stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InternetRadioStation {
    pub id: uuid::Uuid,
    #[serde(default)]
    pub name: String,
    #[serde(rename = "streamURL")]
    pub stream_url: Url,
    #[serde(default, rename = "allowHTTP")]
    pub allow_http: bool,
}

impl InternetRadioStation {
    pub fn new(id: Option<uuid::Uuid>, name: &str, stream_url: &str, allow_http: bool) -> Result<Self, MusicError> {
        let url = Url::parse(stream_url.trim()).map_err(|_| MusicError::InvalidServer)?;
        let scheme = url.scheme().to_ascii_lowercase();
        if !(scheme == "https" || scheme == "http")
            || url.host_str().is_none_or(str::is_empty)
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err(MusicError::InvalidServer);
        }
        if scheme != "https" && !allow_http {
            return Err(MusicError::InsecureServer);
        }
        Ok(Self { id: id.unwrap_or_else(uuid::Uuid::new_v4), name: name.trim().to_string(), stream_url: url, allow_http })
    }
    pub fn display_name(&self) -> String {
        connection_alias::resolve(Some(&self.name), "Radio", &self.stream_url, false)
    }
    pub fn song(&self) -> Song {
        Song {
            id: format!("internet-radio:{}", self.id.to_string().to_uppercase()),
            title: self.display_name(),
            artist: self.stream_url.host_str().map(str::to_string),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_validate_and_trim() {
        let c = Credentials::simple(" https://music.example.com:8443/navidrome/ ", " me ", "pw", false, MusicProvider::Navidrome).unwrap();
        assert_eq!(c.server_string(), "https://music.example.com:8443/navidrome");
        assert_eq!(c.username, "me");
        assert_eq!(Credentials::simple("http://x.local", "a", "b", false, MusicProvider::Subsonic), Err(MusicError::InsecureServer));
        assert_eq!(Credentials::simple("ftp://x", "a", "b", true, MusicProvider::Subsonic), Err(MusicError::InvalidServer));
        assert_eq!(Credentials::simple("https://x?y=1", "a", "b", true, MusicProvider::Subsonic), Err(MusicError::InvalidServer));
        assert_eq!(Credentials::simple("https://x", "", "b", true, MusicProvider::Subsonic), Err(MusicError::MissingCredentials));
    }

    #[test]
    fn root_server_has_no_trailing_slash() {
        let c = Credentials::simple("https://music.example.com/", "me", "pw", false, MusicProvider::Subsonic).unwrap();
        assert_eq!(c.server_string(), "https://music.example.com");
    }

    #[test]
    fn alias_suggestion() {
        let url = Url::parse("https://music.example.com:8443/nd").unwrap();
        assert_eq!(connection_alias::suggested("Navidrome", Some(&url), true), "Navidrome · music.example.com:8443/nd");
        assert_eq!(connection_alias::resolve(Some("  "), "Radio", &url, false), "Radio · music.example.com:8443");
    }

    #[test]
    fn podcast_episode_numeric_stream_id() {
        let e: PodcastEpisode = serde_json::from_str(r#"{"id":"1","title":"t","streamId":42,"status":"completed"}"#).unwrap();
        assert_eq!(e.stream_id.as_deref(), Some("42"));
        assert!(e.is_playable());
        let e: PodcastEpisode = serde_json::from_str(r#"{"id":"1","title":"t","streamId":""}"#).unwrap();
        assert!(!e.is_playable());
    }
}
