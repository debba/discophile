//! The chat list of one library account, ported from the iOS `AssistantChatStore`. The app persists
//! [`AssistantChatStore::encode`] under [`AssistantChatStore::file_name`] and shows notifications.

use uuid::Uuid;

use super::conversation::{
    AssistantArchive, AssistantConversation, AssistantMessage, AssistantProvider,
};
use super::edits::AssistantError;
use crate::l10n::t;
use crate::models::Song;

/// One row of the chat list.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatListItem {
    pub id: Uuid,
    pub title: String,
    pub updated: i64,
    pub unread: bool,
    pub selected: bool,
}

/// What the app should do when a reply arrives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplyNotice {
    /// The chat is on screen: announce "Reply ready" to assistive technologies.
    Announce,
    /// The chat was marked unread: post a notification and update the badge.
    Notify { unread: usize },
    /// The chat no longer exists.
    None,
}

#[derive(Debug, Clone, Default)]
pub struct AssistantChatStore {
    archive: AssistantArchive,
    account: String,
    loaded: bool,
    readable: bool,
    /// The chat on screen while the app is active; its replies need no notification.
    pub visible_conversation: Option<Uuid>,
}

impl AssistantChatStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// The per-account archive file name. Preview libraries use `preview`.
    pub fn file_name(account: &str) -> String {
        format!(
            "AssistantChats-{}.json",
            if account.is_empty() {
                "preview"
            } else {
                account
            }
        )
    }

    pub fn account(&self) -> &str {
        &self.account
    }
    pub fn archive(&self) -> &AssistantArchive {
        &self.archive
    }
    pub fn is_readable(&self) -> bool {
        self.readable
    }

    /// Whether [`Self::load`] must run for `account` (another account, or the last read failed).
    pub fn needs_load(&self, account: &str) -> bool {
        !self.loaded || self.account != account || !self.readable
    }

    /// Loads an account's saved archive (`None` when there is no file). A new chat is created when empty.
    /// On error the store stays unreadable and never overwrites the file.
    pub fn load(
        &mut self,
        account: &str,
        saved: Option<&[u8]>,
        default_provider: AssistantProvider,
    ) -> Result<(), AssistantError> {
        self.account = account.to_string();
        self.loaded = true;
        self.readable = true;
        self.archive = AssistantArchive::default();
        if let Some(data) = saved {
            match AssistantArchive::decode(data) {
                Ok(archive) => self.archive = archive,
                Err(error) => {
                    self.readable = false;
                    return Err(error);
                }
            }
        }
        if self.archive.conversations.is_empty() {
            self.new_conversation(default_provider);
        }
        Ok(())
    }

    /// The archive to save, or `None` while nothing readable is loaded.
    pub fn encode(&self) -> Option<Vec<u8>> {
        (self.loaded && self.readable).then(|| self.archive.encode())
    }

    pub fn selected_id(&self) -> Option<Uuid> {
        self.archive.selected_id
    }
    pub fn selected(&self) -> Option<&AssistantConversation> {
        self.conversation(self.archive.selected_id?)
    }
    pub fn conversation(&self, id: Uuid) -> Option<&AssistantConversation> {
        self.archive.conversations.iter().find(|c| c.id == id)
    }

    pub fn unread_count(&self) -> usize {
        self.archive
            .conversations
            .iter()
            .filter(|c| c.is_unread())
            .count()
    }

    /// Chats newest first, filtered by a case-insensitive search of titles and messages.
    pub fn chat_list(&self, search: &str) -> Vec<ChatListItem> {
        let query = search.trim().to_lowercase();
        let mut items: Vec<ChatListItem> = self
            .archive
            .conversations
            .iter()
            .filter(|c| {
                query.is_empty()
                    || c.title.to_lowercase().contains(&query)
                    || c.messages
                        .iter()
                        .any(|m| m.text.to_lowercase().contains(&query))
            })
            .map(|c| ChatListItem {
                id: c.id,
                title: c.title.clone(),
                updated: c.updated,
                unread: c.is_unread(),
                selected: Some(c.id) == self.archive.selected_id,
            })
            .collect();
        items.sort_by_key(|item| std::cmp::Reverse(item.updated));
        items
    }

    /// Starts a chat with the provider and model of the selected one.
    pub fn new_conversation(&mut self, default_provider: AssistantProvider) -> Uuid {
        let previous = self.selected();
        let provider = previous.map_or(default_provider, |c| c.provider);
        let model = previous.map(|c| c.model_id.clone()).unwrap_or_default();
        let conversation = AssistantConversation::new(t("New chat"), provider, model);
        let id = conversation.id;
        self.archive.conversations.insert(0, conversation);
        self.archive.selected_id = Some(id);
        id
    }

    /// Opens a new chat, reusing the selected one while it is still empty.
    pub fn start_fresh_conversation(&mut self, default_provider: AssistantProvider) -> Uuid {
        if let Some(selected) = self.selected()
            && selected.messages.is_empty()
            && selected.draft.is_empty()
        {
            return selected.id;
        }
        self.new_conversation(default_provider)
    }

    pub fn select(&mut self, id: Uuid) -> bool {
        let exists = self.conversation(id).is_some();
        if exists {
            self.archive.selected_id = Some(id);
        }
        exists
    }

    /// Deletes a chat; a new one is created when none remain.
    pub fn delete(&mut self, id: Uuid, default_provider: AssistantProvider) {
        self.archive.conversations.retain(|c| c.id != id);
        if self.archive.selected_id == Some(id) {
            self.archive.selected_id = self.archive.conversations.first().map(|c| c.id);
        }
        if self.archive.conversations.is_empty() {
            self.new_conversation(default_provider);
        }
    }

    /// Changes a chat and stamps its update time. Returns false when it no longer exists.
    pub fn update(&mut self, id: Uuid, change: impl FnOnce(&mut AssistantConversation)) -> bool {
        let Some(conversation) = self.archive.conversations.iter_mut().find(|c| c.id == id) else {
            return false;
        };
        change(conversation);
        conversation.updated = crate::now_millis();
        true
    }

    /// Records a finished reply. `on_screen` is true when the chat is visible and the app is active.
    pub fn reply_finished(&mut self, id: Uuid, app_active: bool) -> ReplyNotice {
        if self.conversation(id).is_none() {
            return ReplyNotice::None;
        }
        if self.visible_conversation == Some(id) && app_active {
            return ReplyNotice::Announce;
        }
        self.update(id, |c| c.unread = Some(true));
        ReplyNotice::Notify {
            unread: self.unread_count(),
        }
    }

    /// Clears a chat's unread mark. Returns true when it changed (update the badge and notifications).
    pub fn mark_read(&mut self, id: Uuid) -> bool {
        if !self.conversation(id).is_some_and(|c| c.is_unread()) {
            return false;
        }
        self.update(id, |c| c.unread = None)
    }

    /// Opens a playlist or curated mix for editing in a fresh chat, anchored by a focus message.
    pub fn focus_collection(
        &mut self,
        default_provider: AssistantProvider,
        message: AssistantMessage,
    ) -> Uuid {
        let id = self.start_fresh_conversation(default_provider);
        let title = message.playlist_title.clone().unwrap_or_default();
        self.update(id, |c| {
            c.title = title;
            c.include_track = false;
            c.focus_message_id = Some(message.id);
            c.messages.push(message);
        });
        id
    }

    /// Records that a reviewed selection was saved as a server playlist.
    pub fn mark_saved(
        &mut self,
        conversation: Uuid,
        message_id: Uuid,
        title: &str,
        songs: Vec<Song>,
        playlist_id: Option<String>,
    ) {
        self.update(conversation, |c| {
            c.mark_saved(message_id, title, songs, playlist_id)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assistant::turn::focus_message;

    #[test]
    fn loading_creates_a_chat_and_rejects_corrupt_archives_without_overwriting() {
        let mut store = AssistantChatStore::new();
        assert!(store.needs_load("acct"));
        store
            .load("acct", None, AssistantProvider::ChatGpt)
            .unwrap();
        assert!(!store.needs_load("acct"));
        assert_eq!(store.archive().conversations.len(), 1);
        assert_eq!(
            store.selected().unwrap().provider,
            AssistantProvider::ChatGpt
        );
        let saved = store.encode().unwrap();
        let mut other = AssistantChatStore::new();
        other
            .load("acct", Some(&saved), AssistantProvider::Local)
            .unwrap();
        assert_eq!(other.selected_id(), store.selected_id());
        assert!(
            other
                .load("acct", Some(b"broken"), AssistantProvider::Local)
                .is_err()
        );
        assert!(other.encode().is_none());
        assert!(other.needs_load("acct"));
        assert_eq!(
            AssistantChatStore::file_name(""),
            "AssistantChats-preview.json"
        );
    }

    #[test]
    fn unread_replies_count_until_read() {
        let mut store = AssistantChatStore::new();
        store.load("acct", None, AssistantProvider::Local).unwrap();
        let first = store.selected_id().unwrap();
        store.update(first, |c| c.messages.push(AssistantMessage::user("hi")));
        let second = store.new_conversation(AssistantProvider::ChatGpt);
        assert_eq!(store.selected().unwrap().provider, AssistantProvider::Local);
        store.visible_conversation = Some(second);
        assert_eq!(store.reply_finished(second, true), ReplyNotice::Announce);
        assert_eq!(
            store.reply_finished(first, true),
            ReplyNotice::Notify { unread: 1 }
        );
        assert_eq!(
            store.reply_finished(second, false),
            ReplyNotice::Notify { unread: 2 }
        );
        assert_eq!(store.unread_count(), 2);
        assert!(store.mark_read(first));
        assert!(!store.mark_read(first));
        assert_eq!(store.unread_count(), 1);
        assert_eq!(
            store.reply_finished(Uuid::new_v4(), true),
            ReplyNotice::None
        );
        let list = store.chat_list("");
        assert_eq!(list.len(), 2);
        assert!(list[0].updated >= list[1].updated);
        assert_eq!(store.chat_list("HI").len(), 1);
    }

    #[test]
    fn fresh_chats_reuse_empty_ones_and_deleting_the_last_creates_one() {
        let mut store = AssistantChatStore::new();
        store.load("acct", None, AssistantProvider::Local).unwrap();
        let first = store.selected_id().unwrap();
        assert_eq!(
            store.start_fresh_conversation(AssistantProvider::Local),
            first
        );
        let message = focus_message(
            "Evening",
            vec![Song::new("1", "One")],
            false,
            Some("p".into()),
            None,
        );
        let focused = store.focus_collection(AssistantProvider::Local, message);
        assert_eq!(focused, first);
        let chat = store.selected().unwrap();
        assert_eq!(chat.title, "Evening");
        assert!(!chat.include_track);
        assert!(chat.active_selection().unwrap().saved);
        let second = store.start_fresh_conversation(AssistantProvider::Local);
        assert_ne!(second, first);
        store.delete(second, AssistantProvider::Local);
        assert_eq!(store.selected_id(), Some(first));
        store.delete(first, AssistantProvider::Local);
        assert_eq!(store.archive().conversations.len(), 1);
        assert!(!store.select(first));
    }
}
