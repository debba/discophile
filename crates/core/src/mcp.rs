//! Model Context Protocol for external agents: JSON-RPC messages, the tool catalog, argument validation
//! and compact JSON for library items. The app owns the transport (HTTP on the loopback interface with a
//! bearer token), confirmations and execution.
//!
//! Tools cover the library (search, albums, artists, playlists, favorites, podcasts, music information),
//! playlist and favorite changes, and playback. Removing songs from a playlist and deleting a playlist are
//! destructive: the app asks the person to confirm them in its window.

use serde_json::{Map, Value, json};

use crate::models::{Album, Artist, Playlist, PodcastChannel, PodcastEpisode, Song};
use crate::music_info::{InfoSource, prefix};

/// Supported protocol revisions, newest first.
pub const PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26"];
pub const SERVER_NAME: &str = "discophile";
pub const MAX_SONG_IDS: usize = 500;
pub const MAX_RESULTS: usize = 50;
pub const MAX_BODY_BYTES: usize = 1_000_000;

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;

// MARK: JSON-RPC

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Request { id: Value, method: String, params: Value },
    Notification { method: String, params: Value },
    /// A response or anything else a client may post; it needs no answer.
    Other,
}

/// The messages of a POST body (a single message or a legacy batch), or the error response to send.
pub fn parse_body(body: &[u8]) -> Result<Vec<Value>, Value> {
    let value: Value = serde_json::from_slice(body).map_err(|_| error(Value::Null, PARSE_ERROR, "Parse error"))?;
    match value {
        Value::Array(items) if items.is_empty() => Err(error(Value::Null, INVALID_REQUEST, "Empty batch")),
        Value::Array(items) => Ok(items),
        Value::Object(_) => Ok(vec![value]),
        _ => Err(error(Value::Null, INVALID_REQUEST, "Invalid request")),
    }
}

pub fn classify(message: &Value) -> Result<Message, Value> {
    let Some(object) = message.as_object() else {
        return Err(error(Value::Null, INVALID_REQUEST, "Invalid request"));
    };
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err(error(object.get("id").cloned().unwrap_or(Value::Null), INVALID_REQUEST, "Invalid request"));
    }
    let params = object.get("params").cloned().unwrap_or(Value::Null);
    match (object.get("method").and_then(Value::as_str), object.get("id")) {
        (Some(method), Some(id)) if id.is_string() || id.is_number() => {
            Ok(Message::Request { id: id.clone(), method: method.to_string(), params })
        }
        (Some(_), Some(id)) => Err(error(id.clone(), INVALID_REQUEST, "Invalid request ID")),
        (Some(method), None) => Ok(Message::Notification { method: method.to_string(), params }),
        (None, _) => Ok(Message::Other),
    }
}

pub fn response(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

pub fn error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

/// The protocol revision to answer with: the client's when supported, otherwise the newest.
pub fn negotiate(requested: Option<&str>) -> &'static str {
    requested.and_then(|r| PROTOCOL_VERSIONS.iter().find(|v| **v == r).copied()).unwrap_or(PROTOCOL_VERSIONS[0])
}

pub fn initialize_result(params: &Value, version: &str) -> Value {
    let requested = params.get("protocolVersion").and_then(Value::as_str);
    json!({
        "protocolVersion": negotiate(requested),
        "capabilities": {"tools": {"listChanged": false}},
        "serverInfo": {"name": SERVER_NAME, "title": "Discophile", "version": version},
        "instructions": INSTRUCTIONS,
    })
}

/// The client name from `initialize`, bounded and printable.
pub fn client_name(params: &Value) -> Option<String> {
    let info = params.get("clientInfo")?;
    let name = info.get("title").or_else(|| info.get("name"))?.as_str()?;
    let name: String = name.chars().filter(|c| !c.is_control()).take(60).collect();
    (!name.trim().is_empty()).then(|| name.trim().to_string())
}

