//! The music assistant: model plans, bounded library discovery, ChatGPT and local model providers,
//! persistent conversations and reviewed edits.
//!
//! Privacy rules carried over from the iOS app: models only see bounded public metadata (title, artist,
//! album, years, genre) with temporary `t0…tN` IDs, never server identifiers or credentials. Returned IDs
//! are validated against the candidates, and library edits are proposals the user must confirm.

pub mod chat_store;
pub mod chatgpt;
pub mod conversation;
pub mod dictation;
pub mod edits;
pub mod library;
pub mod local_model;
pub mod music_assistant;
pub mod turn;

pub use chat_store::AssistantChatStore;
pub use conversation::{
    AssistantArchive, AssistantConversation, AssistantMessage, AssistantProvider, AssistantRole,
};
pub use dictation::{DictationTranscript, DictationWord};
pub use edits::{
    AssistantEditOperation, AssistantEditProposal, AssistantEditStatus, AssistantEditTarget,
    AssistantEditTargetKind, AssistantError, AssistantTargetResolver, normalize,
};
pub use library::{AssistantLibrary, Catalog, MaybeSend, MaybeSync};
pub use music_assistant::{
    AssistantIntent, AssistantPlan, AssistantPrompts, AssistantSchema, AssistantSelection,
};
pub use turn::{AssistantResponder, TurnError};

use crate::error::MusicError;
use crate::l10n::t;

/// The localized message for an assistant failure, as shown in the chat.
pub fn assistant_error_message(error: AssistantError) -> String {
    use AssistantError::*;
    t(match error {
        SignInRequired => "Sign in to ChatGPT again to continue.",
        ConsentRequired => "Reconnect ChatGPT and allow access to model usage.",
        InvalidIdentity => "The ChatGPT sign-in could not be verified. Try again.",
        UsageLimit => "Your ChatGPT usage limit has been reached. Check your usage in ChatGPT.",
        Restricted => "ChatGPT model access is not available for this account.",
        LocalUnavailable => {
            "The local model could not be reached. Check that it is running at the configured address."
        }
        InvalidResponse | Incomplete => {
            "The assistant could not complete this request. Try a shorter or more specific request."
        }
        NoMatches => "No matching songs were found in this library. Try another title or artist.",
        AmbiguousTarget => {
            "The playlist could not be identified uniquely. Open the playlist you want and choose Edit with assistant."
        }
        Refused => "The assistant could not help with this request. Try a different music request.",
        Cancelled | Unavailable => {
            "The assistant is unavailable. Check your connection and try again."
        }
    })
}

/// The localized message for a failed turn, either a library or an assistant error.
pub fn turn_error_message(error: TurnError) -> String {
    match error {
        TurnError::Music(error) => crate::l10n::music_error(error),
        TurnError::Assistant(error) => assistant_error_message(error),
    }
}

impl From<MusicError> for TurnError {
    fn from(error: MusicError) -> Self {
        TurnError::Music(error)
    }
}

impl From<AssistantError> for TurnError {
    fn from(error: AssistantError) -> Self {
        TurnError::Assistant(error)
    }
}
