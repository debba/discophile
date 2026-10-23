//! Assistant operations, ported from the iOS `MusicAssistantModel`: one reply at a time (it keeps running
//! while the assistant is hidden or another chat is open), reviewed edits, saving a selection as a server
//! playlist, and curated selections (which run separately, like the iOS curator screen).
//!
//! Changing the waiting chat's library, provider or model, deleting it, or pressing Stop cancels a reply.

use std::collections::HashSet;
use std::future::Future;

use discophile_core::assistant::turn::{self, CuratedChange, TurnContext, TurnLimits, TurnReply};
use discophile_core::assistant::{
    AssistantEditOperation, AssistantEditProposal, AssistantError, AssistantProvider, TurnError, assistant_error_message,
    turn_error_message,
};
use discophile_core::client::MusicClient;
use discophile_core::curated_mix::{self, CuratedMix, CuratorPreset};
use discophile_core::{MusicError, MusicResult, Song, l10n};
use futures::future::AbortHandle;
use uuid::Uuid;

use super::providers;
use crate::app::App;
use crate::features::library::{self, PlaylistChange};
use crate::features::{discovery, downloads};
use crate::platform::MaybeSend;
use crate::player;
use crate::tasks::{Update, update};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunKind {
    Reply,
    Edit,
    SavePlaylist,
}

/// The operation in progress and the chat state it was started with.
pub struct Running {
    pub conversation: Uuid,
    pub kind: RunKind,
    provider: AssistantProvider,
    model: String,
    account: String,
    downloaded_only: bool,
    abort: Option<AbortHandle>,
}

pub struct CuratorState {
    pub busy: bool,
    pub error: Option<String>,
    pub preset: CuratorPreset,
    /// Free-text request that opens the chat.
    pub prompt: String,
    revision: u64,
    abort: Option<AbortHandle>,
}

impl Default for CuratorState {
    fn default() -> Self {
        Self { busy: false, error: None, preset: CuratorPreset::Evening, prompt: String::new(), revision: 0, abort: None }
    }
}

pub fn is_busy(app: &App) -> bool {
    app.assistant.running.is_some()
}

/// The chat waiting for a reply.
pub fn answering(app: &App) -> Option<Uuid> {
    app.assistant.running.as_ref().filter(|r| r.kind == RunKind::Reply).map(|r| r.conversation)
}

/// Stops the current operation. Its late result is ignored.
pub fn cancel(app: &mut App) {
    app.assistant.revision += 1;
    if let Some(running) = app.assistant.running.take()
        && let Some(abort) = running.abort
    {
        abort.abort();
    }
}

pub fn cancel_curator(app: &mut App) {
    let curator = &mut app.assistant.curator;
    curator.revision += 1;
    curator.busy = false;
    if let Some(abort) = curator.abort.take() {
        abort.abort();
    }
}

/// Cancels only when the waiting chat itself changed: its library, provider, model or filter, or it was deleted.
pub(super) fn check_context(app: &mut App) {
    let Some(running) = &app.assistant.running else { return };
    if running.kind == RunKind::SavePlaylist {
        return;
    }
    let chat = app.assistant.store.conversation(running.conversation);
    let changed = chat.is_none_or(|c| c.provider != running.provider || c.model_id != running.model)
        || running.account != app.session.account_key()
        || running.downloaded_only != app.settings.downloaded_only;
    if changed {
        cancel(app);
    }
}

/// The ChatGPT account changed or signed out: requests made with the previous account stop.
pub(super) fn on_chatgpt_changed(app: &mut App) {
    if app.assistant.running.as_ref().is_some_and(|r| r.provider == AssistantProvider::ChatGpt && r.kind == RunKind::Reply) {
        cancel(app);
    }
    cancel_curator(app);
}

pub(super) fn on_local_changed(app: &mut App) {
    if app.assistant.running.as_ref().is_some_and(|r| r.provider == AssistantProvider::Local && r.kind == RunKind::Reply) {
        cancel(app);
    }
}

fn spawn_abortable<F>(app: &App, future: F) -> AbortHandle
where
    F: Future<Output = Update> + MaybeSend + 'static,
{
    let (future, handle) = futures::future::abortable(future);
    app.tasks.spawn(async move { future.await.unwrap_or_else(|_| update(|_: &mut App| {})) });
    handle
}

fn begin(app: &mut App, conversation: Uuid, kind: RunKind, abort: Option<AbortHandle>) {
    let (provider, model) = app.assistant.store.conversation(conversation).map(|c| (c.provider, c.model_id.clone())).unwrap_or_default();
    app.assistant.running = Some(Running {
        conversation,
        kind,
        provider,
        model,
        account: app.session.account_key(),
        downloaded_only: app.settings.downloaded_only,
        abort,
    });
}