const INSTRUCTIONS: &str = "Discophile is the person's music player, connected to their own music server (Subsonic, \
Navidrome or Jellyfin). Find IDs with search_library and the get_/list_ tools before using them. Playlists can only \
contain songs from this library. Removing songs from a playlist and deleting a playlist must be confirmed by the person \
in the Discophile window and may be declined. get_music_info returns sourced notes about an artist, album, song or \
podcast; prefer them over guessing. Playback tools control what plays on this computer.";

// MARK: Tools

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InfoKind {
    Artist,
    Album,
    Song,
    Podcast,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackAction {
    Pause,
    Resume,
    Toggle,
    Next,
    Previous,
    Stop,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ToolCall {
    Search { query: String, limit: usize },
    GetAlbum { id: String },
    GetArtist { id: String },
    ListPlaylists,
    GetPlaylist { id: String },
    ListFavorites,
    ListPodcasts,
    GetPodcast { id: String },
    RandomSongs { count: usize },
    SimilarSongs { song_id: String, count: usize },
    MusicInfo { kind: InfoKind, id: Option<String>, name: Option<String>, artist: Option<String> },
    CreatePlaylist { name: String, song_ids: Vec<String> },
    AddToPlaylist { playlist_id: String, song_ids: Vec<String> },
    RenamePlaylist { playlist_id: String, name: String },
    RemoveFromPlaylist { playlist_id: String, song_ids: Vec<String> },
    DeletePlaylist { playlist_id: String },
    SetFavorite { song_ids: Vec<String>, favorite: bool },
    NowPlaying,
    Play { song_ids: Vec<String>, album_id: Option<String>, playlist_id: Option<String>, shuffle: bool, start_index: usize },
    Control { action: Option<PlaybackAction>, seek_seconds: Option<f64>, volume: Option<f32> },
    Enqueue { song_ids: Vec<String>, next: bool },
    StartRadio { song_id: String },
}

impl ToolCall {
    /// Changes the person must confirm in the app.
    pub fn is_destructive(&self) -> bool {
        matches!(self, Self::RemoveFromPlaylist { .. } | Self::DeletePlaylist { .. })
    }

    /// Calls that run on the interface thread because they use the player.
    pub fn uses_player(&self) -> bool {
        matches!(
            self,
            Self::NowPlaying | Self::Play { .. } | Self::Control { .. } | Self::Enqueue { .. } | Self::StartRadio { .. }
        )
    }

    pub fn changes_library(&self) -> bool {
        matches!(
            self,
            Self::CreatePlaylist { .. }
                | Self::AddToPlaylist { .. }
                | Self::RenamePlaylist { .. }
                | Self::RemoveFromPlaylist { .. }
                | Self::DeletePlaylist { .. }
                | Self::SetFavorite { .. }
        )
    }
}

struct Spec {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    properties: Value,
    required: &'static [&'static str],
    read_only: bool,
    destructive: bool,
    open_world: bool,
}

fn ids(description: &str) -> Value {
    json!({"type": "array", "items": {"type": "string"}, "minItems": 1, "maxItems": MAX_SONG_IDS, "description": description})
}

fn id(description: &str) -> Value {
    json!({"type": "string", "minLength": 1, "description": description})
}

fn specs() -> Vec<Spec> {
    let read = |name, title, description, properties: Value, required| Spec {
        name,
        title,
        description,
        properties,
        required,
        read_only: true,
        destructive: false,
        open_world: false,
    };
    let write = |name, title, description, properties: Value, required, destructive| Spec {
        name,
        title,
        description,
        properties,
        required,
        read_only: false,
        destructive,
        open_world: false,
    };
    vec![
        read(
            "search_library",
            "Search the library",
            "Searches artists, albums and songs in the music library by name. Returns IDs for the other tools.",
            json!({
                "query": {"type": "string", "minLength": 1, "description": "Artist, album or song name."},
                "limit": {"type": "integer", "minimum": 1, "maximum": MAX_RESULTS, "description": "Results per kind (default 20)."}
            }),
            &["query"],
        ),
        read("get_album", "Get album", "An album with its tracks.", json!({"album_id": id("Album ID.")}), &["album_id"]),
        read("get_artist", "Get artist", "An artist with their albums.", json!({"artist_id": id("Artist ID.")}), &["artist_id"]),
        read("list_playlists", "List playlists", "The playlists on the music server.", json!({}), &[]),
        read("get_playlist", "Get playlist", "A playlist with its songs in order.", json!({"playlist_id": id("Playlist ID.")}), &["playlist_id"]),
        read("list_favorites", "List favorites", "Favorite artists, albums and songs.", json!({}), &[]),
        read("list_podcasts", "List podcasts", "Podcasts subscribed on the music server.", json!({}), &[]),
        read(
            "get_podcast",
            "Get podcast",
            "A podcast with its episodes available for streaming, newest first.",
            json!({"podcast_id": id("Podcast ID.")}),
            &["podcast_id"],
        ),
        read(
            "random_songs",
            "Random songs",
            "Random songs from the library, useful for discovery.",
            json!({"count": {"type": "integer", "minimum": 1, "maximum": MAX_RESULTS, "description": "How many (default 20)."}}),
            &[],
        ),
        read(
            "similar_songs",
            "Similar songs",
            "Songs similar to a song's artist, when the server supports it.",
            json!({
                "song_id": id("Song ID."),
                "count": {"type": "integer", "minimum": 1, "maximum": MAX_RESULTS, "description": "How many (default 20)."}
            }),
            &["song_id"],
        ),
        Spec {
            open_world: true,
            ..read(
                "get_music_info",
                "Get music information",
                "Sourced notes about an artist, album, song or podcast: the server's biography or notes and, when the person \
                 enabled online music information, MusicBrainz facts and the Wikipedia summary. Give an ID from the library, or a name.",
                json!({
                    "kind": {"type": "string", "enum": ["artist", "album", "song", "podcast"]},
                    "id": {"type": "string", "description": "Library ID of the item (preferred)."},
                    "name": {"type": "string", "description": "Name or title when there is no ID."},
                    "artist": {"type": "string", "description": "Artist of an album or song given by name."}
                }),
                &["kind"],
            )
        },
        write(
            "create_playlist",
            "Create playlist",
            "Creates a playlist on the music server with songs from the library.",
            json!({"name": {"type": "string", "minLength": 1, "maxLength": 200}, "song_ids": ids("Song IDs in order.")}),
            &["name", "song_ids"],
            false,
        ),
        write(
            "add_to_playlist",
            "Add to playlist",
            "Adds songs to the end of a playlist, skipping songs it already contains.",
            json!({"playlist_id": id("Playlist ID."), "song_ids": ids("Song IDs to add.")}),
            &["playlist_id", "song_ids"],
            false,
        ),
        write(
            "rename_playlist",
            "Rename playlist",
            "Renames a playlist.",
            json!({"playlist_id": id("Playlist ID."), "name": {"type": "string", "minLength": 1, "maxLength": 200}}),
            &["playlist_id", "name"],
            false,
        ),
        write(
            "remove_from_playlist",
            "Remove from playlist",
            "Removes every occurrence of the given songs from a playlist. The person must confirm in Discophile.",
            json!({"playlist_id": id("Playlist ID."), "song_ids": ids("Song IDs to remove.")}),
            &["playlist_id", "song_ids"],
            true,
        ),
        write(
            "delete_playlist",
            "Delete playlist",
            "Deletes a playlist from the music server. The person must confirm in Discophile.",
            json!({"playlist_id": id("Playlist ID.")}),
            &["playlist_id"],
            true,
        ),
        write(
            "set_favorite",
            "Set favorite",
            "Adds songs to favorites or removes them.",
            json!({"song_ids": ids("Song IDs."), "favorite": {"type": "boolean"}}),
            &["song_ids", "favorite"],
            false,
        ),
        read("get_now_playing", "Now playing", "What plays now, the playback state and the next songs in the queue.", json!({}), &[]),
        write(
            "play",
            "Play",
            "Replaces the queue and starts playing songs, an album or a playlist. Give exactly one of song_ids, album_id or playlist_id.",
            json!({
                "song_ids": ids("Song IDs in order."),
                "album_id": {"type": "string"},
                "playlist_id": {"type": "string"},
                "shuffle": {"type": "boolean", "description": "Shuffle the queue (default false)."},
                "start_index": {"type": "integer", "minimum": 0, "description": "Song to start from (default 0)."}
            }),
            &[],
            false,
        ),
        write(
            "playback_control",
            "Playback control",
            "Pauses, resumes, skips or stops playback, seeks within the current song or sets the volume.",
            json!({
                "action": {"type": "string", "enum": ["pause", "resume", "toggle", "next", "previous", "stop"]},
                "seek_seconds": {"type": "number", "minimum": 0},
                "volume": {"type": "number", "minimum": 0, "maximum": 1, "description": "0 to 1."}
            }),
            &[],
            false,
        ),
        write(
            "add_to_queue",
            "Add to queue",
            "Adds songs to the end of the queue, or right after the current song.",
            json!({"song_ids": ids("Song IDs."), "next": {"type": "boolean", "description": "Play right after the current song."}}),
            &["song_ids"],
            false,
        ),
        write(
            "start_radio",
            "Start personal radio",
            "Starts Discophile's personal radio from a song: similar music keeps playing.",
            json!({"song_id": id("Seed song ID.")}),
            &["song_id"],
            false,
        ),
    ]
}

/// The `tools/list` result.
pub fn tools_list() -> Value {
    let tools: Vec<Value> = specs()
        .into_iter()
        .map(|spec| {
            json!({
                "name": spec.name,
                "title": spec.title,
                "description": spec.description,
                "inputSchema": {
                    "type": "object",
                    "properties": spec.properties,
                    "required": spec.required,
                    "additionalProperties": false
                },
                "annotations": {
                    "title": spec.title,
                    "readOnlyHint": spec.read_only,
                    "destructiveHint": spec.destructive,
                    "idempotentHint": spec.read_only,
                    "openWorldHint": spec.open_world
                }
            })
        })
        .collect();
    json!({"tools": tools})
}

fn string(arguments: &Map<String, Value>, key: &str) -> Result<Option<String>, String> {
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.trim().to_string()).filter(|s| !s.is_empty())),
        Some(_) => Err(format!("{key} must be a string")),
    }
}

