//! Library search: debounced `search3`, podcast title and episode matches from an index loaded in
//! parallel, a local fallback over downloads, and the on-demand Apple Podcasts directory search.

use discophile_core::client::SearchLimits;
use discophile_core::listening_history::fold;
use discophile_core::podcast_directory::{self, EPISODE_SEARCH_LIMIT, PodcastCandidate, PodcastDirectory, PodcastSearchResults};
use discophile_core::{SearchResults, Song, l10n};

use crate::app::App;
use crate::tasks::update;

/// Typing pauses this long before the server is searched.
const DEBOUNCE_SECONDS: f64 = 0.3;

/// Filter chips; changing them only hides sections and never repeats the search.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SearchScope {
    #[default]
    All,
    Library,
    Artists,
    Albums,
    Songs,
    Podcasts,
}

impl SearchScope {
    pub const ALL: [SearchScope; 6] = [Self::All, Self::Library, Self::Artists, Self::Albums, Self::Songs, Self::Podcasts];
    pub fn title(self) -> String {
        l10n::t(match self {
            Self::All => "All",
            Self::Library => "Library",
            Self::Artists => "Artists",
            Self::Albums => "Albums",
            Self::Songs => "Songs",
            Self::Podcasts => "Podcasts",
        })
    }
    pub fn includes_artists(self) -> bool {
        matches!(self, Self::All | Self::Library | Self::Artists)
    }
    pub fn includes_albums(self) -> bool {
        matches!(self, Self::All | Self::Library | Self::Albums)
    }
    pub fn includes_songs(self) -> bool {
        matches!(self, Self::All | Self::Library | Self::Songs)
    }
    pub fn includes_podcasts(self) -> bool {
        matches!(self, Self::All | Self::Podcasts)
    }
}

#[derive(Default)]
pub struct SearchState {
    pub scope: SearchScope,
    /// Term, filter and library the current results belong to.
    key: String,
    changed_at: f64,
    pending: bool,
    sequence: u64,
    pub searching: bool,
    pub term: String,
    pub results: SearchResults,
    pub podcasts: PodcastSearchResults,
    pub error: Option<String>,
    /// Apple Podcasts directory results, only after the user asks.
    pub directory: Option<Vec<PodcastCandidate>>,
    pub searching_directory: bool,
    pub directory_error: Option<String>,
    /// Feed being added from the directory.
    pub adding: Option<String>,
}

impl SearchState {
    /// Forces a new search on the next frame (filter or library changed).
    pub fn invalidate(&mut self) {
        self.key.clear();
    }

    pub fn has_visible_results(&self) -> bool {
        let scope = self.scope;
        (scope.includes_artists() && !self.results.artists().is_empty())
            || (scope.includes_albums() && !self.results.albums().is_empty())
            || (scope.includes_songs() && !self.results.songs().is_empty())
            || (scope.includes_podcasts() && !self.podcasts.is_empty())
    }
}

/// Called every frame by the Search page: debounces typing and starts searches.
pub fn drive(app: &mut App) {
    if app.nav.changed {
        prefetch_podcast_index(app);
    }
    let term = app.nav.search_text.trim().to_string();
    let key = format!("{}\n{}\n{}", app.session.generation, app.settings.downloaded_only, term);
    let now = app.ctx().input(|i| i.time);
    let search = &mut app.library.search;
    if key != search.key {
        let term_changed = search.term != term;
        search.key = key;
        search.changed_at = now;
        search.sequence += 1;
        search.error = None;
        if term_changed {
            search.directory = None;
            search.directory_error = None;
            search.searching_directory = false;
        }
        if term.is_empty() {
            search.pending = false;
            search.searching = false;
            search.term.clear();
            search.results = SearchResults::default();
            search.podcasts = PodcastSearchResults::default();
            return;
        }
        search.pending = true;
        search.searching = true;
    }
    if !search.pending {
        return;
    }
    let wait = DEBOUNCE_SECONDS - (now - search.changed_at);
    if wait > 0.0 {
        app.ctx().request_repaint_after(std::time::Duration::from_secs_f64(wait));
        return;
    }
    search.pending = false;
    start(app, term);
}

fn start(app: &mut App, term: String) {
    let sequence = app.library.search.sequence;
    let generation = app.session.generation;

    if app.settings.downloaded_only {
        let results = local_search(app, &term);
        let podcasts = if super::supports_podcasts(app) { downloaded_podcast_search(app, &term) } else { PodcastSearchResults::default() };
        return finish(app, term, results, podcasts, None);
    }
    let Some(client) = app.client() else { return };
    // Runs alongside search3; podcast matches join the results whenever the index is ready.
    prefetch_podcast_index(app);
    let fallback = local_search(app, &term);
    app.tasks.spawn(async move {
        let result = client.search(&term, SearchLimits::default()).await;
        update(move |app: &mut App| {
            if app.session.generation != generation || app.library.search.sequence != sequence {
                return;
            }
            let podcasts = podcast_matches(app, &term);
            match result {
                Ok(results) => finish(app, term, results, podcasts, None),
                Err(_) if !fallback.is_empty() => finish(app, term, fallback, podcasts, None),
                Err(error) => finish(app, term, SearchResults::default(), podcasts, Some(l10n::music_error(error))),
            }
        })
    });
}

