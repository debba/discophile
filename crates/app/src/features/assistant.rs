//! The music assistant: per-library chats (core `AssistantChatStore`), replies that keep running while the
//! assistant is hidden, reviewed library edits, curated selections ("Ask your curator") and the ChatGPT
//! and local model connections.
//!
//! Chats are saved per library under `library/<account>/assistant` (drafts with a short debounce). A reply
//! that finishes in a chat that is not on screen marks it unread (the toolbar badge) and posts a system
//! notification (the browser's Notification API on the web).
//!
//! Desktop differences from iOS: reopening the side panel keeps the selected chat (Edit with assistant and
//! curator prompts still start a new one, reusing an untouched empty chat); "On this iPhone" becomes a
//! local OpenAI-compatible model server; dictation is not offered because desktop platforms have no
//! portable on-device speech recognition.

mod notify;
mod providers;
mod reply;

use discophile_core::assistant::turn;
use discophile_core::assistant::{AssistantChatStore, AssistantMessage, AssistantProvider};
use discophile_core::{Playlist, Song, l10n};
use uuid::Uuid;

use crate::app::App;
use crate::features::discovery;
use crate::nav::{Route, SidePanel};
use crate::platform::storage;
use crate::player::Mode;
use crate::tasks::update;

#[cfg(not(target_arch = "wasm32"))]
pub use providers::sign_in;
pub use providers::{
    AssistantConfig, ChatGptState, LocalState, cancel_sign_in, has_local_api_key, load_models, provider_name, save_config,
    save_local_api_key, set_default_model, set_local_base_url, set_local_model, sign_out, test_local, unavailable_reason,
};
pub use reply::{
    CuratorState, RunKind, answering, cancel, cancel_curator, cancel_edit, confirm_edit, curate, is_busy, save_playlist, send,
};

/// Debounce for draft saves, as on iOS.
const DRAFT_SAVE_SECONDS: f64 = 0.4;

/// "Create playlist" / "Review playlist": the name and tracks before saving to the server.
pub struct PlaylistEditor {
    pub conversation: Uuid,
    pub message: Uuid,
    pub title: String,
    pub songs: Vec<Song>,
    pub error: Option<String>,
}

/// Edits of a saved selection page (name and track order) before they are saved.
pub struct SelectionEdit {
    pub id: Uuid,
    pub title: String,
    pub songs: Vec<Song>,
    /// The saved selection the edits started from, to pick up changes made in a chat.
    pub base_title: String,
    pub base_songs: Vec<Song>,
    pub saving: bool,
    pub notice: bool,
    pub error: Option<String>,
    pub confirm_delete: bool,
}

/// Interface state of the assistant windows and pages.
#[derive(Default)]
pub struct AssistantUi {
    pub history: bool,
    pub history_search: String,
    pub rename: Option<(Uuid, String)>,
    pub delete: Option<Uuid>,
    /// Message with the edit proposal under review, in the selected chat.
    pub review: Option<Uuid>,
    pub editor: Option<PlaylistEditor>,
    pub privacy: bool,
    pub focus_composer: bool,
    pub scroll_to_bottom: bool,
    /// Loading a playlist before opening it in a chat.
    pub focusing: bool,
    pub selection: Option<SelectionEdit>,
}

#[derive(Default)]
pub struct AssistantState {
    pub store: AssistantChatStore,
    /// Account whose chats are loaded.
    account: String,
    /// When the debounced save is due (egui time, seconds).
    save_due: Option<f64>,
    /// Chats could not be read or saved.
    pub error: Option<String>,
    running: Option<reply::Running>,
    revision: u64,
    pub curator: CuratorState,
    pub config: AssistantConfig,
    pub chatgpt: ChatGptState,
    pub local: LocalState,
    pub ui: AssistantUi,
}

fn store_key(account: &str) -> String {
    format!("library/{account}/assistant")
}

pub fn start(app: &mut App) {
    providers::load(app);
    load_store(app);
}

fn load_store(app: &mut App) {
    let account = app.session.account_key();
    let state = &mut app.assistant;
    state.account = account.clone();
    state.error = None;
    state.save_due = None;
    state.store = AssistantChatStore::new();
    if account.is_empty() {
        return;
    }
    let saved = storage::read(&store_key(&account));
    let provider = state.config.provider;
    if state.store.load(&account, saved.as_deref().map(str::as_bytes), provider).is_err() {
        state.error = Some(l10n::t("Saved conversations could not be read. They were left unchanged on this computer."));
    }
}