fn required(arguments: &Map<String, Value>, key: &str) -> Result<String, String> {
    string(arguments, key)?.ok_or_else(|| format!("{key} is required"))
}

fn count(arguments: &Map<String, Value>, key: &str, default: usize, max: usize) -> Result<usize, String> {
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(value) => value
            .as_u64()
            .map(|n| (n as usize).clamp(1, max))
            .ok_or_else(|| format!("{key} must be a positive integer")),
    }
}

fn flag(arguments: &Map<String, Value>, key: &str) -> Result<Option<bool>, String> {
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(format!("{key} must be true or false")),
    }
}

fn song_ids(arguments: &Map<String, Value>, key: &str, needed: bool) -> Result<Vec<String>, String> {
    let ids: Vec<String> = match arguments.get(key) {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|v| v.as_str().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| format!("{key} must contain non-empty strings"))?,
        Some(_) => return Err(format!("{key} must be an array of strings")),
    };
    if ids.len() > MAX_SONG_IDS {
        return Err(format!("{key} accepts at most {MAX_SONG_IDS} IDs"));
    }
    if needed && ids.is_empty() {
        return Err(format!("{key} is required"));
    }
    Ok(ids)
}

fn bounded_name(arguments: &Map<String, Value>) -> Result<String, String> {
    let name = required(arguments, "name")?;
    if name.chars().count() > 200 || name.chars().any(char::is_control) {
        return Err("name must be at most 200 printable characters".into());
    }
    Ok(name)
}

