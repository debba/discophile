//! Resolves public podcast links (Apple Podcasts, Spotify, RSS) to feeds the server can follow,
//! and searches subscribed channels. Requests are unauthenticated: server credentials never reach
//! Apple or Spotify.

use std::collections::HashSet;
use std::future::Future;

use serde::Deserialize;
use url::Url;

use crate::error::MusicError;
use crate::l10n::{music_error, t};
use crate::listening_history::{fold, normalize_key};
use crate::models::{PodcastChannel, Song, podcast_order};

pub const USER_AGENT: &str = "Discophile/0.1.0 (https://github.com/debba/discophile)";
pub const SEARCH_LIMIT: usize = 25;
pub const SPOTIFY_MATCH_LIMIT: usize = 10;
pub const EPISODE_SEARCH_LIMIT: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PodcastError {
    #[error("invalid podcast link")]
    InvalidLink,
    #[error("episode link")]
    EpisodeLink,
    #[error("no public feed")]
    NoPublicFeed,
    #[error(transparent)]
    Music(#[from] MusicError),
}

impl PodcastError {
    /// The localized message shown when adding a podcast fails.
    pub fn message(self) -> String {
        match self {
            Self::InvalidLink => {
                t("Paste a podcast link from Apple Podcasts or Spotify, or the address of an RSS feed.")
            }
            Self::EpisodeLink => t("This is a link to an episode. Share the podcast's page instead."),
            Self::NoPublicFeed => t("No public feed was found for this podcast. It may be exclusive to Spotify."),
            Self::Music(MusicError::Forbidden) => t("Your account is not allowed to manage podcasts on this server."),
            Self::Music(MusicError::NotFound) => t("This podcast could not be found. Check the link and try again."),
            Self::Music(other) => music_error(other),
        }
    }
}

/// A link pasted by the user, before any network lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PodcastSource {
    Feed(Url),
    ApplePodcasts { id: String },
    SpotifyShow { id: String },
}

impl PodcastSource {
    pub fn parse(link: &str) -> Result<Self, PodcastError> {
        let url = Url::parse(link.trim()).map_err(|_| PodcastError::InvalidLink)?;
        let scheme = url.scheme().to_ascii_lowercase();
        let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
        if !(scheme == "https" || scheme == "http")
            || host.is_empty()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(PodcastError::InvalidLink);
        }
        let path: Vec<&str> = url.path().split('/').filter(|p| !p.is_empty()).collect();
        if host == "podcasts.apple.com" || host == "itunes.apple.com" {
            let id = path
                .last()
                .and_then(|last| last.strip_prefix("id"))
                .filter(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))
                .ok_or(PodcastError::InvalidLink)?;
            Ok(Self::ApplePodcasts { id: id.to_string() })
        } else if host == "open.spotify.com" {
            // Localized links look like /intl-it/show/<id>.
            let parts = if path.first().is_some_and(|p| p.starts_with("intl-")) { &path[1..] } else { &path[..] };
            if parts.len() < 2 {
                return Err(PodcastError::InvalidLink);
            }
            if parts[0] == "episode" {
                return Err(PodcastError::EpisodeLink);
            }
            if parts[0] != "show" || parts[1].len() != 22 || !parts[1].bytes().all(|b| b.is_ascii_alphanumeric()) {
                return Err(PodcastError::InvalidLink);
            }
            Ok(Self::SpotifyShow { id: parts[1].to_string() })
        } else if host.ends_with("spotify.com")
            || host == "spotify.link"
            || host == "spoti.fi"
            || host.ends_with("apple.com")
        {
            Err(PodcastError::InvalidLink)
        } else {
            Ok(Self::Feed(url))
        }
    }
}

/// A feed the user confirms before the server subscribes to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PodcastCandidate {
    pub title: String,
    pub author: Option<String>,
    pub feed_url: Url,
    pub artwork_url: Option<Url>,
}

impl PodcastCandidate {
    pub fn id(&self) -> &str {
        self.feed_url.as_str()
    }

    fn feed(url: &Url) -> Self {
        Self {
            title: url.host_str().unwrap_or(url.as_str()).to_string(),
            author: None,
            feed_url: url.clone(),
            artwork_url: None,
        }
    }
}

#[derive(Deserialize)]
struct ITunesResults {
    #[serde(default)]
    results: Vec<ITunesItem>,
}

