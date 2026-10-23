//! Browsing Audiobookshelf libraries (paging and search) and loading a book's details.

use discophile_core::audiobookshelf::{Audiobook, AudiobookLibrary, AudiobookProgress};
use discophile_core::l10n::audiobook_error;
use discophile_core::{MusicError, now_millis};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::app::App;
use crate::platform::storage;
use crate::tasks::update;

const BROWSE_KEY: &str = "audiobooks/browse";
/// Search waits for typing to pause.
const SEARCH_DELAY_MS: i64 = 350;

#[derive(Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Saved {
    downloaded_only: bool,
    library: String,
}

#[derive(Default)]
pub struct Browse {
    pub libraries: Vec<AudiobookLibrary>,
    pub selected: String,
    pub books: Vec<Audiobook>,
    pub query: String,
    pub page: usize,
    pub total: usize,
    pub loading: bool,
    pub error: Option<String>,
    /// The Audiobooks tab's own "Downloaded only" filter, separate from music.
    pub downloaded_only: bool,
    revision: u64,
    /// Connection whose libraries are loaded.
    loaded: Option<Uuid>,
    query_changed: Option<i64>,
}

impl Browse {
    pub fn load() -> Self {
        let saved: Saved = storage::load(BROWSE_KEY).unwrap_or_default();
        Self { downloaded_only: saved.downloaded_only, selected: saved.library, ..Default::default() }
    }

    fn save(&self) {
        storage::save(BROWSE_KEY, &Saved { downloaded_only: self.downloaded_only, library: self.selected.clone() });
    }

    /// Forgets loaded content after the connection changes.
    pub fn reset(&mut self) {
        let downloaded_only = self.downloaded_only;
        *self = Self { downloaded_only, revision: self.revision + 1, ..Default::default() };
    }

    pub fn search_text(&self) -> &str {
        self.query.trim()
    }
}

fn connection(app: &App) -> Option<Uuid> {
    app.audiobooks.account.as_ref().map(|a| a.connection_id)
}

/// Loads libraries and the first page when the page opens or the connection changed.
pub fn ensure_loaded(app: &mut App) {
    let browse = &app.audiobooks.browse;
    if browse.downloaded_only || browse.loading || browse.error.is_some() || app.audiobooks.client.is_none() {
        return;
    }
    if browse.loaded != connection(app) {
        refresh(app);
    }
}

/// Every frame on the Audiobooks page: runs a search once typing pauses.
pub fn tick(app: &mut App) {
    if let Some(changed) = app.audiobooks.browse.query_changed {
        let wait = changed + SEARCH_DELAY_MS - now_millis();
        if wait <= 0 {
            app.audiobooks.browse.query_changed = None;
            load_books(app);
        } else {
            app.ctx().request_repaint_after(std::time::Duration::from_millis(wait as u64));
        }
    }
}

pub fn query_changed(app: &mut App) {
    app.audiobooks.browse.query_changed = Some(now_millis());
}

pub fn set_downloaded_only(app: &mut App, value: bool) {
    let browse = &mut app.audiobooks.browse;
    browse.downloaded_only = value;
    browse.save();
    if value {
        browse.revision += 1;
        browse.loading = false;
        browse.error = None;
    } else {
        refresh(app);
    }
}

pub fn select_library(app: &mut App, id: String) {
    let browse = &mut app.audiobooks.browse;
    if browse.selected == id {
        return;
    }
    browse.selected = id;
    browse.save();
    load_books(app);
}

