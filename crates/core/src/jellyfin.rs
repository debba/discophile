//! Native Jellyfin music API. Passwords are exchanged for an access token before an account is saved;
//! the token then lives in `Credentials::password`, with `user_id` and `device_id` alongside it.

use std::collections::{BTreeMap, HashMap, HashSet};

use reqwest::Method;
use serde::Deserialize;
use serde_json::json;
use url::Url;

use crate::assistant::edits::{AssistantEditOperation, AssistantEditProposal};
use crate::error::{MusicError, MusicResult};
use crate::lyrics::{LyricLine, SongLyrics};
use crate::models::{
    Album, Artist, Authentication, Credentials, MusicBrainzId, MusicProvider, Playlist, PlaylistAdditionResult,
    PodcastChannel, SearchResults, ServerInfo, Song, StreamingQuality,
};
use crate::playback_progress::{ProgressUpload, REMOTE_LIMIT, RemoteProgress, parse_timestamp};
use crate::subsonic::{SearchLimits, occurrence_index};

const FIELDS: &str = "Genres,MediaStreams,ChildCount,Overview";
const PAGE_SIZE: usize = 500;
const TICKS_PER_SECOND: f64 = 10_000_000.0;

#[cfg(not(target_arch = "wasm32"))]
const DEVICE: &str = "Desktop";
#[cfg(target_arch = "wasm32")]
const DEVICE: &str = "Browser";

type Query = BTreeMap<&'static str, String>;