/// Writes the chats now. Unreadable archives are never overwritten.
pub fn save_now(app: &mut App) {
    let state = &mut app.assistant;
    state.save_due = None;
    if state.account.is_empty() {
        return;
    }
    if let Some(bytes) = state.store.encode() {
        storage::write(&store_key(&state.account), &String::from_utf8_lossy(&bytes));
    }
}

fn save_later(app: &mut App) {
    let now = app.ctx().input(|i| i.time);
    app.assistant.save_due = Some(now + DRAFT_SAVE_SECONDS);
    app.ctx().request_repaint_after(std::time::Duration::from_secs_f64(DRAFT_SAVE_SECONDS));
}

pub fn tick(app: &mut App) {
    reply::check_context(app);
    let visible = is_visible(app);
    let focused = app.ctx().input(|i| i.focused);
    let selected = app.assistant.store.selected_id();
    app.assistant.store.visible_conversation = if visible { selected } else { None };
    if visible
        && focused
        && let Some(id) = selected
        && app.assistant.store.mark_read(id)
    {
        save_now(app);
    }
    if let Some(due) = app.assistant.save_due {
        let now = app.ctx().input(|i| i.time);
        if now >= due {
            save_now(app);
        } else {
            app.ctx().request_repaint_after(std::time::Duration::from_secs_f64(due - now));
        }
    }
}

pub fn windows(app: &mut App, ctx: &egui::Context) {
    crate::ui::assistant::windows(app, ctx);
}

pub fn on_library_changed(app: &mut App) {
    save_now(app);
    reply::cancel(app);
    reply::cancel_curator(app);
    app.assistant.curator.error = None;
    let ui = &mut app.assistant.ui;
    *ui = AssistantUi { history_search: std::mem::take(&mut ui.history_search), ..Default::default() };
    load_store(app);
}

/// Chats with a reply that has not been seen (the toolbar badge).
pub fn unread_count(app: &App) -> usize {
    app.assistant.store.unread_count()
}

/// Whether the chat is on screen: the side panel or the full page.
pub fn is_visible(app: &App) -> bool {
    app.session.has_library() && !app.nav.now_playing && (app.nav.side_panel == SidePanel::Assistant || app.nav.current == Route::Assistant)
}

/// Shows the assistant in the side panel, unless its page is already open.
pub fn open(app: &mut App) {
    app.nav.now_playing = false;
    if app.nav.current != Route::Assistant {
        app.nav.side_panel = SidePanel::Assistant;
    }
    app.assistant.ui.focus_composer = true;
    app.assistant.ui.scroll_to_bottom = true;
}

/// BCP 47 tag of the interface language for prompts.
fn locale() -> &'static str {
    match l10n::language() {
        l10n::Language::Italian => "it-IT",
        l10n::Language::English => "en-US",
    }
}

/// The music track the chat can refer to ("Include this track").
pub fn context_song(app: &App) -> Option<Song> {
    (app.player.mode == Mode::Music).then(|| app.player.current()).flatten().filter(|s| !s.is_podcast())
}

// MARK: Chats

fn default_provider(app: &App) -> AssistantProvider {
    app.assistant.config.provider
}

/// New chat: reuses the selected chat while it is still untouched and empty.
pub fn new_chat(app: &mut App) {
    if app.assistant.account.is_empty() {
        return;
    }
    let provider = default_provider(app);
    app.assistant.store.start_fresh_conversation(provider);
    app.assistant.ui.review = None;
    app.assistant.ui.focus_composer = true;
    save_now(app);
}

pub fn select_chat(app: &mut App, id: Uuid) {
    if app.assistant.store.select(id) {
        app.assistant.ui.review = None;
        app.assistant.ui.scroll_to_bottom = true;
        save_now(app);
    }
}

pub fn rename_chat(app: &mut App, id: Uuid, title: &str) {
    let title: String = title.trim().chars().take(100).collect();
    if title.is_empty() {
        return;
    }
    app.assistant.store.update(id, |c| c.title = title);
    save_now(app);
}

/// Deletes a chat. Saved playlists and selections are kept.
pub fn delete_chat(app: &mut App, id: Uuid) {
    let provider = default_provider(app);
    app.assistant.store.delete(id, provider);
    save_now(app);
    reply::check_context(app);
}

