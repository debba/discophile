//! Audiobookshelf audiobooks: the connection, browsing, book details, downloads (desktop), reader
//! preferences and bookmarks, position sync, and playback on the shared audio output.
//!
//! While `app.player.mode` is `Audiobook`, the player forwards output events to
//! [`on_output_event`] and calls [`tick`] every frame; [`now_playing`] and [`seek`] give the player
//! bar book-level times.

pub mod account;
pub mod covers;
#[cfg(not(target_arch = "wasm32"))]
pub mod downloads;
pub mod library;
pub mod playback;
pub mod reader;
pub mod sync;

use discophile_core::audiobookshelf::{AudiobookshelfAccount, AudiobookshelfClient};

use crate::app::App;
use crate::audio::AudioEvent;
use crate::player::Mode;

/// Downloads are a desktop feature; the web build keeps an always-empty catalog.
#[cfg(target_arch = "wasm32")]
pub mod downloads {
    use discophile_core::offline_audiobook::OfflineAudiobook;

    use crate::app::App;

    #[derive(Default)]
    pub struct Downloads {
        pub records: Vec<OfflineAudiobook>,
        pub error: Option<String>,
    }

    pub fn load(_app: &mut App) {}
    pub fn ready_record(_app: &App, _book: &str) -> Option<OfflineAudiobook> {
        None
    }
    pub fn remove_account(_app: &mut App, _account: &str) {}
}

/// UI state of the embedded audiobook player that outlives a frame.
#[derive(Default)]
pub struct PlayerUi {
    /// The timeline window and offset being dragged.
    pub scrub: Option<(discophile_core::audiobook_reader::TimelineWindow, f64)>,
    pub show_bookmarks: bool,
    /// Bookmark being renamed, with the edited title.
    pub renaming: Option<(uuid::Uuid, String)>,
    /// "Go to time" window: hours, minutes, seconds.
    pub jump: Option<(u32, u32, u32)>,
    /// "Custom timer" window: minutes.
    pub custom_timer: Option<u32>,
    /// Time when "Bookmark saved" was confirmed (egui seconds).
    pub bookmark_saved: Option<f64>,
}

/// Confirmation dialogs.
#[derive(Clone, Debug, PartialEq)]
pub enum Dialog {
    Disconnect,
    RemoveDownloads,
    RemoveDownload(String),
    /// The Connect Audiobookshelf window, opened from the Audiobooks page or Settings.
    Connect,
}

#[derive(Default)]
pub struct AudiobooksState {
    pub account: Option<AudiobookshelfAccount>,
    pub client: Option<AudiobookshelfClient>,
    /// The saved connection could not be read or needs signing in again.
    pub error: Option<String>,
    pub sync: sync::Sync,
    pub reader: reader::Reader,
    pub playback: Option<playback::Playback>,
    pub browse: library::Browse,
    pub detail: Option<library::Detail>,
    pub downloads: downloads::Downloads,
    pub covers: covers::Covers,
    pub form: account::ConnectForm,
    pub player_ui: PlayerUi,
    pub dialog: Option<Dialog>,
    /// A book being prepared for streaming, with its start generation.
    pub starting: Option<(String, u64)>,
    start_generation: u64,
}

pub fn start(app: &mut App) {
    app.audiobooks.reader = reader::Reader::load();
    app.audiobooks.sync = sync::Sync::load();
    app.audiobooks.browse = library::Browse::load();
    account::load(app);
    downloads::load(app);
    if app.audiobooks.client.is_some() {
        sync::retry_pending(app, true);
    }
}

pub fn tick(app: &mut App) {
    playback::tick(app);
}

pub fn windows(app: &mut App, ctx: &egui::Context) {
    crate::ui::audiobooks::windows(app, ctx);
}

pub fn on_library_changed(_app: &mut App) {}

/// Every frame, whatever plays: retries unsent positions.
pub fn tick_always(app: &mut App) {
    sync::tick(app);
}

/// Other content replaces the audiobook; save its position.
pub fn on_replaced(app: &mut App) {
    playback::replaced(app);
}

pub fn skip_forward(app: &mut App) {
    app.player.control_generation += 1;
    playback::skip(app, true);
}

pub fn skip_backward(app: &mut App) {
    app.player.control_generation += 1;
    playback::skip(app, false);
}

pub fn on_output_event(app: &mut App, event: AudioEvent) {
    playback::on_output_event(app, event);
}

/// Whether an Audiobookshelf account is connected.
pub fn has_account(app: &App) -> bool {
    app.audiobooks.account.is_some()
}

