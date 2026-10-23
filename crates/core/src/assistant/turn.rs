//! One assistant turn, ported from the iOS `MusicAssistantModel`: request validation, the plan input,
//! library candidates, selection, playback proposals, edit proposals and curated mixes.
//!
//! These functions never play audio or change the library. They return what the app should do
//! ([`TurnReply::play`]) and messages to append; edits stay proposals until the person confirms them,
//! after which the app applies the server change and calls [`finish_confirmed_edit`].

use std::collections::HashSet;
use std::future::Future;

use uuid::Uuid;

use super::chatgpt::ChatGptClient;
use super::conversation::{AssistantConversation, AssistantMessage, AssistantProvider};
use super::edits::{
    AssistantEditOperation, AssistantEditProposal, AssistantEditStatus, AssistantEditTarget,
    AssistantEditTargetKind, AssistantError, AssistantTargetResolver, normalize,
};
use super::library::{self, AssistantLibrary, Catalog, MaybeSend, MaybeSync};
use super::local_model::LocalModelClient;
use super::music_assistant::{
    AssistantIntent, AssistantPlan, AssistantPrompts, AssistantSchema, AssistantSelection,
};
use crate::curated_mix::{CuratedMix, CuratorPreset};
use crate::error::MusicError;
use crate::l10n::{count, t};
use crate::models::{Album, Song};

/// A library or assistant failure. Show it with [`super::turn_error_message`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TurnError {
    #[error(transparent)]
    Music(MusicError),
    #[error(transparent)]
    Assistant(AssistantError),
}

/// A model provider that answers with JSON matching a strict schema.
pub trait AssistantResponder: MaybeSync {
    fn respond(
        &self,
        instructions: &str,
        input: &str,
        schema: &AssistantSchema,
    ) -> impl Future<Output = Result<String, AssistantError>> + MaybeSend;
}

impl AssistantResponder for LocalModelClient {
    async fn respond(
        &self,
        instructions: &str,
        input: &str,
        schema: &AssistantSchema,
    ) -> Result<String, AssistantError> {
        LocalModelClient::respond(self, instructions, input, schema).await
    }
}

/// ChatGPT with a fresh access token (see [`ChatGptClient::current_credential`]) and an offered model.
#[derive(Debug, Clone)]
pub struct ChatGptResponder {
    pub client: ChatGptClient,
    pub token: String,
    pub model: String,
}

impl AssistantResponder for ChatGptResponder {
    async fn respond(
        &self,
        instructions: &str,
        input: &str,
        schema: &AssistantSchema,
    ) -> Result<String, AssistantError> {
        if self.model.is_empty() {
            return Err(AssistantError::Restricted);
        }
        self.client
            .respond(&self.token, &self.model, instructions, input, schema)
            .await
    }
}

/// Either provider, for apps that pick one per chat.
#[derive(Debug, Clone)]
pub enum ProviderResponder {
    ChatGpt(ChatGptResponder),
    Local(LocalModelClient),
}

impl AssistantResponder for ProviderResponder {
    async fn respond(
        &self,
        instructions: &str,
        input: &str,
        schema: &AssistantSchema,
    ) -> Result<String, AssistantError> {
        match self {
            Self::ChatGpt(responder) => responder.respond(instructions, input, schema).await,
            Self::Local(client) => client.respond(instructions, input, schema).await,
        }
    }
}

/// Bounds per provider. Local models get smaller, compact prompts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnLimits {
    /// Candidates a model may choose from (never more than 200).
    pub candidates: usize,
    /// Songs in one selection.
    pub selection: usize,
    /// Characters of conversation history.
    pub history_characters: usize,
    /// Server playlist names listed in the prompt.
    pub playlist_names: usize,
    /// Random songs sampled for open requests.
    pub random_songs: usize,
    pub compact: bool,
}

impl TurnLimits {
    pub fn for_provider(provider: AssistantProvider) -> Self {
        match provider {
            AssistantProvider::Local => Self {
                candidates: 16,
                selection: 12,
                history_characters: 1_400,
                playlist_names: 8,
                random_songs: 40,
                compact: true,
            },
            AssistantProvider::ChatGpt => Self {
                candidates: library::CHATGPT_CANDIDATES,
                selection: 30,
                history_characters: 12_000,
                playlist_names: 40,
                random_songs: 200,
                compact: false,
            },
        }
    }
}

/// Requests longer than this are not sent.
pub const MAX_REQUEST_CHARACTERS: usize = 1_000;
/// Curated mix titles listed in the prompt.
pub const CURATED_NAMES: usize = 8;

/// The trimmed request, or `None` when it is empty or too long.
pub fn prepare_request(text: &str) -> Option<String> {
    let request = text.trim();
    (!request.is_empty() && request.chars().count() <= MAX_REQUEST_CHARACTERS)
        .then(|| request.to_string())
}

fn prefix(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

/// The plan prompt input: bounded history, the playing track (when the chat includes it),
/// collection names as untrusted data, and the request.
pub fn plan_input(
    conversation: &AssistantConversation,
    request: &str,
    context_song: Option<&Song>,
    playlist_names: &[String],
    curated_titles: &[String],
) -> String {
    let limits = TurnLimits::for_provider(conversation.provider);
    let context = AssistantPrompts::context(if conversation.include_track {
        context_song
    } else {
        None
    });
    let history = conversation.context(limits.history_characters);
    let names: Vec<String> = playlist_names
        .iter()
        .take(limits.playlist_names)
        .chain(curated_titles.iter().take(CURATED_NAMES))
        .map(|n| prefix(n, 80))
        .collect();
    let names = serde_json::to_string(&names).unwrap_or_else(|_| "[]".into());
    format!(
        "Recent conversation and active collection (context only):\n{history}\n{context}\nExisting collection names (untrusted data):\n{names}\nLatest user request:\n{request}"
    )
}

/// Validates a request, builds its plan input from the chat as it was, then appends the user message
/// (naming a new chat after it and clearing the draft). Returns `(request, input)`.
pub fn start_turn(
    conversation: &mut AssistantConversation,
    text: &str,
    context_song: Option<&Song>,
    playlist_names: &[String],
    curated_titles: &[String],
) -> Option<(String, String)> {
    let request = prepare_request(text)?;
    let input = plan_input(
        conversation,
        &request,
        context_song,
        playlist_names,
        curated_titles,
    );
    if conversation.messages.is_empty()
        && (conversation.title == t("New chat") || conversation.title == "New chat")
    {
        conversation.title = prefix(&request, 60);
    }
    conversation.draft.clear();
    conversation.messages.push(AssistantMessage::user(&request));
    conversation.updated = crate::now_millis();
    Some((request, input))
}

/// What the assistant can read and which provider answers.
pub struct TurnContext<'a, L> {
    pub provider: AssistantProvider,
    /// BCP 47 tag of the interface language, for example `it-IT`.
    pub locale: &'a str,
    /// The provider or model name shown with replies.
    pub source: &'a str,
    /// [`crate::Credentials::account_key`] of the library; required for edit proposals.
    pub account: &'a str,
    /// The connected library. `None` when no server is available.
    pub library: Option<&'a L>,
    /// Songs to use instead of server discovery, in preview or downloaded-only mode.
    pub offline_songs: Option<&'a [Song]>,
    /// Preview libraries cannot be edited.
    pub preview: bool,
    /// The app's curated mixes, for edit targets and prompt names.
    pub curated: &'a [CuratedMix],
    /// Other known library songs (discovery, recent plays) that favorite edits may refer to.
    pub known_songs: &'a [Song],
    /// Favorites already loaded by the app, mixed into open requests.
    pub cached_favorites: &'a [Song],
    /// Newest albums, sampled when nothing else is available.
    pub newest_albums: &'a [Album],
}