/// Validates `tools/call` arguments. Errors are messages for the agent.
pub fn parse_tool_call(name: &str, arguments: &Value) -> Result<ToolCall, String> {
    let empty = Map::new();
    let a = match arguments {
        Value::Object(map) => map,
        Value::Null => &empty,
        _ => return Err("arguments must be an object".into()),
    };
    Ok(match name {
        "search_library" => ToolCall::Search { query: required(a, "query")?, limit: count(a, "limit", 20, MAX_RESULTS)? },
        "get_album" => ToolCall::GetAlbum { id: required(a, "album_id")? },
        "get_artist" => ToolCall::GetArtist { id: required(a, "artist_id")? },
        "list_playlists" => ToolCall::ListPlaylists,
        "get_playlist" => ToolCall::GetPlaylist { id: required(a, "playlist_id")? },
        "list_favorites" => ToolCall::ListFavorites,
        "list_podcasts" => ToolCall::ListPodcasts,
        "get_podcast" => ToolCall::GetPodcast { id: required(a, "podcast_id")? },
        "random_songs" => ToolCall::RandomSongs { count: count(a, "count", 20, MAX_RESULTS)? },
        "similar_songs" => {
            ToolCall::SimilarSongs { song_id: required(a, "song_id")?, count: count(a, "count", 20, MAX_RESULTS)? }
        }
        "get_music_info" => {
            let kind = match required(a, "kind")?.as_str() {
                "artist" => InfoKind::Artist,
                "album" => InfoKind::Album,
                "song" => InfoKind::Song,
                "podcast" => InfoKind::Podcast,
                _ => return Err("kind must be artist, album, song or podcast".into()),
            };
            let (id, name) = (string(a, "id")?, string(a, "name")?);
            if id.is_none() && name.is_none() {
                return Err("give id or name".into());
            }
            ToolCall::MusicInfo { kind, id, name, artist: string(a, "artist")? }
        }
        "create_playlist" => ToolCall::CreatePlaylist { name: bounded_name(a)?, song_ids: song_ids(a, "song_ids", true)? },
        "add_to_playlist" => {
            ToolCall::AddToPlaylist { playlist_id: required(a, "playlist_id")?, song_ids: song_ids(a, "song_ids", true)? }
        }
        "rename_playlist" => ToolCall::RenamePlaylist { playlist_id: required(a, "playlist_id")?, name: bounded_name(a)? },
        "remove_from_playlist" => {
            ToolCall::RemoveFromPlaylist { playlist_id: required(a, "playlist_id")?, song_ids: song_ids(a, "song_ids", true)? }
        }
        "delete_playlist" => ToolCall::DeletePlaylist { playlist_id: required(a, "playlist_id")? },
        "set_favorite" => ToolCall::SetFavorite {
            song_ids: song_ids(a, "song_ids", true)?,
            favorite: flag(a, "favorite")?.ok_or("favorite is required")?,
        },
        "get_now_playing" => ToolCall::NowPlaying,
        "play" => {
            let song_ids = song_ids(a, "song_ids", false)?;
            let (album_id, playlist_id) = (string(a, "album_id")?, string(a, "playlist_id")?);
            let given = usize::from(!song_ids.is_empty()) + usize::from(album_id.is_some()) + usize::from(playlist_id.is_some());
            if given != 1 {
                return Err("give exactly one of song_ids, album_id or playlist_id".into());
            }
            let start_index = match a.get("start_index") {
                None | Some(Value::Null) => 0,
                Some(v) => v.as_u64().ok_or("start_index must be a non-negative integer")? as usize,
            };
            ToolCall::Play { song_ids, album_id, playlist_id, shuffle: flag(a, "shuffle")?.unwrap_or(false), start_index }
        }
        "playback_control" => {
            let action = match string(a, "action")?.as_deref() {
                None => None,
                Some("pause") => Some(PlaybackAction::Pause),
                Some("resume") => Some(PlaybackAction::Resume),
                Some("toggle") => Some(PlaybackAction::Toggle),
                Some("next") => Some(PlaybackAction::Next),
                Some("previous") => Some(PlaybackAction::Previous),
                Some("stop") => Some(PlaybackAction::Stop),
                Some(_) => return Err("action must be pause, resume, toggle, next, previous or stop".into()),
            };
            let number = |key: &str| -> Result<Option<f64>, String> {
                match a.get(key) {
                    None | Some(Value::Null) => Ok(None),
                    Some(v) => v.as_f64().filter(|n| n.is_finite() && *n >= 0.0).map(Some).ok_or(format!("{key} must be a non-negative number")),
                }
            };
            let seek_seconds = number("seek_seconds")?;
            let volume = number("volume")?.map(|v| v.min(1.0) as f32);
            if action.is_none() && seek_seconds.is_none() && volume.is_none() {
                return Err("give action, seek_seconds or volume".into());
            }
            ToolCall::Control { action, seek_seconds, volume }
        }
        "add_to_queue" => {
            ToolCall::Enqueue { song_ids: song_ids(a, "song_ids", true)?, next: flag(a, "next")?.unwrap_or(false) }
        }
        "start_radio" => ToolCall::StartRadio { song_id: required(a, "song_id")? },
        _ => return Err(format!("Unknown tool: {}", prefix(name, 80))),
    })
}

