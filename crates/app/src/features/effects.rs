//! Equalizer and sound: persisted effects settings pushed to the audio output, the AutoEq headphone browser
//! and ParametricEQ import, and the Audio details window.

mod autoeq;
mod details;

use discophile_core::Song;
use discophile_core::audio_effects::{AudioEffectsSettings, AutoEqEntry};

use crate::app::App;
use crate::platform::storage;

pub use autoeq::{choose_profile, import_profile, load_index, search};

const KEY: &str = "effects";

#[derive(Default)]
pub struct EffectsState {
    /// What the person chose. Transitions are kept in `app.settings` and merged in [`output_settings`].
    pub settings: AudioEffectsSettings,
    /// The settings last handed to the output, so each change is pushed once.
    applied: Option<AudioEffectsSettings>,
    /// AutoEq index entries, downloaded only when the headphone browser opens.
    pub entries: Vec<AutoEqEntry>,
    pub loading_index: bool,
    /// The entry path whose profile is downloading.
    pub loading_profile: Option<String>,
    pub importing: bool,
    pub error: Option<String>,
    /// Choose headphones window.
    pub browser_open: bool,
    pub query: String,
    pub results: Vec<AutoEqEntry>,
    results_query: Option<String>,
    /// Audio details window for this item.
    pub details: Option<Song>,
}

pub fn start(app: &mut App) {
    app.effects.settings = storage::load(KEY).unwrap_or_default();
    push(app);
}

pub fn tick(app: &mut App) {
    push(app);
}

pub fn windows(app: &mut App, ctx: &egui::Context) {
    // `features::tick` doesn't call this module, so changes are pushed here once per frame.
    push(app);
    autoeq::browser_window(app, ctx);
    details::window(app, ctx);
}

pub fn on_library_changed(_app: &mut App) {}

/// Saves and applies a change made to `app.effects.settings`.
pub fn changed(app: &mut App) {
    storage::save(KEY, &app.effects.settings);
    push(app);
}

/// Applies a change without saving it, while a slider is dragged.
pub fn preview(app: &mut App) {
    push(app);
}

/// Saves transition preferences and hands them to the output, then re-prepares the next song.
pub fn transitions_changed(app: &mut App) {
    app.settings.save();
    app.player.output.set_transitions(app.settings.transitions());
    crate::player::refresh_next(app);
    push(app);
}

/// The effects settings for the output, with transitions taken from the shared preferences.
pub fn output_settings(app: &App) -> AudioEffectsSettings {
    let mut settings = app.effects.settings.clone();
    let transitions = app.settings.transitions();
    settings.gapless = transitions.gapless;
    settings.crossfade = transitions.crossfade.map(f64::from).unwrap_or(0.0);
    settings.crossfade_skips_albums = transitions.keep_album_gapless;
    settings
}

fn push(app: &mut App) {
    let settings = output_settings(app);
    if app.effects.applied.as_ref() == Some(&settings) {
        return;
    }
    app.player.output.set_effects(&settings);
    app.effects.applied = Some(settings);
}

/// Whether any effect changes the sound (for Audio details).
pub fn is_active(app: &App) -> bool {
    let s = &app.effects.settings;
    s.active_equalizer().is_some() || s.replay_gain != discophile_core::audio_effects::ReplayGainMode::Off || s.volume_limit < 0.0
}

/// Manual equalizer presets: (English key, ten band gains in dB).
pub const PRESETS: [(&str, [f64; 10]); 6] = [
    ("Flat", [0.0; 10]),
    ("Bass boost", [6.0, 5.0, 4.0, 2.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0]),
    ("Treble boost", [0.0, 0.0, 0.0, 0.0, 0.0, 0.5, 2.0, 3.5, 5.0, 6.0]),
    ("Vocal", [-2.0, -1.5, -1.0, 0.5, 2.5, 3.5, 3.0, 1.5, 0.0, -1.0]),
    ("Loudness", [5.0, 4.0, 2.0, 0.0, -1.0, -1.0, 0.0, 1.5, 3.5, 4.5]),
    ("Acoustic", [3.0, 2.5, 1.5, 0.5, 1.0, 1.0, 2.0, 2.5, 2.0, 1.0]),
];

pub fn apply_preset(app: &mut App, bands: [f64; 10]) {
    app.effects.settings.equalizer = discophile_core::audio_effects::EqualizerMode::Manual;
    for (index, gain) in bands.into_iter().enumerate() {
        app.effects.settings.set_band(index, gain);
    }
    changed(app);
}

/// The preset matching the current bands, if any.
pub fn current_preset(app: &App) -> Option<&'static str> {
    PRESETS
        .iter()
        .find(|(_, bands)| bands.iter().zip(&app.effects.settings.bands).all(|(a, b)| (a - b).abs() < 0.01))
        .map(|(name, _)| *name)
}