/// The playing book for the player bar and system media controls:
/// title, author, book position and book duration in seconds.
pub fn now_playing(app: &App) -> Option<(String, String, f64, f64)> {
    if app.player.mode != Mode::Audiobook {
        return None;
    }
    let pb = app.audiobooks.playback.as_ref()?;
    Some((pb.book.title().to_string(), pb.book.author(), pb.position, pb.duration()))
}

/// The playing book's id, to open its page from the player bar (`Route::Audiobook(id)`).
pub fn playing_book(app: &App) -> Option<String> {
    app.audiobooks.playback.as_ref().filter(|_| app.player.mode == Mode::Audiobook).map(|pb| pb.book.id.clone())
}

/// The current chapter's title, for a subtitle in the player bar.
pub fn current_chapter(app: &App) -> Option<String> {
    app.audiobooks.playback.as_ref().and_then(playback::Playback::chapter_title)
}

/// Seeks on the whole book's timeline (`seconds` as reported by `now_playing`), across files.
pub fn seek(app: &mut App, seconds: f64) {
    app.player.control_generation += 1;
    playback::seek(app, seconds);
}

/// Pauses the book, including a file that is still being prepared.
pub fn pause(app: &mut App) {
    app.player.control_generation += 1;
    playback::pause(app);
}

/// Play/pause that also restarts a finished book and reloads a failed file.
pub fn toggle(app: &mut App) {
    app.player.control_generation += 1;
    playback::toggle(app);
}

/// The cover of the playing book, when loaded.
pub fn cover(app: &mut App) -> Option<egui::TextureHandle> {
    let id = playing_book(app)?;
    covers::texture(app, &id)
}

/// Streams or plays a downloaded book from `position`, or from its saved position.
pub fn start_book(app: &mut App, id: &str, position: Option<f64>) {
    start_book_with_autoplay(app, id, position, true);
}

fn start_book_with_autoplay(app: &mut App, id: &str, position: Option<f64>, autoplay: bool) {
    let Some(account) = app.audiobooks.account.clone() else { return };
    if app.audiobooks.starting.is_some() {
        return;
    }
    let account_id = account.id();
    if let Some(record) = downloads::ready_record(app, id) {
        let saved = app.audiobooks.sync.saved(&account_id, id);
        let resume = position.unwrap_or_else(|| saved.map(|p| if p.is_finished { 0.0 } else { p.current_time }).unwrap_or(0.0));
        playback::begin(app, record.book, record.playback, account_id, account.connection_id, true, resume, autoplay);
        return;
    }
    let Some(book) = library::known_book(app, id) else { return };
    app.audiobooks.start_generation += 1;
    let generation = app.audiobooks.start_generation;
    app.audiobooks.starting = Some((id.to_string(), generation));
    if let Some(detail) = app.audiobooks.detail.as_mut().filter(|d| d.id == id) {
        detail.error = None;
    }
    sync::prepare(app, sync::StartRequest { account: account_id, connection: account.connection_id, book, position, generation, autoplay });
}

/// Handoff can address a book that this computer has never browsed.
#[cfg(not(target_arch = "wasm32"))]
pub fn start_transferred(app: &mut App, id: &str, position: f64, autoplay: bool) -> bool {
    if id.is_empty() || id.len() > 1000 || app.audiobooks.starting.is_some() { return false; }
    if library::known_book(app, id).is_some() || downloads::ready_record(app, id).is_some() {
        start_book_with_autoplay(app, id, Some(position), autoplay);
        return true;
    }
    let Some(account) = app.audiobooks.account.clone() else { return false };
    let Some(client) = app.audiobooks.client.clone() else { return false };
    app.audiobooks.start_generation += 1;
    let generation = app.audiobooks.start_generation;
    let id = id.to_string();
    app.audiobooks.starting = Some((id.clone(), generation));
    app.tasks.spawn(async move {
        let result = client.book(&id).await;
        crate::tasks::update(move |app: &mut App| {
            if app.audiobooks.starting.as_ref().is_none_or(|(_, g)| *g != generation) { return; }
            if app.audiobooks.account.as_ref().is_none_or(|a| a.connection_id != account.connection_id)
                || !crate::features::connect::services::audiobook_start_allowed(app) {
                app.audiobooks.starting = None;
                return;
            }
            match result {
                Ok(book) if book.id == id => sync::prepare(app, sync::StartRequest {
                    account: account.id(), connection: account.connection_id, book, position: Some(position), generation, autoplay,
                }),
                _ => {
                    app.audiobooks.starting = None;
                    crate::features::connect::services::audiobook_failed(app);
                }
            }
        })
    });
    true
}

/// Whether `id` is the book in the player.
pub fn cancel_pending_start(app: &mut App) {
    app.audiobooks.starting = None;
    app.audiobooks.start_generation += 1;
}

pub fn is_current(app: &App, id: &str) -> bool {
    playing_book(app).as_deref() == Some(id)
}