/// Switches the selected chat's provider (keeping its messages) and remembers it for new chats.
pub fn set_provider(app: &mut App, provider: AssistantProvider) {
    if let Some(id) = app.assistant.store.selected_id() {
        app.assistant.store.update(id, |c| c.provider = provider);
        save_now(app);
    }
    app.assistant.config.provider = provider;
    save_config(app);
    if provider == AssistantProvider::ChatGpt && app.assistant.chatgpt.models.is_empty() {
        load_models(app);
    }
}

/// Chooses the ChatGPT model of the selected chat.
pub fn set_model(app: &mut App, model: &str) {
    if let Some(id) = app.assistant.store.selected_id() {
        let model = model.to_string();
        app.assistant.store.update(id, |c| c.model_id = model);
        save_now(app);
    }
}

pub fn set_draft(app: &mut App, text: String) {
    let Some(id) = app.assistant.store.selected_id() else { return };
    app.assistant.store.update(id, |c| c.draft = text);
    save_later(app);
}

pub fn toggle_include_track(app: &mut App) {
    let Some(id) = app.assistant.store.selected_id() else { return };
    app.assistant.store.update(id, |c| c.include_track = !c.include_track);
    save_now(app);
}

/// Starts a new chat with a request (curator prompt) and sends it when the provider is ready.
pub fn ask(app: &mut App, prompt: &str) {
    if app.assistant.account.is_empty() {
        return;
    }
    let provider = default_provider(app);
    let id = app.assistant.store.start_fresh_conversation(provider);
    let draft: String = prompt.trim().chars().take(turn::MAX_REQUEST_CHARACTERS).collect();
    app.assistant.store.update(id, |c| c.draft = draft.clone());
    save_now(app);
    open(app);
    let ready = app.assistant.store.selected().is_some_and(|c| unavailable_reason(app, c.provider).is_none());
    if ready && !is_busy(app) {
        send(app, &draft);
    }
}

// MARK: Edit with assistant

/// Opens a new assistant chat scoped to an existing server playlist ("Edit with assistant").
pub fn edit_playlist_with_assistant(app: &mut App, playlist: &Playlist) {
    focus_playlist(app, playlist.id.clone(), None);
}

/// Opens a saved selection in a new chat scoped to it.
pub fn edit_selection_with_assistant(app: &mut App, mix_id: Uuid) {
    let Some(mix) = discovery::archive(app).curated.iter().find(|m| m.id == mix_id).cloned() else { return };
    match mix.playlist_id.clone() {
        Some(playlist) => focus_playlist(app, playlist, Some(mix.id)),
        None => {
            let saved = mix.playlist_saved == Some(true);
            focus(app, turn::focus_message(&mix.title, mix.songs, saved, None, Some(mix.id)));
        }
    }
}

/// Loads the current playlist from the server, then anchors a new chat on it.
fn focus_playlist(app: &mut App, playlist_id: String, mix: Option<Uuid>) {
    let Some(client) = app.client() else {
        return app.show_error(l10n::music_error(discophile_core::MusicError::MissingCredentials));
    };
    if app.assistant.ui.focusing {
        return;
    }
    app.assistant.ui.focusing = true;
    open(app);
    let generation = app.session.generation;
    app.tasks.spawn(async move {
        let result = client.playlist(&playlist_id).await;
        update(move |app: &mut App| {
            if app.session.generation != generation {
                return;
            }
            app.assistant.ui.focusing = false;
            let playlist = match result {
                Ok(playlist) => playlist,
                Err(error) => return app.show_error(l10n::music_error(error)),
            };
            let songs = playlist.entry.clone().unwrap_or_default();
            if let Some(mix_id) = mix
                && let Some(mut saved) = discovery::archive(app).curated.iter().find(|m| m.id == mix_id).cloned()
            {
                saved.title = playlist.name.clone();
                saved.songs = songs.clone();
                discovery::save_mix(app, saved);
            }
            focus(app, turn::focus_message(&playlist.name, songs, true, Some(playlist.id.clone()), mix));
        })
    });
}

fn focus(app: &mut App, message: AssistantMessage) {
    if app.assistant.account.is_empty() {
        return;
    }
    let provider = default_provider(app);
    app.assistant.store.focus_collection(provider, message);
    app.assistant.ui.review = None;
    save_now(app);
    open(app);
}