fn next_revision(app: &mut App) -> u64 {
    app.assistant.revision += 1;
    app.assistant.revision
}

/// Library data a turn may read, cloned for the background task.
struct TurnData {
    account: String,
    client: Option<MusicClient>,
    offline: Option<Vec<Song>>,
    curated: Vec<CuratedMix>,
    known: Vec<Song>,
    favorites: Vec<Song>,
    newest: Vec<discophile_core::Album>,
}

impl TurnData {
    fn collect(app: &App) -> Self {
        Self {
            account: app.session.account_key(),
            client: app.client(),
            offline: app.settings.downloaded_only.then(|| downloads::downloaded_songs(app)),
            curated: discovery::archive(app).curated.clone(),
            known: discovery::pool(app),
            favorites: app.library.snapshot.favorites.song.clone().unwrap_or_default(),
            newest: app.library.snapshot.newest.clone(),
        }
    }
}

// MARK: Replies

/// Sends the request in the selected chat.
pub fn send(app: &mut App, text: &str) {
    if is_busy(app) || app.session.account_key().is_empty() {
        return;
    }
    let Some((id, provider, model)) = app.assistant.store.selected().map(|c| (c.id, c.provider, c.model_id.clone())) else { return };
    if providers::unavailable_reason(app, provider).is_some() {
        return;
    }
    let song = super::context_song(app);
    let names: Vec<String> = app.library.snapshot.playlists.iter().map(|p| p.name.clone()).collect();
    let data = TurnData::collect(app);
    let titles: Vec<String> = data.curated.iter().map(|m| m.title.clone()).collect();
    let mut started = None;
    app.assistant.store.update(id, |c| started = turn::start_turn(c, text, song.as_ref(), &names, &titles));
    let Some((request, input)) = started else { return };
    super::save_now(app);
    app.assistant.ui.scroll_to_bottom = true;
    let plan = match providers::plan_responder(app, provider, &model) {
        Ok(plan) => plan,
        Err(error) => {
            let source = providers::provider_name(app, provider, &model);
            app.assistant.store.update(id, |c| c.messages.push(turn::error_reply(TurnError::Assistant(error), &source)));
            return super::save_now(app);
        }
    };
    let Some(conversation) = app.assistant.store.conversation(id).cloned() else { return };
    let revision = next_revision(app);
    let tasks = app.tasks.clone();
    let locale = super::locale();
    let fallback = plan.source();
    let future = async move {
        let outcome = match plan.build(tasks).await {
            Ok((responder, source)) => {
                let ctx = TurnContext {
                    provider,
                    locale,
                    source: &source,
                    account: &data.account,
                    library: data.client.as_ref(),
                    offline_songs: data.offline.as_deref(),
                    preview: false,
                    curated: &data.curated,
                    known_songs: &data.known,
                    cached_favorites: &data.favorites,
                    newest_albums: &data.newest,
                };
                let result = turn::run_turn(&ctx, &responder, &conversation, &request, &input, song.as_ref()).await;
                (result, source)
            }
            Err(error) => (Err(TurnError::Assistant(error)), fallback),
        };
        update(move |app: &mut App| finish_reply(app, revision, id, outcome))
    };
    let abort = spawn_abortable(app, future);
    begin(app, id, RunKind::Reply, Some(abort));
}

fn finish_reply(app: &mut App, revision: u64, id: Uuid, (result, source): (Result<TurnReply, TurnError>, String)) {
    if app.assistant.revision != revision {
        return;
    }
    app.assistant.running = None;
    match result {
        Ok(mut reply) => {
            if let Some(songs) = reply.play.clone() {
                app.player.error = None;
                player::play_songs(app, songs, 0, false);
                // The error is shown in the reply instead of the alert.
                let error = app.player.error.take();
                reply.apply_playback_status(error.as_deref());
            }
            app.assistant.store.update(id, |c| reply.apply(c));
        }
        Err(error) => {
            app.assistant.store.update(id, |c| c.messages.push(turn::error_reply(error, &source)));
        }
    }
    app.assistant.ui.scroll_to_bottom = true;
    reply_finished(app, id);
}