impl<'a, L: AssistantLibrary> TurnContext<'a, L> {
    pub fn limits(&self) -> TurnLimits {
        TurnLimits::for_provider(self.provider)
    }

    fn library(&self) -> Result<&'a L, TurnError> {
        self.library
            .ok_or(TurnError::Music(MusicError::MissingCredentials))
    }

    /// The bounded catalog for a plan: offline songs, or server discovery plus a sample for open requests.
    pub async fn library_candidates(&self, plan: &AssistantPlan) -> Result<Catalog, TurnError> {
        let limit = self.limits().candidates;
        if let Some(songs) = self.offline_songs {
            return Ok(library::catalog(songs, plan, limit, false));
        }
        let result = library::resolve(self.library()?, plan, limit).await?;
        if plan.has_scope() {
            return Ok(result);
        }
        let mut songs = result.songs;
        songs.extend(self.sample_candidates().await?);
        Ok(library::catalog(&songs, plan, limit, result.limited))
    }

    /// Random songs and favorites, or tracks of the newest albums when the server offers neither.
    pub async fn sample_candidates(&self) -> Result<Vec<Song>, TurnError> {
        let limits = self.limits();
        if let Some(songs) = self.offline_songs {
            return Ok(AssistantPrompts::candidates(songs, limits.candidates));
        }
        let library = self.library()?;
        let random = match library.random_songs(limits.random_songs).await {
            Ok(songs) => songs,
            Err(MusicError::Unsupported | MusicError::NotFound) => Vec::new(),
            Err(error) if self.cached_favorites.is_empty() => return Err(error.into()),
            Err(_) => Vec::new(),
        };
        let mut candidates: Vec<Song> = random
            .into_iter()
            .chain(self.cached_favorites.iter().cloned())
            .collect();
        if candidates.is_empty() {
            for album in self.newest_albums.iter().take(3) {
                candidates.extend(library.album(&album.id).await?.song.unwrap_or_default());
                if candidates.len() >= limits.candidates {
                    break;
                }
            }
        }
        Ok(AssistantPrompts::candidates(&candidates, limits.candidates))
    }
}

/// A finished turn. Call [`TurnReply::apply_playback_status`] after starting playback, then [`TurnReply::apply`].
#[derive(Debug, Clone, PartialEq)]
pub struct TurnReply {
    pub message: AssistantMessage,
    /// Songs the app should start playing now.
    pub play: Option<Vec<Song>>,
    /// The reply becomes the chat's active collection.
    pub focus: bool,
}

impl TurnReply {
    fn text(message: AssistantMessage) -> Self {
        Self {
            message,
            play: None,
            focus: false,
        }
    }

    /// Adds "Playback requested." or the player's error to a reply that plays songs.
    pub fn apply_playback_status(&mut self, player_error: Option<&str>) {
        if self.play.is_none() {
            return;
        }
        let status = player_error
            .map(str::to_string)
            .unwrap_or_else(|| t("Playback requested."));
        if self.message.text.is_empty() {
            self.message.text = status;
        } else {
            self.message.text += &format!("\n\n{status}");
        }
    }

    /// Appends the reply to its chat.
    pub fn apply(self, conversation: &mut AssistantConversation) {
        let id = self.message.id;
        conversation.messages.push(self.message);
        if self.focus {
            conversation.focus_message_id = Some(id);
        }
        conversation.updated = crate::now_millis();
    }
}

/// The error message appended when a turn fails.
pub fn error_reply(error: TurnError, source: &str) -> AssistantMessage {
    AssistantMessage::error(super::turn_error_message(error), Some(source))
}

