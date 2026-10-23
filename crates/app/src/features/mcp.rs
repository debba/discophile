//! Agents (MCP): a Model Context Protocol server inside the desktop app, so external agents such as Claude
//! can search the library, manage playlists and favorites, read music information and control playback
//! (see `discophile_core::mcp`).
//!
//! The server listens on `http://127.0.0.1:<port>/mcp` only while enabled in Settings > Agents. Every request
//! needs the bearer token kept in the system keychain; requests with a browser origin other than this
//! computer are refused. Library calls run in the background with the active library's client. Player calls
//! and confirmations run on the interface thread. Removing songs from a playlist and deleting a playlist wait
//! for the person to allow them in a dialog, for up to two minutes.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use discophile_core::client::{MusicClient, SearchLimits};
use discophile_core::l10n::{t, tf};
use discophile_core::lrclib::{SleepFuture, Sleeper};
use discophile_core::mcp::{self, InfoKind, Message, PlaybackAction, ToolCall};
use discophile_core::music_info::{InfoSubject, MusicInfoClient};
use discophile_core::{MusicError, Song, now_millis, podcast_order};
use serde_json::{Value, json};
use tokio::sync::oneshot;

use crate::app::App;
use crate::features::{discovery, library};
use crate::platform::http_server::{self, Request, Response};
use crate::platform::secrets;
use crate::player::{self, Mode};
use crate::tasks::{Tasks, update};

pub const DEFAULT_PORT: u16 = 47_321;
const TOKEN_SECRET: &str = "mcp:token";
const CONFIRM_TIMEOUT: Duration = Duration::from_secs(120);
const PLAYER_TIMEOUT: Duration = Duration::from_secs(15);
const ACTIVITY_LIMIT: usize = 30;
const SONG_CACHE_LIMIT: usize = 10_000;
/// Songs looked up one by one when an agent passes IDs it did not get from a tool.
const MAX_UNKNOWN_SONGS: usize = 100;
const SESSION_LIMIT: usize = 32;

#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Done,
    Failed(String),
    Declined,
}

/// A tool call shown in Settings > Agents.
#[derive(Clone, Debug)]
pub struct Activity {
    /// Unix milliseconds.
    pub time: i64,
    pub client: String,
    pub tool: String,
    pub outcome: Outcome,
}

/// A destructive change waiting for the person's decision.
pub struct Confirmation {
    pub id: u64,
    pub client: String,
    pub title: String,
    pub detail: String,
    reply: Option<oneshot::Sender<bool>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Off,
    Running { port: u16 },
    Failed(String),
}

/// What background calls read: the active library, preferences and songs seen in earlier results.
#[derive(Default)]
struct Context {
    client: Option<MusicClient>,
    online_info: bool,
    language: &'static str,
    songs: HashMap<String, Song>,
}

struct Shared {
    tasks: Tasks,
    token: Mutex<String>,
    context: Mutex<Context>,
    sessions: Mutex<HashMap<String, String>>,
    activity: Mutex<VecDeque<Activity>>,
    info: MusicInfoClient,
    next_confirmation: Mutex<u64>,
}