/// Marks the chat unread and notifies when it is not on screen.
fn reply_finished(app: &mut App, id: Uuid) {
    let active = app.ctx().input(|i| i.focused);
    let notice = app.assistant.store.reply_finished(id, active);
    super::save_now(app);
    if let discophile_core::assistant::chat_store::ReplyNotice::Notify { .. } = notice
        && let Some(chat) = app.assistant.store.conversation(id)
    {
        let reply = chat.messages.iter().rev().find(|m| m.role == discophile_core::assistant::AssistantRole::Assistant);
        super::notify::reply_ready(&chat.title, reply.map(|m| m.text.as_str()).unwrap_or_default());
    }
}

// MARK: Edits

/// Cancels a pending proposal in the selected chat.
pub fn cancel_edit(app: &mut App, message_id: Uuid) {
    if is_busy(app) {
        return;
    }
    let Some(id) = app.assistant.store.selected_id() else { return };
    app.assistant.store.update(id, |c| turn::cancel_edit(c, message_id));
    super::save_now(app);
}

/// Applies a reviewed proposal after the explicit confirmation.
pub fn confirm_edit(app: &mut App, message_id: Uuid) {
    if is_busy(app) {
        return;
    }
    let Some(conversation) = app.assistant.store.selected().cloned() else { return };
    let id = conversation.id;
    let account = app.session.account_key();
    let mut started = None;
    app.assistant.store.update(id, |c| started = turn::begin_confirmed_edit(c, message_id, &account));
    let Some(edit) = started else { return };
    super::save_now(app);
    let curated = discovery::archive(app).curated.clone();
    if let Err(error) = turn::verify_local_target(&conversation, &edit, &curated) {
        return fail_edit(app, id, message_id, &l10n::music_error(error));
    }
    if !edit.is_favorite_change() && edit.target.playlist_id.is_none() {
        return complete_edit(app, id, message_id, &edit);
    }
    let Some(client) = app.client() else {
        return fail_edit(app, id, message_id, &l10n::music_error(MusicError::MissingCredentials));
    };
    let revision = next_revision(app);
    let generation = app.session.generation;
    let future = async move {
        let result =
            if edit.is_favorite_change() { apply_favorite_edit(&client, &edit).await } else { client.apply_playlist_edit(&edit).await };
        update(move |app: &mut App| {
            if app.session.generation != generation {
                return;
            }
            if edit.is_favorite_change() {
                library::load_favorites(app);
            } else if let Some(playlist) = &edit.target.playlist_id {
                library::load_playlists(app);
                if edit.operation != AssistantEditOperation::Delete {
                    library::load_playlist(app, playlist);
                }
            }
            if app.assistant.revision != revision {
                return;
            }
            app.assistant.running = None;
            match result {
                Ok(()) => complete_edit(app, id, message_id, &edit),
                Err(error) => fail_edit(app, id, message_id, &l10n::music_error(error)),
            }
        })
    };
    let abort = spawn_abortable(app, future);
    begin(app, id, RunKind::Edit, Some(abort));
}

/// Applies the curated selection change of a confirmed edit, then updates the chat.
fn complete_edit(app: &mut App, id: Uuid, message_id: Uuid, edit: &AssistantEditProposal) {
    match turn::curated_change(edit) {
        Some(CuratedChange::Remove(mix)) => {
            if !discovery::remove_mix(app, mix) {
                return fail_edit(app, id, message_id, &assistant_error_message(AssistantError::Unavailable));
            }
        }
        Some(CuratedChange::Update { id: mix_id, title, songs }) => {
            let Some(mut mix) = discovery::archive(app).curated.iter().find(|m| m.id == mix_id).cloned() else {
                return fail_edit(app, id, message_id, &assistant_error_message(AssistantError::Unavailable));
            };
            mix.title = title;
            mix.songs = songs;
            if !discovery::save_mix(app, mix) {
                return fail_edit(app, id, message_id, &assistant_error_message(AssistantError::Unavailable));
            }
        }
        None => {}
    }
    app.assistant.store.update(id, |c| turn::finish_confirmed_edit(c, message_id, edit));
    super::save_now(app);
}

fn fail_edit(app: &mut App, id: Uuid, message_id: Uuid, detail: &str) {
    app.assistant.store.update(id, |c| turn::fail_confirmed_edit(c, message_id, detail));
    super::save_now(app);
}