/// Runs a turn: plan, then answer, play, select, or propose an edit.
pub async fn run_turn<L: AssistantLibrary, R: AssistantResponder>(
    ctx: &TurnContext<'_, L>,
    responder: &R,
    conversation: &AssistantConversation,
    request: &str,
    input: &str,
    context_song: Option<&Song>,
) -> Result<TurnReply, TurnError> {
    let text = responder
        .respond(
            &AssistantPrompts::plan_instructions(ctx.locale),
            input,
            &AssistantSchema::Plan,
        )
        .await?;
    let plan = AssistantPlan::decode(&text)?.enforcing_explicit_favorites(request);
    let source = Some(ctx.source);
    match plan.intent {
        AssistantIntent::Answer => Ok(TurnReply::text(AssistantMessage::assistant(
            &plan.message,
            source,
        ))),
        AssistantIntent::Manage => prepare_edit(
            ctx,
            responder,
            conversation,
            &plan,
            request,
            input,
            context_song,
        )
        .await
        .map(TurnReply::text),
        AssistantIntent::Search | AssistantIntent::Play | AssistantIntent::Playlist => {
            let limits = ctx.limits();
            let catalog = ctx.library_candidates(&plan).await?;
            let candidates = catalog.songs;
            if candidates.is_empty() {
                return Err(AssistantError::NoMatches.into());
            }
            if !plan.title.is_empty()
                && !plan.has_years()
                && plan.genre().is_empty()
                && plan.intent != AssistantIntent::Playlist
            {
                if let Some(song) = plan
                    .unique_playback_match(&candidates)
                    .filter(|_| !catalog.limited)
                {
                    let mut message =
                        AssistantMessage::assistant("", source).with_songs(vec![song.clone()]);
                    message.criteria = Some(plan.context());
                    return Ok(TurnReply {
                        message,
                        play: Some(vec![song.clone()]),
                        focus: false,
                    });
                }
                let mut message = AssistantMessage::assistant(
                    t("Choose a song from the results to play it."),
                    source,
                )
                .with_songs(candidates);
                message.criteria = Some(plan.context());
                return Ok(TurnReply::text(message));
            }
            let limit = limits.selection.min(candidates.len());
            let selection_input = format!(
                "{input}\nResolved library criteria:\n{text}\nCatalog:\n{}",
                AssistantPrompts::catalog(&candidates, limits.compact, false)
            );
            let output = responder
                .respond(
                    &AssistantPrompts::selection_instructions(ctx.locale, limit),
                    &selection_input,
                    &AssistantSchema::selection(candidates.len(), limit),
                )
                .await?;
            let (proposal, tracks) = match AssistantSelection::decode(&output, &candidates, limit) {
                Err(AssistantError::NoMatches) => dated_fallback(&plan, &candidates, limit)?,
                other => other?,
            };
            let mut text = format!(
                "{}\n\n{}",
                proposal.message,
                count("Selection of %lld songs", tracks.len() as i64)
            );
            if plan.has_years() {
                text += &format!(
                    "\n{}",
                    t(
                        "Library years can describe reissues. Original periods may be inferred by AI from the song and album."
                    )
                );
            }
            if catalog.limited {
                text += &format!(
                    "\n{}",
                    t("This selection uses a limited part of your library.")
                );
            }
            let mut message = AssistantMessage::assistant(text, source).with_songs(tracks.clone());
            if plan.intent == AssistantIntent::Playlist {
                message.playlist_title = Some(proposal.title.clone());
            }
            message.criteria = Some(plan.context());
            message.selection_title = Some(proposal.title);
            let play = (plan.intent == AssistantIntent::Play).then_some(tracks);
            Ok(TurnReply {
                message,
                play,
                focus: true,
            })
        }
    }
}

/// A model refusing every dated catalog entry must not hide real tag matches.
fn dated_fallback(
    plan: &AssistantPlan,
    candidates: &[Song],
    limit: usize,
) -> Result<(AssistantSelection, Vec<Song>), AssistantError> {
    let ids: Vec<String> = candidates
        .iter()
        .enumerate()
        .filter(|(_, s)| {
            plan.has_years()
                && s.album_original_year
                    .or(s.year)
                    .or(s.album_year)
                    .is_some_and(|y| plan.includes(y))
        })
        .take(limit)
        .map(|(i, _)| format!("t{i}"))
        .collect();
    if ids.is_empty() {
        return Err(AssistantError::NoMatches);
    }
    let fallback = serde_json::json!({
        "title": t("Library selection"),
        "message": t("Selected using the years in your library metadata."),
        "track_ids": ids,
    });
    AssistantSelection::decode(&fallback.to_string(), candidates, limit)
}

fn operation_name(operation: AssistantEditOperation) -> &'static str {
    use AssistantEditOperation::*;
    match operation {
        Add => "add",
        Remove => "remove",
        Replace => "replace",
        Rename => "rename",
        Delete => "delete",
        Favorite => "favorite",
        Unfavorite => "unfavorite",
    }
}

/// Finds the collection an edit refers to: the chat's active collection, or one named playlist or curated mix.
pub async fn edit_target<L: AssistantLibrary>(
    ctx: &TurnContext<'_, L>,
    conversation: &AssistantConversation,
    name: &str,
) -> Result<(AssistantEditTarget, Vec<Song>), TurnError> {
    let wanted = normalize(name);
    let current = name.is_empty() || wanted == "current";
    if let Some(focused) = conversation.active_selection()
        && (current || wanted == normalize(focused.collection_title().unwrap_or_default()))
    {
        if let Some(id) = &focused.playlist_id {
            let playlist = ctx.library()?.playlist(id).await?;
            let target = AssistantEditTarget {
                kind: AssistantEditTargetKind::Playlist,
                message_id: Some(focused.id),
                playlist_id: Some(id.clone()),
                curated_id: focused.curated_mix_id,
                title: playlist.name.clone(),
            };
            return Ok((target, playlist.entry.unwrap_or_default()));
        }
        if let Some(id) = focused.curated_mix_id
            && let Some(mix) = ctx.curated.iter().find(|m| m.id == id)
        {
            if mix.playlist_saved == Some(true) && mix.playlist_id.is_none() {
                return Err(AssistantError::AmbiguousTarget.into());
            }
            let target = AssistantEditTarget {
                kind: AssistantEditTargetKind::Curated,
                message_id: Some(focused.id),
                playlist_id: mix.playlist_id.clone(),
                curated_id: Some(id),
                title: mix.title.clone(),
            };
            return Ok((target, mix.songs.clone()));
        }
        if focused.saved {
            return Err(AssistantError::AmbiguousTarget.into());
        }
        let title = focused
            .collection_title()
            .map(str::to_string)
            .unwrap_or_else(|| t("Library selection"));
        let target = AssistantEditTarget {
            message_id: Some(focused.id),
            ..AssistantEditTarget::new(AssistantEditTargetKind::Selection, title)
        };
        return Ok((target, focused.songs.clone()));
    }
    if current {
        return Err(AssistantError::NoMatches.into());
    }
    let library = ctx.library()?;
    let mut targets: Vec<AssistantEditTarget> = library
        .playlists()
        .await?
        .into_iter()
        .map(|p| AssistantEditTarget::playlist(p.name, p.id))
        .collect();
    targets.extend(ctx.curated.iter().map(|m| AssistantEditTarget {
        playlist_id: m.playlist_id.clone(),
        curated_id: Some(m.id),
        ..AssistantEditTarget::new(AssistantEditTargetKind::Curated, &m.title)
    }));
    let target = AssistantTargetResolver::named(name, &targets)?;
    if let Some(id) = &target.playlist_id {
        let playlist = library.playlist(id).await?;
        let resolved = AssistantEditTarget {
            title: playlist.name.clone(),
            message_id: None,
            ..target
        };
        return Ok((resolved, playlist.entry.unwrap_or_default()));
    }
    let mix = ctx
        .curated
        .iter()
        .find(|m| Some(m.id) == target.curated_id)
        .ok_or(AssistantError::NoMatches)?;
    if mix.playlist_saved == Some(true) {
        return Err(AssistantError::AmbiguousTarget.into());
    }
    Ok((target, mix.songs.clone()))
}