pub struct McpState {
    pub status: Status,
    pub pending: VecDeque<Confirmation>,
    server: Option<http_server::Server>,
    shared: Option<Arc<Shared>>,
    /// Library generation, online information and language last copied to the shared context.
    synced: Option<(u64, bool, &'static str)>,
}

impl Default for McpState {
    fn default() -> Self {
        Self { status: Status::Off, pending: VecDeque::new(), server: None, shared: None, synced: None }
    }
}

pub fn start(app: &mut App) {
    let sleeper: Sleeper = Arc::new(|ms| -> SleepFuture { Box::pin(crate::platform::sleep_ms(ms)) });
    let token = secrets::get(TOKEN_SECRET).filter(|t| t.len() >= 32).unwrap_or_default();
    app.mcp.shared = Some(Arc::new(Shared {
        tasks: app.tasks.clone(),
        token: Mutex::new(token),
        context: Mutex::default(),
        sessions: Mutex::default(),
        activity: Mutex::default(),
        info: MusicInfoClient::new().with_sleeper(sleeper),
        next_confirmation: Mutex::new(0),
    }));
    if app.settings.mcp_enabled {
        start_server(app);
    }
}

/// Keeps the background context in step with the active library and preferences.
pub fn tick(app: &mut App) {
    let Some(shared) = &app.mcp.shared else { return };
    let current = (app.session.generation, app.settings.online_music_info, app.settings.effective_language().code());
    if app.mcp.synced == Some(current) {
        return;
    }
    let library_changed = app.mcp.synced.is_none_or(|(generation, _, _)| generation != current.0);
    app.mcp.synced = Some(current);
    if let Ok(mut context) = shared.context.lock() {
        context.client = app.client();
        context.online_info = current.1;
        context.language = current.2;
        if library_changed {
            context.songs.clear();
        }
    }
}

pub fn on_library_changed(app: &mut App) {
    // Changes waiting for a decision belong to the previous library.
    while let Some(mut confirmation) = app.mcp.pending.pop_front() {
        if let Some(reply) = confirmation.reply.take() {
            let _ = reply.send(false);
        }
    }
}

// MARK: Settings

pub fn token(app: &App) -> String {
    app.mcp.shared.as_ref().and_then(|s| s.token.lock().ok().map(|t| t.clone())).unwrap_or_default()
}

pub fn endpoint(app: &App) -> String {
    let port = match app.mcp.status {
        Status::Running { port } => port,
        _ => app.settings.mcp_port,
    };
    format!("http://127.0.0.1:{port}/mcp")
}

pub fn activity(app: &App) -> Vec<Activity> {
    app.mcp.shared.as_ref().and_then(|s| s.activity.lock().ok().map(|a| a.iter().rev().cloned().collect())).unwrap_or_default()
}

pub fn set_enabled(app: &mut App, enabled: bool) {
    app.settings.mcp_enabled = enabled;
    app.settings.save();
    if enabled {
        start_server(app);
    } else {
        stop_server(app);
    }
}

pub fn set_port(app: &mut App, port: u16) {
    if port < 1024 || port == app.settings.mcp_port {
        return;
    }
    app.settings.mcp_port = port;
    app.settings.save();
    if app.settings.mcp_enabled {
        start_server(app);
    }
}

/// A new token: agents set up with the old one stop working.
pub fn regenerate_token(app: &mut App) {
    let token = new_token();
    secrets::set(TOKEN_SECRET, &token);
    if let Some(shared) = &app.mcp.shared
        && let Ok(mut current) = shared.token.lock()
    {
        *current = token;
    }
    if let Some(shared) = &app.mcp.shared
        && let Ok(mut sessions) = shared.sessions.lock()
    {
        sessions.clear();
    }
}

fn new_token() -> String {
    let bytes: [u8; 32] = rand::random();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn start_server(app: &mut App) {
    stop_server(app);
    let Some(shared) = app.mcp.shared.clone() else { return };
    if token(app).is_empty() {
        regenerate_token(app);
    }
    let handler: http_server::Handler = Arc::new(move |request| {
        let shared = shared.clone();
        Box::pin(async move { handle(shared, request).await })
    });
    match http_server::start(app.settings.mcp_port, mcp::MAX_BODY_BYTES, handler) {
        Ok(server) => {
            app.mcp.status = Status::Running { port: server.port };
            app.mcp.server = Some(server);
        }
        Err(error) => {
            log::warn!("Unable to start the MCP server: {error}");
            app.mcp.status = Status::Failed(if error.kind() == std::io::ErrorKind::AddrInUse {
                tf("Port %lld is already in use. Choose another port.", &[&app.settings.mcp_port])
            } else {
                t("The agent server could not start.")
            });
        }
    }
}

fn stop_server(app: &mut App) {
    app.mcp.server = None;
    app.mcp.status = Status::Off;
}

// MARK: HTTP

/// Browsers send an Origin; only pages served from this computer may call the endpoint.
pub fn allowed_origin(origin: Option<&str>) -> bool {
    let Some(origin) = origin else { return true };
    let Ok(url) = url::Url::parse(origin) else { return false };
    matches!(url.scheme(), "http" | "https")
        && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
}

/// Compares the bearer token in constant time.
pub fn authorized(header: Option<&str>, token: &str) -> bool {
    let Some(given) = header.and_then(|h| h.strip_prefix("Bearer ").or_else(|| h.strip_prefix("bearer "))) else {
        return false;
    };
    let given = given.trim().as_bytes();
    !token.is_empty()
        && given.len() == token.len()
        && given.iter().zip(token.as_bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
}

async fn handle(shared: Arc<Shared>, request: Request) -> Response {
    if request.path != "/mcp" {
        return Response::empty(404);
    }
    if !allowed_origin(request.header("origin")) {
        return Response::empty(403);
    }
    let token = shared.token.lock().map(|t| t.clone()).unwrap_or_default();
    if !authorized(request.header("authorization"), &token) {
        return Response::empty(401).with_header("WWW-Authenticate", "Bearer");
    }
    match request.method.as_str() {
        "POST" => {}
        "DELETE" => {
            if let Some(session) = request.header("mcp-session-id")
                && let Ok(mut sessions) = shared.sessions.lock()
            {
                sessions.remove(session);
            }
            return Response::empty(204);
        }
        // No server-initiated stream is offered.
        _ => return Response::empty(405).with_header("Allow", "POST, DELETE"),
    }
    if let Some(version) = request.header("mcp-protocol-version")
        && !mcp::PROTOCOL_VERSIONS.contains(&version)
    {
        return Response::json(400, mcp::error(Value::Null, mcp::INVALID_REQUEST, "Unsupported protocol version").to_string());
    }
    let messages = match mcp::parse_body(&request.body) {
        Ok(messages) => messages,
        Err(error) => return Response::json(400, error.to_string()),
    };
    let batch = messages.len() > 1 || request.body.trim_ascii_start().starts_with(b"[");
    let session = request.header("mcp-session-id").map(str::to_string);
    let client = session
        .as_ref()
        .and_then(|s| shared.sessions.lock().ok().and_then(|m| m.get(s).cloned()))
        .unwrap_or_else(|| t("Agent"));
    let mut replies = Vec::new();
    let mut new_session = None;
    for message in messages {
        match mcp::classify(&message) {
            Err(error) => replies.push(error),
            Ok(Message::Notification { .. } | Message::Other) => {}
            Ok(Message::Request { id, method, params }) => {
                if method == "initialize" {
                    let name = mcp::client_name(&params).unwrap_or_else(|| t("Agent"));
                    let session = uuid::Uuid::new_v4().to_string();
                    if let Ok(mut sessions) = shared.sessions.lock() {
                        if sessions.len() >= SESSION_LIMIT {
                            sessions.clear();
                        }
                        sessions.insert(session.clone(), name);
                    }
                    new_session = Some(session);
                }
                replies.push(dispatch(&shared, &client, id, &method, params).await);
            }
        }
    }
    if replies.is_empty() {
        return Response::empty(202);
    }
    let body = if batch { Value::Array(replies).to_string() } else { replies.remove(0).to_string() };
    let response = Response::json(200, body);
    match new_session {
        Some(session) => response.with_header("Mcp-Session-Id", session),
        None => response,
    }
}

async fn dispatch(shared: &Arc<Shared>, client: &str, id: Value, method: &str, params: Value) -> Value {
    match method {
        "initialize" => mcp::response(id, mcp::initialize_result(&params, env!("CARGO_PKG_VERSION"))),
        "ping" => mcp::response(id, json!({})),
        "tools/list" => mcp::response(id, mcp::tools_list()),
        "tools/call" => {
            let Some(name) = params.get("name").and_then(Value::as_str) else {
                return mcp::error(id, mcp::INVALID_PARAMS, "name is required");
            };
            let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);
            let result = match mcp::parse_tool_call(name, &arguments) {
                Err(message) => Err(Failure::Invalid(message)),
                Ok(call) => run(shared, client, call).await,
            };
            let outcome = match &result {
                Ok(_) => Outcome::Done,
                Err(Failure::Declined) => Outcome::Declined,
                Err(failure) => Outcome::Failed(failure.message()),
            };
            record(shared, client, name, outcome);
            mcp::response(
                id,
                match result {
                    Ok(value) => mcp::tool_result(value),
                    Err(failure) => mcp::tool_error(&failure.message()),
                },
            )
        }
        _ => mcp::error(id, mcp::METHOD_NOT_FOUND, "Method not found"),
    }
}

fn record(shared: &Shared, client: &str, tool: &str, outcome: Outcome) {
    if let Ok(mut activity) = shared.activity.lock() {
        if activity.len() >= ACTIVITY_LIMIT {
            activity.pop_front();
        }
        activity.push_back(Activity { time: now_millis(), client: client.to_string(), tool: tool.to_string(), outcome });
    }
    shared.tasks.ctx().request_repaint();
}

// MARK: Tools

enum Failure {
    Invalid(String),
    Music(MusicError),
    Message(String),
    Declined,
}

impl Failure {
    /// English text for the agent.
    fn message(&self) -> String {
        match self {
            Self::Invalid(message) | Self::Message(message) => message.clone(),
            Self::Declined => "The person declined this change in Discophile.".into(),
            Self::Music(error) => match error {
                MusicError::NotFound => "Not found in this library.".into(),
                MusicError::Unsupported => "This music server does not support that request.".into(),
                MusicError::Authentication | MusicError::Forbidden => "The music server refused the request.".into(),
                MusicError::PlaylistChanged => "The playlist changed meanwhile. Read it again and retry.".into(),
                _ => "The music server could not be reached.".into(),
            },
        }
    }
}

impl From<MusicError> for Failure {
    fn from(error: MusicError) -> Self {
        Self::Music(error)
    }
}

fn client(shared: &Shared) -> Result<MusicClient, Failure> {
    shared
        .context
        .lock()
        .ok()
        .and_then(|c| c.client.clone())
        .ok_or_else(|| Failure::Message("No music library is connected in Discophile.".into()))
}

/// Remembers songs from results so later calls can use their IDs without extra requests.
fn remember(shared: &Shared, songs: &[Song]) {
    if let Ok(mut context) = shared.context.lock() {
        if context.songs.len() + songs.len() > SONG_CACHE_LIMIT {
            context.songs.clear();
        }
        for song in songs {
            context.songs.insert(song.id.clone(), song.clone());
        }
    }
}

/// Songs for IDs, in order: remembered ones first, the rest fetched from the server.
async fn songs(shared: &Shared, client: &MusicClient, ids: &[String]) -> Result<Vec<Song>, Failure> {
    let known: HashMap<String, Song> = {
        let context = shared.context.lock().map_err(|_| Failure::Message("Busy, try again.".into()))?;
        ids.iter().filter_map(|id| context.songs.get(id).map(|s| (id.clone(), s.clone()))).collect()
    };
    let unknown: Vec<&String> = ids.iter().filter(|id| !known.contains_key(*id)).collect();
    if unknown.len() > MAX_UNKNOWN_SONGS {
        return Err(Failure::Invalid(format!(
            "Too many song IDs not seen in earlier results ({}). Get them with search_library, get_album or get_playlist first.",
            unknown.len()
        )));
    }
    let mut fetched = HashMap::new();
    for id in unknown {
        match client.song(id).await {
            Ok(song) => {
                fetched.insert(id.clone(), song);
            }
            Err(MusicError::NotFound | MusicError::InvalidResponse) => {
                return Err(Failure::Invalid(format!("Unknown song ID: {id}")));
            }
            Err(error) => return Err(error.into()),
        }
    }
    remember(shared, &fetched.values().cloned().collect::<Vec<_>>());
    Ok(ids.iter().filter_map(|id| known.get(id).or_else(|| fetched.get(id)).cloned()).collect())
}

async fn run(shared: &Arc<Shared>, agent: &str, call: ToolCall) -> Result<Value, Failure> {
    let reload = call.changes_library().then(|| match &call {
        ToolCall::AddToPlaylist { playlist_id, .. }
        | ToolCall::RenamePlaylist { playlist_id, .. }
        | ToolCall::RemoveFromPlaylist { playlist_id, .. }
        | ToolCall::DeletePlaylist { playlist_id } => Some(playlist_id.clone()),
        _ => None,
    });
    let favorites = matches!(call, ToolCall::SetFavorite { .. });
    let result = execute(shared, agent, call).await;
    if result.is_ok()
        && let Some(playlist) = reload
    {
        shared.tasks.send(update(move |app| {
            if favorites {
                library::load_favorites(app);
            } else {
                library::load_playlists(app);
                if let Some(id) = playlist
                    && library::find_playlist(app, &id).is_some()
                {
                    library::load_playlist(app, &id);
                }
            }
        }));
    }
    result
}

async fn execute(shared: &Arc<Shared>, agent: &str, call: ToolCall) -> Result<Value, Failure> {
    if call.uses_player() {
        return player_call(shared, call).await;
    }
    let client = client(shared)?;
    match call {
        ToolCall::Search { query, limit } => {
            let limits = SearchLimits { song_count: limit, album_count: limit, artist_count: limit, ..Default::default() };
            let results = client.search(&query, limits).await?;
            remember(shared, results.songs());
            Ok(json!({
                "artists": results.artists().iter().map(mcp::artist_json).collect::<Vec<_>>(),
                "albums": results.albums().iter().map(mcp::album_json).collect::<Vec<_>>(),
                "songs": mcp::songs_json(results.songs()),
            }))
        }
        ToolCall::GetAlbum { id } => {
            let album = client.album(&id).await?;
            remember(shared, album.song.as_deref().unwrap_or_default());
            Ok(mcp::album_json(&album))
        }
        ToolCall::GetArtist { id } => Ok(mcp::artist_json(&client.artist(&id).await?)),
        ToolCall::ListPlaylists => {
            let playlists = client.playlists().await?;
            Ok(json!({"playlists": playlists.iter().map(mcp::playlist_json).collect::<Vec<_>>()}))
        }
        ToolCall::GetPlaylist { id } => {
            let playlist = client.playlist(&id).await?;
            remember(shared, playlist.songs());
            Ok(mcp::playlist_json(&playlist))
        }
        ToolCall::ListFavorites => {
            let favorites = client.favorites().await?;
            remember(shared, favorites.songs());
            Ok(json!({
                "artists": favorites.artists().iter().map(mcp::artist_json).collect::<Vec<_>>(),
                "albums": favorites.albums().iter().map(mcp::album_json).collect::<Vec<_>>(),
                "songs": mcp::songs_json(favorites.songs()),
            }))
        }
        ToolCall::ListPodcasts => {
            let channels = client.podcasts(None, Some(false)).await?;
            Ok(json!({"podcasts": channels.iter().map(|c| mcp::podcast_json(c, None)).collect::<Vec<_>>()}))
        }
        ToolCall::GetPodcast { id } => {
            let channel = client.podcasts(Some(&id), Some(true)).await?.into_iter().next().ok_or(MusicError::NotFound)?;
            let playable: Vec<_> = channel.episodes().iter().filter(|e| e.is_playable()).cloned().collect();
            let episodes: Vec<Song> = podcast_order::newest_first(&playable).iter().map(|e| e.song(&channel)).collect();
            remember(shared, &episodes);
            Ok(mcp::podcast_json(&channel, Some(&episodes)))
        }
        ToolCall::RandomSongs { count } => {
            let songs = client.random_songs(count).await?;
            remember(shared, &songs);
            Ok(json!({"songs": mcp::songs_json(&songs)}))
        }
        ToolCall::SimilarSongs { song_id, count } => {
            let seed = songs(shared, &client, &[song_id]).await?.remove(0);
            let artist = seed.artist_id.clone().ok_or_else(|| Failure::Message("This song has no artist on the server.".into()))?;
            let similar = client.similar_songs(&artist, count).await?;
            remember(shared, &similar);
            Ok(json!({"songs": mcp::songs_json(&similar)}))
        }
        ToolCall::MusicInfo { kind, id, name, artist } => {
            let subject = match (kind, id) {
                (InfoKind::Artist, Some(id)) => InfoSubject::artist(&client.artist(&id).await?),
                (InfoKind::Album, Some(id)) => InfoSubject::album(&client.album(&id).await?),
                (InfoKind::Song, Some(id)) => InfoSubject::song(&songs(shared, &client, &[id]).await?.remove(0)),
                (InfoKind::Podcast, Some(id)) => InfoSubject::podcast(
                    &client.podcasts(Some(&id), Some(false)).await?.into_iter().next().ok_or(MusicError::NotFound)?,
                ),
                (kind, None) => {
                    let name = name.unwrap_or_default();
                    match kind {
                        InfoKind::Artist => InfoSubject::Artist { name, id: None },
                        InfoKind::Album => InfoSubject::Album { title: name, artist, year: None, genre: None, id: None },
                        InfoKind::Song => InfoSubject::Song {
                            title: name,
                            artist,
                            album: None,
                            year: None,
                            genre: None,
                            album_id: None,
                            artist_id: None,
                        },
                        InfoKind::Podcast => InfoSubject::Podcast { title: name, description: None },
                    }
                }
            };
            let (online, language) = shared.context.lock().map(|c| (c.online_info, c.language)).unwrap_or((false, "en"));
            let sources = shared.info.gather(&subject, Some(&client), online, if language.is_empty() { "en" } else { language }).await;
            let mut value = mcp::sources_json(&sources.sources);
            value["subject"] = subject.metadata();
            if !online {
                value["note"] = "Online music information (Wikipedia, MusicBrainz) is off in Discophile's settings.".into();
            }
            Ok(value)
        }
        ToolCall::CreatePlaylist { name, song_ids } => {
            let songs = songs(shared, &client, &song_ids).await?;
            let playlist = client.create_playlist(&name, &songs).await?;
            Ok(json!({"created": true, "playlist": playlist.as_ref().map(mcp::playlist_json), "song_count": songs.len()}))
        }
        ToolCall::AddToPlaylist { playlist_id, song_ids } => {
            let songs = songs(shared, &client, &song_ids).await?;
            let result = client.add_to_playlist(&playlist_id, &songs).await?;
            Ok(json!({"added": result.added_count, "skipped_duplicates": result.skipped_count}))
        }
        ToolCall::RenamePlaylist { playlist_id, name } => {
            client.rename_playlist(&playlist_id, &name).await?;
            Ok(json!({"renamed": true, "name": name}))
        }
        ToolCall::RemoveFromPlaylist { playlist_id, song_ids } => {
            let playlist = client.playlist(&playlist_id).await?;
            let current = playlist.songs().to_vec();
            let remaining: Vec<Song> = current.iter().filter(|s| !song_ids.contains(&s.id)).cloned().collect();
            let removed = current.len() - remaining.len();
            if removed == 0 {
                return Err(Failure::Invalid("None of these songs is in the playlist.".into()));
            }
            let titles: Vec<String> =
                current.iter().filter(|s| song_ids.contains(&s.id)).take(5).map(|s| format!("• {}", s.title)).collect();
            let title = tf("Remove %lld songs from “%@”?", &[&(removed as i64), &playlist.name]);
            confirm(shared, agent, title, titles.join("\n")).await?;
            client.replace_playlist_songs(&playlist_id, &current, &remaining).await?;
            Ok(json!({"removed": removed, "song_count": remaining.len()}))
        }
        ToolCall::DeletePlaylist { playlist_id } => {
            let playlist = client.playlist(&playlist_id).await?;
            let title = tf("Delete the playlist “%@”?", &[&playlist.name]);
            let detail = discophile_core::count("%lld songs", playlist.songs().len() as i64);
            confirm(shared, agent, title, detail).await?;
            client.delete_playlist(&playlist_id).await?;
            Ok(json!({"deleted": true}))
        }
        ToolCall::SetFavorite { song_ids, favorite } => {
            for id in &song_ids {
                client.favorite(id, favorite).await?;
            }
            Ok(json!({"updated": song_ids.len(), "favorite": favorite}))
        }
        ToolCall::NowPlaying
        | ToolCall::Play { .. }
        | ToolCall::Control { .. }
        | ToolCall::Enqueue { .. }
        | ToolCall::StartRadio { .. } => unreachable!("player calls are handled above"),
    }
}

/// Shows the confirmation dialog and waits for the decision.
async fn confirm(shared: &Shared, agent: &str, title: String, detail: String) -> Result<(), Failure> {
    let (sender, receiver) = oneshot::channel();
    let id = {
        let mut next = shared.next_confirmation.lock().map_err(|_| Failure::Message("Busy, try again.".into()))?;
        *next += 1;
        *next
    };
    let client = agent.to_string();
    let notice = title.clone();
    shared.tasks.send(update(move |app| {
        app.mcp.pending.push_back(Confirmation { id, client: client.clone(), title, detail, reply: Some(sender) });
        notify(&client, &notice);
    }));
    match tokio::time::timeout(CONFIRM_TIMEOUT, receiver).await {
        Ok(Ok(true)) => Ok(()),
        Ok(Ok(false)) | Ok(Err(_)) => Err(Failure::Declined),
        Err(_) => {
            shared.tasks.send(update(move |app| app.mcp.pending.retain(|c| c.id != id)));
            Err(Failure::Message("The person did not confirm the change in time.".into()))
        }
    }
}

/// A system notification, since the window may be hidden in the tray.
fn notify(agent: &str, title: &str) {
    let summary = format!("Discophile · {agent}");
    let body = format!("{title} {}", t("Open Discophile to allow or decline."));
    // Showing a notification can block on the session bus, so it never runs on the interface thread.
    let _ = std::thread::Builder::new().name("mcp-notification".into()).spawn(move || {
        if let Err(error) = notify_rust::Notification::new().appname("Discophile").summary(&summary).body(&body).show() {
            log::warn!("Unable to show the agent notification: {error}");
        }
    });
}

/// The person's decision on the first pending confirmation.
pub fn decide(app: &mut App, allow: bool) {
    if let Some(mut confirmation) = app.mcp.pending.pop_front()
        && let Some(reply) = confirmation.reply.take()
    {
        let _ = reply.send(allow);
    }
}

// MARK: Player

async fn player_call(shared: &Arc<Shared>, call: ToolCall) -> Result<Value, Failure> {
    // Songs are resolved here; the interface thread only changes the player.
    let queue = match &call {
        ToolCall::Play { song_ids, album_id, playlist_id, .. } => {
            let client = client(shared)?;
            let songs = if let Some(id) = album_id {
                client.album(id).await?.song.unwrap_or_default()
            } else if let Some(id) = playlist_id {
                client.playlist(id).await?.entry.unwrap_or_default()
            } else {
                songs(shared, &client, song_ids).await?
            };
            if songs.is_empty() {
                return Err(Failure::Message("There is nothing to play.".into()));
            }
            remember(shared, &songs);
            songs
        }
        ToolCall::Enqueue { song_ids, .. } => songs(shared, &client(shared)?, song_ids).await?,
        ToolCall::StartRadio { song_id } => songs(shared, &client(shared)?, std::slice::from_ref(song_id)).await?,
        _ => Vec::new(),
    };
    let (sender, receiver) = oneshot::channel();
    shared.tasks.send(update(move |app| {
        let _ = sender.send(apply_player(app, call, queue));
    }));
    match tokio::time::timeout(PLAYER_TIMEOUT, receiver).await {
        Ok(Ok(result)) => result.map_err(Failure::Message),
        _ => Err(Failure::Message("Discophile did not respond. Check that the app is running.".into())),
    }
}

fn apply_player(app: &mut App, call: ToolCall, songs: Vec<Song>) -> Result<Value, String> {
    match call {
        ToolCall::NowPlaying => Ok(now_playing(app)),
        ToolCall::Play { shuffle, start_index, .. } => {
            let index = start_index.min(songs.len().saturating_sub(1));
            let count = songs.len();
            library::play(app, songs, index, shuffle);
            Ok(json!({"playing": true, "queue_length": count}))
        }
        ToolCall::Control { action, seek_seconds, volume } => {
            if app.player.mode == Mode::Idle && action.is_some_and(|a| a != PlaybackAction::Stop) {
                return Err("Nothing is playing.".into());
            }
            match action {
                Some(PlaybackAction::Pause) => player::pause(app),
                Some(PlaybackAction::Resume) => player::play(app),
                Some(PlaybackAction::Toggle) => player::toggle(app),
                Some(PlaybackAction::Next) => player::next(app),
                Some(PlaybackAction::Previous) => player::previous(app),
                Some(PlaybackAction::Stop) => player::stop(app),
                None => {}
            }
            if let Some(seconds) = seek_seconds {
                if app.player.is_live() {
                    return Err("Live radio cannot seek.".into());
                }
                player::seek(app, seconds);
            }
            if let Some(volume) = volume {
                player::set_volume(app, volume);
            }
            Ok(now_playing(app))
        }
        ToolCall::Enqueue { next, .. } => {
            if app.player.mode != Mode::Music {
                return Err("The queue is available while music plays. Use play instead.".into());
            }
            let count = songs.len();
            if next { player::play_next(app, songs) } else { player::add_to_queue(app, songs) }
            Ok(json!({"queued": count}))
        }
        ToolCall::StartRadio { .. } => {
            let seed = songs.into_iter().next().ok_or("Unknown song.")?;
            if seed.is_podcast() {
                return Err("Personal radio starts from songs, not podcast episodes.".into());
            }
            discovery::start_personal_radio(app, seed);
            Ok(json!({"started": true}))
        }
        _ => Err("Not a playback tool.".into()),
    }
}

fn now_playing(app: &App) -> Value {
    let mode = match app.player.mode {
        Mode::Idle => "idle",
        Mode::Music => "music",
        Mode::Audiobook => "audiobook",
        Mode::Radio => "radio",
    };
    let mut value = json!({
        "mode": mode,
        "playing": app.player.is_playing(),
        "volume": (app.player.volume * 100.0).round() / 100.0,
        "shuffle": app.player.shuffled,
        "repeat": format!("{:?}", player::repeat_mode(app)).to_lowercase(),
    });
    if app.player.mode != Mode::Idle {
        value["position_seconds"] = json!(app.player.position().round() as i64);
        if let Some(duration) = app.player.duration() {
            value["duration_seconds"] = json!(duration.round() as i64);
        }
    }
    match app.player.mode {
        Mode::Music => {
            if let Some(song) = app.player.queue.current() {
                value["current"] = mcp::song_json(song);
            }
            let upcoming: Vec<Value> = app.player.queue.upcoming().iter().take(20).map(mcp::song_json).collect();
            value["upcoming_count"] = json!(app.player.queue.upcoming().len());
            value["upcoming"] = Value::Array(upcoming);
        }
        Mode::Radio | Mode::Audiobook => {
            if let Some((title, subtitle)) = crate::ui::player_bar::actions::now_playing_text(app) {
                value["current"] = json!({"title": title, "subtitle": subtitle});
            }
        }
        Mode::Idle => {}
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_are_limited_to_this_computer() {
        assert!(allowed_origin(None));
        assert!(allowed_origin(Some("http://localhost:3000")));
        assert!(allowed_origin(Some("http://127.0.0.1")));
        assert!(!allowed_origin(Some("https://evil.example")));
        assert!(!allowed_origin(Some("http://localhost.evil.example")));
        assert!(!allowed_origin(Some("null")));
    }

    #[test]
    fn bearer_tokens_must_match_exactly() {
        let token = "a".repeat(64);
        assert!(authorized(Some(&format!("Bearer {token}")), &token));
        assert!(!authorized(Some(&format!("Bearer {}", "b".repeat(64))), &token));
        assert!(!authorized(Some("Bearer short"), &token));
        assert!(!authorized(Some(&token), &token));
        assert!(!authorized(None, &token));
        assert!(!authorized(Some("Bearer "), ""));
        assert_eq!(new_token().len(), 64);
    }
}