/// Reloads the libraries, then the selected library's first page.
pub fn refresh(app: &mut App) {
    let Some(client) = app.audiobooks.client.clone() else {
        let browse = &mut app.audiobooks.browse;
        browse.books.clear();
        browse.libraries.clear();
        return;
    };
    let connection = client.account().connection_id;
    let browse = &mut app.audiobooks.browse;
    browse.loaded = Some(connection);
    if browse.downloaded_only {
        browse.loading = false;
        browse.error = None;
        return;
    }
    browse.error = None;
    browse.loading = true;
    browse.revision += 1;
    let revision = browse.revision;
    app.tasks.spawn(async move {
        let result = client.libraries().await;
        update(move |app| {
            let browse = &mut app.audiobooks.browse;
            if browse.revision != revision || app.audiobooks.account.as_ref().map(|a| a.connection_id) != Some(connection) {
                return;
            }
            match result {
                Ok(libraries) => {
                    if !libraries.iter().any(|l| l.id == browse.selected) {
                        browse.selected = libraries.first().map(|l| l.id.clone()).unwrap_or_default();
                        browse.save();
                    }
                    browse.libraries = libraries;
                    load_books(app);
                }
                Err(error) => {
                    browse.error = Some(audiobook_error(error));
                    browse.loading = false;
                }
            }
        })
    });
}

/// The first page of the selected library, or search results.
pub fn load_books(app: &mut App) {
    let client = app.audiobooks.client.clone();
    let browse = &mut app.audiobooks.browse;
    browse.revision += 1;
    browse.query_changed = None;
    if browse.downloaded_only {
        browse.loading = false;
        browse.error = None;
        return;
    }
    let Some(client) = client.filter(|_| !browse.selected.is_empty()) else {
        browse.books.clear();
        browse.loading = false;
        return;
    };
    browse.loading = true;
    browse.error = None;
    browse.books.clear();
    browse.page = 0;
    browse.total = 0;
    let revision = browse.revision;
    let library = browse.selected.clone();
    let search = browse.search_text().to_string();
    app.tasks.spawn(async move {
        let result = if search.is_empty() {
            client.books(&library, 0).await.map(|page| (page.results, page.total.max(0) as usize))
        } else {
            client.search(&search, &library).await.map(|books| {
                let count = books.len();
                (books, count)
            })
        };
        update(move |app| {
            let browse = &mut app.audiobooks.browse;
            if browse.revision != revision {
                return;
            }
            browse.loading = false;
            match result {
                Ok((books, total)) => {
                    let mut seen = std::collections::HashSet::new();
                    browse.books = books.into_iter().filter(|b| b.media_type == "book" && seen.insert(b.id.clone())).collect();
                    browse.total = total;
                }
                Err(error) => browse.error = Some(audiobook_error(error)),
            }
        })
    });
}

pub fn can_load_more(app: &App) -> bool {
    let browse = &app.audiobooks.browse;
    browse.search_text().is_empty() && browse.books.len() < browse.total && !browse.downloaded_only
}

pub fn load_more(app: &mut App) {
    let Some(client) = app.audiobooks.client.clone() else { return };
    let browse = &mut app.audiobooks.browse;
    if browse.loading {
        return;
    }
    browse.loading = true;
    let revision = browse.revision;
    let (library, page) = (browse.selected.clone(), browse.page + 1);
    app.tasks.spawn(async move {
        let result = client.books(&library, page).await;
        update(move |app| {
            let browse = &mut app.audiobooks.browse;
            if browse.revision != revision {
                return;
            }
            browse.loading = false;
            match result {
                Ok(result) => {
                    let existing: std::collections::HashSet<String> = browse.books.iter().map(|b| b.id.clone()).collect();
                    let additions: Vec<Audiobook> =
                        result.results.into_iter().filter(|b| b.media_type == "book" && !existing.contains(&b.id)).collect();
                    let empty = additions.is_empty();
                    browse.books.extend(additions);
                    browse.page = page;
                    browse.total = if empty { browse.books.len() } else { result.total.max(0) as usize };
                }
                Err(error) => browse.error = Some(audiobook_error(error)),
            }
        })
    });
}

// MARK: Book details