/// Builds a reviewed edit proposal message for a `manage` plan. Nothing is changed.
pub async fn prepare_edit<L: AssistantLibrary, R: AssistantResponder>(
    ctx: &TurnContext<'_, L>,
    responder: &R,
    conversation: &AssistantConversation,
    plan: &AssistantPlan,
    request: &str,
    input: &str,
    context_song: Option<&Song>,
) -> Result<AssistantMessage, TurnError> {
    use AssistantEditOperation::*;
    let action = plan
        .edit_operation()
        .ok_or(AssistantError::InvalidResponse)?;
    if ctx.preview {
        return Err(AssistantError::InvalidResponse.into());
    }
    let limits = ctx.limits();
    let favorite = matches!(action, Favorite | Unfavorite);
    let favorite_songs = if favorite {
        ctx.library()?.favorites().await?.song.unwrap_or_default()
    } else {
        Vec::new()
    };
    let all_tracks = plan.all_tracks == Some(true);
    let (target, before) = if favorite {
        let target = AssistantEditTarget::new(AssistantEditTargetKind::Favorites, t("Favorites"));
        let name = plan.target.as_deref().unwrap_or("current");
        let wanted = normalize(name);
        let favorite_target = [
            "favorites".to_string(),
            "preferiti".to_string(),
            normalize(&t("Favorites")),
        ]
        .contains(&wanted);
        let before = if favorite_target && action == Unfavorite && all_tracks {
            favorite_songs.clone()
        } else if !name.is_empty() && wanted != "current" && !favorite_target {
            edit_target(ctx, conversation, name).await?.1
        } else {
            match conversation.active_selection() {
                Some(active) => active.songs.clone(),
                None => context_song.map(|s| vec![s.clone()]).unwrap_or_default(),
            }
        };
        (target, before)
    } else {
        edit_target(
            ctx,
            conversation,
            plan.target.as_deref().unwrap_or("current"),
        )
        .await?
    };
    let mut selected: Vec<Song> = Vec::new();
    if !matches!(action, Rename | Delete) {
        let known = || {
            before
                .iter()
                .chain(ctx.known_songs)
                .cloned()
                .collect::<Vec<_>>()
        };
        let named = match action {
            Unfavorite => AssistantPrompts::explicitly_named_songs(&favorite_songs, request),
            Favorite => AssistantPrompts::explicitly_named_songs(&known(), request),
            _ => Vec::new(),
        };
        if named.len() == 1 {
            selected = named;
        } else if all_tracks && matches!(action, Remove | Favorite | Unfavorite) {
            selected = before.clone();
        } else {
            let source = match action {
                Unfavorite => AssistantPrompts::favorite_candidates(
                    &favorite_songs,
                    request,
                    limits.candidates,
                ),
                Favorite => {
                    let mut songs = ctx.library_candidates(plan).await?.songs;
                    songs.extend(known());
                    AssistantPrompts::favorite_candidates(&songs, request, limits.candidates)
                }
                Remove => library::catalog(&before, plan, limits.candidates, false).songs,
                _ => ctx.library_candidates(plan).await?.songs,
            };
            let existing: HashSet<&str> = before.iter().map(Song::stream_identifier).collect();
            let candidates: Vec<Song> = source
                .into_iter()
                .filter(|s| action != Add || !existing.contains(s.stream_identifier()))
                .collect();
            if candidates.is_empty() {
                return Err(AssistantError::NoMatches.into());
            }
            if !plan.title.is_empty() && candidates.len() == 1 {
                selected = candidates;
            } else {
                let limit = limits.selection.min(candidates.len());
                let edit_input = format!(
                    "{input}\nSelect the songs for this edit only: {}.\nCatalog:\n{}",
                    operation_name(action),
                    AssistantPrompts::catalog(&candidates, limits.compact, false)
                );
                let output = responder
                    .respond(
                        &AssistantPrompts::selection_instructions(ctx.locale, limit),
                        &edit_input,
                        &AssistantSchema::selection(candidates.len(), limit),
                    )
                    .await?;
                selected = AssistantSelection::decode(&output, &candidates, limit)?.1;
            }
        }
    }
    let favorite_ids: HashSet<String> = favorite_songs
        .iter()
        .map(|s| s.stream_identifier().to_string())
        .collect();
    let proposal = AssistantEditProposal::new(
        ctx.account,
        action,
        target,
        before,
        selected,
        plan.new_name.as_deref().unwrap_or_default(),
        &favorite_ids,
    )?;
    if proposal.is_no_op() {
        return Ok(AssistantMessage::assistant(
            t("This collection already matches your request. Nothing was changed."),
            Some(ctx.source),
        ));
    }
    let mut message = AssistantMessage::assistant(
        t("Review these changes. They are applied only after your confirmation."),
        Some(ctx.source),
    );
    message.edit = Some(proposal);
    Ok(message)
}

/// Asks the model for a curated mix of at most half an hour. `candidates` come from
/// [`crate::curated_mix::curator_candidates`] with [`TurnLimits::candidates`] as the limit.
pub async fn curate<R: AssistantResponder>(
    responder: &R,
    provider: AssistantProvider,
    locale: &str,
    source: &str,
    preset: CuratorPreset,
    candidates: &[Song],
) -> Result<CuratedMix, TurnError> {
    if candidates.is_empty() {
        return Err(AssistantError::NoMatches.into());
    }
    let limits = TurnLimits::for_provider(provider);
    let limit = limits.selection.min(candidates.len());
    let input = format!(
        "{}\nCandidates are ordered by fewer local plays, then oldest local play. Prefer those earlier candidates when musically appropriate. Select only known-duration tracks, totaling at most 1800 seconds.\nCatalog:\n{}",
        preset.request(),
        AssistantPrompts::catalog(candidates, limits.compact, true)
    );
    let output = responder
        .respond(
            &AssistantPrompts::selection_instructions(locale, limit),
            &input,
            &AssistantSchema::selection(candidates.len(), limit),
        )
        .await?;
    let (proposal, songs) = AssistantSelection::decode(&output, candidates, limit)?;
    let mix = CuratedMix::new(
        preset,
        &proposal.title,
        &proposal.message,
        &songs,
        source,
        crate::now_millis(),
    );
    if mix.songs.is_empty() {
        return Err(AssistantError::NoMatches.into());
    }
    Ok(mix)
}