/// A `tools/call` result with the value as text and structured content.
pub fn tool_result(value: Value) -> Value {
    let structured = if value.is_object() { value } else { json!({"result": value}) };
    json!({
        "content": [{"type": "text", "text": structured.to_string()}],
        "structuredContent": structured,
        "isError": false
    })
}

/// A tool failure the agent can read and act on.
pub fn tool_error(message: &str) -> Value {
    json!({"content": [{"type": "text", "text": message}], "isError": true})
}

// MARK: Item JSON

fn insert<T: Into<Value>>(map: &mut Map<String, Value>, key: &str, value: Option<T>) {
    if let Some(value) = value {
        map.insert(key.into(), value.into());
    }
}

fn non_empty(value: &Option<String>) -> Option<String> {
    value.clone().filter(|v| !v.trim().is_empty())
}

pub fn song_json(song: &Song) -> Value {
    let mut map = Map::new();
    map.insert("id".into(), song.id.clone().into());
    map.insert("title".into(), song.title.clone().into());
    insert(&mut map, "artist", non_empty(&song.artist));
    insert(&mut map, "album", non_empty(&song.album));
    insert(&mut map, "artist_id", non_empty(&song.artist_id));
    if !song.is_podcast() {
        insert(&mut map, "album_id", non_empty(&song.album_id));
    }
    insert(&mut map, "year", song.album_original_year.or(song.year).filter(|y| *y > 0));
    insert(&mut map, "genre", non_empty(&song.genre));
    insert(&mut map, "track", song.track);
    insert(&mut map, "duration_seconds", song.duration.filter(|d| d.is_finite() && *d > 0.0).map(|d| d.round() as i64));
    if song.is_starred() {
        map.insert("favorite".into(), true.into());
    }
    if song.is_podcast() {
        map.insert("podcast_episode".into(), true.into());
    }
    Value::Object(map)
}

