//! Provider-independent music client. Authentication and wire formats stay in the provider clients.

use url::Url;

use crate::assistant::edits::AssistantEditProposal;
use crate::error::MusicResult;
use crate::jellyfin::JellyfinClient;
use crate::lyrics::SongLyrics;
use crate::models::{
    Album, Artist, Credentials, MusicProvider, Playlist, PlaylistAdditionResult, PodcastChannel, SearchResults, Song,
    StreamingQuality,
};
pub use crate::subsonic::SearchLimits;
use crate::subsonic::SubsonicClient;

/// Default page size and type for album lists, as in the iOS app.
pub const DEFAULT_ALBUM_LIST_TYPE: &str = "newest";
pub const DEFAULT_ALBUM_PAGE_SIZE: usize = 30;
pub const DEFAULT_RANDOM_SONGS: usize = 100;
pub const DEFAULT_SIMILAR_SONGS: usize = 50;
pub const DEFAULT_ARTWORK_SIZE: u32 = 600;

#[derive(Debug, Clone)]
pub enum MusicClient {
    Subsonic(SubsonicClient),
    Jellyfin(JellyfinClient),
}

macro_rules! dispatch {
    ($self:ident, $client:ident => $call:expr) => {
        match $self {
            MusicClient::Subsonic($client) => $call,
            MusicClient::Jellyfin($client) => $call,
        }
    };
}

impl From<SubsonicClient> for MusicClient {
    fn from(client: SubsonicClient) -> Self {
        Self::Subsonic(client)
    }
}

impl From<JellyfinClient> for MusicClient {
    fn from(client: JellyfinClient) -> Self {
        Self::Jellyfin(client)
    }
}

impl MusicClient {
    /// A client for saved credentials. Jellyfin credentials must already hold a token and user ID.
    pub fn new(credentials: Credentials) -> Self {
        Self::with_http_client(credentials, crate::http::server_client())
    }

    /// `http` must not follow redirects; see `http::server_client`.
    pub fn with_http_client(credentials: Credentials, http: reqwest::Client) -> Self {
        match credentials.provider {
            MusicProvider::Jellyfin => Self::Jellyfin(JellyfinClient::with_http_client(credentials, http)),
            MusicProvider::Subsonic | MusicProvider::Navidrome => {
                Self::Subsonic(SubsonicClient::with_http_client(credentials, http))
            }
        }
    }

    /// Verifies a new connection. Jellyfin exchanges the password for a token; Subsonic pings and falls back to
    /// password authentication when the server rejects tokens. Persist `credentials()` of the result.
    pub async fn connect(credentials: Credentials) -> MusicResult<Self> {
        Self::connect_with(credentials, crate::http::server_client()).await
    }

    pub async fn connect_with(credentials: Credentials, http: reqwest::Client) -> MusicResult<Self> {
        Ok(match credentials.provider {
            MusicProvider::Jellyfin => Self::Jellyfin(JellyfinClient::connect_with(credentials, http).await?),
            MusicProvider::Subsonic | MusicProvider::Navidrome => {
                Self::Subsonic(SubsonicClient::connect_with(credentials, http).await?)
            }
        })
    }

    pub fn credentials(&self) -> &Credentials {
        dispatch!(self, c => c.credentials())
    }

    pub fn provider(&self) -> MusicProvider {
        self.credentials().provider
    }

    pub async fn ping(&self) -> MusicResult<()> {
        dispatch!(self, c => c.ping().await)
    }

    pub async fn song(&self, id: &str) -> MusicResult<Song> {
        dispatch!(self, c => c.song(id).await)
    }

    /// `list_type` is a Subsonic album list type (`newest`, `alphabeticalByName`, ...).
    pub async fn albums(&self, list_type: &str, offset: usize, size: usize) -> MusicResult<Vec<Album>> {
        dispatch!(self, c => c.albums(list_type, offset, size).await)
    }

    pub async fn album(&self, id: &str) -> MusicResult<Album> {
        dispatch!(self, c => c.album(id).await)
    }

    pub async fn artists(&self) -> MusicResult<Vec<Artist>> {
        dispatch!(self, c => c.artists().await)
    }

    pub async fn artist(&self, id: &str) -> MusicResult<Artist> {
        dispatch!(self, c => c.artist(id).await)
    }

    pub async fn playlists(&self) -> MusicResult<Vec<Playlist>> {
        dispatch!(self, c => c.playlists().await)
    }

    pub async fn playlist(&self, id: &str) -> MusicResult<Playlist> {
        dispatch!(self, c => c.playlist(id).await)
    }

