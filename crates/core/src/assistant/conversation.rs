//! Persistent chats: messages, the active collection and the bounded context sent to models.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::edits::{AssistantEditProposal, AssistantError};
use crate::models::Song;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssistantRole {
    User,
    Assistant,
}

impl AssistantRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }
}

/// Which model answers a chat. `Local` is an OpenAI-compatible server such as Ollama.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssistantProvider {
    #[default]
    Local,
    #[serde(rename = "chatgpt")]
    ChatGpt,
}

impl AssistantProvider {
    pub const ALL: [AssistantProvider; 2] = [Self::Local, Self::ChatGpt];
    pub fn is_local(self) -> bool {
        self == Self::Local
    }
    /// The English localization key.
    pub fn title(self) -> &'static str {
        match self {
            Self::Local => "Local model",
            Self::ChatGpt => "ChatGPT",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantMessage {
    pub id: Uuid,
    pub role: AssistantRole,
    pub text: String,
    #[serde(default)]
    pub songs: Vec<Song>,
    #[serde(default)]
    pub playlist_title: Option<String>,
    #[serde(default)]
    pub saved: bool,
    #[serde(default)]
    pub is_error: bool,
    /// The provider or model name that wrote the reply.
    #[serde(default)]
    pub source: Option<String>,
    /// The resolved plan criteria (JSON), kept for follow-ups.
    #[serde(default)]
    pub criteria: Option<String>,
    #[serde(default)]
    pub selection_title: Option<String>,
    #[serde(default, rename = "playlistID")]
    pub playlist_id: Option<String>,
    #[serde(default, rename = "curatedMixID")]
    pub curated_mix_id: Option<Uuid>,
    #[serde(default)]
    pub edit: Option<AssistantEditProposal>,
    #[serde(default)]
    pub collection_deleted: Option<bool>,
}

impl AssistantMessage {
    pub fn new(role: AssistantRole, text: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            role,
            text: text.into(),
            songs: Vec::new(),
            playlist_title: None,
            saved: false,
            is_error: false,
            source: None,
            criteria: None,
            selection_title: None,
            playlist_id: None,
            curated_mix_id: None,
            edit: None,
            collection_deleted: None,
        }
    }
    pub fn user(text: impl Into<String>) -> Self {
        Self::new(AssistantRole::User, text)
    }
    pub fn assistant(text: impl Into<String>, source: Option<&str>) -> Self {
        Self {
            source: source.map(str::to_string),
            ..Self::new(AssistantRole::Assistant, text)
        }
    }
    pub fn error(text: impl Into<String>, source: Option<&str>) -> Self {
        Self {
            is_error: true,
            ..Self::assistant(text, source)
        }
    }
    pub fn with_songs(mut self, songs: Vec<Song>) -> Self {
        self.songs = songs;
        self
    }
    /// The collection title shown above its songs.
    pub fn collection_title(&self) -> Option<&str> {
        self.playlist_title
            .as_deref()
            .or(self.selection_title.as_deref())
    }
    fn is_deleted(&self) -> bool {
        self.collection_deleted == Some(true)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantConversation {
    pub id: Uuid,
    pub title: String,
    /// Unix milliseconds of the last change.
    pub updated: i64,
    #[serde(default)]
    pub provider: AssistantProvider,
    #[serde(default, rename = "modelID")]
    pub model_id: String,
    #[serde(default)]
    pub messages: Vec<AssistantMessage>,
    #[serde(default)]
    pub draft: String,
    #[serde(default = "yes")]
    pub include_track: bool,
    #[serde(default, rename = "focusMessageID")]
    pub focus_message_id: Option<Uuid>,
    /// A reply arrived while this chat was not on screen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unread: Option<bool>,
}

fn yes() -> bool {
    true
}

impl AssistantConversation {
    pub fn new(
        title: impl Into<String>,
        provider: AssistantProvider,
        model_id: impl Into<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            title: title.into(),
            updated: crate::now_millis(),
            provider,
            model_id: model_id.into(),
            messages: Vec::new(),
            draft: String::new(),
            include_track: true,
            focus_message_id: None,
            unread: None,
        }
    }

    pub fn is_unread(&self) -> bool {
        self.unread == Some(true)
    }

    pub fn message(&self, id: Uuid) -> Option<&AssistantMessage> {
        self.messages.iter().find(|m| m.id == id)
    }
    pub fn message_mut(&mut self, id: Uuid) -> Option<&mut AssistantMessage> {
        self.messages.iter_mut().find(|m| m.id == id)
    }

    /// The collection the chat is editing: the focused message, or the latest reply with songs or a playlist.
    pub fn active_selection(&self) -> Option<&AssistantMessage> {
        if let Some(focus) = self.focus_message_id {
            return self
                .messages
                .iter()
                .find(|m| m.id == focus && !m.is_deleted());
        }
        self.messages.iter().rev().find(|m| {
            m.role == AssistantRole::Assistant
                && !m.is_error
                && m.edit.is_none()
                && !m.is_deleted()
                && (!m.songs.is_empty() || m.playlist_id.is_some())
        })
    }

    /// Only bounded recent text and public track metadata enter model context, never server IDs.
    pub fn context(&self, character_limit: usize) -> String {
        if character_limit < 120 {
            return String::new();
        }
        let per_entry = (character_limit / 10).max(40);
        let prefix = |text: &str, n: usize| text.chars().take(n).collect::<String>();
        let recent: Vec<&AssistantMessage> = self.messages.iter().filter(|m| !m.is_error).collect();
        let mut entries: Vec<BTreeMap<&str, String>> = recent[recent.len().saturating_sub(6)..]
            .iter()
            .map(|message| {
                let mut text = message.text.clone();
                if let Some(criteria) = &message.criteria {
                    text += "\nCriteria: ";
                    text += criteria;
                }
                for song in message.songs.iter().take(3) {
                    text += &format!(
                        "\n{} / {}",
                        prefix(&song.title, 80),
                        prefix(song.artist.as_deref().unwrap_or_default(), 60)
                    );
                }
                BTreeMap::from([
                    ("role", message.role.as_str().to_string()),
                    ("text", prefix(&text, per_entry)),
                ])
            })
            .collect();
        if let Some(active) = self.active_selection() {
            let mut summary = format!(
                "Active collection: {}",
                active.collection_title().unwrap_or("Selection")
            );
            if let Some(criteria) = &active.criteria {
                summary += &format!("\nCriteria: {}", prefix(criteria, 200));
            }
            summary += &format!("\nSongs: {}. ", active.songs.len());
            summary += &active
                .songs
                .iter()
                .take(12)
                .map(|s| {
                    format!(
                        "{} / {}",
                        prefix(&s.title, 60),
                        prefix(s.artist.as_deref().unwrap_or_default(), 40)
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            entries.insert(
                0,
                BTreeMap::from([
                    ("role", "collection".to_string()),
                    ("text", prefix(&summary, (character_limit / 3).max(80))),
                ]),
            );
        }
        while !entries.is_empty() {
            let json = serde_json::to_string(&entries).unwrap_or_default();
            if json.chars().count() <= character_limit {
                return json;
            }
            if entries[0]["role"] == "collection" && entries.len() > 2 {
                entries.remove(1);
            } else {
                entries.remove(0);
            }
        }
        String::new()
    }

    /// Records that a reviewed collection was saved as a server playlist.
    pub fn mark_saved(
        &mut self,
        message_id: Uuid,
        title: &str,
        songs: Vec<Song>,
        playlist_id: Option<String>,
    ) {
        if let Some(message) = self.message_mut(message_id) {
            message.saved = true;
            message.songs = songs;
            message.playlist_title = Some(title.to_string());
            message.playlist_id = playlist_id;
            self.focus_message_id = Some(message_id);
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantArchive {
    #[serde(default)]
    pub conversations: Vec<AssistantConversation>,
    #[serde(default, rename = "selectedID")]
    pub selected_id: Option<Uuid>,
}

impl AssistantArchive {
    /// Decodes a saved archive, rejecting duplicate chats and a dangling selection.
    pub fn decode(data: &[u8]) -> Result<Self, AssistantError> {
        let value: Self =
            serde_json::from_slice(data).map_err(|_| AssistantError::InvalidResponse)?;
        let ids: HashSet<Uuid> = value.conversations.iter().map(|c| c.id).collect();
        if ids.len() != value.conversations.len()
            || value.selected_id.is_some_and(|id| !ids.contains(&id))
        {
            return Err(AssistantError::InvalidResponse);
        }
        Ok(value)
    }

    pub fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assistant::music_assistant::tests::library_plan;
    use serde_json::{Value, json};

    #[test]
    fn unread_reply_state_survives_round_trip_and_older_chats_decode_as_read() {
        let mut chat = AssistantConversation::new("Evening", AssistantProvider::Local, "");
        chat.unread = Some(true);
        let archive = AssistantArchive {
            conversations: vec![
                chat,
                AssistantConversation::new("Older", AssistantProvider::Local, ""),
            ],
            selected_id: None,
        };
        let mut json: Value = serde_json::to_value(&archive).unwrap();
        json["conversations"][1]
            .as_object_mut()
            .unwrap()
            .remove("unread");
        let restored = AssistantArchive::decode(json.to_string().as_bytes()).unwrap();
        assert_eq!(
            restored
                .conversations
                .iter()
                .map(|c| c.unread)
                .collect::<Vec<_>>(),
            [Some(true), None]
        );
    }

    #[test]
    fn archive_round_trip_preserves_chats_provider_model_and_playlist_edits() {
        let mut first =
            AssistantConversation::new("Evening", AssistantProvider::ChatGpt, "available-model");
        first.draft = "A follow-up".into();
        first.include_track = false;
        let mut reply = AssistantMessage::assistant("A proposal", Some("Model"))
            .with_songs(vec![Song::new("private-id", "Song")]);
        reply.playlist_title = Some("Mix".into());
        reply.saved = true;
        first.messages = vec![AssistantMessage::user("Make a mix"), reply];
        let second = AssistantConversation::new("Local", AssistantProvider::Local, "");
        let archive = AssistantArchive {
            conversations: vec![first, second.clone()],
            selected_id: Some(second.id),
        };
        let restored = AssistantArchive::decode(&archive.encode()).unwrap();
        assert_eq!(restored.selected_id, Some(second.id));
        assert_eq!(restored.conversations[0].model_id, "available-model");
        assert_eq!(
            restored.conversations[0].provider,
            AssistantProvider::ChatGpt
        );
        assert_eq!(restored.conversations[1].provider, AssistantProvider::Local);
        assert_eq!(restored.conversations[0].draft, "A follow-up");
        assert!(!restored.conversations[0].include_track);
        assert!(restored.conversations[0].messages[1].saved);
        assert_eq!(
            restored.conversations[0].messages[1].songs[0].id,
            "private-id"
        );
        assert!(
            String::from_utf8(archive.encode())
                .unwrap()
                .contains("\"provider\":\"chatgpt\"")
        );
    }

    #[test]
    fn context_contains_recent_referents_without_private_identifiers_or_failed_responses() {
        let mut chat = AssistantConversation::new("Chat", AssistantProvider::Local, "");
        let song = Song {
            artist: Some("Public artist".into()),
            album_id: Some("private-album".into()),
            cover_art: Some("private-cover".into()),
            stream_id: Some("private-stream".into()),
            ..Song::new("private-song", "Public title")
        };
        chat.messages = vec![
            AssistantMessage::user("Find a song"),
            AssistantMessage::assistant("Choose a result", None).with_songs(vec![song]),
            AssistantMessage::error("Failed secret detail", None),
        ];
        let context = chat.context(900);
        assert!(context.contains("Public title") && context.contains("Public artist"));
        for private in [
            "private-song",
            "private-album",
            "private-cover",
            "private-stream",
            "Failed secret detail",
        ] {
            assert!(!context.contains(private));
        }
        assert!(serde_json::from_str::<Value>(&context).is_ok());
    }

    #[test]
    fn context_stays_bounded_and_retains_most_recent_messages_across_provider_changes() {
        let mut chat = AssistantConversation::new("Chat", AssistantProvider::ChatGpt, "");
        chat.messages = (0..20)
            .map(|i| {
                let role = if i % 2 == 0 {
                    AssistantRole::User
                } else {
                    AssistantRole::Assistant
                };
                AssistantMessage::new(role, format!("message-{i} {}", "🎵\n\"".repeat(200)))
            })
            .collect();
        for limit in [120, 900, 8_000] {
            let context = chat.context(limit);
            assert!(context.chars().count() <= limit);
            if !context.is_empty() {
                assert!(serde_json::from_str::<Value>(&context).is_ok());
                assert!(context.contains("message-19"));
                assert!(!context.contains("message-0 "));
            }
        }
        let before = chat.context(900);
        chat.provider = AssistantProvider::Local;
        assert_eq!(chat.messages.len(), 20);
        assert_eq!(chat.context(900), before);
    }

    #[test]
    fn selection_criteria_survive_history_and_provider_switches_without_private_ids() {
        let value = library_plan(json!({})).unwrap();
        let mut conversation = AssistantConversation::new("Period", AssistantProvider::ChatGpt, "");
        let mut reply = AssistantMessage::assistant("A selection", None).with_songs(vec![Song {
            artist: Some("The Beatles".into()),
            ..Song::new("private-server-id", "Taxman")
        }]);
        reply.criteria = Some(value.context());
        reply.selection_title = Some("Beatles 1966–1967".into());
        conversation.messages = vec![
            AssistantMessage::user("Play Beatles from 1966 to 1967"),
            reply,
        ];
        conversation.provider = AssistantProvider::Local;
        let restored: AssistantConversation =
            serde_json::from_str(&serde_json::to_string(&conversation).unwrap()).unwrap();
        assert_eq!(
            restored.messages.last().unwrap().criteria.as_deref(),
            Some(value.context().as_str())
        );
        assert_eq!(
            restored.messages.last().unwrap().selection_title.as_deref(),
            Some("Beatles 1966–1967")
        );
        for limit in [900, 8_000] {
            let context = restored.context(limit);
            assert!(
                context.contains("1966") && context.contains("1967") && context.contains("Beatles")
            );
            assert!(!context.contains("private-server-id"));
            assert!(context.chars().count() <= limit);
        }
    }

    #[test]
    fn invalid_selection_and_duplicate_conversations_are_rejected() {
        let chat = AssistantConversation::new("Chat", AssistantProvider::Local, "");
        let mut archive = AssistantArchive {
            conversations: vec![chat.clone()],
            selected_id: Some(Uuid::new_v4()),
        };
        assert!(AssistantArchive::decode(&archive.encode()).is_err());
        archive.selected_id = Some(chat.id);
        archive.conversations.push(chat);
        assert!(AssistantArchive::decode(&archive.encode()).is_err());
        assert!(AssistantArchive::decode(b"incomplete").is_err());
    }

    #[test]
    fn active_selection_skips_errors_edits_and_deleted_collections() {
        let mut chat = AssistantConversation::new("Chat", AssistantProvider::Local, "");
        let first =
            AssistantMessage::assistant("First", None).with_songs(vec![Song::new("1", "One")]);
        let mut deleted =
            AssistantMessage::assistant("Deleted", None).with_songs(vec![Song::new("2", "Two")]);
        deleted.collection_deleted = Some(true);
        chat.messages = vec![
            first.clone(),
            deleted.clone(),
            AssistantMessage::error("x", None).with_songs(vec![Song::new("3", "Three")]),
        ];
        assert_eq!(chat.active_selection().map(|m| m.id), Some(first.id));
        chat.focus_message_id = Some(deleted.id);
        assert!(chat.active_selection().is_none());
    }
}