#[derive(Deserialize)]
struct ITunesItem {
    #[serde(default, rename = "collectionName")]
    collection_name: Option<String>,
    #[serde(default, rename = "artistName")]
    artist_name: Option<String>,
    #[serde(default, rename = "feedUrl")]
    feed_url: Option<String>,
    #[serde(default, rename = "artworkUrl600")]
    artwork_url600: Option<String>,
    #[serde(default, rename = "artworkUrl100")]
    artwork_url100: Option<String>,
}

fn sorted_url(base: &str, mut parameters: Vec<(&str, String)>) -> Url {
    parameters.sort_by(|a, b| a.0.cmp(b.0));
    let mut url = Url::parse(base).expect("static directory URL");
    if !parameters.is_empty() {
        url.query_pairs_mut().extend_pairs(parameters);
    }
    url
}

/// `https://itunes.apple.com/lookup?entity=podcast&id=…`
pub fn itunes_lookup_url(id: &str) -> Url {
    sorted_url("https://itunes.apple.com/lookup", vec![("id", id.to_string()), ("entity", "podcast".into())])
}

/// A podcast search in the public Apple Podcasts directory. Only the term is sent.
pub fn itunes_search_url(term: &str, limit: usize) -> Url {
    sorted_url(
        "https://itunes.apple.com/search",
        vec![
            ("term", term.to_string()),
            ("media", "podcast".into()),
            ("entity", "podcast".into()),
            ("limit", limit.to_string()),
        ],
    )
}

pub fn spotify_show_url(id: &str) -> Url {
    sorted_url(&format!("https://open.spotify.com/show/{id}"), Vec::new())
}

/// Public HTTP(S) feeds from an iTunes response, without duplicates.
pub fn parse_itunes(body: &[u8]) -> Result<Vec<PodcastCandidate>, MusicError> {
    let decoded: ITunesResults = serde_json::from_slice(body).map_err(|_| MusicError::InvalidResponse)?;
    let mut seen = HashSet::new();
    Ok(decoded
        .results
        .into_iter()
        .filter_map(|item| {
            let feed = item.feed_url?;
            let url = Url::parse(&feed).ok().filter(|u| u.scheme() == "https" || u.scheme() == "http")?;
            if !seen.insert(url.as_str().to_string()) {
                return None;
            }
            let title = item.collection_name.map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
            Some(PodcastCandidate {
                title: title.unwrap_or_else(|| url.host_str().map(str::to_string).unwrap_or(feed)),
                author: item.artist_name,
                artwork_url: item.artwork_url600.or(item.artwork_url100).and_then(|a| Url::parse(&a).ok()),
                feed_url: url,
            })
        })
        .collect())
}

/// What a Spotify show page reveals: its title and, when present, its publisher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpotifyShow {
    pub title: String,
    pub publisher: Option<String>,
}

/// Reads a show page's Open Graph tags (oEmbed titles name the latest episode instead).
pub fn parse_spotify_show(page: &str) -> Result<SpotifyShow, MusicError> {
    let title = meta("og:title", page).filter(|t| !t.is_empty()).ok_or(MusicError::InvalidResponse)?;
    // The description reads "Podcast · Publisher · Summary".
    let publisher = meta("og:description", page).and_then(|d| d.split(" · ").nth(1).map(str::to_string));
    Ok(SpotifyShow { title, publisher })
}

/// Spotify never exposes feeds, so shows are matched to directory results by exact title,
/// the same publisher first.
pub fn match_spotify_show(
    show: &SpotifyShow,
    results: Vec<PodcastCandidate>,
) -> Result<Vec<PodcastCandidate>, PodcastError> {
    let key = normalize_key(&show.title);
    let exact: Vec<PodcastCandidate> = results.into_iter().filter(|c| normalize_key(&c.title) == key).collect();
    if exact.is_empty() {
        return Err(PodcastError::NoPublicFeed);
    }
    let Some(publisher) = show.publisher.as_deref().map(normalize_key) else { return Ok(exact) };
    let (same, other): (Vec<_>, Vec<_>) =
        exact.into_iter().partition(|c| c.author.as_deref().map(normalize_key).as_deref() == Some(publisher.as_str()));
    Ok(same.into_iter().chain(other).collect())
}

/// Maps a directory reply's status: 400 and 404 mean not found.
pub fn check_status(status: u16) -> Result<(), MusicError> {
    match status {
        400 | 404 => Err(MusicError::NotFound),
        200..=299 => Ok(()),
        _ => Err(MusicError::ServerUnavailable),
    }
}

