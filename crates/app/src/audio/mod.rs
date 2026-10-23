//! Audio output. The player model drives an `AudioOutput`; the desktop build decodes and renders
//! audio itself (effects, gapless, crossfade), the web build uses an HTML media element with Web Audio.

use discophile_core::ReplayGain;
use discophile_core::audio_effects::AudioEffectsSettings;

#[cfg(not(target_arch = "wasm32"))]
pub mod native;
pub mod transition;
#[cfg(target_arch = "wasm32")]
pub mod web;

#[derive(Clone, Debug, PartialEq)]
pub enum AudioSource {
    /// A stream URL with any authentication already in its query string.
    Url(String),
    /// A downloaded file (desktop only).
    #[cfg(not(target_arch = "wasm32"))]
    File(std::path::PathBuf),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ContentKind {
    #[default]
    Music,
    Podcast,
    Audiobook,
    Radio,
}

/// One playable item, with what the output needs to treat it correctly.
#[derive(Clone, Debug, PartialEq)]
pub struct MediaItem {
    /// Stable identity, used to report which item finished or started.
    pub id: String,
    pub source: AudioSource,
    pub kind: ContentKind,
    /// Known duration in seconds, used until the decoder reports one.
    pub duration_hint: Option<f64>,
    pub replay_gain: Option<ReplayGain>,
    /// Consecutive songs with the same key (album played in order) are never crossfaded when
    /// "Keep album tracks gapless" is on, and use album gain in Automatic ReplayGain mode.
    pub album_key: Option<String>,
    /// Format reported by the server, for Audio details.
    pub server_format: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransitionSettings {
    pub gapless: bool,
    /// 1–12 seconds, or `None` for no crossfade.
    pub crossfade: Option<f32>,
    pub keep_album_gapless: bool,
}

impl Default for TransitionSettings {
    fn default() -> Self {
        Self { gapless: true, crossfade: None, keep_album_gapless: true }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioDetails {
    pub codec: Option<String>,
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u32>,
    pub channels: Option<u32>,
    pub bit_rate: Option<u32>,
    pub output_device: Option<String>,
    pub output_sample_rate: Option<u32>,
    pub streamed: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct OutputStatus {
    pub item_id: Option<String>,
    pub position: f64,
    pub duration: Option<f64>,
    pub playing: bool,
    pub buffering: bool,
    pub details: Option<AudioDetails>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AudioEvent {
    /// The current item played to its end and nothing was queued with `set_next`.
    Ended { item_id: String },
    /// The output moved to the item given to `set_next` by itself (gapless or crossfade).
    Advanced { from: String, to: String },
    Failed { item_id: String, message: String },
}

pub trait AudioOutput {
    /// Replaces whatever is playing.
    fn load(&mut self, item: MediaItem, start_at: f64, autoplay: bool);
    /// The item to continue with automatically. The output prepares it ahead of the end
    /// (30 seconds for gapless) and reports `Advanced` when it takes over.
    fn set_next(&mut self, item: Option<MediaItem>);
    fn play(&mut self);
    fn pause(&mut self);
    fn stop(&mut self);
    fn seek(&mut self, seconds: f64);
    /// Linear 0–1 application volume.
    fn set_volume(&mut self, volume: f32);
    /// Playback speed, 0.5–4 (audiobooks and podcasts).
    fn set_rate(&mut self, rate: f32);
    fn set_transitions(&mut self, settings: TransitionSettings);
    /// Equalizer, ReplayGain and volume limit. Music and podcasts get every effect, radio has no
    /// ReplayGain, audiobooks only use the volume limit.
    fn set_effects(&mut self, settings: &AudioEffectsSettings);
    fn status(&self) -> OutputStatus;
    /// Events since the last call. Called every frame by the player model.
    fn poll_events(&mut self) -> Vec<AudioEvent>;
}

/// An output that plays nothing, used before a device is available and in tests.
#[derive(Default)]
pub struct NullOutput {
    status: OutputStatus,
    events: Vec<AudioEvent>,
}

impl AudioOutput for NullOutput {
    fn load(&mut self, item: MediaItem, start_at: f64, autoplay: bool) {
        self.status = OutputStatus { item_id: Some(item.id), position: start_at, duration: item.duration_hint, playing: autoplay, ..Default::default() };
    }
    fn set_next(&mut self, _item: Option<MediaItem>) {}
    fn play(&mut self) {
        self.status.playing = self.status.item_id.is_some();
    }
    fn pause(&mut self) {
        self.status.playing = false;
    }
    fn stop(&mut self) {
        self.status = OutputStatus::default();
    }
    fn seek(&mut self, seconds: f64) {
        self.status.position = seconds.max(0.0);
    }
    fn set_volume(&mut self, _volume: f32) {}
    fn set_rate(&mut self, _rate: f32) {}
    fn set_transitions(&mut self, _settings: TransitionSettings) {}
    fn set_effects(&mut self, _settings: &AudioEffectsSettings) {}
    fn status(&self) -> OutputStatus {
        self.status.clone()
    }
    fn poll_events(&mut self) -> Vec<AudioEvent> {
        std::mem::take(&mut self.events)
    }
}

/// The best output for this platform: the native engine on desktop, the media element backend on
/// the web, or `NullOutput` if neither can start. Both wake the interface when events arrive.
pub fn create_output(ctx: &egui::Context) -> Box<dyn AudioOutput> {
    #[cfg(not(target_arch = "wasm32"))]
    let output = native::NativeOutput::new(ctx).map(|o| Box::new(o) as Box<dyn AudioOutput>);
    #[cfg(target_arch = "wasm32")]
    let output = web::WebOutput::new(ctx).map(|o| Box::new(o) as Box<dyn AudioOutput>);
    output.unwrap_or_else(|error| {
        log::warn!("Audio output unavailable: {error}");
        Box::new(NullOutput::default())
    })
}
