//! Reviewed library edits proposed by the assistant. Nothing is mutated until the user confirms a concrete snapshot.

use std::collections::{BTreeSet, HashSet};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::models::{Playlist, Song};

/// Assistant failures. Defined here because edit proposals need it; the rest of the assistant shares it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AssistantError {
    #[error("invalid response")]
    InvalidResponse,
    #[error("incomplete")]
    Incomplete,
    #[error("sign in required")]
    SignInRequired,
    #[error("consent required")]
    ConsentRequired,
    #[error("invalid identity")]
    InvalidIdentity,
    #[error("cancelled")]
    Cancelled,
    #[error("unavailable")]
    Unavailable,
    #[error("restricted")]
    Restricted,
    #[error("usage limit")]
    UsageLimit,
    #[error("refused")]
    Refused,
    #[error("no matches")]
    NoMatches,
    #[error("local unavailable")]
    LocalUnavailable,
    #[error("ambiguous target")]
    AmbiguousTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssistantEditOperation {
    Add,
    Remove,
    Replace,
    Rename,
    Delete,
    Favorite,
    Unfavorite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssistantEditTargetKind {
    Selection,
    Playlist,
    Curated,
    Favorites,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssistantEditTarget {
    pub kind: AssistantEditTargetKind,
    #[serde(default, rename = "messageID")]
    pub message_id: Option<Uuid>,
    #[serde(default, rename = "playlistID")]
    pub playlist_id: Option<String>,
    #[serde(default, rename = "curatedID")]
    pub curated_id: Option<Uuid>,
    pub title: String,
}

impl AssistantEditTarget {
    pub fn new(kind: AssistantEditTargetKind, title: impl Into<String>) -> Self {
        Self {
            kind,
            message_id: None,
            playlist_id: None,
            curated_id: None,
            title: title.into(),
        }
    }

    pub fn playlist(title: impl Into<String>, playlist_id: impl Into<String>) -> Self {
        Self {
            playlist_id: Some(playlist_id.into()),
            ..Self::new(AssistantEditTargetKind::Playlist, title)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AssistantEditStatus {
    #[default]
    Pending,
    Applying,
    Applied,
    Cancelled,
    Failed,
}

/// A concrete local snapshot for review. No mutation is authorized by model output alone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantEditProposal {
    pub id: Uuid,
    pub account: String,
    pub operation: AssistantEditOperation,
    pub target: AssistantEditTarget,
    pub before: Vec<Song>,
    pub after: Vec<Song>,
    pub title: String,
    pub tracks: Vec<Song>,
    #[serde(default, rename = "expectedFavoriteIDs")]
    pub expected_favorite_ids: BTreeSet<String>,
    #[serde(default)]
    pub status: AssistantEditStatus,
}

pub const MAX_BEFORE_TRACKS: usize = 10_000;
pub const MAX_SELECTED_TRACKS: usize = 500;
pub const MAX_NAME_LENGTH: usize = 200;

/// Distinct, non-podcast songs in first-occurrence order.
fn unique(songs: Vec<Song>) -> Vec<Song> {
    let mut seen = HashSet::new();
    songs
        .into_iter()
        .filter(|s| {
            !s.is_podcast()
                && !s.stream_identifier().is_empty()
                && seen.insert(s.stream_identifier().to_string())
        })
        .collect()
}

fn identifiers(songs: &[Song]) -> impl Iterator<Item = &str> {
    songs.iter().map(Song::stream_identifier)
}

impl AssistantEditProposal {
    /// Builds the reviewed snapshot. `selected` is required except for rename and delete;
    /// `new_name` is used only by rename; `favorite_ids` is narrowed to the selected tracks.
    pub fn new(
        account: &str,
        operation: AssistantEditOperation,
        target: AssistantEditTarget,
        before: Vec<Song>,
        selected: Vec<Song>,
        new_name: &str,
        favorite_ids: &HashSet<String>,
    ) -> Result<Self, AssistantError> {
        use AssistantEditOperation::*;
        if account.is_empty()
            || before.len() > MAX_BEFORE_TRACKS
            || selected.len() > MAX_SELECTED_TRACKS
        {
            return Err(AssistantError::InvalidResponse);
        }
        if !matches!(operation, Rename | Delete) && selected.is_empty() {
            return Err(AssistantError::NoMatches);
        }
        let name = new_name.trim();
        if operation == Rename && (name.is_empty() || name.chars().count() > MAX_NAME_LENGTH) {
            return Err(AssistantError::InvalidResponse);
        }
        let selected_ids: HashSet<&str> = identifiers(&selected).collect();
        let expected_favorite_ids = favorite_ids
            .iter()
            .filter(|id| selected_ids.contains(id.as_str()))
            .cloned()
            .collect::<BTreeSet<_>>();
        let tracks = unique(selected);
        let title = if operation == Rename {
            name.to_string()
        } else {
            target.title.clone()
        };
        let after = match operation {
            Add => {
                let mut seen: HashSet<&str> = identifiers(&before).collect();
                let additions = tracks
                    .iter()
                    .filter(|s| seen.insert(s.stream_identifier()))
                    .cloned()
                    .collect::<Vec<_>>();
                before.iter().cloned().chain(additions).collect()
            }
            Remove => {
                let ids: HashSet<&str> = identifiers(&tracks).collect();
                let present: HashSet<&str> = identifiers(&before).collect();
                if !ids.is_subset(&present) {
                    return Err(AssistantError::InvalidResponse);
                }
                before
                    .iter()
                    .filter(|s| !ids.contains(s.stream_identifier()))
                    .cloned()
                    .collect()
            }
            Replace => tracks.clone(),
            Delete => Vec::new(),
            Rename | Favorite | Unfavorite => before.clone(),
        };
        Ok(Self {
            id: Uuid::new_v4(),
            account: account.to_string(),
            operation,
            target,
            before,
            after,
            title,
            tracks,
            expected_favorite_ids,
            status: AssistantEditStatus::Pending,
        })
    }

    pub fn is_favorite_change(&self) -> bool {
        matches!(
            self.operation,
            AssistantEditOperation::Favorite | AssistantEditOperation::Unfavorite
        )
    }

    pub fn is_no_op(&self) -> bool {
        match self.operation {
            AssistantEditOperation::Rename => self.title == self.target.title,
            AssistantEditOperation::Delete => false,
            AssistantEditOperation::Favorite => self
                .tracks
                .iter()
                .all(|s| self.expected_favorite_ids.contains(s.stream_identifier())),
            AssistantEditOperation::Unfavorite => self
                .tracks
                .iter()
                .all(|s| !self.expected_favorite_ids.contains(s.stream_identifier())),
            _ => identifiers(&self.before).eq(identifiers(&self.after)),
        }
    }

    /// True when the server playlist is still the reviewed one: same ID, name and entries in order.
    pub fn matches(&self, playlist: &Playlist) -> bool {
        Some(playlist.id.as_str()) == self.target.playlist_id.as_deref()
            && playlist.name == self.target.title
            && identifiers(playlist.songs()).eq(identifiers(&self.before))
    }
}

/// Case-, diacritic- and width-insensitive comparison key with collapsed whitespace.
pub fn normalize(text: &str) -> String {
    let folded: String = text
        .chars()
        .flat_map(char::to_lowercase)
        .map(fold)
        .collect();
    folded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Removes common Latin diacritics and maps full-width ASCII to ASCII.
fn fold(c: char) -> char {
    if ('\u{FF01}'..='\u{FF5E}').contains(&c) {
        return char::from_u32(c as u32 - 0xFF01 + 0x21).unwrap_or(c);
    }
    if c == '\u{3000}' {
        return ' ';
    }
    const TABLE: &[(&str, char)] = &[
        ("àáâãäåāăą", 'a'),
        ("çćĉċč", 'c'),
        ("ďđ", 'd'),
        ("èéêëēĕėęě", 'e'),
        ("ĝğġģ", 'g'),
        ("ĥħ", 'h'),
        ("ìíîïĩīĭįı", 'i'),
        ("ĵ", 'j'),
        ("ķ", 'k'),
        ("ĺļľŀł", 'l'),
        ("ñńņňŉ", 'n'),
        ("òóôõöøōŏő", 'o'),
        ("ŕŗř", 'r'),
        ("śŝşšș", 's'),
        ("ţťŧț", 't'),
        ("ùúûüũūŭůűų", 'u'),
        ("ŵ", 'w'),
        ("ýÿŷ", 'y'),
        ("źżž", 'z'),
    ];
    TABLE
        .iter()
        .find(|(set, _)| set.contains(c))
        .map_or(c, |(_, base)| *base)
}

pub struct AssistantTargetResolver;

impl AssistantTargetResolver {
    /// Exactly one target must match the normalized name.
    pub fn named(
        name: &str,
        targets: &[AssistantEditTarget],
    ) -> Result<AssistantEditTarget, AssistantError> {
        let wanted = normalize(name);
        let mut matches = targets.iter().filter(|t| normalize(&t.title) == wanted);
        match (matches.next(), matches.next()) {
            (Some(target), None) => Ok(target.clone()),
            (None, _) => Err(AssistantError::NoMatches),
            (Some(_), Some(_)) => Err(AssistantError::AmbiguousTarget),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use AssistantEditOperation::*;

    fn a() -> Song {
        Song::new("a", "First")
    }
    fn b() -> Song {
        Song::new("b", "Second")
    }
    fn target() -> AssistantEditTarget {
        AssistantEditTarget::playlist("Evening", "list")
    }
    fn proposal(
        operation: AssistantEditOperation,
        target: AssistantEditTarget,
        before: Vec<Song>,
        selected: Vec<Song>,
        name: &str,
        favorites: &[&str],
    ) -> Result<AssistantEditProposal, AssistantError> {
        let favorites = favorites.iter().map(|s| s.to_string()).collect();
        AssistantEditProposal::new(
            "fixture", operation, target, before, selected, name, &favorites,
        )
    }
    fn ids(songs: &[Song]) -> Vec<&str> {
        songs.iter().map(|s| s.id.as_str()).collect()
    }

    #[test]
    fn adding_keeps_existing_duplicates_and_skips_new_aliases() {
        let alias = Song {
            stream_id: Some("b".into()),
            ..Song::new("alias", "Alias")
        };
        let edit = proposal(
            Add,
            target(),
            vec![a(), a()],
            vec![a(), b(), alias],
            "",
            &[],
        )
        .unwrap();
        assert_eq!(ids(&edit.after), ["a", "a", "b"]);
        assert_eq!(edit.status, AssistantEditStatus::Pending);
        assert!(!edit.is_no_op());
        assert!(
            proposal(Add, target(), vec![a()], vec![a()], "", &[])
                .unwrap()
                .is_no_op()
        );
    }

    #[test]
    fn removal_removes_every_occurrence_and_rejects_unknown_tracks() {
        let edit = proposal(Remove, target(), vec![a(), b(), a()], vec![a()], "", &[]).unwrap();
        assert_eq!(ids(&edit.after), ["b"]);
        assert_eq!(
            proposal(Remove, target(), vec![a()], vec![b()], "", &[]),
            Err(AssistantError::InvalidResponse)
        );
    }

    #[test]
    fn name_resolution_requires_one_exact_normalized_target() {
        assert_eq!(
            AssistantTargetResolver::named("  EVENING  ", &[target()]),
            Ok(target())
        );
        assert_eq!(
            AssistantTargetResolver::named("Even", &[target()]),
            Err(AssistantError::NoMatches)
        );
        assert_eq!(
            AssistantTargetResolver::named("Evening", &[target(), target()]),
            Err(AssistantError::AmbiguousTarget)
        );
        let accented = AssistantEditTarget::playlist("Caffè  Notturno", "x");
        assert_eq!(
            AssistantTargetResolver::named("caffe notturno", std::slice::from_ref(&accented)),
            Ok(accented)
        );
    }

    #[test]
    fn favorites_snapshot_is_bounded_to_selected_tracks_and_no_op_is_idempotent() {
        let favorites = AssistantEditTarget::new(AssistantEditTargetKind::Favorites, "Favorites");
        let edit = proposal(
            Favorite,
            favorites,
            vec![],
            vec![a()],
            "",
            &["a", "unrelated"],
        )
        .unwrap();
        assert_eq!(
            edit.expected_favorite_ids,
            BTreeSet::from(["a".to_string()])
        );
        assert!(edit.is_no_op());
        assert!(edit.is_favorite_change());
        assert!(
            !proposal(Unfavorite, target(), vec![], vec![a()], "", &["a"])
                .unwrap()
                .is_no_op()
        );
    }

    #[test]
    fn renaming_deletion_and_invalid_plans() {
        assert!(
            proposal(Rename, target(), vec![a()], vec![], " Evening ", &[])
                .unwrap()
                .is_no_op()
        );
        assert!(
            proposal(Delete, target(), vec![a()], vec![], "", &[])
                .unwrap()
                .after
                .is_empty()
        );
        assert_eq!(
            proposal(Rename, target(), vec![], vec![], " ", &[]),
            Err(AssistantError::InvalidResponse)
        );
        assert_eq!(
            proposal(Replace, target(), vec![], vec![], "", &[]),
            Err(AssistantError::NoMatches)
        );
        assert_eq!(
            proposal(Rename, target(), vec![], vec![], &"x".repeat(201), &[]),
            Err(AssistantError::InvalidResponse)
        );
        let too_many = (0..501).map(|i| Song::new(i.to_string(), "t")).collect();
        assert_eq!(
            proposal(Add, target(), vec![], too_many, "", &[]),
            Err(AssistantError::InvalidResponse)
        );
    }

    #[test]
    fn matches_requires_same_identifier_name_and_order() {
        let edit = proposal(Delete, target(), vec![a(), b()], vec![], "", &[]).unwrap();
        let playlist = |id: &str, name: &str, entries: Vec<Song>| Playlist {
            id: id.into(),
            name: name.into(),
            entry: Some(entries),
            ..Default::default()
        };
        assert!(edit.matches(&playlist("list", "Evening", vec![a(), b()])));
        assert!(!edit.matches(&playlist("list", "Other", vec![a(), b()])));
        assert!(!edit.matches(&playlist("list", "Evening", vec![b(), a()])));
        assert!(!edit.matches(&playlist("other", "Evening", vec![a(), b()])));
    }

    #[test]
    fn applied_status_survives_round_trip() {
        let mut edit = proposal(Add, target(), vec![a()], vec![b()], "", &[]).unwrap();
        edit.status = AssistantEditStatus::Applied;
        let json = serde_json::to_string(&edit).unwrap();
        assert!(
            json.contains("\"playlistID\":\"list\"") && json.contains("\"expectedFavoriteIDs\"")
        );
        let restored: AssistantEditProposal = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, edit);
    }
}