fn query<const N: usize>(pairs: [(&'static str, String); N]) -> Query {
    pairs.into_iter().collect()
}

/// Maps an HTTP status to an error. Redirects are not followed and count as unavailable.
pub fn check_status(status: u16) -> MusicResult<()> {
    match status {
        200..=299 => Ok(()),
        401 => Err(MusicError::Authentication),
        403 => Err(MusicError::Forbidden),
        404 => Err(MusicError::NotFound),
        405 | 501 => Err(MusicError::Unsupported),
        _ => Err(MusicError::ServerUnavailable),
    }
}

fn decode<'a, T: Deserialize<'a>>(data: &'a [u8]) -> MusicResult<T> {
    serde_json::from_slice(data).map_err(|_| MusicError::InvalidResponse)
}

/// The `Authorization` header. The token is included only for authenticated requests.
pub fn authorization(credentials: &Credentials, authenticated: bool) -> String {
    let device_id = credentials.device_id.as_deref().unwrap_or("Discophile");
    let mut value = format!(
        "MediaBrowser Client=\"Discophile\", Device=\"{DEVICE}\", DeviceId=\"{device_id}\", Version=\"{}\"",
        env!("CARGO_PKG_VERSION")
    );
    if authenticated {
        value.push_str(&format!(", Token=\"{}\"", credentials.password));
    }
    value
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Login {
    access_token: String,
    user: LoginUser,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct LoginUser {
    id: String,
}

/// Validates a login response and returns the credentials to persist: the token replaces the password.
pub fn login_credentials(credentials: &Credentials, body: &[u8]) -> MusicResult<Credentials> {
    let login: Login = decode(body)?;
    let token = login.access_token;
    // The token is sent inside a quoted header value.
    if token.is_empty() || login.user.id.is_empty() || token.chars().any(|c| matches!(c, '"' | '\\') || is_newline(c)) {
        return Err(MusicError::InvalidResponse);
    }
    Ok(Credentials {
        password: token,
        authentication: Authentication::Token,
        provider: MusicProvider::Jellyfin,
        user_id: Some(login.user.id),
        ..credentials.clone()
    })
}

fn is_newline(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{0B}' | '\u{0C}' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct Page {
    items: Vec<Item>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Named {
    id: String,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct UserInfo {
    #[serde(default)]
    is_favorite: Option<bool>,
    #[serde(default)]
    playback_position_ticks: Option<f64>,
    #[serde(default)]
    last_played_date: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct MediaStream {
    #[serde(default, rename = "Type")]
    kind: Option<String>,
    #[serde(default)]
    bit_rate: Option<i64>,
    #[serde(default)]
    sample_rate: Option<i64>,
    #[serde(default)]
    bit_depth: Option<i64>,
    #[serde(default)]
    channels: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct Item {
    id: String,
    name: String,
    #[serde(default, rename = "Type")]
    kind: Option<String>,
    #[serde(default)]
    artists: Option<Vec<String>>,
    #[serde(default)]
    artist_items: Option<Vec<Named>>,
    #[serde(default)]
    album_artists: Option<Vec<Named>>,
    #[serde(default)]
    album: Option<String>,
    #[serde(default)]
    album_id: Option<String>,
    #[serde(default)]
    album_primary_image_tag: Option<String>,
    #[serde(default)]
    image_tags: Option<HashMap<String, String>>,
    #[serde(default)]
    run_time_ticks: Option<f64>,
    #[serde(default)]
    index_number: Option<i64>,
    #[serde(default)]
    parent_index_number: Option<i64>,
    #[serde(default)]
    production_year: Option<i64>,
    #[serde(default)]
    genres: Option<Vec<String>>,
    #[serde(default)]
    container: Option<String>,
    #[serde(default)]
    media_streams: Option<Vec<MediaStream>>,
    #[serde(default)]
    user_data: Option<UserInfo>,
    #[serde(default)]
    child_count: Option<i64>,
    #[serde(default)]
    overview: Option<String>,
    #[serde(default)]
    provider_ids: Option<HashMap<String, String>>,
}

impl Item {
    fn cover(&self) -> Option<String> {
        if self.image_tags.as_ref().is_some_and(|t| t.contains_key("Primary")) {
            Some(self.id.clone())
        } else if self.album_primary_image_tag.is_some() {
            self.album_id.clone()
        } else {
            None
        }
    }
    fn seconds(&self) -> Option<f64> {
        self.run_time_ticks.map(|t| t / TICKS_PER_SECOND)
    }
    fn starred(&self) -> Option<String> {
        (self.user_data.as_ref().and_then(|u| u.is_favorite) == Some(true)).then(|| "true".to_string())
    }
    fn genre(&self) -> Option<String> {
        self.genres.as_ref().and_then(|g| g.first().cloned())
    }
    fn remote_progress(&self) -> Option<RemoteProgress> {
        let data = self.user_data.as_ref()?;
        let position = data.playback_position_ticks.filter(|t| t.is_finite() && *t > 0.0)? / TICKS_PER_SECOND;
        let updated = data.last_played_date.as_deref().and_then(parse_timestamp);
        Some(RemoteProgress { id: self.id.clone(), position, duration: self.seconds(), updated })
    }

    fn song(&self) -> Song {
        let audio = self.media_streams.iter().flatten().find(|s| s.kind.as_deref() == Some("Audio"));
        Song {
            id: self.id.clone(),
            title: self.name.clone(),
            artist: self.artists.as_ref().map(|a| a.join(", ")),
            album: self.album.clone(),
            album_id: self.album_id.clone(),
            artist_id: self.artist_items.as_ref().and_then(|a| a.first()).map(|a| a.id.clone()),
            cover_art: self.cover(),
            duration: self.seconds(),
            track: self.index_number,
            disc_number: self.parent_index_number,
            suffix: self.container.clone(),
            bit_rate: audio.and_then(|a| a.bit_rate).map(|b| b / 1000),
            starred: self.starred(),
            year: self.production_year,
            genre: self.genre(),
            sampling_rate: audio.and_then(|a| a.sample_rate),
            bit_depth: audio.and_then(|a| a.bit_depth),
            channel_count: audio.and_then(|a| a.channels),
            ..Default::default()
        }
    }

    fn album(&self, songs: Option<Vec<Song>>) -> Album {
        let artist = match (&self.album_artists, &self.artists) {
            (Some(album_artists), _) => {
                album_artists.iter().filter_map(|a| a.name.clone()).collect::<Vec<_>>().join(", ")
            }
            (None, Some(artists)) => artists.join(", "),
            (None, None) => String::new(),
        };
        Album {
            id: self.id.clone(),
            name: self.name.clone(),
            artist: Some(artist),
            artist_id: self.album_artists.as_ref().and_then(|a| a.first()).map(|a| a.id.clone()),
            cover_art: self.cover(),
            song_count: self.child_count,
            duration: self.seconds(),
            year: self.production_year,
            genre: self.genre(),
            starred: self.starred(),
            song: songs,
            ..Default::default()
        }
    }

    fn artist(&self, albums: Option<Vec<Album>>) -> Artist {
        Artist {
            id: self.id.clone(),
            name: self.name.clone(),
            cover_art: self.cover(),
            album_count: self.child_count,
            album: albums,
        }
    }

    fn server_info(&self) -> ServerInfo {
        let id = |key: &str| {
            self.provider_ids.as_ref()?.get(key).map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
        };
        let musicbrainz = id("MusicBrainzArtist")
            .filter(|_| self.kind.as_deref() == Some("MusicArtist"))
            .map(MusicBrainzId::Artist)
            .or_else(|| id("MusicBrainzReleaseGroup").map(MusicBrainzId::ReleaseGroup))
            .or_else(|| id("MusicBrainzAlbum").map(MusicBrainzId::Release));
        ServerInfo {
            text: self.overview.as_ref().map(|t| t.trim().to_string()).filter(|t| !t.is_empty()),
            musicbrainz,
            link: None,
        }
    }

    fn playlist(&self, songs: Option<Vec<Song>>) -> Playlist {
        Playlist {
            id: self.id.clone(),
            name: self.name.clone(),
            cover_art: self.cover(),
            song_count: self.child_count,
            duration: self.seconds(),
            comment: self.overview.clone(),
            entry: songs,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct JellyfinLyrics {
    lyrics: Vec<JellyfinLyricLine>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct JellyfinLyricLine {
    text: String,
    #[serde(default)]
    start: Option<f64>,
}

/// Converts Jellyfin's tick-based lyrics; empty lyrics are absent lyrics.
pub fn decode_lyrics(body: &[u8]) -> MusicResult<Option<SongLyrics>> {
    let result: JellyfinLyrics = decode(body)?;
    let lyrics = SongLyrics::new(
        result
            .lyrics
            .into_iter()
            .map(|l| LyricLine { text: l.text, start: l.start.map(|s| s / TICKS_PER_SECOND) })
            .collect(),
    );
    Ok((!lyrics.is_empty()).then_some(lyrics))
}

#[derive(Clone)]
pub struct JellyfinClient {
    credentials: Credentials,
    http: reqwest::Client,
}

impl std::fmt::Debug for JellyfinClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JellyfinClient").field("server", &self.credentials.server_string()).finish_non_exhaustive()
    }
}

impl JellyfinClient {
    pub fn new(credentials: Credentials) -> Self {
        Self::with_http_client(credentials, crate::http::server_client())
    }

    /// `http` must not follow redirects; see `http::server_client`.
    pub fn with_http_client(credentials: Credentials, http: reqwest::Client) -> Self {
        Self { credentials, http }
    }

    pub fn credentials(&self) -> &Credentials {
        &self.credentials
    }

    fn user_id(&self) -> String {
        self.credentials.user_id.clone().unwrap_or_default()
    }

    /// Exchanges the username and password (sent only in the request body) for an access token.
    pub async fn connect(credentials: Credentials) -> MusicResult<Self> {
        Self::connect_with(credentials, crate::http::server_client()).await
    }

    pub async fn connect_with(credentials: Credentials, http: reqwest::Client) -> MusicResult<Self> {
        let client = Self::with_http_client(credentials, http);
        let body = json!({ "Username": client.credentials.username, "Pw": client.credentials.password });
        let response =
            client.data(&["Users", "AuthenticateByName"], Query::new(), Method::POST, Some(body), false).await?;
        let saved = login_credentials(&client.credentials, &response)?;
        Ok(Self::with_http_client(saved, client.http))
    }

    /// A server URL whose path segments are percent-encoded. Media URLs carry the token as `api_key`.
    pub fn url(&self, segments: &[&str], mut parameters: Query, media: bool) -> Url {
        if media {
            parameters.insert("api_key", self.credentials.password.clone());
        }
        let mut url = self.credentials.server.clone();
        if let Ok(mut path) = url.path_segments_mut() {
            path.pop_if_empty().extend(segments);
        }
        if !parameters.is_empty() {
            url.query_pairs_mut().extend_pairs(parameters.iter().map(|(k, v)| (*k, v.as_str())));
        }
        url
    }

    async fn data(
        &self,
        segments: &[&str],
        parameters: Query,
        method: Method,
        body: Option<serde_json::Value>,
        authenticated: bool,
    ) -> MusicResult<Vec<u8>> {
        if authenticated && self.user_id().is_empty() {
            return Err(MusicError::Authentication);
        }
        let mut request = self
            .http
            .request(method, self.url(segments, parameters, false))
            .header(reqwest::header::AUTHORIZATION, authorization(&self.credentials, authenticated))
            .header(reqwest::header::ACCEPT, "application/json");
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.map_err(|_| MusicError::ServerUnavailable)?;
        check_status(response.status().as_u16())?;
        let bytes = response.bytes().await.map_err(|_| MusicError::ServerUnavailable)?;
        Ok(bytes.to_vec())
    }

    async fn get(&self, segments: &[&str], parameters: Query) -> MusicResult<Vec<u8>> {
        self.data(segments, parameters, Method::GET, None, true).await
    }

    pub async fn ping(&self) -> MusicResult<()> {
        self.get(&["Users", "Me"], Query::new()).await.map(|_| ())
    }

    async fn items(
        &self,
        kind: &str,
        offset: usize,
        limit: usize,
        extra: Query,
        path: &[&str],
    ) -> MusicResult<Vec<Item>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut parameters = query([
            ("userId", self.user_id()),
            ("recursive", "true".into()),
            ("includeItemTypes", kind.into()),
            ("fields", FIELDS.into()),
            ("startIndex", offset.to_string()),
            ("limit", limit.min(PAGE_SIZE).to_string()),
            ("sortBy", "SortName".into()),
        ]);
        parameters.extend(extra);
        Ok(decode::<Page>(&self.get(path, parameters).await?)?.items)
    }

    async fn all_items(&self, kind: &str, extra: Query, path: &[&str]) -> MusicResult<Vec<Item>> {
        let mut result = Vec::new();
        loop {
            let next = self.items(kind, result.len(), PAGE_SIZE, extra.clone(), path).await?;
            let done = next.len() < PAGE_SIZE;
            result.extend(next);
            if done {
                return Ok(result);
            }
        }
    }

    async fn item(&self, id: &str) -> MusicResult<Item> {
        decode(&self.get(&["Items", id], query([("userId", self.user_id())])).await?)
    }

    pub async fn albums(&self, list_type: &str, offset: usize, size: usize) -> MusicResult<Vec<Album>> {
        let sort = if list_type == "newest" {
            query([("sortBy", "DateCreated".into()), ("sortOrder", "Descending".into())])
        } else {
            query([("sortBy", "SortName".into())])
        };
        Ok(self.items("MusicAlbum", offset, size, sort, &["Items"]).await?.iter().map(|i| i.album(None)).collect())
    }

    pub async fn album(&self, id: &str) -> MusicResult<Album> {
        let album = self.item(id).await?;
        let extra = query([("parentId", id.into()), ("sortBy", "ParentIndexNumber,IndexNumber,SortName".into())]);
        let songs = self.all_items("Audio", extra, &["Items"]).await?;
        Ok(album.album(Some(songs.iter().map(Item::song).collect())))
    }

    pub async fn artists(&self) -> MusicResult<Vec<Artist>> {
        let items = self.all_items("MusicArtist", Query::new(), &["Artists", "AlbumArtists"]).await?;
        Ok(items.iter().map(|i| i.artist(None)).collect())
    }

    pub async fn artist(&self, id: &str) -> MusicResult<Artist> {
        let artist = self.item(id).await?;
        let albums = self.all_items("MusicAlbum", query([("albumArtistIds", id.into())]), &["Items"]).await?;
        Ok(artist.artist(Some(albums.iter().map(|a| a.album(None)).collect())))
    }

    pub async fn song(&self, id: &str) -> MusicResult<Song> {
        Ok(self.item(id).await?.song())
    }

    pub async fn artist_info(&self, id: &str) -> MusicResult<ServerInfo> {
        Ok(self.item(id).await?.server_info())
    }

    pub async fn album_info(&self, id: &str) -> MusicResult<ServerInfo> {
        Ok(self.item(id).await?.server_info())
    }

    pub async fn search(&self, text: &str, limits: SearchLimits) -> MusicResult<SearchResults> {
        let parameters = if text.is_empty() { Query::new() } else { query([("searchTerm", text.into())]) };
        let (songs, albums, artists) = futures::try_join!(
            self.items("Audio", limits.song_offset, limits.song_count, parameters.clone(), &["Items"]),
            self.items("MusicAlbum", limits.album_offset, limits.album_count, parameters.clone(), &["Items"]),
            self.items("MusicArtist", limits.artist_offset, limits.artist_count, parameters.clone(), &["Artists"]),
        )?;
        Ok(SearchResults::new(
            artists.iter().map(|i| i.artist(None)).collect(),
            albums.iter().map(|i| i.album(None)).collect(),
            songs.iter().map(Item::song).collect(),
        ))
    }

    pub async fn favorites(&self) -> MusicResult<SearchResults> {
        let favorite = || query([("isFavorite", "true".into())]);
        let (songs, albums, artists) = futures::try_join!(
            self.all_items("Audio", favorite(), &["Items"]),
            self.all_items("MusicAlbum", favorite(), &["Items"]),
            self.all_items("MusicArtist", favorite(), &["Artists"]),
        )?;
        Ok(SearchResults::new(
            artists.iter().map(|i| i.artist(None)).collect(),
            albums.iter().map(|i| i.album(None)).collect(),
            songs.iter().map(Item::song).collect(),
        ))
    }

    pub async fn favorite(&self, id: &str, starred: bool) -> MusicResult<()> {
        let method = if starred { Method::POST } else { Method::DELETE };
        self.data(&["UserFavoriteItems", id], query([("userId", self.user_id())]), method, None, true).await.map(|_| ())
    }

    pub async fn random_songs(&self, size: usize) -> MusicResult<Vec<Song>> {
        let songs = self.items("Audio", 0, size, query([("sortBy", "Random".into())]), &["Items"]).await?;
        Ok(songs.iter().map(Item::song).collect())
    }

    /// Random tracks from up to ten similar artists.
    pub async fn similar_songs(&self, artist_id: &str, count: usize) -> MusicResult<Vec<Song>> {
        let related: Page = decode(
            &self
                .get(&["Items", artist_id, "Similar"], query([("userId", self.user_id()), ("limit", "10".into())]))
                .await?,
        )?;
        let ids: Vec<&str> =
            related.items.iter().filter(|i| i.kind.as_deref() == Some("MusicArtist")).map(|i| i.id.as_str()).collect();
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let extra = query([("artistIds", ids.join(",")), ("sortBy", "Random".into())]);
        Ok(self.items("Audio", 0, count, extra, &["Items"]).await?.iter().map(Item::song).collect())
    }

    /// Audio left part-way, most recently played first, with Jellyfin's resume positions.
    pub async fn resume_positions(&self) -> MusicResult<Vec<RemoteProgress>> {
        let extra = query([
            ("filters", "IsResumable".into()),
            ("sortBy", "DatePlayed".into()),
            ("sortOrder", "Descending".into()),
        ]);
        let items = self.items("Audio", 0, REMOTE_LIMIT, extra, &["Items"]).await?;
        Ok(items.iter().filter_map(Item::remote_progress).collect())
    }

    /// Stores the resume position in the user's item data; clearing sets it back to the start.
    pub async fn save_progress(&self, id: &str, upload: ProgressUpload) -> MusicResult<()> {
        let body = match upload {
            ProgressUpload::Save(position) => json!({
                "PlaybackPositionTicks": (position.max(0.0) * TICKS_PER_SECOND).round() as i64,
                "LastPlayedDate": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            }),
            ProgressUpload::Clear => json!({ "PlaybackPositionTicks": 0 }),
        };
        let parameters = query([("userId", self.user_id())]);
        self.data(&["UserItems", id, "UserData"], parameters, Method::POST, Some(body), true).await.map(|_| ())
    }

    /// Jellyfin has no "now playing" scrobble here; a submission marks the item played.
    pub async fn scrobble(&self, id: &str, submission: bool) -> MusicResult<()> {
        if submission {
            self.data(&["UserPlayedItems", id], query([("userId", self.user_id())]), Method::POST, None, true).await?;
        }
        Ok(())
    }

    pub fn stream_url(&self, id: &str, quality: StreamingQuality) -> Url {
        if quality == StreamingQuality::Original {
            return self.url(&["Audio", id, "stream"], query([("static", "true".into())]), true);
        }
        let bit_rate = if quality == StreamingQuality::High { "320000" } else { "128000" };
        let parameters = query([
            ("audioCodec", "mp3".into()),
            ("audioBitRate", bit_rate.into()),
            ("static", "false".into()),
            ("enableAutoStreamCopy", "false".into()),
        ]);
        self.url(&["Audio", id, "stream.mp3"], parameters, true)
    }

    pub fn artwork_url(&self, id: &str, size: u32) -> Url {
        let parameters = query([("maxWidth", size.to_string()), ("maxHeight", size.to_string())]);
        self.url(&["Items", id, "Images", "Primary"], parameters, true)
    }

    pub async fn lyrics(&self, song: &Song) -> MusicResult<Option<SongLyrics>> {
        match self.get(&["Audio", song.stream_identifier(), "Lyrics"], Query::new()).await {
            Ok(body) => decode_lyrics(&body),
            Err(MusicError::NotFound | MusicError::Unsupported) => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub async fn podcasts(
        &self,
        _id: Option<&str>,
        _include_episodes: Option<bool>,
    ) -> MusicResult<Vec<PodcastChannel>> {
        Err(MusicError::Unsupported)
    }

    pub async fn create_podcast_channel(&self, _feed: &Url) -> MusicResult<()> {
        Err(MusicError::Unsupported)
    }

    pub async fn delete_podcast_channel(&self, _id: &str) -> MusicResult<()> {
        Err(MusicError::Unsupported)
    }

    pub async fn playlists(&self) -> MusicResult<Vec<Playlist>> {
        let items = self.all_items("Playlist", query([("mediaTypes", "Audio".into())]), &["Items"]).await?;
        Ok(items.iter().map(|i| i.playlist(None)).collect())
    }

    pub async fn playlist(&self, id: &str) -> MusicResult<Playlist> {
        let metadata = self.item(id).await?;
        let entries = self.all_items("Audio", Query::new(), &["Playlists", id, "Items"]).await?;
        Ok(metadata.playlist(Some(entries.iter().map(Item::song).collect())))
    }

    pub async fn create_playlist(&self, name: &str, songs: &[Song]) -> MusicResult<Option<Playlist>> {
        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct Created {
            id: String,
        }
        let mut seen = HashSet::new();
        let ids: Vec<&str> = songs.iter().map(Song::stream_identifier).filter(|id| seen.insert(*id)).collect();
        let body =
            json!({ "Name": name, "Ids": ids, "UserId": self.user_id(), "MediaType": "Audio", "IsPublic": false });
        let created: Created = decode(&self.data(&["Playlists"], Query::new(), Method::POST, Some(body), true).await?)?;
        Ok(Some(Playlist {
            id: created.id,
            name: name.to_string(),
            song_count: Some(ids.len() as i64),
            ..Default::default()
        }))
    }

    pub async fn rename_playlist(&self, id: &str, name: &str) -> MusicResult<()> {
        let body = json!({ "Name": name });
        self.data(&["Playlists", id], Query::new(), Method::POST, Some(body), true).await.map(|_| ())
    }

    pub async fn delete_playlist(&self, id: &str) -> MusicResult<()> {
        self.data(&["Items", id], Query::new(), Method::DELETE, None, true).await.map(|_| ())
    }

    pub async fn add_to_playlist(&self, id: &str, songs: &[Song]) -> MusicResult<PlaylistAdditionResult> {
        let existing = self.playlist(id).await?.entry.unwrap_or_default();
        let mut seen: HashSet<&str> = existing.iter().map(Song::stream_identifier).collect();
        let additions: Vec<&str> = songs.iter().map(Song::stream_identifier).filter(|id| seen.insert(*id)).collect();
        if !additions.is_empty() {
            let parameters = query([("ids", additions.join(",")), ("userId", self.user_id())]);
            self.data(&["Playlists", id, "Items"], parameters, Method::POST, None, true).await?;
        }
        Ok(PlaylistAdditionResult { added_count: additions.len(), skipped_count: songs.len() - additions.len() })
    }

    /// Replaces the contents only if the server still holds `expected`, in order.
    pub async fn replace_playlist_songs(&self, id: &str, expected: &[Song], songs: &[Song]) -> MusicResult<()> {
        let current = self.playlist(id).await?.entry.unwrap_or_default();
        if !current.iter().map(Song::stream_identifier).eq(expected.iter().map(Song::stream_identifier)) {
            return Err(MusicError::PlaylistChanged);
        }
        let ids: Vec<&str> = songs.iter().map(Song::stream_identifier).collect();
        self.data(&["Playlists", id], Query::new(), Method::POST, Some(json!({ "Ids": ids })), true).await.map(|_| ())
    }

    pub async fn remove_from_playlist(
        &self,
        id: &str,
        song: &Song,
        occurrence: usize,
        expected: &[Song],
    ) -> MusicResult<Vec<Song>> {
        let index = occurrence_index(expected, song, occurrence)?;
        let mut remaining = expected.to_vec();
        remaining.remove(index);
        self.replace_playlist_songs(id, expected, &remaining).await?;
        Ok(remaining)
    }

    /// Applies a reviewed edit only if the playlist's name and contents still match the reviewed snapshot.
    pub async fn apply_playlist_edit(&self, edit: &AssistantEditProposal) -> MusicResult<()> {
        let id = edit.target.playlist_id.as_deref().ok_or(MusicError::InvalidResponse)?;
        let current = self.playlist(id).await?;
        if !edit.matches(&current) {
            return Err(MusicError::PlaylistChanged);
        }
        match edit.operation {
            AssistantEditOperation::Add | AssistantEditOperation::Remove | AssistantEditOperation::Replace => {
                self.replace_playlist_songs(id, &edit.before, &edit.after).await
            }
            AssistantEditOperation::Rename => self.rename_playlist(id, &edit.title).await,
            AssistantEditOperation::Delete => self.delete_playlist(id).await,
            AssistantEditOperation::Favorite | AssistantEditOperation::Unfavorite => Err(MusicError::InvalidResponse),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::test_server::TestServer;

    fn client(server: &TestServer) -> JellyfinClient {
        let credentials = Credentials::new(
            &server.server(""),
            "fixture",
            "token-fixture",
            true,
            Authentication::Token,
            MusicProvider::Jellyfin,
            Some("user-fixture".into()),
            Some("device-fixture".into()),
        )
        .unwrap();
        JellyfinClient::new(credentials)
    }

    #[tokio::test]
    async fn resume_positions_read_resumable_audio_most_recent_first() {
        let server = TestServer::start(|_| {
            (
                200,
                r#"{"Items":[
                    {"Id":"mix","Name":"Mix","RunTimeTicks":36000000000,"UserData":{"PlaybackPositionTicks":12345000000,"LastPlayedDate":"2024-05-01T10:00:00.0000000Z"}},
                    {"Id":"untimed","Name":"Untimed","UserData":{"PlaybackPositionTicks":10000000}},
                    {"Id":"start","Name":"Start","UserData":{"PlaybackPositionTicks":0}},
                    {"Id":"plain","Name":"Plain"}
                ]}"#
                .into(),
            )
        })
        .await;
        let positions = client(&server).resume_positions().await.unwrap();
        assert_eq!(
            positions,
            vec![
                RemoteProgress {
                    id: "mix".into(),
                    position: 1234.5,
                    duration: Some(3600.0),
                    updated: Some(1_714_557_600_000)
                },
                RemoteProgress { id: "untimed".into(), position: 1.0, duration: None, updated: None },
            ]
        );
        let request = &server.requests()[0];
        assert_eq!(request.path, "/Items");
        assert_eq!(request.query("filters"), Some("IsResumable"));
        assert_eq!(request.query("includeItemTypes"), Some("Audio"));
        assert_eq!(request.query("sortBy"), Some("DatePlayed"));
        assert_eq!(request.query("sortOrder"), Some("Descending"));
        assert_eq!(request.query("limit"), Some("500"));
        assert_eq!(request.query("userId"), Some("user-fixture"));
    }

    #[tokio::test]
    async fn saving_progress_updates_the_user_item_data() {
        let server = TestServer::start(|_| (200, "{}".into())).await;
        let jellyfin = client(&server);
        jellyfin.save_progress("mix", ProgressUpload::Save(1234.5)).await.unwrap();
        jellyfin.save_progress("mix", ProgressUpload::Clear).await.unwrap();
        let requests = server.requests();
        assert_eq!((requests[0].method.as_str(), requests[0].path.as_str()), ("POST", "/UserItems/mix/UserData"));
        assert_eq!(requests[0].query("userId"), Some("user-fixture"));
        assert!(requests[0].header("authorization").unwrap().contains("Token=\"token-fixture\""));
        let saved = requests[0].json();
        assert_eq!(saved["PlaybackPositionTicks"], 12_345_000_000_i64);
        assert!(saved["LastPlayedDate"].as_str().and_then(parse_timestamp).is_some());
        assert_eq!(requests[1].json(), json!({ "PlaybackPositionTicks": 0 }));

        let failing = TestServer::start(|_| (403, String::new())).await;
        assert_eq!(client(&failing).save_progress("mix", ProgressUpload::Clear).await, Err(MusicError::Forbidden));
    }
}