    pub async fn create_playlist(&self, name: &str, songs: &[Song]) -> MusicResult<Option<Playlist>> {
        dispatch!(self, c => c.create_playlist(name, songs).await)
    }

    pub async fn rename_playlist(&self, id: &str, name: &str) -> MusicResult<()> {
        dispatch!(self, c => c.rename_playlist(id, name).await)
    }

    /// Adds songs not already in the playlist (by stream identifier), deduplicating the batch.
    pub async fn add_to_playlist(&self, id: &str, songs: &[Song]) -> MusicResult<PlaylistAdditionResult> {
        dispatch!(self, c => c.add_to_playlist(id, songs).await)
    }

    /// Fails with `PlaylistChanged`, without mutating, if the server contents differ from `expected`.
    pub async fn replace_playlist_songs(&self, id: &str, expected: &[Song], songs: &[Song]) -> MusicResult<()> {
        dispatch!(self, c => c.replace_playlist_songs(id, expected, songs).await)
    }

    /// Removes the `occurrence`-th (0-based) entry of `song` from `expected` and returns the remaining entries.
    pub async fn remove_from_playlist(
        &self,
        id: &str,
        song: &Song,
        occurrence: usize,
        expected: &[Song],
    ) -> MusicResult<Vec<Song>> {
        dispatch!(self, c => c.remove_from_playlist(id, song, occurrence, expected).await)
    }

    pub async fn delete_playlist(&self, id: &str) -> MusicResult<()> {
        dispatch!(self, c => c.delete_playlist(id).await)
    }

    /// Applies a reviewed playlist edit. A missing playlist ID or a favorites edit is `InvalidResponse`;
    /// a playlist changed since review is `PlaylistChanged`.
    pub async fn apply_playlist_edit(&self, edit: &AssistantEditProposal) -> MusicResult<()> {
        dispatch!(self, c => c.apply_playlist_edit(edit).await)
    }

    pub async fn search(&self, query: &str, limits: SearchLimits) -> MusicResult<SearchResults> {
        dispatch!(self, c => c.search(query, limits).await)
    }

    pub async fn random_songs(&self, size: usize) -> MusicResult<Vec<Song>> {
        dispatch!(self, c => c.random_songs(size).await)
    }

    pub async fn favorites(&self) -> MusicResult<SearchResults> {
        dispatch!(self, c => c.favorites().await)
    }

    pub async fn similar_songs(&self, artist_id: &str, count: usize) -> MusicResult<Vec<Song>> {
        dispatch!(self, c => c.similar_songs(artist_id, count).await)
    }

    pub async fn favorite(&self, id: &str, starred: bool) -> MusicResult<()> {
        dispatch!(self, c => c.favorite(id, starred).await)
    }

    pub async fn scrobble(&self, id: &str, submission: bool) -> MusicResult<()> {
        dispatch!(self, c => c.scrobble(id, submission).await)
    }

    /// Jellyfin returns `Unsupported`.
    pub async fn podcasts(&self, id: Option<&str>, include_episodes: Option<bool>) -> MusicResult<Vec<PodcastChannel>> {
        dispatch!(self, c => c.podcasts(id, include_episodes).await)
    }

    /// Non-HTTP(S) feeds are rejected with `InvalidServer`.
    pub async fn create_podcast_channel(&self, feed: &Url) -> MusicResult<()> {
        dispatch!(self, c => c.create_podcast_channel(feed).await)
    }

    pub async fn delete_podcast_channel(&self, id: &str) -> MusicResult<()> {
        dispatch!(self, c => c.delete_podcast_channel(id).await)
    }

    /// `Ok(None)` when the server has no lyrics or no lyrics endpoint; other failures are errors.
    pub async fn lyrics(&self, song: &Song) -> MusicResult<Option<SongLyrics>> {
        dispatch!(self, c => c.lyrics(song).await)
    }

    /// A self-authenticating URL for an audio player. Treat it as a secret: never log or persist it.
    pub fn stream_url(&self, id: &str, quality: StreamingQuality) -> Url {
        dispatch!(self, c => c.stream_url(id, quality))
    }

    /// A self-authenticating image URL. Treat it as a secret: never log or persist it.
    pub fn artwork_url(&self, id: &str, size: u32) -> Url {
        dispatch!(self, c => c.artwork_url(id, size))
    }
}

/// A minimal HTTP/1.1 server for provider tests. Each connection serves one request and closes.
#[cfg(test)]
pub(crate) mod test_server {
    use std::sync::{Arc, Mutex};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use url::Url;

    #[derive(Debug, Clone)]
    pub struct Request {
        pub method: String,
        pub path: String,
        pub query: Vec<(String, String)>,
        pub headers: Vec<(String, String)>,
        pub body: Vec<u8>,
    }