/// The content of `<meta property="…" content="…">`, entity-decoded and trimmed.
pub fn meta(property: &str, page: &str) -> Option<String> {
    let marker = format!("<meta property=\"{property}\" content=\"");
    let start = page.find(&marker)? + marker.len();
    let end = page[start..].find('"')? + start;
    Some(decode_entities(&page[start..end]).trim().to_string())
}

/// Decodes named and numeric HTML entities, keeping unknown and unterminated references.
pub fn decode_entities(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        result.push_str(&rest[..amp]);
        let candidate = &rest[amp..];
        let window: usize = candidate.char_indices().nth(10).map_or(candidate.len(), |(i, _)| i);
        let decoded = candidate[..window].find(';').and_then(|semicolon| {
            let name = &candidate[1..semicolon];
            let named = match name {
                "amp" => Some('&'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "nbsp" => Some(' '),
                _ => None,
            };
            let numeric = if let Some(hex) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
                u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
            } else if let Some(decimal) = name.strip_prefix('#') {
                decimal.parse::<u32>().ok().and_then(char::from_u32)
            } else {
                None
            };
            named.or(numeric).map(|c| (c, semicolon))
        });
        match decoded {
            Some((c, semicolon)) => {
                result.push(c);
                rest = &candidate[semicolon + 1..];
            }
            None => {
                result.push('&');
                rest = &candidate[1..];
            }
        }
    }
    result.push_str(rest);
    result
}

