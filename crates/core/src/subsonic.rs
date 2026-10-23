//! Subsonic / OpenSubsonic REST client (API 1.16.1, JSON).
//!
//! Requests use salted token authentication. When a server rejects tokens (error 41), `connect` retries once with the
//! hex-encoded password and returns credentials that remember that method.

use serde::Deserialize;
use url::Url;

use crate::assistant::edits::{AssistantEditOperation, AssistantEditProposal};
use crate::error::{MusicError, MusicResult};
use crate::lyrics::{SongLyrics, StructuredLyrics};
use crate::md5_hex;
use crate::models::{
    Album, Artist, Authentication, Credentials, MusicBrainzId, Playlist, PlaylistAdditionResult, PodcastChannel,
    SearchResults, ServerInfo, Song, StreamingQuality,
};

pub const API_VERSION: &str = "1.16.1";
pub const CLIENT_NAME: &str = "Discophile";

/// Subsonic's token: `md5(password + salt)` in lowercase hex.
pub fn token(password: &str, salt: &str) -> String {
    md5_hex(format!("{password}{salt}").as_bytes())
}

/// The `subsonic-response` wrapper every endpoint returns.
#[derive(Debug, Clone, Deserialize)]
pub struct ApiEnvelope {
    #[serde(rename = "subsonic-response")]
    pub response: ApiResponse,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiResponse {
    pub status: String,
    #[serde(default)]
    pub error: Option<ApiFailure>,
    #[serde(default)]
    pub song: Option<Song>,
    #[serde(default, rename = "albumList2")]
    pub album_list2: Option<AlbumList>,
    #[serde(default)]
    pub album: Option<Album>,
    #[serde(default)]
    pub artist: Option<Artist>,
    #[serde(default)]
    pub artists: Option<ArtistIndexes>,
    #[serde(default)]
    pub playlists: Option<Playlists>,
    #[serde(default)]
    pub playlist: Option<Playlist>,
    #[serde(default, rename = "searchResult3")]
    pub search_result3: Option<SearchResults>,
    #[serde(default)]
    pub random_songs: Option<Songs>,
    #[serde(default, rename = "similarSongs2")]
    pub similar_songs2: Option<Songs>,
    #[serde(default)]
    pub starred2: Option<SearchResults>,
    #[serde(default)]
    pub podcasts: Option<Podcasts>,
    #[serde(default)]
    pub lyrics_list: Option<LyricsList>,
    #[serde(default)]
    pub lyrics: Option<LegacyLyrics>,
    #[serde(default, rename = "artistInfo2")]
    pub artist_info2: Option<InfoPayload>,
    #[serde(default, rename = "albumInfo")]
    pub album_info: Option<InfoPayload>,
}

/// `artistInfo2` and `albumInfo` share their descriptive fields.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InfoPayload {
    #[serde(default)]
    pub biography: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub music_brainz_id: Option<String>,
    #[serde(default)]
    pub last_fm_url: Option<String>,
}