    impl Request {
        pub fn last_path_component(&self) -> &str {
            self.path.rsplit('/').next().unwrap_or_default()
        }
        pub fn query(&self, name: &str) -> Option<&str> {
            self.query.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
        }
        pub fn header(&self, name: &str) -> Option<&str> {
            self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
        }
        pub fn form(&self) -> Vec<(String, String)> {
            url::form_urlencoded::parse(&self.body).into_owned().collect()
        }
        pub fn form_values(&self, name: &str) -> Vec<String> {
            self.form().into_iter().filter(|(k, _)| k == name).map(|(_, v)| v).collect()
        }
        pub fn json(&self) -> serde_json::Value {
            serde_json::from_slice(&self.body).expect("JSON body")
        }
    }

    type Handler = dyn Fn(&Request) -> (u16, String) + Send + Sync;

    /// Handler panics (failed assertions) are recorded and reported when the server is dropped.
    /// For 3xx statuses the body is sent as the `Location` header.
    pub struct TestServer {
        pub base: Url,
        handler: Arc<Mutex<Arc<Handler>>>,
        pub requests: Arc<Mutex<Vec<Request>>>,
        failures: Arc<Mutex<Vec<String>>>,
    }

    impl TestServer {
        pub async fn start(handler: impl Fn(&Request) -> (u16, String) + Send + Sync + 'static) -> Self {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let handler: Arc<Mutex<Arc<Handler>>> = Arc::new(Mutex::new(Arc::new(handler)));
            let requests = Arc::new(Mutex::new(Vec::new()));
            let failures = Arc::new(Mutex::new(Vec::new()));
            let (h, r, f) = (handler.clone(), requests.clone(), failures.clone());
            tokio::spawn(async move {
                while let Ok((mut socket, _)) = listener.accept().await {
                    let (h, r, f) = (h.clone(), r.clone(), f.clone());
                    tokio::spawn(async move {
                        let Some(request) = read_request(&mut socket).await else { return };
                        r.lock().unwrap().push(request.clone());
                        let handler = h.lock().unwrap().clone();
                        let (status, body) =
                            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(&request))) {
                                Ok(result) => result,
                                Err(panic) => {
                                    let message = panic
                                        .downcast_ref::<String>()
                                        .cloned()
                                        .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                                        .unwrap_or_default();
                                    f.lock().unwrap().push(format!("{} {}: {message}", request.method, request.path));
                                    (500, String::new())
                                }
                            };
                        let (location, body) = if (300..400).contains(&status) {
                            (format!("Location: {body}\r\n"), String::new())
                        } else {
                            (String::new(), body)
                        };
                        let response = format!(
                            "HTTP/1.1 {status} Status\r\nContent-Type: application/json\r\n{location}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        let _ = socket.write_all(response.as_bytes()).await;
                        let _ = socket.shutdown().await;
                    });
                }
            });
            let base = Url::parse(&format!("http://{address}")).unwrap();
            Self { base, handler, requests, failures }
        }

        pub fn set_handler(&self, handler: impl Fn(&Request) -> (u16, String) + Send + Sync + 'static) {
            *self.handler.lock().unwrap() = Arc::new(handler);
        }

        /// The server address with a path prefix, e.g. `http://127.0.0.1:1234/prefix`.
        pub fn server(&self, prefix: &str) -> String {
            format!("{}{}", self.base.as_str().trim_end_matches('/'), prefix)
        }

