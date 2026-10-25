//! Playback handoff draft: transport-agnostic messages for moving playback between devices, in the
//! spirit of Spotify Connect (desktop → phone, phone → desktop, browser → desktop).
//!
//! The handoff transport is not implemented yet. It will follow the Discophile Connect protocol's future
//! `handoff` capability, which needs its own reviewed specification; until then nothing sends or receives
//! these messages. A snapshot never contains passwords or tokens: the receiving device must already have
//! the same library configured, matched by `account_key`.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::models::{InternetRadioStation, MusicProvider, Song};
use crate::queue::RepeatMode;

/// Version of the handoff messages below (not the Connect account protocol).
pub const PROTOCOL_VERSION: u32 = 1;
pub const MDNS_SERVICE: &str = "_discophile._tcp.local.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceKind {
    Desktop,
    Web,
    Phone,
    Tablet,
    Watch,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub id: Uuid,
    pub name: String,
    pub kind: DeviceKind,
    pub protocol_version: u32,
    pub app_version: String,
    /// Library namespaces this device can play (see `Credentials::account_key`).
    pub account_keys: Vec<String>,
    pub can_play_audiobooks: bool,
    pub can_play_radio: bool,
}

/// What is playing, enough to resume on another device.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum PlaybackContent {
    Music {
        account_key: String,
        provider: MusicProvider,
        queue: Vec<Song>,
        index: usize,
        repeat_mode: RepeatMode,
    },
    Audiobook {
        /// Audiobookshelf server namespace and library item.
        account_key: String,
        item_id: String,
        title: String,
    },
    Radio {
        station: InternetRadioStation,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackSnapshot {
    pub content: PlaybackContent,
    /// Seconds into the current item.
    pub position: f64,
    pub playing: bool,
    pub volume: f32,
    pub rate: f32,
    /// Unix milliseconds when the snapshot was taken; receivers add elapsed time when `playing`.
    pub captured_at: i64,
}

impl PlaybackSnapshot {
    /// The position to resume at `now_ms`, accounting for time spent in transit while playing.
    pub fn position_at(&self, now_ms: i64) -> f64 {
        if !self.playing {
            return self.position;
        }
        let elapsed = (now_ms - self.captured_at).max(0) as f64 / 1000.0;
        self.position + elapsed.min(30.0) * self.rate as f64
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "command")]
pub enum RemoteCommand {
    Play,
    Pause,
    Next,
    Previous,
    Seek { position: f64 },
    SetVolume { volume: f32 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "message")]
pub enum ConnectMessage {
    Hello { device: DeviceInfo },
    /// Periodic state from the active device so others can show "Playing on …".
    State { device_id: Uuid, snapshot: Option<PlaybackSnapshot> },
    /// Asks the active device to hand playback to `target`.
    TransferRequest { target: Uuid },
    /// Sent to the target; it starts playing and the sender pauses after `TransferComplete`.
    Transfer { from: Uuid, snapshot: PlaybackSnapshot },
    TransferComplete { device_id: Uuid },
    TransferRejected { device_id: Uuid, reason: String },
    Command { target: Uuid, command: RemoteCommand },
}

impl ConnectMessage {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
    pub fn from_json(text: &str) -> Option<Self> {
        serde_json::from_str(text).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_round_trip() {
        let snapshot = PlaybackSnapshot {
            content: PlaybackContent::Music {
                account_key: "abc".into(),
                provider: MusicProvider::Navidrome,
                queue: vec![Song::new("1", "One")],
                index: 0,
                repeat_mode: RepeatMode::All,
            },
            position: 12.0,
            playing: true,
            volume: 1.0,
            rate: 1.0,
            captured_at: 1_000,
        };
        let message = ConnectMessage::Transfer { from: Uuid::nil(), snapshot: snapshot.clone() };
        assert_eq!(ConnectMessage::from_json(&message.to_json()), Some(message));
        assert_eq!(snapshot.position_at(3_000), 14.0);
        assert_eq!(snapshot.position_at(500), 12.0);
    }
}