pub fn songs_json(songs: &[Song]) -> Value {
    Value::Array(songs.iter().map(song_json).collect())
}

pub fn album_json(album: &Album) -> Value {
    let mut map = Map::new();
    map.insert("id".into(), album.id.clone().into());
    map.insert("title".into(), album.name.clone().into());
    insert(&mut map, "artist", non_empty(&album.artist));
    insert(&mut map, "artist_id", non_empty(&album.artist_id));
    insert(&mut map, "year", album.original_release_date.as_ref().and_then(|d| d.year).or(album.year).filter(|y| *y > 0));
    insert(&mut map, "genre", non_empty(&album.genre));
    insert(&mut map, "song_count", album.song_count);
    insert(&mut map, "duration_seconds", album.duration.filter(|d| d.is_finite() && *d > 0.0).map(|d| d.round() as i64));
    if album.starred.is_some() {
        map.insert("favorite".into(), true.into());
    }
    if let Some(songs) = &album.song {
        map.insert("tracks".into(), songs_json(songs));
    }
    Value::Object(map)
}

pub fn artist_json(artist: &Artist) -> Value {
    let mut map = Map::new();
    map.insert("id".into(), artist.id.clone().into());
    map.insert("name".into(), artist.name.clone().into());
    insert(&mut map, "album_count", artist.album_count);
    if let Some(albums) = &artist.album {
        map.insert("albums".into(), Value::Array(albums.iter().map(album_json).collect()));
    }
    Value::Object(map)
}

