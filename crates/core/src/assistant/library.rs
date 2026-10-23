//! Resolves model search suggestions against the library before a model can select playable IDs.
//!
//! Discovery is bounded: at most 8 search terms, 5 pages per term, 4 artists from the index and 60 album
//! lookups (4 at a time). The resulting catalog is ranked for requested periods and truncated to `limit`.

use std::collections::{HashMap, HashSet};
use std::future::Future;

use super::edits::normalize;
use super::music_assistant::AssistantPlan;
use crate::error::{MusicError, MusicResult};
use crate::models::{Album, Artist, Playlist, SearchResults, Song};

/// `Send` on native targets, nothing in the browser where futures are single-threaded.
#[cfg(not(target_arch = "wasm32"))]
pub trait MaybeSend: Send {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Send + ?Sized> MaybeSend for T {}
#[cfg(target_arch = "wasm32")]
pub trait MaybeSend {}
#[cfg(target_arch = "wasm32")]
impl<T: ?Sized> MaybeSend for T {}

/// `Sync` on native targets, nothing in the browser.
#[cfg(not(target_arch = "wasm32"))]
pub trait MaybeSync: Sync {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Sync + ?Sized> MaybeSync for T {}
#[cfg(target_arch = "wasm32")]
pub trait MaybeSync {}
#[cfg(target_arch = "wasm32")]
impl<T: ?Sized> MaybeSync for T {}

/// The library calls the assistant needs. Implemented for [`crate::client::MusicClient`]; tests use fakes.
pub trait AssistantLibrary: MaybeSync {
    #[allow(clippy::too_many_arguments)]
    fn search(
        &self,
        query: &str,
        song_count: usize,
        song_offset: usize,
        album_count: usize,
        album_offset: usize,
        artist_count: usize,
        artist_offset: usize,
    ) -> impl Future<Output = MusicResult<SearchResults>> + MaybeSend;
    fn album(&self, id: &str) -> impl Future<Output = MusicResult<Album>> + MaybeSend;
    fn artists(&self) -> impl Future<Output = MusicResult<Vec<Artist>>> + MaybeSend;
    fn artist(&self, id: &str) -> impl Future<Output = MusicResult<Artist>> + MaybeSend;
    fn playlists(&self) -> impl Future<Output = MusicResult<Vec<Playlist>>> + MaybeSend;
    fn playlist(&self, id: &str) -> impl Future<Output = MusicResult<Playlist>> + MaybeSend;
    fn favorites(&self) -> impl Future<Output = MusicResult<SearchResults>> + MaybeSend;
    fn random_songs(&self, size: usize)
    -> impl Future<Output = MusicResult<Vec<Song>>> + MaybeSend;
}

impl AssistantLibrary for crate::client::MusicClient {
    async fn search(
        &self,
        query: &str,
        song_count: usize,
        song_offset: usize,
        album_count: usize,
        album_offset: usize,
        artist_count: usize,
        artist_offset: usize,
    ) -> MusicResult<SearchResults> {
        let limits = crate::client::SearchLimits {
            song_count,
            song_offset,
            album_count,
            album_offset,
            artist_count,
            artist_offset,
        };
        crate::client::MusicClient::search(self, query, limits).await
    }
    async fn album(&self, id: &str) -> MusicResult<Album> {
        crate::client::MusicClient::album(self, id).await
    }
    async fn artists(&self) -> MusicResult<Vec<Artist>> {
        crate::client::MusicClient::artists(self).await
    }
    async fn artist(&self, id: &str) -> MusicResult<Artist> {
        crate::client::MusicClient::artist(self, id).await
    }
    async fn playlists(&self) -> MusicResult<Vec<Playlist>> {
        crate::client::MusicClient::playlists(self).await
    }
    async fn playlist(&self, id: &str) -> MusicResult<Playlist> {
        crate::client::MusicClient::playlist(self, id).await
    }
    async fn favorites(&self) -> MusicResult<SearchResults> {
        crate::client::MusicClient::favorites(self).await
    }
    async fn random_songs(&self, size: usize) -> MusicResult<Vec<Song>> {
        crate::client::MusicClient::random_songs(self, size).await
    }
}

pub const MAX_SEARCH_TERMS: usize = 8;
pub const MAX_SEARCH_PAGES: usize = 5;
pub const MAX_ALBUM_LOOKUPS: usize = 60;
pub const MAX_INDEX_ARTISTS: usize = 4;
pub const ALBUM_BATCH: usize = 4;
pub const SONG_PAGE: usize = 100;
pub const ALBUM_PAGE: usize = 50;
pub const ARTIST_PAGE: usize = 50;
/// Candidates a ChatGPT model may see.
pub const CHATGPT_CANDIDATES: usize = 200;

/// Songs a model may choose from. `limited` means part of the library was not considered.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Catalog {
    pub songs: Vec<Song>,
    pub limited: bool,
}

impl Catalog {
    pub fn new(songs: Vec<Song>, limited: bool) -> Self {
        Self { songs, limited }
    }
}

/// Hard plan requirements: artist, title and album, and never podcasts.
pub fn matches(song: &Song, plan: &AssistantPlan) -> bool {
    !song.is_podcast()
        && (plan.artist.is_empty()
            || AssistantPlan::same_artist(song.artist.as_deref().unwrap_or_default(), &plan.artist))
        && (plan.title.is_empty() || normalize(&song.title).contains(&normalize(&plan.title)))
        && (plan.album().is_empty()
            || normalize(song.album.as_deref().unwrap_or_default())
                .contains(&normalize(plan.album())))
}

/// Filters, deduplicates and ranks songs for a plan. Requested periods rank first; reissues are kept.
pub fn catalog(songs: &[Song], plan: &AssistantPlan, limit: usize, limited: bool) -> Catalog {
    // Merge album-enriched rows before deduplication so a search row cannot hide their dates.
    let mut lookup: HashMap<String, Song> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for song in songs.iter().filter(|s| matches(s, plan)) {
        let key = song.stream_identifier().to_string();
        if let Some(previous) = lookup.get(&key) {
            let mut enriched = song.clone();
            enriched.album_year = song.album_year.or(previous.album_year);
            enriched.album_original_year =
                song.album_original_year.or(previous.album_original_year);
            enriched.year = song.year.or(previous.year);
            enriched.genre = song.genre.clone().or_else(|| previous.genre.clone());
            lookup.insert(key, enriched);
        } else {
            order.push(key.clone());
            lookup.insert(key, song.clone());
        }
    }
    let rank = |song: &Song| -> u8 {
        if !plan.has_years() {
            return 0;
        }
        if song.album_original_year.is_some_and(|y| plan.includes(y)) {
            return 0;
        }
        if song
            .year
            .or(song.album_year)
            .is_some_and(|y| plan.includes(y))
        {
            return 1;
        }
        2 // Reissues and compilations need musical knowledge, not rejection by edition year.
    };
    let mut ranked: Vec<Song> = order
        .into_iter()
        .filter_map(|id| lookup.remove(&id))
        .collect();
    ranked.sort_by_key(|s| rank(s)); // Stable: ties keep library order.
    let limited = limited || ranked.len() > limit;
    ranked.truncate(limit);
    Catalog {
        songs: ranked,
        limited,
    }
}

/// Searches the library for a plan's artist, title, album and suggested queries, loads matching albums,
/// and returns a bounded catalog. Authentication and server errors are returned, not hidden as no matches.
pub async fn resolve<L: AssistantLibrary>(
    library: &L,
    plan: &AssistantPlan,
    limit: usize,
) -> MusicResult<Catalog> {
    let mut songs: Vec<Song> = Vec::new();
    let mut albums: Vec<Album> = Vec::new();
    let mut seen_albums: HashSet<String> = HashSet::new();
    let mut limited = false;
    let mut add_albums = |albums: &mut Vec<Album>, values: Vec<Album>| {
        albums.extend(
            values
                .into_iter()
                .filter(|a| seen_albums.insert(a.id.clone())),
        );
    };
    if !plan.artist.is_empty() {
        // The artist index avoids brittle full-text queries such as "Beatles 1966 1967".
        let index: MusicResult<()> = async {
            let artists: Vec<Artist> = library
                .artists()
                .await?
                .into_iter()
                .filter(|a| AssistantPlan::same_artist(&a.name, &plan.artist))
                .collect();
            limited = artists.len() > MAX_INDEX_ARTISTS;
            for artist in artists.iter().take(MAX_INDEX_ARTISTS) {
                let full = library.artist(&artist.id).await?;
                add_albums(&mut albums, full.album.unwrap_or_default());
            }
            Ok(())
        }
        .await;
        match index {
            // Search results can still expose artists and albums; older servers may omit the index.
            Ok(()) | Err(MusicError::Unsupported | MusicError::NotFound) => {}
            Err(error) => return Err(error),
        }
    }
    let mut terms: Vec<String> = Vec::new();
    let mut seen_terms: HashSet<String> = HashSet::new();
    let fixed = [
        plan.title.as_str(),
        plan.artist.as_str(),
        plan.album(),
        plan.query.as_str(),
    ];
    for term in fixed
        .into_iter()
        .chain(plan.queries().iter().map(String::as_str))
    {
        let term = term.trim();
        if !term.is_empty() && seen_terms.insert(normalize(term)) {
            terms.push(term.to_string());
        }
    }
    if terms.is_empty() && plan.has_years() {
        terms.push(String::new()); // OpenSubsonic catalog enumeration.
    }
    if terms.len() > MAX_SEARCH_TERMS {
        limited = true;
    }
    let mut seen_artists: HashSet<String> = HashSet::new();
    for term in terms.iter().take(MAX_SEARCH_TERMS) {
        let (mut song_offset, mut album_offset, mut more_songs, mut more_albums) =
            (0, 0, true, true);
        let mut seen_rows: HashSet<String> = HashSet::new();
        for page in 0..MAX_SEARCH_PAGES {
            let results = library
                .search(
                    term,
                    if more_songs { SONG_PAGE } else { 0 },
                    song_offset,
                    if more_albums { ALBUM_PAGE } else { 0 },
                    album_offset,
                    if page == 0 { ARTIST_PAGE } else { 0 },
                    0,
                )
                .await?;
            let tracks = results.song.unwrap_or_default();
            let releases = results.album.unwrap_or_default();
            let keys: Vec<String> = tracks
                .iter()
                .map(|s| format!("s:{}", s.id))
                .chain(releases.iter().map(|a| format!("a:{}", a.id)))
                .collect();
            if !keys.is_empty() && keys.iter().all(|k| seen_rows.contains(k)) {
                // The server ignores offsets: stop instead of repeating, and admit the catalog is partial.
                limited = true;
                break;
            }
            seen_rows.extend(keys);
            let (track_count, release_count) = (tracks.len(), releases.len());
            songs.extend(tracks);
            add_albums(&mut albums, releases);
            for artist in results.artist.unwrap_or_default() {
                if !plan.artist.is_empty()
                    && AssistantPlan::same_artist(&artist.name, &plan.artist)
                    && seen_artists.insert(artist.id.clone())
                    && albums.is_empty()
                {
                    let full = library.artist(&artist.id).await?;
                    add_albums(&mut albums, full.album.unwrap_or_default());
                }
            }
            more_songs = more_songs && track_count >= SONG_PAGE;
            more_albums = more_albums && release_count >= ALBUM_PAGE;
            song_offset += track_count;
            album_offset += release_count;
            if !more_songs && !more_albums {
                break;
            }
            if page == MAX_SEARCH_PAGES - 1 {
                limited = true;
            }
        }
    }
    // Fetch real album tracks, including songs not returned by the first search page.
    // Prioritize requested years, while retaining remasters for the model's original-era reasoning.
    let album_rank = |album: &Album| -> u8 {
        if !plan.has_years() {
            return 0;
        }
        match album
            .original_release_date
            .as_ref()
            .and_then(|d| d.year)
            .or(album.year)
        {
            Some(year) if plan.includes(year) => 0,
            _ => 1,
        }
    };
    albums.sort_by_key(|a| album_rank(a));
    if albums.len() > MAX_ALBUM_LOOKUPS {
        limited = true;
    }
    albums.truncate(MAX_ALBUM_LOOKUPS);
    for batch in albums.chunks(ALBUM_BATCH) {
        let loaded = futures::future::try_join_all(batch.iter().map(|album| async move {
            let full = library.album(&album.id).await?;
            let album_year = full.year.or(album.year);
            let original = full
                .original_release_date
                .as_ref()
                .and_then(|d| d.year)
                .or_else(|| album.original_release_date.as_ref().and_then(|d| d.year));
            let genre = full.genre.clone().or_else(|| album.genre.clone());
            Ok::<_, MusicError>(
                full.song
                    .unwrap_or_default()
                    .into_iter()
                    .map(|mut song| {
                        song.album_year = album_year;
                        song.album_original_year = original;
                        if song.genre.is_none() {
                            song.genre = genre.clone();
                        }
                        song
                    })
                    .collect::<Vec<_>>(),
            )
        }))
        .await?;
        songs.extend(loaded.into_iter().flatten());
    }
    Ok(catalog(&songs, plan, limit, limited))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::assistant::music_assistant::tests::library_plan;
    use crate::models::ReleaseDate;
    use serde_json::json;
    use std::sync::Mutex;

    type SearchHandler =
        Box<dyn Fn(&str, usize, usize) -> MusicResult<SearchResults> + Send + Sync>;

    /// An in-memory library. Each call is logged as `name:args`.
    #[derive(Default)]
    pub(crate) struct FakeLibrary {
        pub artists: Vec<Artist>,
        pub artist_albums: HashMap<String, Vec<Album>>,
        pub albums: HashMap<String, Album>,
        pub search: Option<SearchHandler>,
        pub playlists: Vec<Playlist>,
        pub favorites: Vec<Song>,
        pub random: Vec<Song>,
        pub artists_error: Option<MusicError>,
        pub calls: Mutex<Vec<String>>,
    }

    impl FakeLibrary {
        fn log(&self, call: String) {
            self.calls.lock().unwrap().push(call);
        }
        pub fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl AssistantLibrary for FakeLibrary {
        async fn search(
            &self,
            query: &str,
            song_count: usize,
            song_offset: usize,
            _: usize,
            _: usize,
            _: usize,
            _: usize,
        ) -> MusicResult<SearchResults> {
            self.log(format!("search:{query}:{song_offset}"));
            match &self.search {
                Some(handler) => handler(query, song_count, song_offset),
                None => Ok(SearchResults::default()),
            }
        }
        async fn album(&self, id: &str) -> MusicResult<Album> {
            self.log(format!("album:{id}"));
            Ok(self.albums.get(id).cloned().unwrap_or(Album {
                id: id.into(),
                ..Default::default()
            }))
        }
        async fn artists(&self) -> MusicResult<Vec<Artist>> {
            self.log("artists".into());
            match self.artists_error {
                Some(error) => Err(error),
                None => Ok(self.artists.clone()),
            }
        }
        async fn artist(&self, id: &str) -> MusicResult<Artist> {
            self.log(format!("artist:{id}"));
            let artist = self
                .artists
                .iter()
                .find(|a| a.id == id)
                .cloned()
                .unwrap_or(Artist {
                    id: id.into(),
                    ..Default::default()
                });
            Ok(Artist {
                album: self.artist_albums.get(id).cloned(),
                ..artist
            })
        }
        async fn playlists(&self) -> MusicResult<Vec<Playlist>> {
            self.log("playlists".into());
            Ok(self.playlists.clone())
        }
        async fn playlist(&self, id: &str) -> MusicResult<Playlist> {
            self.log(format!("playlist:{id}"));
            self.playlists
                .iter()
                .find(|p| p.id == id)
                .cloned()
                .ok_or(MusicError::NotFound)
        }
        async fn favorites(&self) -> MusicResult<SearchResults> {
            self.log("favorites".into());
            Ok(SearchResults {
                song: Some(self.favorites.clone()),
                ..Default::default()
            })
        }
        async fn random_songs(&self, size: usize) -> MusicResult<Vec<Song>> {
            self.log(format!("random:{size}"));
            Ok(self.random.iter().take(size).cloned().collect())
        }
    }

    fn block_on<F: Future>(future: F) -> F::Output {
        futures::executor::block_on(future)
    }

    fn song(id: &str, title: &str, artist: &str) -> Song {
        Song {
            artist: Some(artist.into()),
            ..Song::new(id, title)
        }
    }

    #[test]
    fn catalog_keeps_reissues_for_original_era_reasoning_and_merges_album_dates() {
        let plan = library_plan(json!({})).unwrap();
        let search_row = Song {
            album: Some("Revolver".into()),
            year: Some(2009),
            ..song("reissue", "Taxman", "The Beatles")
        };
        let enriched = Song {
            album_year: Some(2009),
            album_original_year: Some(1966),
            ..search_row.clone()
        };
        let songs = vec![
            Song {
                year: Some(1965),
                ..song("older", "Older", "Beatles")
            },
            search_row,
            Song {
                year: Some(1966),
                ..song("cover", "Taxman", "Beatles Tribute")
            },
            song("unknown", "Unknown", "Beatles"),
            enriched,
            Song {
                is_podcast: Some(true),
                ..song("podcast", "Interview", "Beatles")
            },
        ];
        let result = catalog(&songs, &plan, 20, false);
        assert_eq!(
            result
                .songs
                .iter()
                .map(|s| s.id.as_str())
                .collect::<Vec<_>>(),
            ["reissue", "older", "unknown"]
        );
        assert_eq!(result.songs[0].album_original_year, Some(1966));
        assert_eq!(result.songs[0].year, Some(2009));
        assert!(!result.limited);
        let metadata = crate::assistant::AssistantPrompts::catalog(&result.songs, false, false);
        assert!(metadata.contains("album_original_year") && metadata.contains("1966"));
        assert!(!metadata.contains("reissue") && !metadata.contains("unknown"));
        assert!(catalog(&songs, &plan, 1, false).limited);
    }

    #[test]
    fn artist_album_discovery_finds_songs_when_search_has_no_song_matches() {
        let revolver = Album {
            year: Some(2009),
            ..Album::new("album", "Revolver", "The Beatles")
        };
        let full = Album {
            original_release_date: Some(ReleaseDate { year: Some(1966) }),
            song: Some(vec![Song {
                album: Some("Revolver".into()),
                year: Some(2009),
                ..song("real-song", "Taxman", "The Beatles")
            }]),
            ..revolver.clone()
        };
        let library = FakeLibrary {
            artists: vec![Artist {
                id: "artist".into(),
                name: "Beatles".into(),
                ..Default::default()
            }],
            artist_albums: HashMap::from([("artist".to_string(), vec![revolver])]),
            albums: HashMap::from([("album".to_string(), full)]),
            ..Default::default()
        };
        let result = block_on(resolve(
            &library,
            &library_plan(json!({})).unwrap(),
            CHATGPT_CANDIDATES,
        ))
        .unwrap();
        assert_eq!(
            result
                .songs
                .iter()
                .map(|s| s.id.as_str())
                .collect::<Vec<_>>(),
            ["real-song"]
        );
        assert_eq!(result.songs[0].album_original_year, Some(1966));
        assert_eq!(result.songs[0].album_year, Some(2009));
        assert!(!result.limited);
    }

    #[test]
    fn paginated_search_finds_period_songs_beyond_first_page_before_limiting_catalog() {
        let library = FakeLibrary {
            search: Some(Box::new(|query, _, offset| {
                assert_eq!(query, "The Beatles");
                let songs = if offset == 0 {
                    (0..100)
                        .map(|i| Song {
                            year: Some(1965),
                            ..song(&format!("old-{i}"), &format!("Old {i}"), "The Beatles")
                        })
                        .collect()
                } else {
                    assert_eq!(offset, 100);
                    vec![Song {
                        year: Some(1967),
                        ..song("period-match", "Period", "The Beatles")
                    }]
                };
                Ok(SearchResults {
                    song: Some(songs),
                    ..Default::default()
                })
            })),
            ..Default::default()
        };
        let result = block_on(resolve(
            &library,
            &library_plan(json!({"query": "The Beatles"})).unwrap(),
            1,
        ))
        .unwrap();
        assert_eq!(
            result
                .songs
                .iter()
                .map(|s| s.id.as_str())
                .collect::<Vec<_>>(),
            ["period-match"]
        );
        assert!(result.limited);
    }

    #[test]
    fn server_ignoring_offsets_stops_without_repeating_or_claiming_complete_catalog() {
        let library = FakeLibrary {
            search: Some(Box::new(|_, _, offset| {
                assert!(offset == 0 || offset == 100);
                Ok(SearchResults {
                    song: Some(
                        (0..100)
                            .map(|i| Song::new(format!("song-{i}"), format!("Song {i}")))
                            .collect(),
                    ),
                    ..Default::default()
                })
            })),
            ..Default::default()
        };
        let result = block_on(resolve(
            &library,
            &library_plan(json!({"artist": "", "query": "Some music"})).unwrap(),
            CHATGPT_CANDIDATES,
        ))
        .unwrap();
        assert_eq!(result.songs.len(), 100);
        assert!(result.limited);
        assert_eq!(library.calls().len(), 2);
    }

    #[test]
    fn library_resolution_preserves_authentication_errors_instead_of_no_matches() {
        let library = FakeLibrary {
            artists_error: Some(MusicError::Authentication),
            ..Default::default()
        };
        assert_eq!(
            block_on(resolve(&library, &library_plan(json!({})).unwrap(), 200)),
            Err(MusicError::Authentication)
        );
        let library = FakeLibrary {
            artists_error: Some(MusicError::Unsupported),
            search: Some(Box::new(|_, _, _| Err(MusicError::Authentication))),
            ..Default::default()
        };
        assert_eq!(
            block_on(resolve(&library, &library_plan(json!({})).unwrap(), 200)),
            Err(MusicError::Authentication)
        );
    }

    #[test]
    fn discovery_is_bounded_to_eight_terms_five_pages_and_sixty_albums() {
        let library = FakeLibrary {
            search: Some(Box::new(|query, _, offset| {
                let songs = (0..100)
                    .map(|i| Song::new(format!("{query}-{offset}-{i}"), "x"))
                    .collect();
                let albums = if offset == 0 {
                    (0..50)
                        .map(|i| Album::new(format!("{query}-a{i}"), "A", "B"))
                        .collect()
                } else {
                    vec![]
                };
                Ok(SearchResults {
                    song: Some(songs),
                    album: Some(albums),
                    ..Default::default()
                })
            })),
            ..Default::default()
        };
        let queries: Vec<String> = (0..8).map(|i| format!("q{i}")).collect();
        let plan = library_plan(json!({"artist": "", "query": "base", "from_year": 0, "to_year": 0, "queries": queries})).unwrap();
        let result = block_on(resolve(&library, &plan, CHATGPT_CANDIDATES)).unwrap();
        assert!(result.limited);
        assert_eq!(result.songs.len(), CHATGPT_CANDIDATES);
        let calls = library.calls();
        assert_eq!(
            calls.iter().filter(|c| c.starts_with("search:")).count(),
            8 * 5
        );
        assert!(!calls.iter().any(|c| c.starts_with("search:q7:")));
        assert_eq!(calls.iter().filter(|c| c.starts_with("album:")).count(), 60);
    }

    fn subsonic(server: &crate::client::test_server::TestServer) -> crate::client::MusicClient {
        let credentials = crate::models::Credentials::simple(
            server.base.as_str(),
            "fixture",
            "fixture",
            true,
            crate::models::MusicProvider::Subsonic,
        )
        .unwrap();
        crate::client::MusicClient::new(credentials)
    }

    #[tokio::test]
    async fn music_client_artist_album_discovery_through_subsonic() {
        let server = crate::client::test_server::TestServer::start(|request| {
            let payload = match request.last_path_component() {
                "getArtists.view" => json!({"artists": {"index": [{"artist": [{"id": "artist", "name": "Beatles"}]}]}}),
                "getArtist.view" => json!({"artist": {"id": "artist", "name": "The Beatles", "album": [{"id": "album", "name": "Revolver", "year": 2009}]}}),
                "search3.view" => json!({"searchResult3": {}}),
                "getAlbum.view" => json!({"album": {"id": "album", "name": "Revolver", "year": 2009, "originalReleaseDate": {"year": 1966},
                    "song": [{"id": "real-song", "title": "Taxman", "artist": "The Beatles", "album": "Revolver", "year": 2009}]}}),
                other => panic!("unexpected endpoint {other}"),
            };
            let mut body = json!({"status": "ok"});
            body.as_object_mut().unwrap().extend(payload.as_object().unwrap().clone());
            (200, json!({"subsonic-response": body}).to_string())
        })
        .await;
        let result = resolve(
            &subsonic(&server),
            &library_plan(json!({})).unwrap(),
            CHATGPT_CANDIDATES,
        )
        .await
        .unwrap();
        assert_eq!(
            result
                .songs
                .iter()
                .map(|s| s.id.as_str())
                .collect::<Vec<_>>(),
            ["real-song"]
        );
        assert_eq!(result.songs[0].album_original_year, Some(1966));
        assert_eq!(result.songs[0].album_year, Some(2009));
        assert!(!result.limited);
    }

    #[tokio::test]
    async fn music_client_authentication_errors_are_preserved() {
        let server = crate::client::test_server::TestServer::start(|_| (401, String::new())).await;
        assert_eq!(
            resolve(&subsonic(&server), &library_plan(json!({})).unwrap(), 200).await,
            Err(MusicError::Authentication)
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn resolution_future_is_send() {
        fn assert_send<T: Send>(_: T) {}
        let library = FakeLibrary::default();
        let plan = library_plan(json!({})).unwrap();
        assert_send(resolve(&library, &plan, 10));
        let credentials = crate::models::Credentials::simple(
            "https://fixture.test",
            "u",
            "p",
            false,
            crate::models::MusicProvider::Navidrome,
        )
        .unwrap();
        let client = crate::client::MusicClient::new(credentials);
        assert_send(resolve(&client, &plan, 10));
    }
}