impl InfoPayload {
    fn server_info(self, musicbrainz: fn(String) -> MusicBrainzId) -> ServerInfo {
        let present = |v: Option<String>| v.map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
        ServerInfo {
            text: present(self.biography).or(present(self.notes)),
            musicbrainz: present(self.music_brainz_id).map(musicbrainz),
            link: present(self.last_fm_url),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApiFailure {
    pub code: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AlbumList {
    #[serde(default)]
    pub album: Option<Vec<Album>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Playlists {
    #[serde(default)]
    pub playlist: Option<Vec<Playlist>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Songs {
    #[serde(default)]
    pub song: Option<Vec<Song>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ArtistIndexes {
    #[serde(default)]
    pub index: Option<Vec<ArtistIndex>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ArtistIndex {
    #[serde(default)]
    pub artist: Option<Vec<Artist>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Podcasts {
    #[serde(default)]
    pub channel: Option<Vec<PodcastChannel>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsList {
    #[serde(default)]
    pub structured_lyrics: Option<Vec<StructuredLyrics>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct LegacyLyrics {
    #[serde(default)]
    pub value: Option<String>,
}

/// Maps an HTTP status to an error before the body is read.
pub fn check_status(status: u16) -> MusicResult<()> {
    match status {
        200..=299 => Ok(()),
        401 => Err(MusicError::Authentication),
        403 => Err(MusicError::Forbidden),
        404 => Err(MusicError::NotFound),
        405 | 501 => Err(MusicError::Unsupported),
        // Redirects are never followed: authenticated URLs must not reach another host.
        _ => Err(MusicError::ServerUnavailable),
    }
}

/// Decodes a response body and maps Subsonic error codes. Server error messages are never surfaced.
pub fn decode_response(body: &[u8]) -> MusicResult<ApiResponse> {
    let envelope: ApiEnvelope = serde_json::from_slice(body).map_err(|_| MusicError::InvalidResponse)?;
    let response = envelope.response;
    if response.status == "ok" {
        return Ok(response);
    }
    Err(match response.error.map(|e| e.code) {
        Some(41) => MusicError::TokenAuthenticationUnsupported,
        Some(40 | 42 | 43 | 44) => MusicError::Authentication,
        Some(50) => MusicError::Forbidden,
        Some(20 | 30) => MusicError::Unsupported,
        Some(70) => MusicError::NotFound,
        _ => MusicError::InvalidResponse,
    })
}

/// `application/x-www-form-urlencoded` body. A literal `+` is encoded as `%2B`, never read back as a space.
pub fn form_body(parameters: &[(&str, String)]) -> String {
    let mut form = url::form_urlencoded::Serializer::new(String::new());
    for (key, value) in parameters {
        form.append_pair(key, value);
    }
    form.finish()
}

/// Synced structured lyrics win; empty entries are ignored.
fn structured_lyrics(response: &ApiResponse) -> Option<SongLyrics> {
    let candidates: Vec<SongLyrics> = response
        .lyrics_list
        .as_ref()
        .and_then(|l| l.structured_lyrics.as_ref())
        .map(|list| list.iter().map(StructuredLyrics::lyrics).filter(|l| !l.is_empty()).collect())
        .unwrap_or_default();
    let synced = candidates.iter().position(SongLyrics::is_synced);
    candidates.into_iter().nth(synced.unwrap_or(0))
}

fn legacy_lyrics(response: &ApiResponse) -> Option<SongLyrics> {
    let text = response.lyrics.as_ref()?.value.as_deref()?;
    Some(SongLyrics::from_text(text)).filter(|l| !l.is_empty())
}

/// Keeps the first occurrence of each stream identifier, skipping identifiers already in `existing`.
fn unique_songs<'a>(songs: &'a [Song], existing: &[Song]) -> Vec<&'a Song> {
    let mut seen: std::collections::HashSet<&str> = existing.iter().map(Song::stream_identifier).collect();
    songs.iter().filter(|s| seen.insert(s.stream_identifier())).collect()
}

fn same_order(current: &[Song], expected: &[Song]) -> bool {
    current.iter().map(Song::stream_identifier).eq(expected.iter().map(Song::stream_identifier))
}

/// Index in `expected` of the `occurrence`-th entry with the song's stream identifier.
pub(crate) fn occurrence_index(expected: &[Song], song: &Song, occurrence: usize) -> MusicResult<usize> {
    expected
        .iter()
        .enumerate()
        .filter(|(_, s)| s.stream_identifier() == song.stream_identifier())
        .nth(occurrence)
        .map(|(i, _)| i)
        .ok_or(MusicError::PlaylistChanged)
}

fn salt() -> String {
    uuid::Uuid::new_v4().to_string().to_uppercase()
}

#[derive(Clone)]
pub struct SubsonicClient {
    credentials: Credentials,
    http: reqwest::Client,
}

impl std::fmt::Debug for SubsonicClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SubsonicClient").field("server", &self.credentials.server_string()).finish_non_exhaustive()
    }
}

impl SubsonicClient {
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

    /// Negotiates password authentication only when the server explicitly rejects tokens.
    pub async fn connect(credentials: Credentials) -> MusicResult<Self> {
        Self::connect_with(credentials, crate::http::server_client()).await
    }

    pub async fn connect_with(credentials: Credentials, http: reqwest::Client) -> MusicResult<Self> {
        let client = Self::with_http_client(credentials, http);
        match client.ping().await {
            Ok(()) => Ok(client),
            Err(MusicError::TokenAuthenticationUnsupported)
                if client.credentials.authentication == Authentication::Token =>
            {
                let mut fallback = client.credentials.clone();
                fallback.authentication = Authentication::Password;
                let password_client = Self::with_http_client(fallback, client.http);
                password_client.ping().await?;
                Ok(password_client)
            }
            Err(error) => Err(error),
        }
    }

    /// An authenticated endpoint URL. Callers cannot override or inject authentication parameters.
    pub fn url(&self, endpoint: &str, parameters: &[(&str, String)], salt: &str) -> Url {
        let c = &self.credentials;
        let mut auth: Vec<(&str, String)> =
            vec![("u", c.username.clone()), ("v", API_VERSION.into()), ("c", CLIENT_NAME.into()), ("f", "json".into())];
        match c.authentication {
            Authentication::Token => {
                auth.push(("t", token(&c.password, salt)));
                auth.push(("s", salt.to_string()));
            }
            Authentication::Password => auth.push(("p", format!("enc:{}", hex::encode(c.password.as_bytes())))),
        }
        let mut items: Vec<(&str, String)> = parameters
            .iter()
            .filter(|(k, _)| !["u", "p", "t", "s"].contains(k) && !auth.iter().any(|(a, _)| a == k))
            .cloned()
            .collect();
        items.extend(auth);
        items.sort_by(|a, b| a.0.cmp(b.0));

        let mut url = c.server.clone();
        if let Ok(mut segments) = url.path_segments_mut() {
            segments.pop_if_empty().push("rest").push(&format!("{endpoint}.view"));
        }
        url.query_pairs_mut().extend_pairs(items.iter().map(|(k, v)| (*k, v.as_str())));
        url
    }

    fn fresh_url(&self, endpoint: &str, parameters: &[(&str, String)]) -> Url {
        self.url(endpoint, parameters, &salt())
    }

    pub async fn request(&self, endpoint: &str, parameters: &[(&str, String)]) -> MusicResult<ApiResponse> {
        self.perform(self.http.get(self.fresh_url(endpoint, parameters))).await
    }

    /// Mutations send their parameters in a POST form body, keeping song lists out of URLs and logs.
    async fn mutate(&self, endpoint: &str, parameters: &[(&str, String)]) -> MusicResult<ApiResponse> {
        let request = self
            .http
            .post(self.fresh_url(endpoint, &[]))
            .header(reqwest::header::CONTENT_TYPE, "application/x-www-form-urlencoded; charset=utf-8")
            .body(form_body(parameters));
        self.perform(request).await
    }

    async fn perform(&self, request: reqwest::RequestBuilder) -> MusicResult<ApiResponse> {
        let response = request.send().await.map_err(|_| MusicError::ServerUnavailable)?;
        check_status(response.status().as_u16())?;
        let body = response.bytes().await.map_err(|_| MusicError::ServerUnavailable)?;
        decode_response(&body)
    }

    pub async fn ping(&self) -> MusicResult<()> {
        self.request("ping", &[]).await.map(|_| ())
    }

    pub async fn song(&self, id: &str) -> MusicResult<Song> {
        self.request("getSong", &[("id", id.into())]).await?.song.ok_or(MusicError::NotFound)
    }

    pub async fn albums(&self, list_type: &str, offset: usize, size: usize) -> MusicResult<Vec<Album>> {
        let parameters = [("type", list_type.to_string()), ("offset", offset.to_string()), ("size", size.to_string())];
        Ok(self.request("getAlbumList2", &parameters).await?.album_list2.and_then(|l| l.album).unwrap_or_default())
    }

    pub async fn album(&self, id: &str) -> MusicResult<Album> {
        self.request("getAlbum", &[("id", id.into())]).await?.album.ok_or(MusicError::InvalidResponse)
    }

    pub async fn artists(&self) -> MusicResult<Vec<Artist>> {
        let indexes = self.request("getArtists", &[]).await?.artists.and_then(|a| a.index).unwrap_or_default();
        Ok(indexes.into_iter().flat_map(|i| i.artist.unwrap_or_default()).collect())
    }

    pub async fn artist(&self, id: &str) -> MusicResult<Artist> {
        self.request("getArtist", &[("id", id.into())]).await?.artist.ok_or(MusicError::InvalidResponse)
    }

    pub async fn playlists(&self) -> MusicResult<Vec<Playlist>> {
        Ok(self.request("getPlaylists", &[]).await?.playlists.and_then(|p| p.playlist).unwrap_or_default())
    }

    pub async fn playlist(&self, id: &str) -> MusicResult<Playlist> {
        self.request("getPlaylist", &[("id", id.into())]).await?.playlist.ok_or(MusicError::InvalidResponse)
    }

    /// Legacy servers may answer without the created playlist.
    pub async fn create_playlist(&self, name: &str, songs: &[Song]) -> MusicResult<Option<Playlist>> {
        let mut form = vec![("name", name.to_string())];
        form.extend(unique_songs(songs, &[]).into_iter().map(|s| ("songId", s.stream_identifier().to_string())));
        Ok(self.mutate("createPlaylist", &form).await?.playlist)
    }

    pub async fn rename_playlist(&self, id: &str, name: &str) -> MusicResult<()> {
        self.mutate("updatePlaylist", &[("playlistId", id.into()), ("name", name.into())]).await.map(|_| ())
    }

    pub async fn add_to_playlist(&self, id: &str, songs: &[Song]) -> MusicResult<PlaylistAdditionResult> {
        if songs.is_empty() {
            return Ok(PlaylistAdditionResult::default());
        }
        // Read the complete current playlist, including tracks hidden by offline filters.
        let existing = self.playlist(id).await?.entry.unwrap_or_default();
        let additions = unique_songs(songs, &existing);
        if !additions.is_empty() {
            let mut form = vec![("playlistId", id.to_string())];
            form.extend(additions.iter().map(|s| ("songIdToAdd", s.stream_identifier().to_string())));
            self.mutate("updatePlaylist", &form).await?;
        }
        Ok(PlaylistAdditionResult { added_count: additions.len(), skipped_count: songs.len() - additions.len() })
    }

    /// Replaces the contents only if the server still holds `expected`, in order.
    pub async fn replace_playlist_songs(&self, id: &str, expected: &[Song], songs: &[Song]) -> MusicResult<()> {
        let current = self.playlist(id).await?.entry.unwrap_or_default();
        if !same_order(&current, expected) {
            return Err(MusicError::PlaylistChanged);
        }
        self.overwrite_playlist(id, songs).await
    }

    /// createPlaylist with an ID replaces the contents, preserving duplicates and their order.
    async fn overwrite_playlist(&self, id: &str, songs: &[Song]) -> MusicResult<()> {
        let mut form = vec![("playlistId", id.to_string())];
        form.extend(songs.iter().map(|s| ("songId", s.stream_identifier().to_string())));
        self.mutate("createPlaylist", &form).await.map(|_| ())
    }

    /// Removes one occurrence of `song`, by its full-playlist index, and returns the remaining entries.
    pub async fn remove_from_playlist(
        &self,
        id: &str,
        song: &Song,
        occurrence: usize,
        expected: &[Song],
    ) -> MusicResult<Vec<Song>> {
        let index = occurrence_index(expected, song, occurrence)?;
        let current = self.playlist(id).await?.entry.unwrap_or_default();
        if !same_order(&current, expected) {
            return Err(MusicError::PlaylistChanged);
        }
        self.mutate("updatePlaylist", &[("playlistId", id.into()), ("songIndexToRemove", index.to_string())]).await?;
        let mut remaining = expected.to_vec();
        remaining.remove(index);
        Ok(remaining)
    }

    pub async fn delete_playlist(&self, id: &str) -> MusicResult<()> {
        self.mutate("deletePlaylist", &[("id", id.into())]).await.map(|_| ())
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
                self.overwrite_playlist(id, &edit.after).await
            }
            AssistantEditOperation::Rename => self.rename_playlist(id, &edit.title).await,
            AssistantEditOperation::Delete => self.delete_playlist(id).await,
            AssistantEditOperation::Favorite | AssistantEditOperation::Unfavorite => Err(MusicError::InvalidResponse),
        }
    }

    pub async fn search(&self, query: &str, limits: SearchLimits) -> MusicResult<SearchResults> {
        let count = |n: usize| n.min(500).to_string();
        let parameters = [
            ("query", query.to_string()),
            ("songCount", count(limits.song_count)),
            ("songOffset", limits.song_offset.to_string()),
            ("albumCount", count(limits.album_count)),
            ("albumOffset", limits.album_offset.to_string()),
            ("artistCount", count(limits.artist_count)),
            ("artistOffset", limits.artist_offset.to_string()),
        ];
        Ok(self.request("search3", &parameters).await?.search_result3.unwrap_or_default())
    }

    pub async fn random_songs(&self, size: usize) -> MusicResult<Vec<Song>> {
        let size = size.clamp(1, 500).to_string();
        Ok(self
            .request("getRandomSongs", &[("size", size)])
            .await?
            .random_songs
            .and_then(|s| s.song)
            .unwrap_or_default())
    }

    pub async fn favorites(&self) -> MusicResult<SearchResults> {
        Ok(self.request("getStarred2", &[]).await?.starred2.unwrap_or_default())
    }

    pub async fn artist_info(&self, id: &str) -> MusicResult<ServerInfo> {
        let response = self.request("getArtistInfo2", &[("id", id.into()), ("count", "0".into())]).await?;
        Ok(response.artist_info2.unwrap_or_default().server_info(MusicBrainzId::Artist))
    }

    /// `musicBrainzId` names a release, not a release group.
    pub async fn album_info(&self, id: &str) -> MusicResult<ServerInfo> {
        let response = self.request("getAlbumInfo2", &[("id", id.into())]).await?;
        Ok(response.album_info.unwrap_or_default().server_info(MusicBrainzId::Release))
    }

    pub async fn similar_songs(&self, artist_id: &str, count: usize) -> MusicResult<Vec<Song>> {
        let parameters = [("id", artist_id.to_string()), ("count", count.clamp(1, 100).to_string())];
        Ok(self.request("getSimilarSongs2", &parameters).await?.similar_songs2.and_then(|s| s.song).unwrap_or_default())
    }

    pub async fn favorite(&self, id: &str, starred: bool) -> MusicResult<()> {
        self.request(if starred { "star" } else { "unstar" }, &[("id", id.into())]).await.map(|_| ())
    }

    pub async fn scrobble(&self, id: &str, submission: bool) -> MusicResult<()> {
        self.request("scrobble", &[("id", id.into()), ("submission", submission.to_string())]).await.map(|_| ())
    }

    /// Episodes are included by default only when a single channel is requested.
    pub async fn podcasts(&self, id: Option<&str>, include_episodes: Option<bool>) -> MusicResult<Vec<PodcastChannel>> {
        let include = include_episodes.unwrap_or(id.is_some());
        let mut parameters = vec![("includeEpisodes", include.to_string())];
        if let Some(id) = id {
            parameters.push(("id", id.to_string()));
        }
        Ok(self.request("getPodcasts", &parameters).await?.podcasts.and_then(|p| p.channel).unwrap_or_default())
    }

    /// The server fetches the feed and its episodes on its own schedule.
    pub async fn create_podcast_channel(&self, feed: &Url) -> MusicResult<()> {
        if !matches!(feed.scheme(), "https" | "http") {
            return Err(MusicError::InvalidServer);
        }
        self.mutate("createPodcastChannel", &[("url", feed.to_string())]).await.map(|_| ())
    }

    pub async fn delete_podcast_channel(&self, id: &str) -> MusicResult<()> {
        self.mutate("deletePodcastChannel", &[("id", id.into())]).await.map(|_| ())
    }

    /// OpenSubsonic structured lyrics first, then the standard `getLyrics` endpoint.
    /// Authentication, network and malformed-response failures are not hidden as "no lyrics".
    pub async fn lyrics(&self, song: &Song) -> MusicResult<Option<SongLyrics>> {
        if song.is_podcast() {
            return Ok(None);
        }
        match self.request("getLyricsBySongId", &[("id", song.stream_identifier().to_string())]).await {
            Ok(response) => {
                if let Some(lyrics) = structured_lyrics(&response) {
                    return Ok(Some(lyrics));
                }
            }
            // Older servers may not expose the extension route.
            Err(MusicError::Unsupported | MusicError::NotFound) => {}
            Err(error) => return Err(error),
        }
        let mut parameters = vec![("title", song.title.clone())];
        if let Some(artist) = song.artist.as_ref().filter(|a| !a.is_empty()) {
            parameters.push(("artist", artist.clone()));
        }
        match self.request("getLyrics", &parameters).await {
            Ok(response) => Ok(legacy_lyrics(&response)),
            Err(MusicError::Unsupported | MusicError::NotFound) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// A self-authenticating stream URL for an audio player.
    pub fn stream_url(&self, id: &str, quality: StreamingQuality) -> Url {
        let mut parameters: Vec<(&str, String)> =
            quality.parameters().into_iter().map(|(k, v)| (k, v.to_string())).collect();
        parameters.push(("id", id.to_string()));
        self.fresh_url("stream", &parameters)
    }

    pub fn artwork_url(&self, id: &str, size: u32) -> Url {
        self.fresh_url("getCoverArt", &[("id", id.to_string()), ("size", size.to_string())])
    }
}

/// search3 page sizes and offsets. Counts are capped at 500 per section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchLimits {
    pub song_count: usize,
    pub song_offset: usize,
    pub album_count: usize,
    pub album_offset: usize,
    pub artist_count: usize,
    pub artist_offset: usize,
}

impl Default for SearchLimits {
    fn default() -> Self {
        Self { song_count: 40, song_offset: 0, album_count: 30, album_offset: 0, artist_count: 20, artist_offset: 0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assistant::edits::{AssistantEditOperation, AssistantEditTarget};
    use crate::client::test_server::{Request, TestServer};
    use crate::models::{MusicProvider, ServerAccount};

    const OK: &str = r#"{"subsonic-response":{"status":"ok"}}"#;

    fn failed(code: i64) -> String {
        format!(r#"{{"subsonic-response":{{"status":"failed","error":{{"code":{code}}}}}}}"#)
    }

    fn reply(status: u16, body: &str) -> (u16, String) {
        (status, body.to_string())
    }

    fn query_value<'a>(url: &'a Url, name: &str) -> Option<std::borrow::Cow<'a, str>> {
        url.query_pairs().find(|(k, _)| k == name).map(|(_, v)| v)
    }

    fn client(server: &TestServer) -> SubsonicClient {
        SubsonicClient::new(
            Credentials::simple(&server.server("/prefix"), "fixture", "fixture", true, MusicProvider::Subsonic)
                .unwrap(),
        )
    }

    async fn stub(status: u16, body: &str) -> (TestServer, SubsonicClient) {
        let body = body.to_string();
        let server = TestServer::start(move |_| (status, body.clone())).await;
        let client = client(&server);
        (server, client)
    }

    async fn serve(
        handler: impl Fn(&Request) -> (u16, String) + Send + Sync + 'static,
    ) -> (TestServer, SubsonicClient) {
        let server = TestServer::start(handler).await;
        let client = client(&server);
        (server, client)
    }

    fn songs(ids: &[&str]) -> Vec<Song> {
        ids.iter().map(|id| Song::new(*id, *id)).collect()
    }

    fn ids(songs: &[Song]) -> Vec<&str> {
        songs.iter().map(|s| s.id.as_str()).collect()
    }

    const LIST_A_B_A: &str = r#"{"subsonic-response":{"status":"ok","playlist":{"id":"list","name":"List","entry":[{"id":"a","title":"A"},{"id":"b","title":"B"},{"id":"a","title":"A"}]}}}"#;

    // Core tests: tokens, URLs and envelopes.

    #[test]
    fn official_token_vector_and_utf8() {
        assert_eq!(token("sesame", "c19b2d"), "26719a1196d2a940705a59634eb18eab");
        assert_eq!(token("pàss🔒", "salt123"), "7065856749c12bcfc0670c5df5a2fb54");
        assert_eq!(md5_hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(md5_hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(md5_hex("a".repeat(1000).as_bytes()), "cabe45dcc9ae5b66ba86600cca6b8ba8");
    }

    #[test]
    fn request_preserves_server_prefix_and_escapes_credentials() {
        let credentials = Credentials::simple(
            "https://example.com:8443/navidrome/",
            "user & name",
            "secret",
            false,
            MusicProvider::Subsonic,
        )
        .unwrap();
        let client = SubsonicClient::new(credentials);
        let url = client.url("search3", &[("query", "Björk & friends".into()), ("u", "override".into())], "abcdef");
        assert_eq!(url.path(), "/navidrome/rest/search3.view");
        assert_eq!(url.port(), Some(8443));
        assert_eq!(query_value(&url, "query").as_deref(), Some("Björk & friends"));
        assert_eq!(query_value(&url, "u").as_deref(), Some("user & name"));
        assert_eq!(query_value(&url, "t").as_deref(), Some(token("secret", "abcdef").as_str()));
        assert_eq!(query_value(&url, "v").as_deref(), Some("1.16.1"));
        assert_eq!(query_value(&url, "f").as_deref(), Some("json"));
        assert!(query_value(&url, "p").is_none());
        assert!(!url.as_str().contains("secret"));
        assert_ne!(client.fresh_url("ping", &[]), client.fresh_url("ping", &[]));
        let keys: Vec<String> = url.query_pairs().map(|(k, _)| k.into_owned()).collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
    }

    #[test]
    fn root_server_url_has_single_slash() {
        let credentials = Credentials::simple("https://example.com", "u", "p", false, MusicProvider::Subsonic).unwrap();
        assert_eq!(SubsonicClient::new(credentials).url("ping", &[], "s").path(), "/rest/ping.view");
    }

    #[test]
    fn password_urls_encode_utf8_and_ignore_overrides() {
        let old: Credentials =
            serde_json::from_str(r#"{"server":"https://example.test/music","username":"user","password":"fixture"}"#)
                .unwrap();
        assert_eq!(old.authentication, Authentication::Token);
        let credentials = Credentials::new(
            "https://example.test/music",
            "user",
            "pàss🔒",
            false,
            Authentication::Password,
            MusicProvider::Subsonic,
            None,
            None,
        )
        .unwrap();
        let overrides: Vec<(&str, String)> =
            ["p", "t", "s", "u"].iter().map(|k| (*k, "override".to_string())).collect();
        let url = SubsonicClient::new(credentials).url("ping", &overrides, "salt");
        assert_eq!(query_value(&url, "p").as_deref(), Some("enc:70c3a07373f09f9492"));
        assert_eq!(query_value(&url, "u").as_deref(), Some("user"));
        assert!(query_value(&url, "t").is_none() && query_value(&url, "s").is_none());
        assert_eq!(url.path(), "/music/rest/ping.view");
        assert_eq!(url.query_pairs().filter(|(k, _)| k == "u").count(), 1);
    }

    #[test]
    fn stream_and_artwork_urls_are_self_authenticating() {
        let credentials =
            Credentials::simple("https://example.test/prefix", "user", "secret", false, MusicProvider::Navidrome)
                .unwrap();
        let client = SubsonicClient::new(credentials);
        let stream = client.stream_url("song id", StreamingQuality::Compact);
        assert_eq!(stream.path(), "/prefix/rest/stream.view");
        assert_eq!(query_value(&stream, "id").as_deref(), Some("song id"));
        assert_eq!(query_value(&stream, "format").as_deref(), Some("mp3"));
        assert_eq!(query_value(&stream, "maxBitRate").as_deref(), Some("128"));
        assert!(query_value(&stream, "t").is_some() && query_value(&stream, "s").is_some());
        let original = client.stream_url("x", StreamingQuality::Original);
        assert_eq!(query_value(&original, "format").as_deref(), Some("raw"));
        assert!(query_value(&original, "maxBitRate").is_none());
        let art = client.artwork_url("cover", 300);
        assert_eq!(art.path(), "/prefix/rest/getCoverArt.view");
        assert_eq!(query_value(&art, "size").as_deref(), Some("300"));
    }

    #[test]
    fn decode_playlist_order_and_empty_response() {
        let response = decode_response(
            br#"{"subsonic-response":{"status":"ok","playlist":{"id":"p1","name":"Mix","entry":[{"id":"2","title":"Two"},{"id":"1","title":"One"}]}}}"#,
        )
        .unwrap();
        assert_eq!(ids(response.playlist.unwrap().songs()), ["2", "1"]);
        let empty = decode_response(br#"{"subsonic-response":{"status":"ok","albumList2":{}}}"#).unwrap();
        assert!(empty.album_list2.unwrap().album.is_none());
    }

    #[test]
    fn error_codes_and_statuses_map_to_music_errors() {
        for (code, expected) in [
            (41, MusicError::TokenAuthenticationUnsupported),
            (40, MusicError::Authentication),
            (44, MusicError::Authentication),
            (50, MusicError::Forbidden),
            (20, MusicError::Unsupported),
            (30, MusicError::Unsupported),
            (70, MusicError::NotFound),
            (0, MusicError::InvalidResponse),
        ] {
            assert_eq!(decode_response(failed(code).as_bytes()).unwrap_err(), expected);
        }
        assert_eq!(
            decode_response(br#"{"subsonic-response":{"status":"failed"}}"#).unwrap_err(),
            MusicError::InvalidResponse
        );
        assert_eq!(decode_response(b"<html>Sign in</html>").unwrap_err(), MusicError::InvalidResponse);
        for (status, expected) in [
            (401, MusicError::Authentication),
            (403, MusicError::Forbidden),
            (404, MusicError::NotFound),
            (405, MusicError::Unsupported),
            (501, MusicError::Unsupported),
            (302, MusicError::ServerUnavailable),
            (503, MusicError::ServerUnavailable),
        ] {
            assert_eq!(check_status(status), Err(expected));
        }
    }

    #[test]
    fn form_body_encodes_plus_and_unicode() {
        let body = form_body(&[("name", "Caffè + rock & roll".into()), ("songId", "b+1".into())]);
        assert_eq!(body, "name=Caff%C3%A8+%2B+rock+%26+roll&songId=b%2B1");
        let parsed: Vec<(String, String)> = url::form_urlencoded::parse(body.as_bytes()).into_owned().collect();
        assert_eq!(parsed, [("name".into(), "Caffè + rock & roll".into()), ("songId".into(), "b+1".into())]);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn futures_are_send() {
        fn assert_send<T: Send>(_: T) {}
        let client = SubsonicClient::new(
            Credentials::simple("https://x.test", "u", "p", false, MusicProvider::Subsonic).unwrap(),
        );
        assert_send(client.albums("newest", 0, 30));
        assert_send(client.lyrics(&Song::new("a", "A")));
        assert_send(SubsonicClient::connect(client.credentials().clone()));
    }

    // Client tests against a local server.

    #[tokio::test]
    async fn reviewed_playlist_edit_rejects_changed_name_or_order_before_any_mutation() {
        for body in [
            r#"{"subsonic-response":{"status":"ok","playlist":{"id":"list","name":"Other","entry":[{"id":"a","title":"A"},{"id":"b","title":"B"}]}}}"#,
            r#"{"subsonic-response":{"status":"ok","playlist":{"id":"list","name":"Mix","entry":[{"id":"b","title":"B"},{"id":"a","title":"A"}]}}}"#,
        ] {
            let (_server, client) = serve(move |r| {
                assert_eq!(r.last_path_component(), "getPlaylist.view");
                reply(200, body)
            })
            .await;
            let edit = AssistantEditProposal::new(
                "fixture",
                AssistantEditOperation::Delete,
                AssistantEditTarget::playlist("Mix", "list"),
                songs(&["a", "b"]),
                vec![],
                "",
                &Default::default(),
            )
            .unwrap();
            assert_eq!(client.apply_playlist_edit(&edit).await, Err(MusicError::PlaylistChanged));
        }
    }

    #[tokio::test]
    async fn reviewed_addition_uses_existing_playlist_and_preserves_occurrences() {
        let (server, client) = serve(|r| {
            if r.last_path_component() == "getPlaylist.view" {
                return reply(
                    200,
                    r#"{"subsonic-response":{"status":"ok","playlist":{"id":"list","name":"Mix","entry":[{"id":"a","title":"A"},{"id":"a","title":"A"}]}}}"#,
                );
            }
            assert_eq!(r.last_path_component(), "createPlaylist.view");
            assert_eq!(r.method, "POST");
            assert_eq!(r.form_values("playlistId"), ["list"]);
            assert!(r.form_values("name").is_empty());
            assert_eq!(r.form_values("songId"), ["a", "a", "b"]);
            reply(200, OK)
        })
        .await;
        let edit = AssistantEditProposal::new(
            "fixture",
            AssistantEditOperation::Add,
            AssistantEditTarget::playlist("Mix", "list"),
            songs(&["a", "a"]),
            songs(&["b"]),
            "",
            &Default::default(),
        )
        .unwrap();
        client.apply_playlist_edit(&edit).await.unwrap();
        assert_eq!(server.requests().len(), 2);
    }

    #[tokio::test]
    async fn reviewed_rename_and_deletion_use_only_the_reviewed_identifier_and_propagate_failure() {
        for operation in [AssistantEditOperation::Rename, AssistantEditOperation::Delete] {
            for denied in [false, true] {
                let (_server, client) = serve(move |r| {
                    if r.last_path_component() == "getPlaylist.view" {
                        return reply(
                            200,
                            r#"{"subsonic-response":{"status":"ok","playlist":{"id":"list","name":"Mix","entry":[]}}}"#,
                        );
                    }
                    if operation == AssistantEditOperation::Rename {
                        assert_eq!(r.last_path_component(), "updatePlaylist.view");
                        assert_eq!(r.form_values("playlistId"), ["list"]);
                        assert_eq!(r.form_values("name"), ["Renamed"]);
                    } else {
                        assert_eq!(r.last_path_component(), "deletePlaylist.view");
                        assert_eq!(r.form_values("id"), ["list"]);
                    }
                    if denied { (200, failed(50)) } else { reply(200, OK) }
                })
                .await;
                let edit = AssistantEditProposal::new(
                    "fixture",
                    operation,
                    AssistantEditTarget::playlist("Mix", "list"),
                    vec![],
                    vec![],
                    "Renamed",
                    &Default::default(),
                )
                .unwrap();
                let result = client.apply_playlist_edit(&edit).await;
                assert_eq!(result, if denied { Err(MusicError::Forbidden) } else { Ok(()) });
            }
        }
    }

    #[tokio::test]
    async fn successful_albums_and_empty_albums() {
        let (_s, first) =
            stub(200, r#"{"subsonic-response":{"status":"ok","albumList2":{"album":[{"id":"a1","name":"First"}]}}}"#)
                .await;
        assert_eq!(first.albums("newest", 0, 30).await.unwrap()[0].name, "First");
        let (_s, empty) = stub(200, r#"{"subsonic-response":{"status":"ok","albumList2":{}}}"#).await;
        assert!(empty.albums("newest", 0, 30).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn album_list_paging_parameters() {
        let (_s, client) = serve(|r| {
            assert_eq!(r.path, "/prefix/rest/getAlbumList2.view");
            assert_eq!(
                (r.query("type"), r.query("offset"), r.query("size")),
                (Some("frequent"), Some("60"), Some("30"))
            );
            reply(200, OK)
        })
        .await;
        assert!(client.albums("frequent", 60, 30).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn similar_songs_uses_artist_identifier_and_bounds_requested_count() {
        let (_s, client) = serve(|r| {
            assert_eq!(r.path, "/prefix/rest/getSimilarSongs2.view");
            assert_eq!(r.query("id"), Some("artist-id"));
            assert_eq!(r.query("count"), Some("100"));
            reply(200, r#"{"subsonic-response":{"status":"ok","similarSongs2":{"song":[{"id":"related","title":"Related"}]}}}"#)
        })
        .await;
        assert_eq!(ids(&client.similar_songs("artist-id", 500).await.unwrap()), ["related"]);
    }

    #[tokio::test]
    async fn search_starred_scrobble_and_artists() {
        let (server, client) = serve(|r| match r.last_path_component() {
            "search3.view" => {
                assert_eq!(r.query("query"), Some("Caffè + rock"));
                assert_eq!(r.query("songCount"), Some("500"));
                assert_eq!(r.query("albumOffset"), Some("30"));
                reply(200, r#"{"subsonic-response":{"status":"ok","searchResult3":{"song":[{"id":"s","title":"S"}]}}}"#)
            }
            "getStarred2.view" => {
                reply(200, r#"{"subsonic-response":{"status":"ok","starred2":{"album":[{"id":"al","name":"Al"}]}}}"#)
            }
            "scrobble.view" => {
                assert_eq!((r.query("id"), r.query("submission")), (Some("s"), Some("true")));
                reply(200, OK)
            }
            "getArtists.view" => reply(
                200,
                r#"{"subsonic-response":{"status":"ok","artists":{"index":[{"artist":[{"id":"1","name":"A"}]},{},{"artist":[{"id":"2","name":"B"}]}]}}}"#,
            ),
            "unstar.view" => reply(200, OK),
            other => panic!("unexpected {other}"),
        })
        .await;
        let limits = SearchLimits { song_count: 9000, album_offset: 30, ..Default::default() };
        assert_eq!(ids(client.search("Caffè + rock", limits).await.unwrap().songs()), ["s"]);
        assert_eq!(client.favorites().await.unwrap().albums()[0].id, "al");
        client.scrobble("s", true).await.unwrap();
        client.favorite("s", false).await.unwrap();
        let artists = client.artists().await.unwrap();
        assert_eq!(artists.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), ["1", "2"]);
        assert_eq!(server.requests().len(), 5);
    }

    #[tokio::test]
    async fn get_song_missing_is_not_found() {
        let (_s, client) = stub(200, OK).await;
        assert_eq!(client.song("x").await, Err(MusicError::NotFound));
        assert_eq!(client.album("x").await, Err(MusicError::InvalidResponse));
    }

    #[tokio::test]
    async fn authentication_failure_is_not_an_empty_library() {
        let (_s, client) = stub(
            200,
            r#"{"subsonic-response":{"status":"failed","error":{"code":40,"message":"Do not expose server detail"}}}"#,
        )
        .await;
        assert_eq!(client.albums("newest", 0, 30).await, Err(MusicError::Authentication));
    }

    #[tokio::test]
    async fn unsupported_podcasts_and_malformed_response() {
        let (_s, unsupported) = stub(501, "").await;
        assert_eq!(unsupported.podcasts(None, None).await, Err(MusicError::Unsupported));
        let (_s, malformed) = stub(200, "<html>Sign in</html>").await;
        assert_eq!(malformed.ping().await, Err(MusicError::InvalidResponse));
    }

    #[tokio::test]
    async fn podcast_channels_include_episodes_only_for_single_channel_by_default() {
        let (server, client) = serve(|r| {
            match r.last_path_component() {
                "getPodcasts.view" => {}
                "createPodcastChannel.view" => {
                    assert_eq!(r.form_values("url"), ["https://feeds.example.test/show.xml"])
                }
                "deletePodcastChannel.view" => assert_eq!(r.form_values("id"), ["c1"]),
                other => panic!("unexpected {other}"),
            }
            reply(200, r#"{"subsonic-response":{"status":"ok","podcasts":{"channel":[{"id":"c1","title":"Show"}]}}}"#)
        })
        .await;
        assert_eq!(client.podcasts(None, None).await.unwrap()[0].id, "c1");
        client.podcasts(Some("c1"), None).await.unwrap();
        client.create_podcast_channel(&Url::parse("https://feeds.example.test/show.xml").unwrap()).await.unwrap();
        assert_eq!(
            client.create_podcast_channel(&Url::parse("ftp://feeds.example.test/show.xml").unwrap()).await,
            Err(MusicError::InvalidServer)
        );
        client.delete_podcast_channel("c1").await.unwrap();
        let requests = server.requests();
        assert_eq!(requests.len(), 4);
        assert_eq!((requests[0].query("includeEpisodes"), requests[0].query("id")), (Some("false"), None));
        assert_eq!((requests[1].query("includeEpisodes"), requests[1].query("id")), (Some("true"), Some("c1")));
    }

    #[tokio::test]
    async fn connection_falls_back_when_tokens_are_unsupported_and_persists_mode() {
        let (_s, stub) = serve(|r| {
            if r.query("t").is_some() {
                assert!(r.query("p").is_none());
                return (200, failed(41));
            }
            assert_eq!(r.query("p"), Some("enc:66697874757265"));
            assert!(r.query("t").is_none() && r.query("s").is_none());
            reply(200, r#"{"subsonic-response":{"status":"ok","albumList2":{"album":[{"id":"1","name":"Library"}]}}}"#)
        })
        .await;
        let connected = SubsonicClient::connect(stub.credentials().clone()).await.unwrap();
        assert_eq!(connected.credentials().authentication, Authentication::Password);
        let account = ServerAccount::new(connected.credentials().clone(), "");
        let restored: ServerAccount = serde_json::from_str(&serde_json::to_string(&account).unwrap()).unwrap();
        assert_eq!(restored.credentials.authentication, Authentication::Password);
        assert_eq!(connected.albums("newest", 0, 30).await.unwrap()[0].id, "1");
    }

    #[tokio::test]
    async fn connection_does_not_retry_wrong_credentials_using_password() {
        let (server, stub) = serve(|r| {
            assert!(r.query("t").is_some());
            assert!(r.query("p").is_none());
            (200, failed(40))
        })
        .await;
        assert_eq!(SubsonicClient::connect(stub.credentials().clone()).await.unwrap_err(), MusicError::Authentication);
        assert_eq!(server.requests().len(), 1);
    }

    #[tokio::test]
    async fn token_capable_connection_retains_token_authentication() {
        let (_s, stub) = stub(200, OK).await;
        let connected = SubsonicClient::connect(stub.credentials().clone()).await.unwrap();
        assert_eq!(connected.credentials().authentication, Authentication::Token);
    }

    #[tokio::test]
    async fn fallback_failure_remains_an_authentication_error() {
        let (server, stub) = serve(|r| (200, failed(if r.query("p").is_some() { 40 } else { 41 }))).await;
        assert_eq!(SubsonicClient::connect(stub.credentials().clone()).await.unwrap_err(), MusicError::Authentication);
        assert_eq!(server.requests().len(), 2);
    }

    #[tokio::test]
    async fn http_failures_do_not_become_empty_results() {
        for (status, expected) in [
            (401, MusicError::Authentication),
            (403, MusicError::Forbidden),
            (404, MusicError::NotFound),
            (405, MusicError::Unsupported),
            (503, MusicError::ServerUnavailable),
        ] {
            let (_s, client) = stub(status, "").await;
            assert_eq!(client.search("fixture", SearchLimits::default()).await, Err(expected));
        }
    }

    #[tokio::test]
    async fn redirects_are_not_followed() {
        let (other, _) = stub(200, OK).await;
        let target = format!("{}rest/ping.view", other.base);
        let (_s, client) = serve(move |_| (302, target.clone())).await;
        assert_eq!(client.ping().await, Err(MusicError::ServerUnavailable));
        assert!(other.requests().is_empty());
    }

    #[tokio::test]
    async fn unreachable_server_is_unavailable() {
        let server = TestServer::start(|_| reply(200, OK)).await;
        let client = client(&server);
        drop(server);
        let unused = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = unused.local_addr().unwrap().port();
        drop(unused);
        let mut credentials = client.credentials().clone();
        credentials.server.set_port(Some(port)).unwrap();
        assert_eq!(SubsonicClient::new(credentials).ping().await, Err(MusicError::ServerUnavailable));
    }

    #[tokio::test]
    async fn lyrics_use_song_identifier_and_prefer_synced_text() {
        let (_s, client) = serve(|r| {
            assert_eq!(r.last_path_component(), "getLyricsBySongId.view");
            assert_eq!(r.query("id"), Some("song-id"));
            reply(
                200,
                r#"{"subsonic-response":{"status":"ok","lyricsList":{"structuredLyrics":[{"synced":false,"line":[{"value":"Plain"}]},{"synced":true,"line":[{"start":2000,"value":"Timed"}]}]}}}"#,
            )
        })
        .await;
        let lyrics = client.lyrics(&Song::new("song-id", "Fixture")).await.unwrap().unwrap();
        assert_eq!(lyrics.lines[0].text, "Timed");
        assert_eq!(lyrics.lines[0].start, Some(2.0));
    }

    #[tokio::test]
    async fn lyrics_fall_back_to_legacy_on_unsupported_missing_and_empty_extension() {
        let failure = failed(30);
        for (status, body) in [
            (200, failure.as_str()),
            (404, ""),
            (405, ""),
            (501, ""),
            (200, r#"{"subsonic-response":{"status":"ok","lyricsList":{}}}"#),
        ] {
            let body = body.to_string();
            let (_s, client) = serve(move |r| {
                if r.last_path_component() == "getLyricsBySongId.view" {
                    return (status, body.clone());
                }
                assert_eq!(r.last_path_component(), "getLyrics.view");
                assert_eq!(r.query("title"), Some("Fixture & title"));
                assert_eq!(r.query("artist"), Some("Test artist"));
                reply(200, r#"{"subsonic-response":{"status":"ok","lyrics":{"value":"[00:02.50]Synthetic text"}}}"#)
            })
            .await;
            let song = Song { artist: Some("Test artist".into()), ..Song::new("fixture", "Fixture & title") };
            let lyrics = client.lyrics(&song).await.unwrap().unwrap();
            assert_eq!(lyrics.lines[0].text, "Synthetic text");
            assert_eq!(lyrics.lines[0].start, Some(2.5));
        }
    }

    #[tokio::test]
    async fn plain_lyrics_and_missing_lyrics_remain_distinct() {
        let (_s, plain) =
            stub(200, r#"{"subsonic-response":{"status":"ok","lyrics":{"value":"Synthetic plain text"}}}"#).await;
        let lyrics = plain.lyrics(&Song::new("fixture", "Fixture")).await.unwrap().unwrap();
        assert_eq!(lyrics.lines[0].text, "Synthetic plain text");
        assert!(!lyrics.is_synced());
        for body in [
            OK.to_string(),
            r#"{"subsonic-response":{"status":"ok","lyrics":{"value":" \n "}}}"#.to_string(),
            failed(20),
        ] {
            let (_s, missing) = stub(200, &body).await;
            assert_eq!(missing.lyrics(&Song::new("fixture", "Fixture")).await, Ok(None));
        }
    }

    #[tokio::test]
    async fn lyrics_do_not_hide_authentication_network_or_malformed_response_failures() {
        for (status, body, expected) in [
            (401, "", MusicError::Authentication),
            (403, "", MusicError::Forbidden),
            (503, "", MusicError::ServerUnavailable),
            (200, "<html>Unavailable</html>", MusicError::InvalidResponse),
        ] {
            let (_s, client) = serve(move |r| {
                assert_eq!(r.last_path_component(), "getLyricsBySongId.view");
                reply(status, body)
            })
            .await;
            assert_eq!(client.lyrics(&Song::new("fixture", "Fixture")).await, Err(expected));
        }
    }

    #[tokio::test]
    async fn podcast_lyrics_do_not_make_network_requests() {
        let (server, client) = serve(|_| panic!("Podcasts should not request song lyrics")).await;
        let episode = Song { is_podcast: Some(true), ..Song::new("podcast:fixture", "Episode") };
        assert_eq!(client.lyrics(&episode).await, Ok(None));
        assert!(server.requests().is_empty());
    }

    #[tokio::test]
    async fn create_playlist_encodes_unicode_and_deduplicates_songs_in_first_occurrence_order() {
        let (_s, client) = serve(|r| {
            assert_eq!(r.method, "POST");
            assert_eq!(r.path, "/prefix/rest/createPlaylist.view");
            assert!(r.header("content-type").unwrap().starts_with("application/x-www-form-urlencoded"));
            assert_eq!(r.form_values("name"), ["Caffè + rock & roll"]);
            assert_eq!(r.form_values("songId"), ["b+1", "a&2"]);
            assert!(r.query("songId").is_none());
            assert!(r.query("t").is_some());
            reply(
                200,
                r#"{"subsonic-response":{"status":"ok","playlist":{"id":"new","name":"Created","songCount":2}}}"#,
            )
        })
        .await;
        let playlist = client.create_playlist("Caffè + rock & roll", &songs(&["b+1", "a&2", "b+1"])).await.unwrap();
        assert_eq!(playlist.unwrap().id, "new");
    }

    #[tokio::test]
    async fn create_empty_playlist_accepts_legacy_response_without_playlist() {
        let (_s, client) = serve(|r| {
            assert_eq!(r.form().into_iter().map(|(k, _)| k).collect::<Vec<_>>(), ["name"]);
            reply(200, OK)
        })
        .await;
        assert_eq!(client.create_playlist("Empty", &[]).await, Ok(None));
    }

    #[tokio::test]
    async fn add_songs_deduplicates_batch_using_stream_identifiers() {
        let (_s, client) = serve(|r| {
            if r.last_path_component() == "getPlaylist.view" {
                return reply(
                    200,
                    r#"{"subsonic-response":{"status":"ok","playlist":{"id":"list","name":"List","entry":[]}}}"#,
                );
            }
            assert_eq!(r.last_path_component(), "updatePlaylist.view");
            assert_eq!(r.form_values("playlistId"), ["list"]);
            assert_eq!(r.form_values("songIdToAdd"), ["stream"]);
            assert!(r.form_values("songIndexToRemove").is_empty() && r.form_values("name").is_empty());
            reply(200, OK)
        })
        .await;
        let local = Song { stream_id: Some("stream".into()), ..Song::new("local", "Song") };
        let alias = Song { stream_id: Some("stream".into()), ..Song::new("alias", "Song") };
        let result = client.add_to_playlist("list", &[local, alias]).await.unwrap();
        assert_eq!(result, PlaylistAdditionResult { added_count: 1, skipped_count: 1 });
    }

    #[tokio::test]
    async fn add_only_existing_songs_does_not_send_mutation() {
        let (server, client) = serve(|r| {
            assert_eq!(r.last_path_component(), "getPlaylist.view");
            reply(
                200,
                r#"{"subsonic-response":{"status":"ok","playlist":{"id":"list","name":"List","entry":[{"id":"stream","title":"Existing"}]}}}"#,
            )
        })
        .await;
        let song = Song { stream_id: Some("stream".into()), ..Song::new("local", "Existing") };
        let result = client.add_to_playlist("list", &[song]).await.unwrap();
        assert_eq!(result, PlaylistAdditionResult { added_count: 0, skipped_count: 1 });
        assert_eq!(server.requests().len(), 1);
        assert_eq!(client.add_to_playlist("list", &[]).await.unwrap(), PlaylistAdditionResult::default());
        assert_eq!(server.requests().len(), 1);
    }

    #[tokio::test]
    async fn add_mixed_songs_skips_server_and_batch_duplicates_preserving_new_order() {
        let (_s, client) = serve(|r| {
            if r.last_path_component() == "getPlaylist.view" {
                return reply(
                    200,
                    r#"{"subsonic-response":{"status":"ok","playlist":{"id":"list","name":"List","entry":[{"id":"b","title":"Existing"}]}}}"#,
                );
            }
            assert_eq!(r.last_path_component(), "updatePlaylist.view");
            assert_eq!(r.form_values("songIdToAdd"), ["c", "a", "d"]);
            reply(200, OK)
        })
        .await;
        let result = client.add_to_playlist("list", &songs(&["b", "c", "a", "c", "b", "d"])).await.unwrap();
        assert_eq!(result, PlaylistAdditionResult { added_count: 3, skipped_count: 3 });
    }

    #[tokio::test]
    async fn failed_playlist_read_does_not_append_unchecked_songs() {
        let (server, client) = serve(|r| {
            assert_eq!(r.last_path_component(), "getPlaylist.view");
            reply(503, "")
        })
        .await;
        assert_eq!(client.add_to_playlist("list", &songs(&["new"])).await, Err(MusicError::ServerUnavailable));
        assert_eq!(server.requests().len(), 1);
    }

    #[tokio::test]
    async fn playlist_replacement_preserves_order_and_can_remove_every_track() {
        for wanted in [vec!["b", "a", "a"], vec![]] {
            let expected_ids: Vec<String> = wanted.iter().map(|s| s.to_string()).collect();
            let (_s, client) = serve(move |r| {
                if r.last_path_component() == "getPlaylist.view" {
                    return reply(200, LIST_A_B_A);
                }
                assert_eq!(r.last_path_component(), "createPlaylist.view");
                assert_eq!(r.form_values("playlistId"), ["list"]);
                assert!(r.form_values("name").is_empty());
                assert_eq!(r.form_values("songId"), expected_ids);
                reply(200, OK)
            })
            .await;
            client.replace_playlist_songs("list", &songs(&["a", "b", "a"]), &songs(&wanted)).await.unwrap();
        }
    }

    #[tokio::test]
    async fn playlist_changed_on_server_does_not_overwrite_new_contents() {
        let (server, client) = serve(|r| {
            assert_eq!(r.last_path_component(), "getPlaylist.view");
            reply(
                200,
                r#"{"subsonic-response":{"status":"ok","playlist":{"id":"list","name":"List","entry":[{"id":"external","title":"New"}]}}}"#,
            )
        })
        .await;
        assert_eq!(
            client.replace_playlist_songs("list", &songs(&["old"]), &[]).await,
            Err(MusicError::PlaylistChanged)
        );
        assert_eq!(server.requests().len(), 1);
    }

    #[tokio::test]
    async fn rename_and_delete_use_distinct_parameters() {
        let (_s, client) = serve(|r| {
            let form = r.form();
            if r.last_path_component() == "updatePlaylist.view" {
                assert_eq!(form, [("playlistId".to_string(), "list".to_string()), ("name".into(), "Renamed".into())]);
            } else {
                assert_eq!(r.last_path_component(), "deletePlaylist.view");
                assert_eq!(form, [("id".to_string(), "list".to_string())]);
            }
            reply(200, OK)
        })
        .await;
        client.rename_playlist("list", "Renamed").await.unwrap();
        client.delete_playlist("list").await.unwrap();
    }

    #[tokio::test]
    async fn playlist_mutation_does_not_hide_permission_or_unsupported_errors() {
        for (code, expected) in [(50, MusicError::Forbidden), (30, MusicError::Unsupported), (70, MusicError::NotFound)]
        {
            let (_s, client) = stub(200, &failed(code)).await;
            assert_eq!(client.delete_playlist("list").await, Err(expected));
        }
    }

    #[tokio::test]
    async fn remove_playlist_track_uses_full_index_and_only_selected_duplicate() {
        let (_s, client) = serve(|r| {
            if r.last_path_component() == "getPlaylist.view" {
                return reply(
                    200,
                    r#"{"subsonic-response":{"status":"ok","playlist":{"id":"list","name":"List","entry":[{"id":"hidden","title":"Hidden"},{"id":"same","title":"Same"},{"id":"middle","title":"Middle"},{"id":"same","title":"Same"},{"id":"last","title":"Last"}]}}}"#,
                );
            }
            assert_eq!(r.last_path_component(), "updatePlaylist.view");
            assert_eq!(r.method, "POST");
            assert_eq!(r.form(), [("playlistId".to_string(), "list".to_string()), ("songIndexToRemove".into(), "3".into())]);
            reply(200, OK)
        })
        .await;
        let expected = songs(&["hidden", "same", "middle", "same", "last"]);
        let alias = Song { stream_id: Some("same".into()), ..Song::new("alias", "Same") };
        let remaining = client.remove_from_playlist("list", &alias, 1, &expected).await.unwrap();
        assert_eq!(ids(&remaining), ["hidden", "same", "middle", "last"]);
    }

    #[tokio::test]
    async fn remove_playlist_track_rejects_external_reorder_without_mutation() {
        let (server, client) = serve(|r| {
            assert_eq!(r.last_path_component(), "getPlaylist.view");
            reply(
                200,
                r#"{"subsonic-response":{"status":"ok","playlist":{"id":"list","name":"List","entry":[{"id":"b","title":"B"},{"id":"a","title":"A"}]}}}"#,
            )
        })
        .await;
        let result = client.remove_from_playlist("list", &Song::new("a", "A"), 0, &songs(&["a", "b"])).await;
        assert_eq!(result, Err(MusicError::PlaylistChanged));
        assert_eq!(server.requests().len(), 1);
    }

    #[tokio::test]
    async fn remove_playlist_track_rejects_invalid_occurrences_without_network_request() {
        let (server, client) = serve(|_| panic!("Invalid selection must not make a request")).await;
        let song = Song::new("a", "A");
        let result = client.remove_from_playlist("list", &song, 1, std::slice::from_ref(&song)).await;
        assert_eq!(result, Err(MusicError::PlaylistChanged));
        assert!(server.requests().is_empty());
    }

    #[tokio::test]
    async fn remove_last_playlist_track_and_permission_failure() {
        for denied in [false, true] {
            let (_s, client) = serve(move |r| {
                if r.last_path_component() == "getPlaylist.view" {
                    return reply(200, r#"{"subsonic-response":{"status":"ok","playlist":{"id":"list","name":"List","entry":[{"id":"a","title":"A"}]}}}"#);
                }
                assert_eq!(r.form_values("songIndexToRemove"), ["0"]);
                if denied { (200, failed(50)) } else { reply(200, OK) }
            })
            .await;
            let result = client.remove_from_playlist("list", &Song::new("a", "A"), 0, &songs(&["a"])).await;
            if denied {
                assert_eq!(result, Err(MusicError::Forbidden));
            } else {
                assert!(result.unwrap().is_empty());
            }
        }
    }
}