pub fn playlist_json(playlist: &Playlist) -> Value {
    let mut map = Map::new();
    map.insert("id".into(), playlist.id.clone().into());
    map.insert("name".into(), playlist.name.clone().into());
    insert(&mut map, "song_count", playlist.song_count);
    insert(&mut map, "duration_seconds", playlist.duration.filter(|d| d.is_finite() && *d > 0.0).map(|d| d.round() as i64));
    insert(&mut map, "comment", non_empty(&playlist.comment).map(|c| prefix(&c, 300)));
    if let Some(songs) = &playlist.entry {
        map.insert("songs".into(), songs_json(songs));
    }
    Value::Object(map)
}

fn description(text: &Option<String>, limit: usize) -> Option<String> {
    let text = crate::music_info::plain_text(text.as_deref()?);
    (!text.is_empty()).then(|| prefix(&text, limit))
}

pub fn podcast_json(channel: &PodcastChannel, episodes: Option<&[Song]>) -> Value {
    let mut map = Map::new();
    map.insert("id".into(), channel.id.clone().into());
    map.insert("title".into(), channel.title.clone().into());
    insert(&mut map, "description", description(&channel.description, 500));
    if let Some(songs) = episodes {
        let details: std::collections::HashMap<&str, &PodcastEpisode> =
            channel.episodes().iter().map(|e| (e.id.as_str(), e)).collect();
        let list: Vec<Value> = songs
            .iter()
            .map(|song| {
                let mut value = song_json(song);
                let episode = song.id.rsplit(':').next().and_then(|id| details.get(id));
                if let (Some(episode), Value::Object(map)) = (episode, &mut value) {
                    insert(map, "published", episode.publish_date.clone().map(|d| d.chars().take(10).collect::<String>()));
                    insert(map, "description", description(&episode.description, 400));
                }
                value
            })
            .collect();
        map.insert("episodes".into(), Value::Array(list));
    }
    Value::Object(map)
}

