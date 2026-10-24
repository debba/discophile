//! Preferences shared across features, persisted as `settings`.

use discophile_core::StreamingQuality;
use discophile_core::l10n::Language;
use serde::{Deserialize, Serialize};

use crate::platform::storage;
use crate::theme::Appearance;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// `None` follows the system language.
    pub language: Option<Language>,
    pub appearance: Appearance,
    pub introduction_completed: bool,
    pub streaming_quality: StreamingQuality,
    pub scrobble: bool,
    pub online_lyrics: bool,
    /// MusicBrainz and Wikipedia lookups for About.
    pub online_music_info: bool,
    /// Music "Downloaded only" filter (desktop).
    pub downloaded_only: bool,
    pub volume: f32,
    pub sidebar_collapsed: bool,
    pub advanced_unlocked: bool,
    /// Transitions (Equalizer and sound > Transitions).
    pub gapless: bool,
    pub crossfade: bool,
    pub crossfade_seconds: f32,
    pub keep_album_gapless: bool,
    /// Prevents the system from sleeping while music plays (desktop "Keep screen awake").
    pub keep_awake: bool,
    /// Close to the tray, or minimize when the desktop has no tray host.
    pub minimize_on_close: bool,
    /// Interface and text scale (egui zoom), also changed with Ctrl + and Ctrl −.
    pub interface_scale: f32,
    /// Settings > Agents: the MCP server on the loopback interface (desktop).
    pub mcp_enabled: bool,
    pub mcp_port: u16,
    /// Settings > About: look for a newer GitHub release at launch (desktop).
    pub check_for_updates: bool,
    /// The release whose sidebar notice was dismissed.
    pub dismissed_update: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: None,
            appearance: Appearance::System,
            introduction_completed: false,
            streaming_quality: StreamingQuality::High,
            scrobble: false,
            online_lyrics: false,
            online_music_info: false,
            downloaded_only: false,
            volume: 1.0,
            sidebar_collapsed: false,
            advanced_unlocked: false,
            gapless: true,
            crossfade: false,
            crossfade_seconds: 6.0,
            keep_album_gapless: true,
            keep_awake: false,
            minimize_on_close: false,
            interface_scale: 1.0,
            mcp_enabled: false,
            mcp_port: 47_321,
            check_for_updates: true,
            dismissed_update: None,
        }
    }
}

impl Settings {
    const KEY: &'static str = "settings";
    pub const SCALES: [f32; 5] = [0.9, 1.0, 1.15, 1.3, 1.5];

    pub fn load() -> Self {
        storage::load(Self::KEY).unwrap_or_default()
    }

    pub fn save(&self) {
        storage::save(Self::KEY, self);
    }

    pub fn effective_language(&self) -> Language {
        self.language.unwrap_or_else(crate::platform::system_language)
    }

    pub fn transitions(&self) -> crate::audio::TransitionSettings {
        crate::audio::TransitionSettings {
            gapless: self.gapless,
            crossfade: self.crossfade.then_some(self.crossfade_seconds.clamp(1.0, 12.0)),
            keep_album_gapless: self.keep_album_gapless,
        }
    }
}