/// The anchor message when the person opens a playlist or curated mix in the assistant to edit it.
pub fn focus_message(
    title: &str,
    songs: Vec<Song>,
    saved: bool,
    playlist_id: Option<String>,
    curated_mix_id: Option<Uuid>,
) -> AssistantMessage {
    let mut message = AssistantMessage::assistant(
        t("This is the collection we are editing. Ask for a change, then review and confirm it."),
        Some(&t("Music assistant")),
    )
    .with_songs(songs);
    message.playlist_title = Some(title.to_string());
    message.saved = saved || playlist_id.is_some();
    message.playlist_id = playlist_id;
    message.curated_mix_id = curated_mix_id;
    message
}

/// Marks a proposal cancelled while it is pending.
pub fn cancel_edit(conversation: &mut AssistantConversation, message_id: Uuid) {
    if let Some(edit) = conversation
        .message_mut(message_id)
        .and_then(|m| m.edit.as_mut())
        && edit.status == AssistantEditStatus::Pending
    {
        edit.status = AssistantEditStatus::Cancelled;
    }
}

/// Starts confirming a pending proposal of `account`, marking it applying. Returns the proposal to apply.
pub fn begin_confirmed_edit(
    conversation: &mut AssistantConversation,
    message_id: Uuid,
    account: &str,
) -> Option<AssistantEditProposal> {
    let edit = conversation.message_mut(message_id)?.edit.as_mut()?;
    if edit.status != AssistantEditStatus::Pending || edit.account != account {
        return None;
    }
    edit.status = AssistantEditStatus::Applying;
    Some(edit.clone())
}

/// Before applying a proposal for a local selection or curated mix, checks it still matches what was reviewed.
pub fn verify_local_target(
    conversation: &AssistantConversation,
    edit: &AssistantEditProposal,
    curated: &[CuratedMix],
) -> Result<(), MusicError> {
    if edit.is_favorite_change() {
        return Ok(());
    }
    let same = |songs: &[Song]| {
        songs
            .iter()
            .map(Song::stream_identifier)
            .eq(edit.before.iter().map(Song::stream_identifier))
    };
    if let Some(id) = edit.target.curated_id {
        let mix = curated
            .iter()
            .find(|m| m.id == id)
            .ok_or(MusicError::PlaylistChanged)?;
        if mix.title != edit.target.title || !same(&mix.songs) {
            return Err(MusicError::PlaylistChanged);
        }
    } else if edit.target.playlist_id.is_none() {
        let original = edit
            .target
            .message_id
            .and_then(|id| conversation.message(id))
            .ok_or(MusicError::PlaylistChanged)?;
        let title = original
            .collection_title()
            .map(str::to_string)
            .unwrap_or_else(|| t("Library selection"));
        if !same(&original.songs) || title != edit.target.title {
            return Err(MusicError::PlaylistChanged);
        }
    }
    Ok(())
}

/// How a confirmed edit changes a curated mix stored by the app.
#[derive(Debug, Clone, PartialEq)]
pub enum CuratedChange {
    Remove(Uuid),
    Update {
        id: Uuid,
        title: String,
        songs: Vec<Song>,
    },
}

/// The curated mix change of a confirmed edit, if it targets one.
pub fn curated_change(edit: &AssistantEditProposal) -> Option<CuratedChange> {
    let id = edit.target.curated_id?;
    if edit.is_favorite_change() {
        return None;
    }
    Some(if edit.operation == AssistantEditOperation::Delete {
        CuratedChange::Remove(id)
    } else {
        CuratedChange::Update {
            id,
            title: edit.title.clone(),
            songs: edit.after.clone(),
        }
    })
}

/// Updates the chat after the app applied a confirmed edit.
pub fn finish_confirmed_edit(
    conversation: &mut AssistantConversation,
    message_id: Uuid,
    edit: &AssistantEditProposal,
) {
    let original = edit
        .target
        .message_id
        .and_then(|id| conversation.messages.iter().position(|m| m.id == id));
    if !edit.is_favorite_change() {
        if let Some(index) = original {
            let message = &mut conversation.messages[index];
            message.songs = edit.after.clone();
            message.collection_deleted = Some(edit.operation == AssistantEditOperation::Delete);
            if message.playlist_title.is_some() {
                message.playlist_title = Some(edit.title.clone());
            } else {
                message.selection_title = Some(edit.title.clone());
            }
            conversation.focus_message_id = Some(message.id);
        } else if edit.operation != AssistantEditOperation::Delete {
            let mut anchor =
                AssistantMessage::assistant(t("Changes applied."), Some(&t("Music assistant")))
                    .with_songs(edit.after.clone());
            anchor.playlist_title = Some(edit.title.clone());
            anchor.saved = edit.target.playlist_id.is_some();
            anchor.playlist_id = edit.target.playlist_id.clone();
            anchor.curated_mix_id = edit.target.curated_id;
            conversation.focus_message_id = Some(anchor.id);
            conversation.messages.push(anchor);
        }
    }
    if let Some(message) = conversation.message_mut(message_id) {
        message.text = t("Changes applied.");
        if let Some(edit) = message.edit.as_mut() {
            edit.status = AssistantEditStatus::Applied;
        }
    }
    conversation.updated = crate::now_millis();
}

