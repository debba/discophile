//! Portable Discophile logic shared by the desktop and web builds.
//! Nothing here depends on a GUI, an audio device or a filesystem; the app crate provides those.

pub mod error;
pub mod http;
pub mod l10n;
pub mod models;
pub mod oauth;
pub mod queue;

// Providers
pub mod audiobookshelf;
pub mod client;
pub mod jellyfin;
pub mod subsonic;

// Library features
pub mod album_listening;
pub mod audiobook_reader;
pub mod automatic_downloads;
pub mod curated_mix;
pub mod listening_history;
pub mod listening_statistics;
pub mod lrclib;
pub mod lyrics;
pub mod lyrics_repository;
pub mod mentions;
pub mod music_info;
pub mod offline_audiobook;
pub mod offline_catalog;
pub mod personal_radio;
pub mod playback_progress;
pub mod playlist_resume;
pub mod podcast_directory;

// Sound
pub mod audio_effects;

// Assistant
pub mod assistant;

// Discophile Connect accounts and the device handoff draft
pub mod connect;

// External agents
pub mod mcp;

// Desktop updates
pub mod updates;

pub use error::{MusicError, MusicResult};
pub use l10n::{count, t, tf};
pub use models::*;

/// Lowercase hexadecimal MD5, as used by Subsonic token authentication and account namespaces.
pub fn md5_hex(input: &[u8]) -> String {
    use md5::{Digest, Md5};
    hex::encode(Md5::digest(input))
}

/// Unix time in milliseconds, on native and in the browser.
pub fn now_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