pub fn sources_json(sources: &[InfoSource]) -> Value {
    let list: Vec<Value> = sources
        .iter()
        .map(|s| {
            let mut map = Map::new();
            map.insert("provider".into(), s.provider().into());
            map.insert("title".into(), s.title.clone().into());
            map.insert("text".into(), s.text.clone().into());
            insert(&mut map, "link", s.link.clone());
            Value::Object(map)
        })
        .collect();
    json!({"sources": list})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_requests_notifications_and_errors() {
        let body = br#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;
        let messages = parse_body(body).unwrap();
        assert_eq!(classify(&messages[0]).unwrap(), Message::Request { id: json!(1), method: "tools/list".into(), params: Value::Null });
        let note = json!({"jsonrpc":"2.0","method":"notifications/initialized"});
        assert!(matches!(classify(&note).unwrap(), Message::Notification { .. }));
        assert_eq!(parse_body(b"{oops").unwrap_err()["error"]["code"], PARSE_ERROR);
        assert_eq!(parse_body(b"[]").unwrap_err()["error"]["code"], INVALID_REQUEST);
        assert_eq!(classify(&json!({"id":1,"method":"x"})).unwrap_err()["error"]["code"], INVALID_REQUEST);
        assert_eq!(classify(&json!({"jsonrpc":"2.0","id":{"a":1},"method":"x"})).unwrap_err()["error"]["code"], INVALID_REQUEST);
        assert_eq!(classify(&json!({"jsonrpc":"2.0","id":1,"result":{}})).unwrap(), Message::Other);
    }

    #[test]
    fn negotiates_protocol_versions() {
        assert_eq!(negotiate(Some("2025-06-18")), "2025-06-18");
        assert_eq!(negotiate(Some("2024-11-05")), PROTOCOL_VERSIONS[0]);
        let result = initialize_result(&json!({"protocolVersion": "2025-03-26"}), "1.0");
        assert_eq!(result["protocolVersion"], "2025-03-26");
        assert_eq!(result["capabilities"]["tools"]["listChanged"], false);
        assert_eq!(client_name(&json!({"clientInfo": {"name": "claude-code\u{7}", "title": ""}})), None);
        assert_eq!(client_name(&json!({"clientInfo": {"name": "claude-code"}})).as_deref(), Some("claude-code"));
    }

    #[test]
    fn tool_catalog_is_consistent() {
        let list = tools_list();
        let tools = list["tools"].as_array().unwrap();
        let mut names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        let count = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), count, "unique names");
        for tool in tools {
            let required = tool["inputSchema"]["required"].as_array().unwrap();
            for key in required {
                assert!(tool["inputSchema"]["properties"].get(key.as_str().unwrap()).is_some(), "{tool}");
            }
        }
        let destructive: Vec<&str> = tools
            .iter()
            .filter(|t| t["annotations"]["destructiveHint"] == true)
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(destructive, ["remove_from_playlist", "delete_playlist"]);
        for name in &destructive {
            let call = parse_tool_call(name, &json!({"playlist_id": "p", "song_ids": ["s"]})).unwrap();
            assert!(call.is_destructive());
        }
    }

    #[test]
    fn validates_arguments() {
        assert_eq!(
            parse_tool_call("search_library", &json!({"query": " Beatles ", "limit": 500})).unwrap(),
            ToolCall::Search { query: "Beatles".into(), limit: MAX_RESULTS }
        );
        assert!(parse_tool_call("search_library", &json!({"query": "  "})).is_err());
        assert!(parse_tool_call("create_playlist", &json!({"name": "Mix", "song_ids": []})).is_err());
        assert!(parse_tool_call("create_playlist", &json!({"name": "Mix", "song_ids": [1]})).is_err());
        let too_many: Vec<String> = (0..=MAX_SONG_IDS).map(|i| i.to_string()).collect();
        assert!(parse_tool_call("add_to_playlist", &json!({"playlist_id": "p", "song_ids": too_many})).is_err());
        assert!(parse_tool_call("play", &json!({"album_id": "a", "playlist_id": "p"})).is_err());
        assert!(parse_tool_call("play", &json!({})).is_err());
        assert_eq!(
            parse_tool_call("play", &json!({"album_id": "a", "shuffle": true})).unwrap(),
            ToolCall::Play { song_ids: vec![], album_id: Some("a".into()), playlist_id: None, shuffle: true, start_index: 0 }
        );
        assert!(parse_tool_call("playback_control", &json!({})).is_err());
        assert!(parse_tool_call("playback_control", &json!({"volume": -1})).is_err());
        assert!(parse_tool_call("get_music_info", &json!({"kind": "artist"})).is_err());
        assert!(parse_tool_call("rename_playlist", &json!({"playlist_id": "p", "name": "a\nb"})).is_err());
        assert!(parse_tool_call("nope", &json!({})).is_err());
        assert!(parse_tool_call("list_playlists", &Value::Null).is_ok());
    }

    #[test]
    fn results_wrap_values_as_objects() {
        let result = tool_result(json!([1, 2]));
        assert_eq!(result["structuredContent"], json!({"result": [1, 2]}));
        assert_eq!(result["content"][0]["text"], r#"{"result":[1,2]}"#);
        assert_eq!(tool_error("nope")["isError"], true);
    }

    #[test]
    fn song_json_is_compact() {
        let mut song = Song::new("s1", "Something");
        song.artist = Some("The Beatles".into());
        song.album = Some(String::new());
        song.duration = Some(182.6);
        song.year = Some(2009);
        song.album_original_year = Some(1969);
        let value = song_json(&song);
        assert_eq!(value, json!({"id": "s1", "title": "Something", "artist": "The Beatles", "year": 1969, "duration_seconds": 183}));
    }
}