/// Stars or unstars the reviewed songs after checking the favorites still match the review.
async fn apply_favorite_edit(client: &MusicClient, edit: &AssistantEditProposal) -> MusicResult<()> {
    let current = client.favorites().await?;
    let ids: HashSet<&str> = current.songs().iter().map(Song::stream_identifier).collect();
    let selected: HashSet<&str> = edit.tracks.iter().map(Song::stream_identifier).collect();
    let now: HashSet<&str> = selected.iter().copied().filter(|id| ids.contains(id)).collect();
    let reviewed: HashSet<&str> = selected.iter().copied().filter(|id| edit.expected_favorite_ids.contains(*id)).collect();
    if now != reviewed {
        return Err(MusicError::PlaylistChanged);
    }
    let starred = edit.operation == AssistantEditOperation::Favorite;
    for song in &edit.tracks {
        if ids.contains(song.stream_identifier()) != starred {
            client.favorite(song.stream_identifier(), starred).await?;
        }
    }
    Ok(())
}

// MARK: Playlists

/// Creates a server playlist from a reviewed selection and keeps its identifier for later edits.
pub fn save_playlist(app: &mut App, conversation: Uuid, message_id: Uuid, title: String, songs: Vec<Song>) {
    if is_busy(app) || songs.is_empty() || title.trim().is_empty() || app.settings.downloaded_only || app.library.changing_playlist {
        return;
    }
    let Some(message) = app.assistant.store.conversation(conversation).and_then(|c| c.message(message_id)).cloned() else { return };
    if message.saved {
        return;
    }
    let title = title.trim().to_string();
    let revision = next_revision(app);
    begin(app, conversation, RunKind::SavePlaylist, None);
    let change = PlaylistChange::Create(title.clone(), songs.clone());
    library::change_playlist(app, change, move |app: &mut App, result| {
        if app.assistant.revision == revision {
            app.assistant.running = None;
        }
        match result {
            Ok(result) => {
                let playlist_id = result.created.map(|p| p.id);
                if let Some(mix_id) = message.curated_mix_id
                    && let Some(mut mix) = discovery::archive(app).curated.iter().find(|m| m.id == mix_id).cloned()
                {
                    mix.title = title.clone();
                    mix.songs = songs.clone();
                    mix.playlist_id = playlist_id.clone();
                    mix.playlist_saved = Some(true);
                    discovery::save_mix(app, mix);
                }
                app.assistant.store.mark_saved(conversation, message_id, &title, songs, playlist_id);
                super::save_now(app);
                app.assistant.ui.editor = None;
                app.toast(l10n::t("Playlist saved"));
            }
            Err(error) => {
                if let Some(editor) = app.assistant.ui.editor.as_mut() {
                    editor.error = Some(l10n::music_error(error));
                } else {
                    app.show_error(l10n::music_error(error));
                }
            }
        }
    });
}

// MARK: Curator

/// Generates a curated half-hour selection with the selected chat's provider and model.
pub fn curate(app: &mut App, preset: CuratorPreset) {
    if app.assistant.curator.busy || app.session.account_key().is_empty() {
        return;
    }
    app.assistant.curator.error = None;
    let (provider, model) =
        app.assistant.store.selected().map(|c| (c.provider, c.model_id.clone())).unwrap_or((app.assistant.config.provider, String::new()));
    if let Some(reason) = providers::unavailable_reason(app, provider) {
        return app.assistant.curator.error = Some(reason);
    }
    let limits = TurnLimits::for_provider(provider);
    let candidates = curated_mix::curator_candidates(&discovery::pool(app), discovery::history(app), limits.candidates);
    if candidates.is_empty() {
        return app.assistant.curator.error = Some(assistant_error_message(AssistantError::NoMatches));
    }
    let plan = match providers::plan_responder(app, provider, &model) {
        Ok(plan) => plan,
        Err(error) => return app.assistant.curator.error = Some(assistant_error_message(error)),
    };
    let curator = &mut app.assistant.curator;
    curator.revision += 1;
    curator.busy = true;
    let revision = curator.revision;
    let generation = app.session.generation;
    let tasks = app.tasks.clone();
    let locale = super::locale();
    let future = async move {
        let result = match plan.build(tasks).await {
            Ok((responder, source)) => turn::curate(&responder, provider, locale, &source, preset, &candidates).await,
            Err(error) => Err(TurnError::Assistant(error)),
        };
        update(move |app: &mut App| {
            let curator = &mut app.assistant.curator;
            if curator.revision != revision || app.session.generation != generation {
                return;
            }
            curator.busy = false;
            curator.abort = None;
            match result {
                Ok(mix) => {
                    if !discovery::save_mix(app, mix) {
                        app.assistant.curator.error = Some(assistant_error_message(AssistantError::Unavailable));
                    }
                }
                Err(error) => app.assistant.curator.error = Some(turn_error_message(error)),
            }
        })
    };
    let abort = spawn_abortable(app, future);
    app.assistant.curator.abort = Some(abort);
}