/// Loads subscribed channels with their episodes in the background, so searching only filters
/// them. Called when the Search page opens and when a search starts; a failure is retried then.
pub fn prefetch_podcast_index(app: &mut App) {
    if !super::supports_podcasts(app) || app.settings.downloaded_only || app.library.podcast_index_loading || super::podcast_index_is_fresh(app) {
        return;
    }
    let Some(client) = app.client() else { return };
    app.library.podcast_index_loading = true;
    let (generation, epoch) = (app.session.generation, app.library.podcast_index_epoch);
    app.tasks.spawn(async move {
        let result = client.podcasts(None, Some(true)).await;
        update(move |app: &mut App| {
            if app.session.generation != generation || app.library.podcast_index_epoch != epoch {
                return;
            }
            app.library.podcast_index_loading = false;
            // Podcast matches are optional: servers without podcasts still show music results.
            if let Ok(channels) = result {
                app.library.podcast_index = Some((channels, discophile_core::now_millis()));
                let search = &app.library.search;
                if !search.term.is_empty() && !search.searching && !app.settings.downloaded_only {
                    let term = search.term.clone();
                    app.library.search.podcasts = podcast_matches(app, &term);
                }
            }
        })
    });
}

/// Podcast matches still on their way: the index is loading and the shown results lack them.
pub fn podcasts_pending(app: &App) -> bool {
    app.library.podcast_index_loading && !app.settings.downloaded_only && app.library.search.scope.includes_podcasts()
}

fn podcast_matches(app: &App, term: &str) -> PodcastSearchResults {
    match &app.library.podcast_index {
        Some((channels, _)) => podcast_directory::search_subscriptions(term, channels, EPISODE_SEARCH_LIMIT),
        None => PodcastSearchResults::default(),
    }
}

fn finish(app: &mut App, term: String, results: SearchResults, podcasts: PodcastSearchResults, error: Option<String>) {
    let search = &mut app.library.search;
    search.term = term;
    search.results = results;
    search.podcasts = podcasts;
    search.error = error;
    search.searching = false;
}

/// Retries the current search right away.
pub fn retry(app: &mut App) {
    app.library.search.invalidate();
}

fn matches_all(words: &[String], fields: &[Option<&str>]) -> bool {
    let text = fold(&fields.iter().flatten().copied().collect::<Vec<_>>().join(" "));
    words.iter().all(|w| text.contains(w.as_str()))
}

/// Searches downloaded albums and songs, as the iOS app does offline.
fn local_search(app: &mut App, term: &str) -> SearchResults {
    let words: Vec<String> = term.split_whitespace().map(fold).collect();
    if words.is_empty() {
        return SearchResults::default();
    }
    let albums =
        super::downloaded_albums(app).into_iter().filter(|a| matches_all(&words, &[Some(a.name.as_str()), a.artist.as_deref()])).collect();
    let songs = super::downloaded_songs(app)
        .into_iter()
        .filter(|s| !s.is_podcast() && matches_all(&words, &[Some(s.title.as_str()), s.artist.as_deref(), s.album.as_deref()]))
        .collect();
    SearchResults::new(Vec::new(), albums, songs)
}

fn downloaded_podcast_search(app: &mut App, term: &str) -> PodcastSearchResults {
    let words: Vec<String> = term.split_whitespace().map(fold).collect();
    let saved: Vec<Song> = super::downloaded_songs(app).into_iter().filter(Song::is_podcast).collect();
    let channels: Vec<_> = app
        .library
        .snapshot
        .podcasts
        .iter()
        .filter(|c| {
            let album = format!("podcast:{}", c.id);
            saved.iter().any(|s| s.album_id.as_deref() == Some(album.as_str()))
        })
        .cloned()
        .collect();
    let episodes = saved.into_iter().filter(|s| matches_all(&words, &[Some(s.title.as_str()), s.album.as_deref()])).collect();
    PodcastSearchResults { channels: podcast_directory::search_subscriptions(term, &channels, 0).channels, episodes }
}

/// Searches the public Apple Podcasts directory for the current term. Only the term is sent.
pub fn search_directory(app: &mut App) {
    let term = app.library.search.term.trim().to_string();
    if term.is_empty() {
        return;
    }
    let search = &mut app.library.search;
    search.searching_directory = true;
    search.directory_error = None;
    let generation = app.session.generation;
    app.tasks.spawn(async move {
        let result = PodcastDirectory::new().search(&term).await;
        update(move |app: &mut App| {
            let search = &mut app.library.search;
            if app.session.generation != generation || search.term != term {
                return;
            }
            search.searching_directory = false;
            match result {
                Ok(found) => search.directory = Some(found),
                Err(error) => search.directory_error = Some(error.message()),
            }
        })
    });
}

/// Adds a directory result to the server's podcasts.
pub fn add_directory_podcast(app: &mut App, candidate: &PodcastCandidate) {
    let id = candidate.id().to_string();
    app.library.search.adding = Some(id);
    app.library.search.directory_error = None;
    super::add_podcast(app, candidate.feed_url.clone(), |app, result| {
        app.library.search.adding = None;
        if let Err(error) = result {
            app.library.search.directory_error = Some(discophile_core::podcast_directory::PodcastError::Music(error).message());
        }
    });
}

pub fn is_subscribed(app: &App, candidate: &PodcastCandidate) -> bool {
    podcast_directory::is_subscribed(candidate.feed_url.as_str(), &app.library.snapshot.podcasts)
}