/// The lookup flow over any transport. `fetch(url, accept)` returns the body of a 2xx reply
/// (see [`check_status`]).
pub async fn resolve_candidates<F, Fut>(
    source: &PodcastSource,
    mut fetch: F,
) -> Result<Vec<PodcastCandidate>, PodcastError>
where
    F: FnMut(Url, &'static str) -> Fut,
    Fut: Future<Output = Result<Vec<u8>, MusicError>>,
{
    match source {
        PodcastSource::Feed(url) => Ok(vec![PodcastCandidate::feed(url)]),
        PodcastSource::ApplePodcasts { id } => {
            let results = parse_itunes(&fetch(itunes_lookup_url(id), "application/json").await?)?;
            if results.is_empty() { Err(PodcastError::NoPublicFeed) } else { Ok(results) }
        }
        PodcastSource::SpotifyShow { id } => {
            let page = fetch(spotify_show_url(id), "text/html").await?;
            let show = parse_spotify_show(&String::from_utf8_lossy(&page))?;
            let results =
                parse_itunes(&fetch(itunes_search_url(&show.title, SPOTIFY_MATCH_LIMIT), "application/json").await?)?;
            match_spotify_show(&show, results)
        }
    }
}

/// Resolves podcast links with a separate, unauthenticated client.
pub struct PodcastDirectory {
    http: reqwest::Client,
}

impl Default for PodcastDirectory {
    fn default() -> Self {
        Self::new()
    }
}

impl PodcastDirectory {
    pub fn new() -> Self {
        Self { http: crate::http::public_client() }
    }

    pub fn with_client(http: reqwest::Client) -> Self {
        Self { http }
    }

    /// Candidates the user confirms before the server subscribes to a feed.
    pub async fn candidates(&self, source: &PodcastSource) -> Result<Vec<PodcastCandidate>, PodcastError> {
        resolve_candidates(source, |url, accept| self.get(url, accept)).await
    }

    /// Searches the public Apple Podcasts directory. Only the search term is sent.
    pub async fn search(&self, term: &str) -> Result<Vec<PodcastCandidate>, PodcastError> {
        let term = term.trim();
        if term.is_empty() {
            return Ok(Vec::new());
        }
        Ok(parse_itunes(&self.get(itunes_search_url(term, SEARCH_LIMIT), "application/json").await?)?)
    }

    async fn get(&self, url: Url, accept: &'static str) -> Result<Vec<u8>, MusicError> {
        let request = self.http.get(url).header(reqwest::header::ACCEPT, accept);
        #[cfg(not(target_arch = "wasm32"))]
        let request =
            request.header(reqwest::header::USER_AGENT, USER_AGENT).timeout(std::time::Duration::from_secs(20));
        let response = request.send().await.map_err(|_| MusicError::ServerUnavailable)?;
        check_status(response.status().as_u16())?;
        Ok(response.bytes().await.map_err(|_| MusicError::ServerUnavailable)?.to_vec())
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PodcastSearchResults {
    pub channels: Vec<PodcastChannel>,
    pub episodes: Vec<Song>,
}

impl PodcastSearchResults {
    pub fn is_empty(&self) -> bool {
        self.channels.is_empty() && self.episodes.is_empty()
    }
}

/// Matches subscribed channels (title or description) and playable episodes (title or channel
/// title). Every word must match, ignoring case and accents. Episodes are newest first.
pub fn search_subscriptions(term: &str, channels: &[PodcastChannel], episode_limit: usize) -> PodcastSearchResults {
    let words: Vec<String> = term.split_whitespace().map(fold).collect();
    if words.is_empty() {
        return PodcastSearchResults::default();
    }
    let contains = |fields: &[Option<&str>]| {
        let text = fold(&fields.iter().flatten().copied().collect::<Vec<_>>().join(" "));
        words.iter().all(|w| text.contains(w.as_str()))
    };
    let found =
        channels.iter().filter(|c| contains(&[Some(c.title.as_str()), c.description.as_deref()])).cloned().collect();
    let mut matched = Vec::new();
    for channel in channels {
        let playable: Vec<_> = channel
            .episodes()
            .iter()
            .filter(|e| e.is_playable() && contains(&[Some(e.title.as_str()), Some(channel.title.as_str())]))
            .cloned()
            .collect();
        for episode in podcast_order::newest_first(&playable) {
            matched.push((channel, episode));
        }
    }
    let sorted = podcast_order::newest_first(&matched.iter().map(|(_, e)| e.clone()).collect::<Vec<_>>());
    let episodes = sorted
        .iter()
        .take(episode_limit)
        .filter_map(|episode| {
            matched.iter().find(|(_, e)| e.id == episode.id).map(|(channel, _)| episode.song(channel))
        })
        .collect();
    PodcastSearchResults { channels: found, episodes }
}

/// True when the server already follows this feed, ignoring scheme, case and trailing slashes.
pub fn is_subscribed(feed: &str, channels: &[PodcastChannel]) -> bool {
    let key = feed_key(feed);
    channels.iter().any(|c| c.url.as_deref().map(feed_key).as_deref() == Some(key.as_str()))
}

fn feed_key(value: &str) -> String {
    let text = value.trim().to_lowercase();
    let text = text.strip_prefix("https://").or_else(|| text.strip_prefix("http://")).unwrap_or(&text);
    text.trim_end_matches('/').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::executor::block_on;
    use std::cell::RefCell;

    const SHOW: &str = "0123456789abcdefABCDEF";

    fn query(url: &Url) -> Vec<(String, String)> {
        let mut pairs: Vec<_> = url.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
        pairs.sort();
        pairs
    }

    fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
        let mut pairs: Vec<_> = items.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        pairs.sort();
        pairs
    }

    /// Resolves a source against a fixture handler, logging requested URLs.
    fn resolve(
        source: &PodcastSource,
        handler: impl Fn(&Url) -> (u16, &'static str),
    ) -> (Result<Vec<PodcastCandidate>, PodcastError>, Vec<Url>) {
        let log = RefCell::new(Vec::new());
        let result = block_on(resolve_candidates(source, |url, _| {
            log.borrow_mut().push(url.clone());
            let (status, body) = handler(&url);
            let reply = check_status(status).map(|_| body.as_bytes().to_vec());
            async move { reply }
        }));
        (result, log.into_inner())
    }

    #[test]
    fn recognizes_apple_spotify_and_feed_links() {
        assert_eq!(
            PodcastSource::parse(" https://podcasts.apple.com/it/podcast/synthetic-show/id1234567890?i=1000 "),
            Ok(PodcastSource::ApplePodcasts { id: "1234567890".into() })
        );
        assert_eq!(
            PodcastSource::parse(&format!("https://open.spotify.com/show/{SHOW}?si=share")),
            Ok(PodcastSource::SpotifyShow { id: SHOW.into() })
        );
        assert_eq!(
            PodcastSource::parse(&format!("https://open.spotify.com/intl-it/show/{SHOW}")),
            Ok(PodcastSource::SpotifyShow { id: SHOW.into() })
        );
        assert_eq!(
            PodcastSource::parse("http://feeds.example.test/show.xml"),
            Ok(PodcastSource::Feed(Url::parse("http://feeds.example.test/show.xml").unwrap()))
        );
    }

    #[test]
    fn rejects_episodes_unknown_service_links_and_non_web_urls() {
        assert_eq!(
            PodcastSource::parse(&format!("https://open.spotify.com/episode/{SHOW}")),
            Err(PodcastError::EpisodeLink)
        );
        for link in [
            "https://open.spotify.com/album/0123456789abcdefABCDEF",
            "https://open.spotify.com/show/short",
            "https://spotify.link/abc",
            "https://music.apple.com/it/album/synthetic/123",
            "https://podcasts.apple.com/it/podcast/synthetic",
            "ftp://example.test/feed.xml",
            "feed.xml",
            "https://user:secret@example.test/feed.xml",
            "",
        ] {
            assert_eq!(PodcastSource::parse(link), Err(PodcastError::InvalidLink), "{link}");
        }
    }

    #[test]
    fn direct_feed_needs_no_lookup() {
        let feed = PodcastSource::Feed(Url::parse("https://feeds.example.test/show.xml").unwrap());
        let (result, log) = resolve(&feed, |_| panic!("unexpected request"));
        assert_eq!(result.unwrap()[0].feed_url.as_str(), "https://feeds.example.test/show.xml");
        assert!(log.is_empty());
    }

    #[test]
    fn apple_lookup_returns_public_feed() {
        let (result, log) = resolve(&PodcastSource::ApplePodcasts { id: "1234567890".into() }, |_| {
            (
                200,
                r#"{"resultCount":1,"results":[{"collectionName":"Synthetic Show","artistName":"Test Host","feedUrl":"https://feeds.example.test/show.xml","artworkUrl600":"https://img.example.test/600.jpg"}]}"#,
            )
        });
        assert_eq!(
            result.unwrap(),
            [PodcastCandidate {
                title: "Synthetic Show".into(),
                author: Some("Test Host".into()),
                feed_url: Url::parse("https://feeds.example.test/show.xml").unwrap(),
                artwork_url: Some(Url::parse("https://img.example.test/600.jpg").unwrap()),
            }]
        );
        assert_eq!(log[0].host_str(), Some("itunes.apple.com"));
        assert_eq!(query(&log[0]), pairs(&[("id", "1234567890"), ("entity", "podcast")]));
    }

    #[test]
    fn apple_lookup_without_feed_reports_no_public_feed() {
        let (result, _) = resolve(&PodcastSource::ApplePodcasts { id: "1".into() }, |_| {
            (200, r#"{"resultCount":1,"results":[{"collectionName":"Private Show"}]}"#)
        });
        assert_eq!(result, Err(PodcastError::NoPublicFeed));
    }

    #[test]
    fn spotify_show_is_matched_by_exact_title_only() {
        let (result, log) = resolve(&PodcastSource::SpotifyShow { id: SHOW.into() }, |url| {
            if url.host_str() == Some("open.spotify.com") {
                return (
                    200,
                    r#"<html><head><meta property="og:title" content="Synthetic &amp; Show"/><meta property="og:description" content="Podcast · Test Host · A synthetic &#x27;fixture&#x27;."/></head></html>"#,
                );
            }
            (
                200,
                r#"{"results":[{"collectionName":"Synthetic & Show Extra","feedUrl":"https://other.example.test/feed"},{"collectionName":"synthetic & show","artistName":"Other Host","feedUrl":"https://feeds.example.test/copy.xml"},{"collectionName":"Synthetic & Show","artistName":"Test Host","feedUrl":"https://feeds.example.test/show.xml"},{"collectionName":"Synthetic & Show","artistName":"Test Host","feedUrl":"https://feeds.example.test/show.xml"}]}"#,
            )
        });
        let feeds: Vec<String> = result.unwrap().iter().map(|c| c.feed_url.to_string()).collect();
        assert_eq!(feeds, ["https://feeds.example.test/show.xml", "https://feeds.example.test/copy.xml"]);
        assert_eq!(log[0].as_str(), format!("https://open.spotify.com/show/{SHOW}"));
        let search = query(&log[1]);
        assert!(search.contains(&("term".into(), "Synthetic & Show".into())));
        assert!(search.contains(&("media".into(), "podcast".into())));
        assert!(search.contains(&("limit".into(), "10".into())));
    }

    #[test]
    fn spotify_exclusive_without_matching_feed_reports_no_public_feed() {
        let (result, _) = resolve(&PodcastSource::SpotifyShow { id: SHOW.into() }, |url| {
            if url.host_str() == Some("open.spotify.com") {
                (200, r#"<meta property="og:title" content="Exclusive Show">"#)
            } else {
                (
                    200,
                    r#"{"results":[{"collectionName":"Another Show","feedUrl":"https://feeds.example.test/other.xml"}]}"#,
                )
            }
        });
        assert_eq!(result, Err(PodcastError::NoPublicFeed));
    }

    #[test]
    fn unknown_spotify_show_and_malformed_responses_are_errors() {
        let spotify = PodcastSource::SpotifyShow { id: SHOW.into() };
        assert_eq!(resolve(&spotify, |_| (404, "")).0, Err(PodcastError::Music(MusicError::NotFound)));
        let apple = PodcastSource::ApplePodcasts { id: "1".into() };
        assert_eq!(resolve(&apple, |_| (200, "<html>")).0, Err(PodcastError::Music(MusicError::InvalidResponse)));
        assert_eq!(resolve(&spotify, |_| (200, "<html>")).0, Err(PodcastError::Music(MusicError::InvalidResponse)));
        assert_eq!(check_status(500), Err(MusicError::ServerUnavailable));
    }

    #[test]
    fn entity_decoding_keeps_unknown_and_unterminated_references() {
        assert_eq!(decode_entities("A &amp; B &#233;t&#xE9; &bogus; R&D &lt;"), "A & B été &bogus; R&D <");
        assert_eq!(decode_entities("tail &"), "tail &");
    }

    #[test]
    fn directory_search_sends_only_the_term() {
        let url = itunes_search_url("science", SEARCH_LIMIT);
        assert_eq!(
            query(&url),
            pairs(&[("term", "science"), ("media", "podcast"), ("entity", "podcast"), ("limit", "25")])
        );
        let found = parse_itunes(
            br#"{"results":[{"collectionName":"Synthetic Science","feedUrl":"https://feeds.example.test/science"}]}"#,
        );
        assert_eq!(found.unwrap()[0].title, "Synthetic Science");
        let empty = block_on(PodcastDirectory::new().search("   "));
        assert_eq!(empty, Ok(Vec::new()));
    }

    #[test]
    fn errors_have_messages() {
        assert!(!PodcastError::NoPublicFeed.message().is_empty());
        assert!(PodcastError::Music(MusicError::Forbidden).message().contains("podcasts"));
    }

    fn channels() -> Vec<PodcastChannel> {
        serde_json::from_str(
            r#"[{"id":"c1","title":"Synthetic Science","description":"Weekly physics talk","url":"https://feeds.example.test/science/","episode":[
            {"id":"e1","title":"Black holes","streamId":"s1","status":"completed","publishDate":"2026-01-01T00:00:00Z"},
            {"id":"e2","title":"Black holes, part two","streamId":"s2","status":"completed","publishDate":"2026-02-01T00:00:00Z"},
            {"id":"e3","title":"Black holes draft","status":"downloading","publishDate":"2026-03-01T00:00:00Z"}]},
         {"id":"c2","title":"Città e cucina","episode":[{"id":"e4","title":"Pasta","streamId":"s4","status":"completed","publishDate":"2026-01-15T00:00:00Z"}]}]"#,
        )
        .unwrap()
    }

    fn ids<T>(items: &[T], id: impl Fn(&T) -> &str) -> Vec<String> {
        items.iter().map(|i| id(i).to_string()).collect()
    }

    #[test]
    fn matches_channels_by_title_or_description_and_playable_episodes_newest_first() {
        let results = search_subscriptions("black HOLES", &channels(), EPISODE_SEARCH_LIMIT);
        assert!(results.channels.is_empty());
        assert_eq!(ids(&results.episodes, |s| &s.id), ["podcast:c1:e2", "podcast:c1:e1"]);
        assert_eq!(results.episodes[0].album.as_deref(), Some("Synthetic Science"));
        assert_eq!(ids(&search_subscriptions("physics", &channels(), 50).channels, |c| &c.id), ["c1"]);
    }

    #[test]
    fn every_word_must_match_ignoring_accents_and_channel_title_finds_its_episodes() {
        let results = search_subscriptions("citta pasta", &channels(), 50);
        assert!(results.channels.is_empty());
        assert_eq!(ids(&results.episodes, |s| &s.id), ["podcast:c2:e4"]);
        assert_eq!(ids(&search_subscriptions("citta", &channels(), 50).channels, |c| &c.id), ["c2"]);
        assert!(search_subscriptions("  ", &channels(), 50).is_empty());
        assert_eq!(search_subscriptions("black", &channels(), 1).episodes.len(), 1);
    }

    #[test]
    fn subscribed_feed_ignores_scheme_case_and_trailing_slash() {
        assert!(is_subscribed("http://FEEDS.example.test/science", &channels()));
        assert!(!is_subscribed("https://feeds.example.test/other", &channels()));
    }
}