        pub fn requests(&self) -> Vec<Request> {
            self.requests.lock().unwrap().clone()
        }
    }

    impl Drop for TestServer {
        fn drop(&mut self) {
            let failures = self.failures.lock().unwrap().clone();
            if !failures.is_empty() && !std::thread::panicking() {
                panic!("test server handler failures: {failures:#?}");
            }
        }
    }

    async fn read_request(socket: &mut tokio::net::TcpStream) -> Option<Request> {
        let mut buffer = Vec::new();
        let mut chunk = [0u8; 4096];
        let header_end = loop {
            if let Some(i) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
                break i;
            }
            let n = socket.read(&mut chunk).await.ok()?;
            if n == 0 {
                return None;
            }
            buffer.extend_from_slice(&chunk[..n]);
        };
        let head = String::from_utf8_lossy(&buffer[..header_end]).to_string();
        let mut lines = head.split("\r\n");
        let mut first = lines.next()?.split(' ');
        let method = first.next()?.to_string();
        let target = first.next()?.to_string();
        let headers: Vec<(String, String)> = lines
            .filter_map(|l| l.split_once(':').map(|(k, v)| (k.trim().to_string(), v.trim().to_string())))
            .collect();
        let length = headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
            .and_then(|(_, v)| v.parse::<usize>().ok())
            .unwrap_or(0);
        let mut body = buffer[header_end + 4..].to_vec();
        while body.len() < length {
            let n = socket.read(&mut chunk).await.ok()?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..n]);
        }
        let url = Url::parse(&format!("http://localhost{target}")).ok()?;
        Some(Request {
            method,
            path: url.path().to_string(),
            query: url.query_pairs().into_owned().collect(),
            headers,
            body,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::test_server::{Request, TestServer};
    use super::*;
    use crate::error::MusicError;
    use crate::models::{Authentication, ServerAccount};

    fn reply(status: u16, body: &str) -> (u16, String) {
        (status, body.to_string())
    }

    fn credentials(server: &str, provider: MusicProvider, token: &str) -> Credentials {
        let user = (provider == MusicProvider::Jellyfin).then(|| "user".to_string());
        Credentials::new(server, "fixture", token, true, Authentication::Token, provider, user, None).unwrap()
    }

    async fn jellyfin(
        handler: impl Fn(&Request) -> (u16, String) + Send + Sync + 'static,
    ) -> (TestServer, MusicClient) {
        let server = TestServer::start(handler).await;
        let client =
            MusicClient::new(credentials(&server.server("/prefix"), MusicProvider::Jellyfin, "synthetic-token"));
        (server, client)
    }

    #[test]
    fn provider_selects_backend_and_exposes_credentials() {
        for (provider, jellyfin) in
            [(MusicProvider::Subsonic, false), (MusicProvider::Navidrome, false), (MusicProvider::Jellyfin, true)]
        {
            let client = MusicClient::new(credentials("https://music.example.test/prefix", provider, "t"));
            assert_eq!(matches!(client, MusicClient::Jellyfin(_)), jellyfin);
            assert_eq!(client.provider(), provider);
            assert_eq!(client.credentials().username, "fixture");
        }
    }

    #[test]
    fn legacy_accounts_keep_their_identity_and_default_to_subsonic() {
        let account: ServerAccount = serde_json::from_str(
            r#"{"credentials":{"server":"https://music.example.test/prefix","username":"fixture","password":"synthetic-password"}}"#,
        )
        .unwrap();
        assert_eq!(account.credentials.provider, MusicProvider::Subsonic);
        assert_eq!(account.id(), crate::subsonic::token("https://music.example.test/prefix", "\nfixture"));
    }

    #[test]
    fn navidrome_shares_subsonic_identity_but_jellyfin_is_isolated() {
        let server = "https://music.example.test/prefix";
        let subsonic = credentials(server, MusicProvider::Subsonic, "t").account_key();
        let navidrome = credentials(server, MusicProvider::Navidrome, "t").account_key();
        let jellyfin = credentials(server, MusicProvider::Jellyfin, "t").account_key();
        assert_eq!(subsonic, navidrome);
        assert_ne!(subsonic, jellyfin);
        assert_eq!(jellyfin, credentials(server, MusicProvider::Jellyfin, "renewed-token").account_key());
        let changed_user = Credentials::simple(server, "other", "t", false, MusicProvider::Subsonic).unwrap();
        assert_ne!(subsonic, changed_user.account_key());
    }

    #[test]
    fn jellyfin_media_urls_preserve_prefix_and_use_requested_quality() {
        let client = MusicClient::new(credentials(
            "https://music.example.test/prefix",
            MusicProvider::Jellyfin,
            "synthetic-token",
        ));
        let value = |url: &Url, name: &str| url.query_pairs().find(|(k, _)| k == name).map(|(_, v)| v.into_owned());
        let original = client.stream_url("song", StreamingQuality::Original);
        assert_eq!(original.path(), "/prefix/Audio/song/stream");
        assert_eq!(value(&original, "static").as_deref(), Some("true"));
        let compact = client.stream_url("song", StreamingQuality::Compact);
        assert_eq!(compact.path(), "/prefix/Audio/song/stream.mp3");
        assert_eq!(value(&compact, "audioBitRate").as_deref(), Some("128000"));
        assert_eq!(value(&compact, "api_key").as_deref(), Some("synthetic-token"));
        let high = client.stream_url("song", StreamingQuality::High);
        assert_eq!(value(&high, "audioBitRate").as_deref(), Some("320000"));
        let art = client.artwork_url("cover", DEFAULT_ARTWORK_SIZE);
        assert_eq!(art.path(), "/prefix/Items/cover/Images/Primary");
        assert_eq!(value(&art, "maxWidth").as_deref(), Some("600"));
        assert_eq!(value(&art, "api_key").as_deref(), Some("synthetic-token"));
        // Identifiers are path-escaped, never interpreted as extra segments or queries.
        assert_eq!(client.artwork_url("a/b?c", 1).path(), "/prefix/Items/a%2Fb%3Fc/Images/Primary");
    }

    #[tokio::test]
    async fn jellyfin_login_uses_password_only_in_body_and_saves_only_token() {
        let server = TestServer::start(|r| {
            assert_eq!(r.path, "/prefix/Users/AuthenticateByName");
            assert_eq!(r.method, "POST");
            assert!(r.query.is_empty());
            let authorization = r.header("authorization").unwrap();
            assert!(authorization.starts_with("MediaBrowser Client=\"Discophile\""));
            assert!(!authorization.contains("Token="));
            assert_eq!(r.json(), serde_json::json!({"Username": "fixture", "Pw": "synthetic-password"}));
            reply(200, r#"{"AccessToken":"synthetic-token","User":{"Id":"user"}}"#)
        })
        .await;
        let input = Credentials::simple(
            &server.server("/prefix"),
            "fixture",
            "synthetic-password",
            true,
            MusicProvider::Jellyfin,
        )
        .unwrap();
        let client = MusicClient::connect(input.clone()).await.unwrap();
        assert_eq!(client.credentials().password, "synthetic-token");
        assert_eq!(client.credentials().user_id.as_deref(), Some("user"));
        assert_eq!(client.credentials().device_id, input.device_id);
        assert!(input.device_id.is_some());
        let json = serde_json::to_string(&ServerAccount::new(client.credentials().clone(), "")).unwrap();
        assert!(!json.contains("synthetic-password"));
        let restored: ServerAccount = serde_json::from_str(&json).unwrap();
        assert_eq!(&restored.credentials, client.credentials());
    }

    #[tokio::test]
    async fn jellyfin_login_rejects_missing_or_unsafe_token_and_http_failures() {
        let server = TestServer::start(|_| reply(500, "")).await;
        for (status, body, expected) in [
            (200, r#"{"AccessToken":"","User":{"Id":"user"}}"#, MusicError::InvalidResponse),
            (200, r#"{"AccessToken":"bad\nheader","User":{"Id":"user"}}"#, MusicError::InvalidResponse),
            (200, r#"{"AccessToken":"bad\"quote","User":{"Id":"user"}}"#, MusicError::InvalidResponse),
            (200, r#"{"AccessToken":"token","User":{"Id":""}}"#, MusicError::InvalidResponse),
            (401, "", MusicError::Authentication),
            (403, "", MusicError::Forbidden),
            (503, "", MusicError::ServerUnavailable),
        ] {
            server.set_handler(move |_| reply(status, body));
            let input = credentials(&server.server("/prefix"), MusicProvider::Jellyfin, "synthetic-password");
            assert_eq!(MusicClient::connect(input).await.unwrap_err(), expected);
        }
    }

    #[tokio::test]
    async fn jellyfin_maps_album_tracks_audio_metadata_and_pagination() {
        let (_s, client) = jellyfin(|r| {
            assert!(r.header("authorization").unwrap().contains("Token=\"synthetic-token\""));
            assert!(r.query("api_key").is_none());
            if r.path == "/prefix/Items/album" {
                return reply(
                    200,
                    r#"{"Id":"album","Name":"An album","AlbumArtists":[{"Id":"artist","Name":"An artist"}],"ImageTags":{"Primary":"cover"},"ProductionYear":1973}"#,
                );
            }
            assert_eq!(r.query("userId"), Some("user"));
            assert_eq!(r.query("parentId"), Some("album"));
            assert_eq!(r.query("includeItemTypes"), Some("Audio"));
            assert_eq!(r.query("startIndex"), Some("0"));
            assert_eq!(r.query("sortBy"), Some("ParentIndexNumber,IndexNumber,SortName"));
            reply(
                200,
                r#"{"Items":[{"Id":"track","Name":"A song","AlbumId":"album","Artists":["An artist"],"RunTimeTicks":1200000000,"ParentIndexNumber":2,"IndexNumber":3,"Container":"flac","MediaStreams":[{"Type":"Audio","SampleRate":96000,"BitDepth":24,"Channels":2,"BitRate":1000000}]}]}"#,
            )
        })
        .await;
        let album = client.album("album").await.unwrap();
        assert_eq!(album.artist_id.as_deref(), Some("artist"));
        assert_eq!(album.artist.as_deref(), Some("An artist"));
        assert_eq!(album.cover_art.as_deref(), Some("album"));
        assert_eq!(album.year, Some(1973));
        let song = &album.songs()[0];
        assert_eq!(song.duration, Some(120.0));
        assert_eq!((song.disc_number, song.track), (Some(2), Some(3)));
        assert_eq!((song.sampling_rate, song.bit_depth, song.channel_count), (Some(96000), Some(24), Some(2)));
        assert_eq!(song.bit_rate, Some(1000));
        assert_eq!(song.suffix.as_deref(), Some("flac"));
        assert_eq!(song.artist.as_deref(), Some("An artist"));
    }

    #[tokio::test]
    async fn jellyfin_pages_through_complete_lists() {
        let (server, client) = jellyfin(|r| {
            let start: usize = r.query("startIndex").unwrap().parse().unwrap();
            let count = if start == 0 { 500 } else { 3 };
            let items: Vec<String> =
                (start..start + count).map(|i| format!(r#"{{"Id":"{i}","Name":"Artist {i}"}}"#)).collect();
            assert_eq!(r.path, "/prefix/Artists/AlbumArtists");
            (200, format!(r#"{{"Items":[{}]}}"#, items.join(",")))
        })
        .await;
        assert_eq!(client.artists().await.unwrap().len(), 503);
        assert_eq!(server.requests().len(), 2);
    }

    #[tokio::test]
    async fn jellyfin_search_skips_disabled_sections_and_forwards_offsets() {
        let (server, client) = jellyfin(|r| {
            assert_eq!(r.query("includeItemTypes"), Some("Audio"));
            assert_eq!(r.query("searchTerm"), Some("Caffè + rock"));
            assert_eq!(r.query("startIndex"), Some("40"));
            assert_eq!(r.query("limit"), Some("40"));
            reply(200, r#"{"Items":[{"Id":"song","Name":"Result"}]}"#)
        })
        .await;
        let limits = SearchLimits { song_offset: 40, album_count: 0, artist_count: 0, ..Default::default() };
        let result = client.search("Caffè + rock", limits).await.unwrap();
        assert_eq!(result.songs()[0].id, "song");
        assert_eq!(result.album, Some(vec![]));
        assert_eq!(result.artist, Some(vec![]));
        assert_eq!(server.requests().len(), 1);
    }

    #[tokio::test]
    async fn jellyfin_requests_do_not_turn_errors_into_empty_library() {
        for (status, body, expected) in [
            (401, "", MusicError::Authentication),
            (200, "{}", MusicError::InvalidResponse),
            (200, "<html>Sign in</html>", MusicError::InvalidResponse),
        ] {
            let (_s, client) = jellyfin(move |_| reply(status, body)).await;
            assert_eq!(client.albums(DEFAULT_ALBUM_LIST_TYPE, 0, DEFAULT_ALBUM_PAGE_SIZE).await, Err(expected));
        }
    }

    #[tokio::test]
    async fn jellyfin_without_user_is_not_authenticated() {
        let server = TestServer::start(|_| panic!("no request without a user")).await;
        let input = Credentials::simple(&server.server(""), "fixture", "token", true, MusicProvider::Jellyfin).unwrap();
        assert_eq!(MusicClient::new(input).ping().await, Err(MusicError::Authentication));
    }

    #[tokio::test]
    async fn navidrome_uses_subsonic_authentication_and_retains_provider_on_fallback() {
        let server = TestServer::start(|r| {
            assert_eq!(r.path, "/prefix/rest/ping.view");
            if r.query("p").is_none() {
                reply(200, r#"{"subsonic-response":{"status":"failed","error":{"code":41}}}"#)
            } else {
                reply(200, r#"{"subsonic-response":{"status":"ok"}}"#)
            }
        })
        .await;
        let client =
            MusicClient::connect(credentials(&server.server("/prefix"), MusicProvider::Navidrome, "pw")).await.unwrap();
        assert_eq!(client.provider(), MusicProvider::Navidrome);
        assert_eq!(client.credentials().authentication, Authentication::Password);
        // The fallback authentication is used for player URLs too.
        let stream = client.stream_url("s", StreamingQuality::High);
        assert!(stream.query_pairs().any(|(k, v)| k == "p" && v == "enc:7077"));
    }

    #[tokio::test]
    async fn jellyfin_playlist_replacement_checks_order_before_mutation() {
        let (server, client) = jellyfin(|r| {
            assert_eq!(r.method, "GET");
            if r.path == "/prefix/Items/list" {
                return reply(200, r#"{"Id":"list","Name":"Mix"}"#);
            }
            assert_eq!(r.path, "/prefix/Playlists/list/Items");
            reply(200, r#"{"Items":[{"Id":"external","Name":"Changed"}]}"#)
        })
        .await;
        let result = client.replace_playlist_songs("list", &[Song::new("old", "Old")], &[]).await;
        assert_eq!(result, Err(MusicError::PlaylistChanged));
        assert_eq!(server.requests().len(), 2);
    }

    #[tokio::test]
    async fn jellyfin_playlist_replacement_preserves_duplicates_and_clears_contents() {
        for wanted in [vec!["b", "a", "a"], vec![]] {
            let expected_body = serde_json::json!({ "Ids": wanted.clone() });
            let (server, client) = jellyfin(move |r| {
                if r.method == "POST" {
                    assert_eq!(r.path, "/prefix/Playlists/list");
                    assert_eq!(r.json(), expected_body);
                    return reply(204, "");
                }
                if r.path == "/prefix/Items/list" {
                    return reply(200, r#"{"Id":"list","Name":"Mix"}"#);
                }
                reply(200, r#"{"Items":[{"Id":"a","Name":"A"},{"Id":"b","Name":"B"},{"Id":"a","Name":"A"}]}"#)
            })
            .await;
            let expected: Vec<Song> = ["a", "b", "a"].iter().map(|id| Song::new(*id, *id)).collect();
            let songs: Vec<Song> = wanted.iter().map(|id| Song::new(*id, *id)).collect();
            client.replace_playlist_songs("list", &expected, &songs).await.unwrap();
            assert_eq!(server.requests().iter().filter(|r| r.method == "POST").count(), 1);
        }
    }

    #[tokio::test]
    async fn jellyfin_removal_and_reviewed_edits_replace_contents() {
        use crate::assistant::edits::{AssistantEditOperation, AssistantEditTarget};
        let (server, client) = jellyfin(|r| match (r.method.as_str(), r.path.as_str()) {
            ("GET", "/prefix/Items/list") => reply(200, r#"{"Id":"list","Name":"Mix"}"#),
            ("GET", _) => {
                reply(200, r#"{"Items":[{"Id":"a","Name":"A"},{"Id":"b","Name":"B"},{"Id":"a","Name":"A"}]}"#)
            }
            ("POST", "/prefix/Playlists/list") => reply(204, ""),
            other => panic!("unexpected {other:?}"),
        })
        .await;
        let expected: Vec<Song> = ["a", "b", "a"].iter().map(|id| Song::new(*id, *id)).collect();
        let remaining = client.remove_from_playlist("list", &Song::new("a", "a"), 1, &expected).await.unwrap();
        assert_eq!(remaining.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(server.requests().last().unwrap().json(), serde_json::json!({"Ids": ["a", "b"]}));

        let edit = AssistantEditProposal::new(
            "fixture",
            AssistantEditOperation::Remove,
            AssistantEditTarget::playlist("Mix", "list"),
            expected.clone(),
            vec![Song::new("b", "b")],
            "",
            &Default::default(),
        )
        .unwrap();
        client.apply_playlist_edit(&edit).await.unwrap();
        assert_eq!(server.requests().last().unwrap().json(), serde_json::json!({"Ids": ["a", "a"]}));

        let stale = AssistantEditProposal::new(
            "fixture",
            AssistantEditOperation::Delete,
            AssistantEditTarget::playlist("Renamed", "list"),
            expected,
            vec![],
            "",
            &Default::default(),
        )
        .unwrap();
        assert_eq!(client.apply_playlist_edit(&stale).await, Err(MusicError::PlaylistChanged));
    }

    #[tokio::test]
    async fn jellyfin_create_add_rename_and_delete_playlists() {
        let (server, client) = jellyfin(|r| match (r.method.as_str(), r.path.as_str()) {
            ("POST", "/prefix/Playlists") => {
                assert_eq!(
                    r.json(),
                    serde_json::json!({"Name": "New", "Ids": ["a", "b"], "UserId": "user", "MediaType": "Audio", "IsPublic": false})
                );
                reply(200, r#"{"Id":"created"}"#)
            }
            ("GET", "/prefix/Items/list") => reply(200, r#"{"Id":"list","Name":"Mix"}"#),
            ("GET", "/prefix/Playlists/list/Items") => reply(200, r#"{"Items":[{"Id":"a","Name":"A"}]}"#),
            ("POST", "/prefix/Playlists/list/Items") => {
                assert_eq!((r.query("ids"), r.query("userId")), (Some("c,b"), Some("user")));
                reply(204, "")
            }
            ("POST", "/prefix/Playlists/list") => {
                assert_eq!(r.json(), serde_json::json!({"Name": "Renamed"}));
                reply(204, "")
            }
            ("DELETE", "/prefix/Items/list") => reply(204, ""),
            other => panic!("unexpected {other:?}"),
        })
        .await;
        let songs = |ids: &[&str]| ids.iter().map(|id| Song::new(*id, *id)).collect::<Vec<_>>();
        let created = client.create_playlist("New", &songs(&["a", "b", "a"])).await.unwrap().unwrap();
        assert_eq!((created.id.as_str(), created.song_count), ("created", Some(2)));
        let added = client.add_to_playlist("list", &songs(&["a", "c", "b", "c"])).await.unwrap();
        assert_eq!(added, PlaylistAdditionResult { added_count: 2, skipped_count: 2 });
        client.rename_playlist("list", "Renamed").await.unwrap();
        client.delete_playlist("list").await.unwrap();
        assert_eq!(server.requests().len(), 6);
    }

    #[tokio::test]
    async fn jellyfin_lyrics_convert_ticks_and_missing_lyrics_are_optional() {
        let (server, client) = jellyfin(|r| {
            assert_eq!(r.path, "/prefix/Audio/song/Lyrics");
            reply(200, r#"{"Lyrics":[{"Text":"First","Start":10000000},{"Text":"Second","Start":25000000}]}"#)
        })
        .await;
        let lyrics = client.lyrics(&Song::new("song", "Song")).await.unwrap().unwrap();
        assert_eq!(lyrics.lines.iter().map(|l| l.start).collect::<Vec<_>>(), [Some(1.0), Some(2.5)]);
        assert!(lyrics.is_synced());
        server.set_handler(|_| reply(404, ""));
        assert_eq!(client.lyrics(&Song::new("missing", "Missing")).await, Ok(None));
        server.set_handler(|_| reply(401, ""));
        assert_eq!(client.lyrics(&Song::new("song", "Song")).await, Err(MusicError::Authentication));
    }

    #[tokio::test]
    async fn jellyfin_favorite_and_scrobble_use_authenticated_user_and_propagate_denial() {
        let (server, client) = jellyfin(|r| {
            assert_eq!(r.path, "/prefix/UserFavoriteItems/song");
            assert_eq!(r.method, "DELETE");
            assert_eq!(r.query("userId"), Some("user"));
            reply(204, "")
        })
        .await;
        client.favorite("song", false).await.unwrap();
        server.set_handler(|r| {
            assert_eq!(r.path, "/prefix/UserPlayedItems/song");
            assert_eq!(r.method, "POST");
            assert_eq!(r.query("userId"), Some("user"));
            reply(403, "private server details")
        });
        assert_eq!(client.scrobble("song", true).await, Err(MusicError::Forbidden));
        // "Now playing" updates are not sent to Jellyfin.
        let before = server.requests().len();
        client.scrobble("song", false).await.unwrap();
        assert_eq!(server.requests().len(), before);
    }

    #[tokio::test]
    async fn jellyfin_similar_songs_use_related_artists() {
        let (server, client) = jellyfin(|r| {
            if r.path == "/prefix/Items/artist/Similar" {
                assert_eq!(r.query("limit"), Some("10"));
                return reply(200, r#"{"Items":[{"Id":"x","Name":"X","Type":"MusicArtist"},{"Id":"al","Name":"Al","Type":"MusicAlbum"},{"Id":"y","Name":"Y","Type":"MusicArtist"}]}"#);
            }
            assert_eq!((r.query("artistIds"), r.query("sortBy"), r.query("limit")), (Some("x,y"), Some("Random"), Some("50")));
            reply(200, r#"{"Items":[{"Id":"s","Name":"S"}]}"#)
        })
        .await;
        assert_eq!(client.similar_songs("artist", DEFAULT_SIMILAR_SONGS).await.unwrap()[0].id, "s");
        assert_eq!(server.requests().len(), 2);
    }

    #[tokio::test]
    async fn jellyfin_podcasts_are_unsupported() {
        let (server, client) = jellyfin(|_| panic!("no request")).await;
        assert_eq!(client.podcasts(None, None).await, Err(MusicError::Unsupported));
        let feed = Url::parse("https://feeds.example.test/show.xml").unwrap();
        assert_eq!(client.create_podcast_channel(&feed).await, Err(MusicError::Unsupported));
        assert_eq!(client.delete_podcast_channel("c").await, Err(MusicError::Unsupported));
        assert!(server.requests().is_empty());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn futures_are_send() {
        fn assert_send<T: Send>(_: T) {}
        let client = MusicClient::new(credentials("https://x.test", MusicProvider::Jellyfin, "t"));
        assert_send(client.search("q", SearchLimits::default()));
        assert_send(client.favorites());
        assert_send(client.lyrics(&Song::new("a", "A")));
        assert_send(client.add_to_playlist("l", &[]));
        assert_send(MusicClient::connect(client.credentials().clone()));
    }
}