pub struct Detail {
    pub id: String,
    /// The expanded item, with chapters and tracks.
    pub book: Option<Audiobook>,
    pub progress: Option<AudiobookProgress>,
    pub loading: bool,
    pub error: Option<String>,
    connection: Option<Uuid>,
}

/// Any copy of a book we already have: details, the playing book, a download or a catalog page.
pub fn known_book(app: &App, id: &str) -> Option<Audiobook> {
    let state = &app.audiobooks;
    if let Some(book) = state.detail.as_ref().filter(|d| d.id == id).and_then(|d| d.book.clone()) {
        return Some(book);
    }
    if let Some(pb) = state.playback.as_ref().filter(|p| p.book.id == id) {
        return Some(pb.book.clone());
    }
    if let Some(record) = state.downloads.records.iter().find(|r| r.book.id == id) {
        return Some(record.book.clone());
    }
    state.browse.books.iter().find(|b| b.id == id).cloned()
}

/// Loads a book's details unless they are loaded for this connection.
pub fn ensure_detail(app: &mut App, id: &str) {
    let current = connection(app);
    if app.audiobooks.detail.as_ref().is_some_and(|d| d.id == id && d.connection == current) {
        return;
    }
    load_detail(app, id);
}

pub fn load_detail(app: &mut App, id: &str) {
    let Some(account) = app.audiobooks.account.clone() else { return };
    let account_id = account.id();
    let client = app.audiobooks.client.clone();
    let progress = app.audiobooks.sync.saved(&account_id, id);
    let mut detail =
        Detail { id: id.to_string(), book: None, progress, loading: false, error: None, connection: Some(account.connection_id) };
    let downloaded = super::downloads::ready_record(app, id);
    if let Some(record) = downloaded {
        // Downloaded books open offline; the server position is fetched when reachable.
        detail.book = Some(record.book);
        app.audiobooks.detail = Some(detail);
        let Some(client) = client else { return };
        let id = id.to_string();
        app.tasks.spawn(async move {
            let remote = client.progress(&id).await;
            update(move |app| {
                if let Ok(Some(remote)) = remote {
                    super::sync::remember(app, &account_id, &id, remote);
                }
                refresh_progress(app, &id);
            })
        });
        return;
    }
    let Some(client) = client else {
        detail.error = Some(audiobook_error(MusicError::Authentication));
        app.audiobooks.detail = Some(detail);
        return;
    };
    detail.loading = true;
    app.audiobooks.detail = Some(detail);
    let id = id.to_string();
    let connection = account.connection_id;
    app.tasks.spawn(async move {
        let result = async {
            let book = client.book(&id).await?;
            let progress = client.progress(&id).await?;
            Ok::<_, MusicError>((book, progress))
        }
        .await;
        update(move |app| {
            let matches = app.audiobooks.detail.as_ref().is_some_and(|d| d.id == id && d.connection == Some(connection));
            if !matches {
                return;
            }
            match result {
                Ok((book, remote)) => {
                    if let Some(remote) = remote {
                        super::sync::remember(app, &account_id, &id, remote);
                    }
                    if let Some(detail) = app.audiobooks.detail.as_mut() {
                        detail.book = Some(book);
                        detail.loading = false;
                    }
                    refresh_progress(app, &id);
                    // Opening an online book retries positions that are still waiting.
                    super::sync::retry_pending(app, false);
                }
                Err(error) => {
                    if let Some(detail) = app.audiobooks.detail.as_mut() {
                        detail.loading = false;
                        detail.error = Some(audiobook_error(error));
                    }
                }
            }
        })
    });
}

/// Updates the detail page's saved position from the local archive.
pub fn refresh_progress(app: &mut App, id: &str) {
    let Some(account) = app.audiobooks.account.as_ref().map(|a| a.id()) else { return };
    let saved = app.audiobooks.sync.saved(&account, id);
    if let Some(detail) = app.audiobooks.detail.as_mut().filter(|d| d.id == id) {
        detail.progress = saved;
    }
}