/// Marks a confirmed edit failed. `detail` is the localized error.
pub fn fail_confirmed_edit(
    conversation: &mut AssistantConversation,
    message_id: Uuid,
    detail: &str,
) {
    if let Some(message) = conversation.message_mut(message_id) {
        if let Some(edit) = message.edit.as_mut() {
            edit.status = AssistantEditStatus::Failed;
        }
        message.text = format!(
            "{}\n{detail}",
            t(
                "The change could not be confirmed. Some server changes may have been saved. Refresh and ask for a new review before trying again."
            )
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assistant::conversation::AssistantRole;
    use crate::assistant::library::tests::FakeLibrary;
    use crate::models::{Playlist, SearchResults};
    use serde_json::{Value, json};
    use std::sync::Mutex;

    /// Replies in order and records every prompt.
    struct ScriptedResponder {
        replies: Mutex<Vec<Value>>,
        prompts: Mutex<Vec<(String, AssistantSchema)>>,
    }

    impl ScriptedResponder {
        fn new(replies: Vec<Value>) -> Self {
            Self {
                replies: Mutex::new(replies),
                prompts: Mutex::new(Vec::new()),
            }
        }
        fn inputs(&self) -> Vec<String> {
            self.prompts
                .lock()
                .unwrap()
                .iter()
                .map(|p| p.0.clone())
                .collect()
        }
    }

    impl AssistantResponder for ScriptedResponder {
        async fn respond(
            &self,
            _: &str,
            input: &str,
            schema: &AssistantSchema,
        ) -> Result<String, AssistantError> {
            self.prompts
                .lock()
                .unwrap()
                .push((input.to_string(), schema.clone()));
            let mut replies = self.replies.lock().unwrap();
            if replies.is_empty() {
                Err(AssistantError::Unavailable)
            } else {
                Ok(replies.remove(0).to_string())
            }
        }
    }

    fn plan(changes: Value) -> Value {
        let mut value = json!({"intent": "play", "query": "", "title": "", "artist": "", "message": "Fixture", "album": "", "genre": "",
            "from_year": 0, "to_year": 0, "queries": [], "action": "none", "target": "", "new_name": "", "all_tracks": false});
        for (k, v) in changes.as_object().unwrap() {
            value[k] = v.clone();
        }
        value
    }

    fn song(id: &str, title: &str, artist: &str) -> Song {
        Song {
            artist: Some(artist.into()),
            duration: Some(200.0),
            ..Song::new(id, title)
        }
    }

    fn context<'a>(
        library: &'a FakeLibrary,
        provider: AssistantProvider,
        curated: &'a [CuratedMix],
    ) -> TurnContext<'a, FakeLibrary> {
        TurnContext {
            provider,
            locale: "en-US",
            source: "Fixture model",
            account: "account",
            library: Some(library),
            offline_songs: None,
            preview: false,
            curated,
            known_songs: &[],
            cached_favorites: &[],
            newest_albums: &[],
        }
    }

    fn searching(songs: Vec<Song>) -> FakeLibrary {
        FakeLibrary {
            search: Some(Box::new(move |_, _, offset| {
                Ok(SearchResults {
                    song: Some(if offset == 0 { songs.clone() } else { vec![] }),
                    ..Default::default()
                })
            })),
            ..Default::default()
        }
    }

    fn run(
        ctx: &TurnContext<'_, FakeLibrary>,
        responder: &ScriptedResponder,
        conversation: &mut AssistantConversation,
        text: &str,
    ) -> Result<TurnReply, TurnError> {
        let (request, input) = start_turn(conversation, text, None, &[], &[]).unwrap();
        futures::executor::block_on(run_turn(
            ctx,
            responder,
            conversation,
            &request,
            &input,
            None,
        ))
    }

    #[test]
    fn requests_are_trimmed_bounded_and_name_new_chats() {
        assert_eq!(prepare_request("  hi  ").as_deref(), Some("hi"));
        assert!(prepare_request(" ").is_none());
        assert!(prepare_request(&"a".repeat(1_001)).is_none());
        let mut chat = AssistantConversation::new("New chat", AssistantProvider::Local, "");
        chat.draft = "draft".into();
        let song = Song {
            album_id: Some("private-album".into()),
            ..song("private-song", "Playing", "Artist")
        };
        let names: Vec<String> = (0..20).map(|i| format!("Playlist {i}")).collect();
        let (_, input) = start_turn(
            &mut chat,
            " Play something calm ",
            Some(&song),
            &names,
            &["Mix".into()],
        )
        .unwrap();
        assert_eq!(chat.title, "Play something calm");
        assert!(chat.draft.is_empty());
        assert_eq!(chat.messages[0].role, AssistantRole::User);
        assert!(input.ends_with("Latest user request:\nPlay something calm"));
        assert!(input.contains("Playing") && !input.contains("private"));
        // Local models see eight playlist names plus curated mixes.
        assert!(
            input.contains("Playlist 7")
                && !input.contains("Playlist 8")
                && input.contains("\"Mix\"")
        );
        chat.include_track = false;
        let (_, input) = start_turn(&mut chat, "Again", Some(&song), &[], &[]).unwrap();
        assert!(!input.contains("Playing\""));
        assert_eq!(chat.title, "Play something calm");
    }

    #[test]
    fn answers_and_unique_title_playback() {
        let library = searching(vec![
            song("1", "Città", "Artist"),
            song("2", "Other", "Artist"),
        ]);
        let ctx = context(&library, AssistantProvider::ChatGpt, &[]);
        let mut chat = AssistantConversation::new("Chat", AssistantProvider::ChatGpt, "");
        let responder =
            ScriptedResponder::new(vec![plan(json!({"intent": "answer", "message": "Facts"}))]);
        let reply = run(&ctx, &responder, &mut chat, "Who wrote it?").unwrap();
        assert_eq!(reply.message.text, "Facts");
        assert!(reply.play.is_none());

        let responder = ScriptedResponder::new(vec![plan(
            json!({"title": "citta", "query": "Città", "artist": "Artist"}),
        )]);
        let mut reply = run(&ctx, &responder, &mut chat, "Play Città").unwrap();
        assert_eq!(reply.play.as_ref().unwrap()[0].id, "1");
        reply.apply_playback_status(None);
        assert_eq!(reply.message.text, "Playback requested.");
        reply.apply(&mut chat);
        assert_eq!(chat.messages.last().unwrap().songs.len(), 1);
        assert_eq!(responder.inputs().len(), 1);
    }

    #[test]
    fn selections_send_temporary_ids_and_play_validated_tracks() {
        let songs: Vec<Song> = (0..250)
            .map(|i| song(&format!("server-{i}"), &format!("Song {i}"), "Band"))
            .collect();
        let library = searching(songs);
        let ctx = context(&library, AssistantProvider::ChatGpt, &[]);
        let mut chat = AssistantConversation::new("Chat", AssistantProvider::ChatGpt, "");
        let responder = ScriptedResponder::new(vec![
            plan(json!({"artist": "Band", "query": "Band"})),
            json!({"title": "Band mix", "message": "Because", "track_ids": ["t3", "t1"]}),
        ]);
        let mut reply = run(&ctx, &responder, &mut chat, "Play Band").unwrap();
        let prompts = responder.prompts.lock().unwrap().clone();
        let AssistantSchema::Selection { ids, limit } = &prompts[1].1 else {
            panic!("selection schema")
        };
        assert_eq!(ids.len(), 200);
        assert_eq!(*limit, 30);
        assert!(!prompts[1].0.contains("server-"));
        assert_eq!(
            reply
                .play
                .as_ref()
                .unwrap()
                .iter()
                .map(|s| s.id.as_str())
                .collect::<Vec<_>>(),
            ["server-3", "server-1"]
        );
        assert!(reply.focus);
        assert!(
            reply
                .message
                .text
                .contains("This selection uses a limited part of your library.")
        );
        reply.apply_playback_status(Some("Player failed"));
        assert!(reply.message.text.ends_with("\n\nPlayer failed"));
        let id = reply.message.id;
        reply.apply(&mut chat);
        assert_eq!(chat.focus_message_id, Some(id));
        assert_eq!(
            chat.active_selection().unwrap().selection_title.as_deref(),
            Some("Band mix")
        );
    }

    #[test]
    fn invented_ids_fail_and_dated_tags_rescue_an_empty_selection() {
        let library = searching(vec![
            Song {
                year: Some(1966),
                ..song("a", "A", "Band")
            },
            Song {
                year: Some(1990),
                ..song("b", "B", "Band")
            },
        ]);
        let ctx = context(&library, AssistantProvider::Local, &[]);
        let mut chat = AssistantConversation::new("Chat", AssistantProvider::Local, "");
        let invented = ScriptedResponder::new(vec![
            plan(json!({"artist": "Band"})),
            json!({"title": "T", "message": "", "track_ids": ["t9"]}),
        ]);
        assert_eq!(
            run(&ctx, &invented, &mut chat, "Play Band").unwrap_err(),
            TurnError::Assistant(AssistantError::InvalidResponse)
        );
        let empty = ScriptedResponder::new(vec![
            plan(json!({"artist": "Band", "from_year": 1960, "to_year": 1969})),
            json!({"title": "T", "message": "", "track_ids": []}),
        ]);
        let reply = run(&ctx, &empty, &mut chat, "Band in the sixties").unwrap();
        assert_eq!(
            reply
                .message
                .songs
                .iter()
                .map(|s| s.id.as_str())
                .collect::<Vec<_>>(),
            ["a"]
        );
        assert_eq!(
            reply.message.selection_title.as_deref(),
            Some("Library selection")
        );
    }

    #[test]
    fn edits_of_the_active_playlist_are_proposals() {
        let playlist = Playlist {
            id: "p1".into(),
            name: "Evening".into(),
            entry: Some(vec![song("a", "A", "X")]),
            ..Default::default()
        };
        let mut library = searching(vec![song("b", "Blue", "Y"), song("a", "A", "X")]);
        library.playlists = vec![playlist];
        let ctx = context(&library, AssistantProvider::ChatGpt, &[]);
        let mut chat = AssistantConversation::new("Chat", AssistantProvider::ChatGpt, "");
        let anchor = focus_message(
            "Evening",
            vec![song("a", "A", "X")],
            true,
            Some("p1".into()),
            None,
        );
        let anchor_id = anchor.id;
        chat.messages.push(anchor);
        chat.focus_message_id = Some(anchor_id);
        let responder = ScriptedResponder::new(vec![plan(
            json!({"intent": "manage", "action": "add", "title": "Blue", "query": "Blue", "target": "current"}),
        )]);
        let reply = run(&ctx, &responder, &mut chat, "Add Blue").unwrap();
        let edit = reply.message.edit.clone().unwrap();
        assert_eq!(edit.target.playlist_id.as_deref(), Some("p1"));
        assert_eq!(
            edit.after.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert_eq!(edit.status, AssistantEditStatus::Pending);
        let message_id = reply.message.id;
        reply.apply(&mut chat);
        assert!(begin_confirmed_edit(&mut chat, message_id, "other-account").is_none());
        let edit = begin_confirmed_edit(&mut chat, message_id, "account").unwrap();
        assert!(begin_confirmed_edit(&mut chat, message_id, "account").is_none());
        assert_eq!(verify_local_target(&chat, &edit, &[]), Ok(()));
        finish_confirmed_edit(&mut chat, message_id, &edit);
        assert_eq!(chat.message(anchor_id).unwrap().songs.len(), 2);
        assert_eq!(
            chat.message(message_id)
                .unwrap()
                .edit
                .as_ref()
                .unwrap()
                .status,
            AssistantEditStatus::Applied
        );
        assert_eq!(chat.focus_message_id, Some(anchor_id));
    }

    #[test]
    fn named_targets_resolve_uniquely_and_preview_cannot_edit() {
        let mut library = FakeLibrary {
            playlists: vec![
                Playlist {
                    id: "p1".into(),
                    name: "Road".into(),
                    entry: Some(vec![song("a", "A", "X"), song("b", "B", "X")]),
                    ..Default::default()
                },
                Playlist {
                    id: "p2".into(),
                    name: "Road".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let ctx = context(&library, AssistantProvider::ChatGpt, &[]);
        let mut chat = AssistantConversation::new("Chat", AssistantProvider::ChatGpt, "");
        let rename = ScriptedResponder::new(vec![plan(
            json!({"intent": "manage", "action": "rename", "target": "road", "new_name": "Trip"}),
        )]);
        assert_eq!(
            run(&ctx, &rename, &mut chat, "Rename road to Trip").unwrap_err(),
            TurnError::Assistant(AssistantError::AmbiguousTarget)
        );
        let none = ScriptedResponder::new(vec![plan(
            json!({"intent": "manage", "action": "delete", "target": "current"}),
        )]);
        assert_eq!(
            run(&ctx, &none, &mut chat, "Delete it").unwrap_err(),
            TurnError::Assistant(AssistantError::NoMatches)
        );
        library.playlists.pop();
        let ctx = context(&library, AssistantProvider::ChatGpt, &[]);
        let remove_all = ScriptedResponder::new(vec![plan(
            json!({"intent": "manage", "action": "remove", "target": "Road", "all_tracks": true}),
        )]);
        let edit = run(&ctx, &remove_all, &mut chat, "Empty Road")
            .unwrap()
            .message
            .edit
            .unwrap();
        assert!(edit.after.is_empty());
        assert_eq!(edit.target.message_id, None);
        let preview = TurnContext {
            preview: true,
            ..context(&library, AssistantProvider::ChatGpt, &[])
        };
        let delete = ScriptedResponder::new(vec![plan(
            json!({"intent": "manage", "action": "delete", "target": "Road"}),
        )]);
        assert_eq!(
            run(&preview, &delete, &mut chat, "Delete Road").unwrap_err(),
            TurnError::Assistant(AssistantError::InvalidResponse)
        );
    }

    #[test]
    fn explicit_favorite_commands_become_favorite_proposals() {
        let library = FakeLibrary {
            favorites: vec![song("f", "First Signal", "X"), song("g", "Other", "X")],
            ..Default::default()
        };
        let ctx = context(&library, AssistantProvider::ChatGpt, &[]);
        let mut chat = AssistantConversation::new("Chat", AssistantProvider::ChatGpt, "");
        let responder = ScriptedResponder::new(vec![plan(
            json!({"intent": "manage", "action": "remove", "title": "First Signal", "target": "preferiti"}),
        )]);
        let edit = run(
            &ctx,
            &responder,
            &mut chat,
            "Rimuovi First Signal dai preferiti",
        )
        .unwrap()
        .message
        .edit
        .unwrap();
        assert_eq!(edit.operation, AssistantEditOperation::Unfavorite);
        assert_eq!(edit.target.kind, AssistantEditTargetKind::Favorites);
        assert_eq!(
            edit.tracks
                .iter()
                .map(|s| s.id.as_str())
                .collect::<Vec<_>>(),
            ["f"]
        );
    }

    #[test]
    fn local_selection_edits_are_verified_before_applying() {
        let mut chat = AssistantConversation::new("Chat", AssistantProvider::Local, "");
        let mut selection = AssistantMessage::assistant("Mix", None)
            .with_songs(vec![song("a", "A", "X"), song("b", "B", "X")]);
        selection.selection_title = Some("Mix".into());
        let selection_id = selection.id;
        chat.messages.push(selection);
        let target = AssistantEditTarget {
            message_id: Some(selection_id),
            ..AssistantEditTarget::new(AssistantEditTargetKind::Selection, "Mix")
        };
        let before = chat.messages[0].songs.clone();
        let edit = AssistantEditProposal::new(
            "account",
            AssistantEditOperation::Delete,
            target,
            before,
            vec![],
            "",
            &HashSet::new(),
        )
        .unwrap();
        assert_eq!(verify_local_target(&chat, &edit, &[]), Ok(()));
        chat.messages[0].songs.pop();
        assert_eq!(
            verify_local_target(&chat, &edit, &[]),
            Err(MusicError::PlaylistChanged)
        );
        finish_confirmed_edit(&mut chat, Uuid::new_v4(), &edit);
        assert_eq!(chat.messages[0].collection_deleted, Some(true));
        assert!(chat.active_selection().is_none());
        fail_confirmed_edit(&mut chat, selection_id, "Detail");
    }

    #[test]
    fn curated_mixes_stay_within_half_an_hour() {
        let candidates: Vec<Song> = (0..20)
            .map(|i| Song {
                duration: Some(400.0),
                ..song(&format!("{i}"), "T", "A")
            })
            .collect();
        let ids: Vec<String> = (0..10).map(|i| format!("t{i}")).collect();
        let responder = ScriptedResponder::new(vec![
            json!({"title": "Calm", "message": "Why", "track_ids": ids}),
        ]);
        let mix = futures::executor::block_on(curate(
            &responder,
            AssistantProvider::Local,
            "en",
            "Local model",
            CuratorPreset::Evening,
            &candidates,
        ))
        .unwrap();
        assert_eq!(mix.songs.len(), 4);
        assert!(mix.duration() <= 1_800.0);
        let input = &responder.inputs()[0];
        assert!(input.contains("duration_seconds") && input.contains("1800 seconds"));
        let empty = ScriptedResponder::new(vec![]);
        assert!(
            futures::executor::block_on(curate(
                &empty,
                AssistantProvider::Local,
                "en",
                "",
                CuratorPreset::Focus,
                &[]
            ))
            .is_err()
        );
    }

    #[test]
    fn open_requests_sample_random_songs_and_favorites() {
        let library = FakeLibrary {
            random: (0..300).map(|i| song(&format!("r{i}"), "R", "A")).collect(),
            ..Default::default()
        };
        let favorites = vec![song("fav", "F", "A")];
        let ctx = TurnContext {
            cached_favorites: &favorites,
            ..context(&library, AssistantProvider::Local, &[])
        };
        let sample = futures::executor::block_on(ctx.sample_candidates()).unwrap();
        assert_eq!(sample.len(), 16);
        assert!(library.calls().contains(&"random:40".to_string()));
        let offline = vec![song("o", "O", "A")];
        let ctx = TurnContext {
            offline_songs: Some(&offline),
            library: None,
            ..context(&library, AssistantProvider::Local, &[])
        };
        assert_eq!(
            futures::executor::block_on(ctx.sample_candidates())
                .unwrap()
                .len(),
            1
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn turn_future_is_send() {
        fn assert_send<T: Send>(_: T) {}
        let library = FakeLibrary::default();
        let ctx = context(&library, AssistantProvider::Local, &[]);
        let responder = ScriptedResponder::new(vec![]);
        let chat = AssistantConversation::new("Chat", AssistantProvider::Local, "");
        assert_send(run_turn(&ctx, &responder, &chat, "r", "i", None));
        let local = LocalModelClient::new(&Default::default()).unwrap();
        assert_send(AssistantResponder::respond(
            &local,
            "i",
            "x",
            &AssistantSchema::Plan,
        ));
    }
}
