//! Equalizer, ReplayGain, volume limit and peak limiter for a native audio engine, plus AutoEq profiles.
//!
//! [`AudioEffectsSettings`] is what the interface edits and persists. [`EffectsProcessor`] applies it to
//! interleaved `f32` audio at any sample rate and channel count. Processing allocates nothing.

use std::f64::consts::PI;

use serde::{Deserialize, Deserializer, Serialize};
use url::Url;

use crate::error::{MusicError, MusicResult};
use crate::models::{ReplayGain, Song};
use crate::queue::PlaybackQueue;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReplayGainMode {
    #[default]
    Off,
    Track,
    Album,
    /// Uses album gain while an album plays in order, otherwise track gain.
    Automatic,
}

impl ReplayGainMode {
    pub const ALL: [ReplayGainMode; 4] = [Self::Off, Self::Track, Self::Album, Self::Automatic];
    /// The English localization key.
    pub fn title(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Track => "Track",
            Self::Album => "Album",
            Self::Automatic => "Automatic",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum EqualizerMode {
    #[default]
    #[serde(rename = "off")]
    Off,
    #[serde(rename = "manual")]
    Manual,
    #[serde(rename = "autoEq")]
    AutoEq,
}

impl EqualizerMode {
    pub const ALL: [EqualizerMode; 3] = [Self::Off, Self::Manual, Self::AutoEq];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FilterKind {
    Peaking,
    LowShelf,
    HighShelf,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EqualizerFilter {
    pub kind: FilterKind,
    /// Center or corner frequency in Hz.
    pub frequency: f64,
    /// Gain in dB.
    pub gain: f64,
    pub q: f64,
}

impl EqualizerFilter {
    pub fn new(kind: FilterKind, frequency: f64, gain: f64, q: f64) -> Self {
        Self {
            kind,
            frequency,
            gain,
            q,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ParametricEqualizer {
    pub preamp: f64,
    pub filters: Vec<EqualizerFilter>,
}

impl ParametricEqualizer {
    pub const MAX_FILTERS: usize = 20;

    pub fn new(preamp: f64, filters: Vec<EqualizerFilter>) -> Self {
        Self { preamp, filters }
    }

    /// Reads an AutoEq or Equalizer APO `ParametricEQ.txt`. Disabled and unsupported filters are skipped.
    pub fn parse(text: &str) -> MusicResult<Self> {
        fn number(text: &str) -> Option<f64> {
            text.replace(',', ".").parse::<f64>().ok()
        }
        fn after(label: &str, words: &[&str]) -> Option<f64> {
            let index = words.iter().position(|w| w.eq_ignore_ascii_case(label))?;
            words.get(index + 1).and_then(|w| number(w))
        }
        let mut preamp = 0.0;
        let mut filters = Vec::new();
        for line in text.lines() {
            let words: Vec<&str> = line
                .split([' ', '\t', ':'])
                .filter(|w| !w.is_empty())
                .collect();
            let Some(first) = words.first().map(|w| w.to_lowercase()) else {
                continue;
            };
            if first == "preamp"
                && let Some(value) = words.get(1).and_then(|w| number(w))
            {
                preamp = value;
                continue;
            }
            let upper: Vec<String> = words.iter().map(|w| w.to_uppercase()).collect();
            let has = |names: &[&str]| upper.iter().any(|w| names.contains(&w.as_str()));
            if first != "filter" || !has(&["ON"]) {
                continue;
            }
            let kind = if has(&["PK", "PEQ"]) {
                FilterKind::Peaking
            } else if has(&["LSC", "LS", "LSQ"]) {
                FilterKind::LowShelf
            } else if has(&["HSC", "HS", "HSQ"]) {
                FilterKind::HighShelf
            } else {
                continue;
            };
            let (Some(frequency), Some(gain)) = (after("Fc", &words), after("Gain", &words)) else {
                continue;
            };
            if !frequency.is_finite() || frequency <= 0.0 || !gain.is_finite() {
                continue;
            }
            let q = after("Q", &words).unwrap_or(if kind == FilterKind::Peaking {
                1.41
            } else {
                0.71
            });
            if !q.is_finite() || q <= 0.0 {
                continue;
            }
            filters.push(EqualizerFilter::new(
                kind,
                frequency,
                gain.clamp(-30.0, 30.0),
                q,
            ));
        }
        if filters.is_empty() || !preamp.is_finite() {
            return Err(MusicError::InvalidResponse);
        }
        filters.truncate(Self::MAX_FILTERS);
        Ok(Self {
            preamp: preamp.clamp(-30.0, 30.0),
            filters,
        })
    }

    /// The combined filter response in dB at each frequency, excluding the preamp.
    pub fn response_curve(&self, sample_rate: f64, frequencies: &[f64]) -> Vec<f64> {
        let sample_rate = valid_rate(sample_rate);
        let biquads = biquads(self, sample_rate);
        frequencies
            .iter()
            .map(|&f| response(&biquads, f, sample_rate))
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutoEqProfile {
    pub name: String,
    pub source: String,
    pub equalizer: ParametricEqualizer,
}

impl AutoEqProfile {
    /// Largest accepted imported `ParametricEQ.txt`, in bytes.
    pub const MAX_IMPORT_BYTES: usize = 64_000;

    /// Reads an imported file. `file_name` is used for the profile name; `source` is shown under it
    /// (the app passes `t("Imported file")`).
    pub fn import(file_name: &str, text: &str, source: &str) -> MusicResult<Self> {
        if text.len() >= Self::MAX_IMPORT_BYTES {
            return Err(MusicError::InvalidResponse);
        }
        let equalizer = ParametricEqualizer::parse(text)?;
        let stem = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
        let stem = match stem.rfind('.') {
            Some(dot) if dot > 0 => &stem[..dot],
            _ => stem,
        };
        Ok(Self {
            name: stem.replace(" ParametricEQ", ""),
            source: source.to_string(),
            equalizer,
        })
    }
}

/// Audio effects chosen by the person. Decoding never fails on bad values: they fall back to defaults or clamp.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioEffectsSettings {
    pub equalizer: EqualizerMode,
    /// Manual band gains in dB, one per [`Self::BAND_FREQUENCIES`] entry.
    pub bands: Vec<f64>,
    pub auto_eq: Option<AutoEqProfile>,
    pub replay_gain: ReplayGainMode,
    /// Added to tagged gains, in dB.
    pub replay_gain_preamp: f64,
    pub prevent_clipping: bool,
    /// Attenuation applied to all Discophile audio, in dB. Zero means no limit.
    pub volume_limit: f64,
    /// Seconds of overlap between automatically advancing songs. Zero turns crossfade off.
    pub crossfade: f64,
    pub gapless: bool,
    /// Consecutive songs of one album keep their gapless join instead of crossfading.
    pub crossfade_skips_albums: bool,
}

impl Default for AudioEffectsSettings {
    fn default() -> Self {
        Self {
            equalizer: EqualizerMode::Off,
            bands: vec![0.0; Self::BAND_FREQUENCIES.len()],
            auto_eq: None,
            replay_gain: ReplayGainMode::Off,
            replay_gain_preamp: 0.0,
            prevent_clipping: true,
            volume_limit: 0.0,
            crossfade: 0.0,
            gapless: true,
            crossfade_skips_albums: true,
        }
    }
}

impl<'de> Deserialize<'de> for AudioEffectsSettings {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde_json::Value;
        let value = Value::deserialize(deserializer)?;
        let field = |key: &str| value.get(key).cloned().filter(|v| !v.is_null());
        fn decode<T: serde::de::DeserializeOwned>(v: Option<Value>) -> Option<T> {
            v.and_then(|v| serde_json::from_value(v).ok())
        }
        let mut settings = Self::default();
        if let Some(mode) = decode(field("equalizer")) {
            settings.equalizer = mode;
        }
        let bands: Vec<f64> = decode(field("bands")).unwrap_or_default();
        if bands.len() == Self::BAND_FREQUENCIES.len() {
            settings.bands = bands.into_iter().map(Self::band).collect();
        }
        settings.auto_eq = decode(field("autoEq"));
        if let Some(mode) = decode(field("replayGain")) {
            settings.replay_gain = mode;
        }
        let finite = |v: Option<f64>| v.filter(|v| v.is_finite()).unwrap_or(0.0);
        settings.replay_gain_preamp = finite(decode(field("replayGainPreamp")))
            .clamp(Self::PREAMP_RANGE.0, Self::PREAMP_RANGE.1);
        if let Some(on) = decode(field("preventClipping")) {
            settings.prevent_clipping = on;
        }
        settings.volume_limit = finite(decode(field("volumeLimit")))
            .clamp(Self::VOLUME_LIMIT_RANGE.0, Self::VOLUME_LIMIT_RANGE.1);
        settings.crossfade = finite(decode(field("crossfade")))
            .clamp(Self::CROSSFADE_RANGE.0, Self::CROSSFADE_RANGE.1);
        if let Some(on) = decode(field("gapless")) {
            settings.gapless = on;
        }
        if let Some(on) = decode(field("crossfadeSkipsAlbums")) {
            settings.crossfade_skips_albums = on;
        }
        Ok(settings)
    }
}

impl AudioEffectsSettings {
    pub const BAND_FREQUENCIES: [f64; 10] = [
        31.0, 62.0, 125.0, 250.0, 500.0, 1_000.0, 2_000.0, 4_000.0, 8_000.0, 16_000.0,
    ];
    pub const BAND_RANGE: (f64, f64) = (-12.0, 12.0);
    pub const VOLUME_LIMIT_RANGE: (f64, f64) = (-24.0, 0.0);
    pub const PREAMP_RANGE: (f64, f64) = (-15.0, 15.0);
    pub const CROSSFADE_RANGE: (f64, f64) = (0.0, 12.0);
    /// Q of the manual equalizer's peaking bands.
    pub const BAND_Q: f64 = 1.41;

    /// Clamps a manual band gain; non-finite values become zero.
    pub fn band(gain: f64) -> f64 {
        if gain.is_finite() {
            gain.clamp(Self::BAND_RANGE.0, Self::BAND_RANGE.1)
        } else {
            0.0
        }
    }

    /// Sets one manual band, clamped. Out-of-range indices are ignored.
    pub fn set_band(&mut self, index: usize, gain: f64) {
        if self.bands.len() != Self::BAND_FREQUENCIES.len() {
            self.bands = vec![0.0; Self::BAND_FREQUENCIES.len()];
        }
        if let Some(band) = self.bands.get_mut(index) {
            *band = Self::band(gain);
        }
    }

    /// The equalizer that applies now, with headroom for manual boosts.
    pub fn active_equalizer(&self) -> Option<ParametricEqualizer> {
        match self.equalizer {
            EqualizerMode::Off => None,
            EqualizerMode::AutoEq => self.auto_eq.as_ref().map(|p| p.equalizer.clone()),
            EqualizerMode::Manual => {
                if !self.bands.iter().any(|&b| b != 0.0) {
                    return None;
                }
                let filters = Self::BAND_FREQUENCIES
                    .iter()
                    .zip(&self.bands)
                    .filter(|(_, g)| **g != 0.0)
                    .map(|(&f, &g)| EqualizerFilter::new(FilterKind::Peaking, f, g, Self::BAND_Q))
                    .collect();
                let max = self.bands.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                Some(ParametricEqualizer::new(-max.max(0.0), filters))
            }
        }
    }

    /// The equalizer response in dB at each frequency, for drawing. Excludes the preamp and gain.
    pub fn response_curve(&self, sample_rate: f64, frequencies: &[f64]) -> Vec<f64> {
        match self.active_equalizer() {
            Some(eq) => eq.response_curve(sample_rate, frequencies),
            None => vec![0.0; frequencies.len()],
        }
    }

    /// The ReplayGain adjustment in dB for one track, or `None` when none applies.
    pub fn replay_gain_decibels(
        &self,
        gain: Option<&ReplayGain>,
        album_context: bool,
    ) -> Option<f64> {
        if self.replay_gain == ReplayGainMode::Off {
            return None;
        }
        let album = self.replay_gain == ReplayGainMode::Album
            || (self.replay_gain == ReplayGainMode::Automatic && album_context);
        let base = gain.and_then(|g| g.base_gain).unwrap_or(0.0);
        let pick = |first: Option<f64>, second: Option<f64>| first.or(second);
        let tagged = gain.and_then(|g| {
            if album {
                pick(g.album_gain, g.track_gain)
            } else {
                pick(g.track_gain, g.album_gain)
            }
        });
        let Some(tagged) = tagged.filter(|t| t.is_finite()) else {
            return gain
                .and_then(|g| g.fallback_gain)
                .filter(|f| f.is_finite())
                .map(|f| f + base);
        };
        let mut decibels = tagged + base + self.replay_gain_preamp;
        let peak = gain.and_then(|g| {
            if album {
                pick(g.album_peak, g.track_peak)
            } else {
                pick(g.track_peak, g.album_peak)
            }
        });
        if self.prevent_clipping
            && let Some(peak) = peak.filter(|p| p.is_finite() && *p > 0.0)
        {
            decibels = decibels.min(-20.0 * peak.log10());
        }
        Some(decibels)
    }

    /// The localized name of the current equalizer: Off, Manual, or the AutoEq profile name.
    pub fn equalizer_name(&self) -> String {
        match self.equalizer {
            EqualizerMode::Off => crate::t("Off"),
            EqualizerMode::Manual => crate::t("Manual"),
            EqualizerMode::AutoEq => self
                .auto_eq
                .as_ref()
                .map(|p| p.name.clone())
                .unwrap_or_else(|| crate::t("Off")),
        }
    }
}

/// How an automatic advance joins two songs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrackTransition {
    /// Wait for the end, then load the next song.
    Sequential,
    /// Prepare the next song so it starts without a pause.
    Gapless,
    Crossfade {
        seconds: f64,
    },
}

impl TrackTransition {
    /// Chooses how an automatic advance joins `from` (of `duration` seconds) to `to`.
    pub fn between(
        from: &Song,
        to: Option<&Song>,
        duration: f64,
        settings: &AudioEffectsSettings,
        in_order: bool,
        repeat_one: bool,
    ) -> Self {
        let Some(to) = to else {
            return Self::Sequential;
        };
        if from.is_podcast() || to.is_podcast() {
            return Self::Sequential;
        }
        let same_album = in_order && from.album_id.is_some() && from.album_id == to.album_id;
        let fade = settings.crossfade;
        if fade > 0.0
            && !repeat_one
            && !(same_album && settings.crossfade_skips_albums)
            && duration.is_finite()
            && duration >= fade * 3.0
            && to.duration.unwrap_or(f64::INFINITY) >= fade * 2.0
        {
            return Self::Crossfade { seconds: fade };
        }
        if settings.gapless {
            Self::Gapless
        } else {
            Self::Sequential
        }
    }

    /// Equal-power volumes `(outgoing, incoming)` at a fade progress from 0 to 1.
    pub fn volumes(progress: f64) -> (f32, f32) {
        let p = if progress.is_finite() { progress } else { 1.0 }.clamp(0.0, 1.0);
        ((p * PI / 2.0).cos() as f32, (p * PI / 2.0).sin() as f32)
    }
}

/// The song an automatic advance would play next, without changing the queue.
pub fn automatic_next(queue: &PlaybackQueue) -> Option<Song> {
    queue.peek_next(true).cloned()
}

/// Track-dependent inputs for one playing item.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioTrackContext {
    pub replay_gain: Option<ReplayGain>,
    pub album_context: bool,
    /// Audiobooks and podcasts-as-books only use the volume limit.
    pub music: bool,
}

impl Default for AudioTrackContext {
    fn default() -> Self {
        Self {
            replay_gain: None,
            album_context: false,
            music: true,
        }
    }
}

/// A normalized RBJ biquad (Audio EQ Cookbook).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Biquad {
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
}

impl Default for Biquad {
    fn default() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
        }
    }
}

impl Biquad {
    pub fn new(filter: &EqualizerFilter, sample_rate: f64) -> Self {
        let a = 10f64.powf(filter.gain / 40.0);
        let w0 = 2.0 * PI * filter.frequency / sample_rate;
        let cosine = w0.cos();
        let alpha = w0.sin() / (2.0 * filter.q);
        let root = 2.0 * a.sqrt() * alpha;
        let (n0, n1, n2, d0, d1, d2) = match filter.kind {
            FilterKind::Peaking => (
                1.0 + alpha * a,
                -2.0 * cosine,
                1.0 - alpha * a,
                1.0 + alpha / a,
                -2.0 * cosine,
                1.0 - alpha / a,
            ),
            FilterKind::LowShelf => (
                a * ((a + 1.0) - (a - 1.0) * cosine + root),
                2.0 * a * ((a - 1.0) - (a + 1.0) * cosine),
                a * ((a + 1.0) - (a - 1.0) * cosine - root),
                (a + 1.0) + (a - 1.0) * cosine + root,
                -2.0 * ((a - 1.0) + (a + 1.0) * cosine),
                (a + 1.0) + (a - 1.0) * cosine - root,
            ),
            FilterKind::HighShelf => (
                a * ((a + 1.0) + (a - 1.0) * cosine + root),
                -2.0 * a * ((a - 1.0) + (a + 1.0) * cosine),
                a * ((a + 1.0) + (a - 1.0) * cosine - root),
                (a + 1.0) - (a - 1.0) * cosine + root,
                2.0 * ((a - 1.0) - (a + 1.0) * cosine),
                (a + 1.0) - (a - 1.0) * cosine - root,
            ),
        };
        Self {
            b0: n0 / d0,
            b1: n1 / d0,
            b2: n2 / d0,
            a1: d1 / d0,
            a2: d2 / d0,
        }
    }

    /// Linear magnitude response at `frequency`.
    pub fn magnitude(&self, frequency: f64, sample_rate: f64) -> f64 {
        let w = 2.0 * PI * frequency / sample_rate;
        let (c1, s1, c2, s2) = (w.cos(), w.sin(), (2.0 * w).cos(), (2.0 * w).sin());
        let nr = self.b0 + self.b1 * c1 + self.b2 * c2;
        let ni = -(self.b1 * s1 + self.b2 * s2);
        let dr = 1.0 + self.a1 * c1 + self.a2 * c2;
        let di = -(self.a1 * s1 + self.a2 * s2);
        ((nr * nr + ni * ni) / (dr * dr + di * di)).sqrt()
    }
}

fn valid_rate(sample_rate: f64) -> f64 {
    if sample_rate > 0.0 && sample_rate.is_finite() {
        sample_rate
    } else {
        44_100.0
    }
}

fn biquads(equalizer: &ParametricEqualizer, sample_rate: f64) -> Vec<Biquad> {
    let nyquist = sample_rate * 0.49;
    equalizer
        .filters
        .iter()
        .filter(|f| f.frequency < nyquist && f.gain != 0.0)
        .map(|f| Biquad::new(f, sample_rate))
        .collect()
}

fn response(filters: &[Biquad], frequency: f64, sample_rate: f64) -> f64 {
    20.0 * filters
        .iter()
        .map(|b| b.magnitude(frequency, sample_rate))
        .product::<f64>()
        .log10()
}

/// `count` logarithmically spaced frequencies from `low` to `high` Hz, for drawing response curves.
pub fn log_frequencies(count: usize, low: f64, high: f64) -> Vec<f64> {
    if count == 0 || low <= 0.0 || high <= low {
        return Vec::new();
    }
    if count == 1 {
        return vec![low];
    }
    let ratio = (high / low).ln();
    (0..count)
        .map(|i| low * (ratio * i as f64 / (count - 1) as f64).exp())
        .collect()
}

/// Real-time processing for interleaved `f32` audio: equalizer, gain and a peak limiter.
/// One instance belongs to one audio stream; call it from the audio callback only.
#[derive(Debug, Clone)]
pub struct EffectsProcessor {
    settings: AudioEffectsSettings,
    track: AudioTrackContext,
    sample_rate: f64,
    channels: usize,
    gain: f64,
    filters: Vec<Biquad>,
    /// Transposed direct form II state, `[channel * filters + filter] = (z1, z2)`.
    state: Vec<(f64, f64)>,
    applied_gain: f64,
    envelope: f64,
    release: f64,
}

impl EffectsProcessor {
    /// Output ceiling, -0.3 dBFS.
    pub const CEILING: f64 = 0.966;
    /// Largest total boost, in dB.
    pub const MAX_GAIN_DB: f64 = 24.0;

    pub fn new(settings: AudioEffectsSettings, sample_rate: f64, channels: usize) -> Self {
        let sample_rate = valid_rate(sample_rate);
        let mut processor = Self {
            settings,
            track: AudioTrackContext::default(),
            sample_rate,
            channels: channels.max(1),
            gain: 1.0,
            filters: Vec::new(),
            state: Vec::new(),
            applied_gain: 1.0,
            envelope: 1.0,
            release: 1.0 - (-1.0 / (0.15 * sample_rate)).exp(),
        };
        processor.configure();
        processor
    }

    pub fn sample_rate(&self) -> f64 {
        self.sample_rate
    }
    pub fn channels(&self) -> usize {
        self.channels
    }
    pub fn settings(&self) -> &AudioEffectsSettings {
        &self.settings
    }
    pub fn track(&self) -> &AudioTrackContext {
        &self.track
    }
    /// The target linear gain (volume limit, equalizer preamp and ReplayGain).
    pub fn gain(&self) -> f64 {
        self.gain
    }

    /// Applies new settings; filter state is kept when the filter count is unchanged, gain changes ramp.
    pub fn update(&mut self, settings: AudioEffectsSettings) {
        self.settings = settings;
        self.configure();
    }

    /// Sets the gain inputs of the music track that starts playing.
    pub fn set_track_gain(&mut self, replay_gain: Option<&ReplayGain>, album_mode: bool) {
        self.track = AudioTrackContext {
            replay_gain: replay_gain.cloned(),
            album_context: album_mode,
            music: true,
        };
        self.configure();
    }

    /// Sets every track input, including whether it is music (audiobooks only use the volume limit).
    pub fn set_track_context(&mut self, track: AudioTrackContext) {
        self.track = track;
        self.configure();
    }

    /// Changes the stream format, clearing filter state.
    pub fn set_format(&mut self, sample_rate: f64, channels: usize) {
        let sample_rate = valid_rate(sample_rate);
        let channels = channels.max(1);
        if sample_rate == self.sample_rate && channels == self.channels {
            return;
        }
        self.sample_rate = sample_rate;
        self.channels = channels;
        self.release = 1.0 - (-1.0 / (0.15 * sample_rate)).exp();
        self.filters.clear();
        self.configure();
    }

    /// Clears filter memory and the limiter, for example after a seek.
    pub fn reset(&mut self) {
        self.state.iter_mut().for_each(|s| *s = (0.0, 0.0));
        self.envelope = 1.0;
        self.applied_gain = self.gain;
    }

    pub fn is_transparent(&self) -> bool {
        self.filters.is_empty()
            && (self.gain - 1.0).abs() < 1e-6
            && (self.applied_gain - 1.0).abs() < 1e-6
            && self.envelope >= 1.0
    }

    fn configure(&mut self) {
        let equalizer = if self.track.music {
            self.settings.active_equalizer()
        } else {
            None
        };
        let biquads = equalizer
            .as_ref()
            .map(|eq| biquads(eq, self.sample_rate))
            .unwrap_or_default();
        if biquads.len() != self.filters.len() || self.state.len() != biquads.len() * self.channels
        {
            self.state = vec![(0.0, 0.0); biquads.len() * self.channels];
        }
        self.filters = biquads;
        let mut decibels =
            self.settings.volume_limit + equalizer.as_ref().map_or(0.0, |eq| eq.preamp);
        if self.track.music
            && let Some(replay) = self
                .settings
                .replay_gain_decibels(self.track.replay_gain.as_ref(), self.track.album_context)
        {
            decibels += replay;
        }
        self.gain = 10f64.powf(decibels.min(Self::MAX_GAIN_DB) / 20.0);
    }

    /// The combined equalizer response in dB at `frequency`, excluding gain.
    pub fn response(&self, frequency: f64) -> f64 {
        response(&self.filters, frequency, self.sample_rate)
    }

    /// The equalizer response in dB at each frequency, for drawing.
    pub fn response_curve(&self, frequencies: &[f64]) -> Vec<f64> {
        frequencies.iter().map(|&f| self.response(f)).collect()
    }

    /// Processes interleaved samples in place. A trailing partial frame is left untouched.
    pub fn process(&mut self, interleaved: &mut [f32]) {
        let channels = self.channels;
        let frames = interleaved.len() / channels;
        if frames == 0 || self.is_transparent() {
            return;
        }
        let count = self.filters.len();
        if count > 0 {
            for frame in interleaved.chunks_exact_mut(channels) {
                for (channel, sample) in frame.iter_mut().enumerate() {
                    let mut x = f64::from(*sample);
                    let state = &mut self.state[channel * count..(channel + 1) * count];
                    for (f, (z1, z2)) in self.filters.iter().zip(state.iter_mut()) {
                        let y = f.b0 * x + *z1;
                        *z1 = f.b1 * x - f.a1 * y + *z2;
                        *z2 = f.b2 * x - f.a2 * y;
                        x = y;
                    }
                    *sample = x as f32;
                }
            }
            // Flush denormals and recover from invalid input.
            let clean = |z: f64| {
                if z.is_finite() && z.abs() > 1e-25 {
                    z
                } else {
                    0.0
                }
            };
            for (z1, z2) in &mut self.state {
                *z1 = clean(*z1);
                *z2 = clean(*z2);
            }
        }
        // Ramp gain changes across one buffer to avoid clicks.
        let start = self.applied_gain;
        let step = (self.gain - start) / frames as f64;
        for (index, frame) in interleaved.chunks_exact_mut(channels).enumerate() {
            let g = start + step * (index + 1) as f64;
            let peak = frame
                .iter()
                .fold(0.0f64, |p, s| p.max(f64::from(*s).abs() * g));
            let target = if peak > Self::CEILING {
                Self::CEILING / peak
            } else {
                1.0
            };
            self.envelope = if target < self.envelope {
                target
            } else {
                self.envelope + (target - self.envelope) * self.release
            };
            let total = (g * self.envelope) as f32;
            frame.iter_mut().for_each(|s| *s *= total);
        }
        self.applied_gain = self.gain;
    }
}

/// An AutoEq index entry.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AutoEqEntry {
    pub name: String,
    pub source: String,
    /// Percent-encoded path below `results/`, as listed in the AutoEq index.
    pub path: String,
}

impl AutoEqEntry {
    pub fn id(&self) -> &str {
        &self.path
    }
}

pub mod auto_eq {
    use super::*;

    pub const RESULTS: &str =
        "https://raw.githubusercontent.com/jaakkopasanen/AutoEq/master/results/";
    pub const INDEX: &str =
        "https://raw.githubusercontent.com/jaakkopasanen/AutoEq/master/results/INDEX.md";
    /// The app may reuse a cached index for this long before downloading it again.
    pub const INDEX_MAX_AGE_MILLIS: i64 = 30 * 86_400 * 1_000;
    /// Default search result count.
    pub const SEARCH_LIMIT: usize = 50;

    /// Whether an index cached at `saved_at_millis` can still be used at `now_millis`.
    pub fn index_is_fresh(saved_at_millis: i64, now_millis: i64) -> bool {
        now_millis - saved_at_millis < INDEX_MAX_AGE_MILLIS
    }

    /// Parses `- [Name](./source/rig/Name) by Source on Rig` lines.
    pub fn entries(index: &str) -> Vec<AutoEqEntry> {
        index.lines().filter_map(entry).collect()
    }

    fn entry(line: &str) -> Option<AutoEqEntry> {
        if !line.starts_with("- [") {
            return None;
        }
        let close = line.find("](")?;
        let rest = &line[close + 2..];
        let end = rest.find(')')?;
        // Names can contain parentheses; the link ends at the last ") by ".
        let tail = rest.rfind(") by ");
        let mut path = &rest[..tail.unwrap_or(end)];
        if let Some(stripped) = path.strip_prefix("./") {
            path = stripped;
        }
        let name = line.get(3..close)?;
        let source = tail
            .map(|t| rest[t + 5..].trim().to_string())
            .unwrap_or_default();
        if name.is_empty()
            || path.is_empty()
            || path.contains("..")
            || percent_decode_strict(path).is_none()
        {
            return None;
        }
        Some(AutoEqEntry {
            name: name.to_string(),
            source,
            path: path.to_string(),
        })
    }

    /// Case- and diacritic-insensitive search where every query word must appear in the name.
    pub fn search(entries: &[AutoEqEntry], query: &str, limit: usize) -> Vec<AutoEqEntry> {
        let query = crate::assistant::normalize(query);
        let words: Vec<&str> = query.split(' ').filter(|w| !w.is_empty()).collect();
        if words.is_empty() {
            return Vec::new();
        }
        entries
            .iter()
            .filter(|e| {
                let name = crate::assistant::normalize(&e.name);
                words.iter().all(|w| name.contains(w))
            })
            .take(limit)
            .cloned()
            .collect()
    }

    /// The `ParametricEQ.txt` URL of an entry.
    pub fn profile_url(entry: &AutoEqEntry) -> Option<Url> {
        let model = percent_decode_strict(entry.path.rsplit('/').next()?)?;
        let file = encode_path_component(&format!("{model} ParametricEQ.txt"));
        Url::parse(RESULTS)
            .ok()?
            .join(&format!("{}/{}", entry.path, file))
            .ok()
    }

    /// Downloads the index. Returns the raw text (for the app's 30-day cache) and the parsed entries.
    pub async fn fetch_index() -> MusicResult<(String, Vec<AutoEqEntry>)> {
        let response = crate::http::public_client().get(INDEX).send().await?;
        if response.status().as_u16() != 200 {
            return Err(MusicError::InvalidResponse);
        }
        let text = response.text().await?;
        let parsed = entries(&text);
        if parsed.is_empty() {
            return Err(MusicError::InvalidResponse);
        }
        Ok((text, parsed))
    }

    /// Downloads and parses one headphone profile.
    pub async fn fetch_profile(entry: &AutoEqEntry) -> MusicResult<AutoEqProfile> {
        let url = profile_url(entry).ok_or(MusicError::NotFound)?;
        let response = crate::http::public_client().get(url).send().await?;
        if response.status().as_u16() != 200 {
            return Err(MusicError::NotFound);
        }
        let bytes = response.bytes().await?;
        let equalizer = ParametricEqualizer::parse(&String::from_utf8_lossy(&bytes))?;
        Ok(AutoEqProfile {
            name: entry.name.clone(),
            source: entry.source.clone(),
            equalizer,
        })
    }

    /// Strict percent decoding: every `%` starts a hex pair and the result is UTF-8.
    pub(crate) fn percent_decode_strict(text: &str) -> Option<String> {
        let bytes = text.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' {
                let hex = text.get(i + 1..i + 3)?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 3;
            } else {
                out.push(bytes[i]);
                i += 1;
            }
        }
        String::from_utf8(out).ok()
    }

    /// Percent-encodes everything outside Foundation's `urlPathAllowed` set, including `/`.
    fn encode_path_component(text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        for byte in text.bytes() {
            if byte.is_ascii_alphanumeric() || b"!$&'()*+,-.:;=@_~".contains(&byte) {
                out.push(byte as char);
            } else {
                out.push_str(&format!("%{byte:02X}"));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue::RepeatMode;

    const PROFILE: &str = "Preamp: -6.3 dB
Filter 1: ON LSC Fc 105 Hz Gain 6.5 dB Q 0.70
Filter 2: ON PK Fc 125 Hz Gain -2.7 dB Q 0.55
Filter 3: OFF PK Fc 8445 Hz Gain 3.3 dB Q 1.61
Filter 4: ON LPQ Fc 18000 Hz Q 0.71
Filter 5: ON HSC Fc 10000 Hz Gain -3.1 dB Q 0.70";

    fn close(a: f64, b: f64, accuracy: f64) {
        assert!((a - b).abs() <= accuracy, "{a} != {b} ± {accuracy}");
    }

    #[test]
    fn parses_auto_eq_parametric_profile_and_skips_disabled_or_unsupported_filters() {
        let equalizer = ParametricEqualizer::parse(PROFILE).unwrap();
        assert_eq!(equalizer.preamp, -6.3);
        assert_eq!(
            equalizer.filters,
            vec![
                EqualizerFilter::new(FilterKind::LowShelf, 105.0, 6.5, 0.7),
                EqualizerFilter::new(FilterKind::Peaking, 125.0, -2.7, 0.55),
                EqualizerFilter::new(FilterKind::HighShelf, 10_000.0, -3.1, 0.7),
            ]
        );
        assert!(ParametricEqualizer::parse("Preamp: -3 dB\n<html>Not found</html>").is_err());
        let profile =
            AutoEqProfile::import("/tmp/HD 600 ParametricEQ.txt", PROFILE, "Imported file")
                .unwrap();
        assert_eq!(profile.name, "HD 600");
        assert!(AutoEqProfile::import("x.txt", &" ".repeat(64_000), "").is_err());
    }

    #[test]
    fn peaking_filter_reaches_its_gain_at_the_center_frequency() {
        let mut settings = AudioEffectsSettings {
            equalizer: EqualizerMode::Manual,
            ..Default::default()
        };
        settings.bands[5] = 6.0; // 1 kHz
        let chain = EffectsProcessor::new(settings.clone(), 48_000.0, 2);
        close(chain.response(1_000.0), 6.0, 0.05);
        close(chain.response(20.0), 0.0, 0.1);
        // Manual boosts get matching headroom.
        close(chain.gain(), 10f64.powf(-6.0 / 20.0), 1e-9);
        let curve = settings.response_curve(48_000.0, &[20.0, 1_000.0]);
        close(curve[1], 6.0, 0.05);
        assert_eq!(log_frequencies(3, 20.0, 20_000.0).len(), 3);
    }

    #[test]
    fn shelves_follow_their_gain_below_and_above_the_corner() {
        let settings = AudioEffectsSettings {
            equalizer: EqualizerMode::AutoEq,
            auto_eq: Some(AutoEqProfile {
                name: "Test".into(),
                source: "Test".into(),
                equalizer: ParametricEqualizer::new(
                    0.0,
                    vec![
                        EqualizerFilter::new(FilterKind::LowShelf, 200.0, 6.0, 0.71),
                        EqualizerFilter::new(FilterKind::HighShelf, 5_000.0, -4.0, 0.71),
                    ],
                ),
            }),
            ..Default::default()
        };
        let mut chain = EffectsProcessor::new(settings, 44_100.0, 2);
        close(chain.response(20.0), 6.0, 0.2);
        close(chain.response(18_000.0), -4.0, 0.3);
        close(chain.response(1_000.0), 0.0, 0.6);
        // Audiobooks keep only the volume limit.
        chain.set_track_context(AudioTrackContext {
            music: false,
            ..Default::default()
        });
        close(chain.response(20.0), 0.0, 1e-9);
    }

    #[test]
    fn replay_gain_chooses_track_or_album_and_respects_peaks() {
        let tags = ReplayGain {
            track_gain: Some(-7.0),
            album_gain: Some(-9.0),
            track_peak: Some(0.9),
            album_peak: Some(1.2),
            ..Default::default()
        };
        let mut settings = AudioEffectsSettings::default();
        assert_eq!(settings.replay_gain_decibels(Some(&tags), true), None);
        settings.replay_gain = ReplayGainMode::Track;
        assert_eq!(settings.replay_gain_decibels(Some(&tags), true), Some(-7.0));
        settings.replay_gain = ReplayGainMode::Automatic;
        assert_eq!(settings.replay_gain_decibels(Some(&tags), true), Some(-9.0));
        assert_eq!(
            settings.replay_gain_decibels(Some(&tags), false),
            Some(-7.0)
        );
        // A preamp boost is limited by the peak when clipping prevention is on.
        settings.replay_gain = ReplayGainMode::Track;
        settings.replay_gain_preamp = 10.0;
        close(
            settings.replay_gain_decibels(Some(&tags), false).unwrap(),
            -20.0 * 0.9f64.log10(),
            1e-9,
        );
        settings.prevent_clipping = false;
        assert_eq!(settings.replay_gain_decibels(Some(&tags), false), Some(3.0));
        let album_only = ReplayGain {
            album_gain: Some(-4.0),
            ..Default::default()
        };
        assert_eq!(
            settings.replay_gain_decibels(Some(&album_only), false),
            Some(6.0)
        );
        assert_eq!(settings.replay_gain_decibels(None, false), None);
        let fallback = ReplayGain {
            fallback_gain: Some(-5.0),
            ..Default::default()
        };
        assert_eq!(
            settings.replay_gain_decibels(Some(&fallback), false),
            Some(-5.0)
        );
    }

    #[test]
    fn chain_applies_gain_and_limits_peaks() {
        let mut settings = AudioEffectsSettings {
            volume_limit: -6.0,
            ..Default::default()
        };
        let mut chain = EffectsProcessor::new(settings.clone(), 44_100.0, 2);
        let mut buffer: Vec<f32> = (0..1024)
            .map(|i| if i % 2 == 0 { 0.5 } else { -0.5 })
            .collect();
        chain.process(&mut buffer);
        // The first buffer ramps from unity to the new gain.
        let expected = 0.5 * 10f64.powf(-6.0 / 20.0);
        close(f64::from(buffer[1022]), expected, 1e-4);
        close(f64::from(buffer[1023]), -expected, 1e-4);

        settings.volume_limit = 0.0;
        settings.replay_gain = ReplayGainMode::Track;
        settings.prevent_clipping = false;
        let mut mono = EffectsProcessor::new(settings, 44_100.0, 1);
        mono.set_track_gain(
            Some(&ReplayGain {
                track_gain: Some(12.0),
                ..Default::default()
            }),
            false,
        );
        for _ in 0..3 {
            let mut loud: Vec<f32> = (0..512)
                .map(|i| ((i as f64 / 5.0).sin() * 0.9) as f32)
                .collect();
            mono.process(&mut loud);
            let max = loud.iter().map(|s| f64::from(s.abs())).fold(0.0, f64::max);
            assert!(max <= EffectsProcessor::CEILING + 1e-6, "{max}");
        }
    }

    #[test]
    fn transparent_chain_leaves_samples_untouched() {
        let mut chain = EffectsProcessor::new(AudioEffectsSettings::default(), 44_100.0, 1);
        let mut samples = [0.1f32, -1.5, 0.3];
        chain.process(&mut samples);
        assert_eq!(samples, [0.1, -1.5, 0.3]);
    }

    #[test]
    fn equalizer_processing_matches_the_response_at_the_center() {
        let mut settings = AudioEffectsSettings {
            equalizer: EqualizerMode::Manual,
            ..Default::default()
        };
        settings.set_band(5, 6.0);
        settings.set_band(0, -40.0);
        assert_eq!(settings.bands[0], -12.0);
        settings.set_band(0, 0.0);
        let rate = 48_000.0;
        let mut chain = EffectsProcessor::new(settings, rate, 2);
        let frames = 9_600;
        let mut buffer = vec![0f32; frames * 2];
        for i in 0..frames {
            let s = ((2.0 * PI * 1_000.0 * i as f64 / rate).sin() * 0.1) as f32;
            buffer[i * 2] = s;
            buffer[i * 2 + 1] = s;
        }
        let input = buffer.clone();
        chain.process(&mut buffer); // Settles the filter and the gain ramp.
        buffer.copy_from_slice(&input);
        chain.process(&mut buffer);
        // +6 dB filter and -6 dB headroom cancel out at 1 kHz.
        let tail = &buffer[frames..];
        let max = tail.iter().map(|s| f64::from(s.abs())).fold(0.0, f64::max);
        close(max, 0.1, 0.005);
    }

    #[test]
    fn settings_decode_missing_or_invalid_values_safely() {
        let decoded: AudioEffectsSettings = serde_json::from_str(
            r#"{"equalizer":"unknown","bands":[1,2],"volumeLimit":-90,"replayGain":"album"}"#,
        )
        .unwrap();
        assert_eq!(decoded.equalizer, EqualizerMode::Off);
        assert_eq!(decoded.bands, vec![0.0; 10]);
        assert_eq!(decoded.volume_limit, -24.0);
        assert_eq!(decoded.replay_gain, ReplayGainMode::Album);
        assert!(decoded.prevent_clipping);
        let mut settings = AudioEffectsSettings {
            equalizer: EqualizerMode::Manual,
            ..Default::default()
        };
        settings.bands[0] = 4.0;
        settings.auto_eq = Some(AutoEqProfile::import("a.txt", PROFILE, "x").unwrap());
        let json = serde_json::to_string(&settings).unwrap();
        assert!(json.contains("\"equalizer\":\"manual\"") && json.contains("\"lowShelf\""));
        assert_eq!(
            serde_json::from_str::<AudioEffectsSettings>(&json).unwrap(),
            settings
        );
    }

    #[test]
    fn song_decodes_open_subsonic_audio_details() {
        let json = r#"{"id":"1","title":"Song","samplingRate":96000,"bitDepth":24,"channelCount":2,"replayGain":{"trackGain":-6.5,"trackPeak":0.98}}"#;
        let song: Song = serde_json::from_str(json).unwrap();
        assert_eq!(song.sampling_rate, Some(96_000));
        assert_eq!(song.bit_depth, Some(24));
        assert_eq!(song.channel_count, Some(2));
        assert_eq!(
            song.replay_gain,
            Some(ReplayGain {
                track_gain: Some(-6.5),
                track_peak: Some(0.98),
                ..Default::default()
            })
        );
    }

    #[test]
    fn auto_eq_index_parsing_search_and_profile_url() {
        let index = "# Index
This is a list of all equalization profiles.

- [Sennheiser HD 600](./oratory1990/over-ear/Sennheiser%20HD%20600) by oratory1990
- [1MORE Aero (ANC Off)](./HypetheSonics/GRAS%20RA0045%20in-ear/1MORE%20Aero%20(ANC%20Off)) by HypetheSonics on GRAS RA0045
- [Broken](../etc/passwd) by nobody";
        let entries = auto_eq::entries(index);
        assert_eq!(
            entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            ["Sennheiser HD 600", "1MORE Aero (ANC Off)"]
        );
        assert_eq!(entries[1].source, "HypetheSonics on GRAS RA0045");
        assert_eq!(
            entries[1].path,
            "HypetheSonics/GRAS%20RA0045%20in-ear/1MORE%20Aero%20(ANC%20Off)"
        );
        assert_eq!(
            auto_eq::search(&entries, "hd 600", 50)
                .iter()
                .map(|e| e.name.as_str())
                .collect::<Vec<_>>(),
            ["Sennheiser HD 600"]
        );
        assert_eq!(auto_eq::search(&entries, "aero anc", 50).len(), 1);
        assert!(auto_eq::search(&entries, " ", 50).is_empty());
        assert_eq!(
            auto_eq::profile_url(&entries[0]).unwrap().as_str(),
            "https://raw.githubusercontent.com/jaakkopasanen/AutoEq/master/results/oratory1990/over-ear/Sennheiser%20HD%20600/Sennheiser%20HD%20600%20ParametricEQ.txt"
        );
        assert!(auto_eq::index_is_fresh(
            0,
            auto_eq::INDEX_MAX_AGE_MILLIS - 1
        ));
        assert!(!auto_eq::index_is_fresh(0, auto_eq::INDEX_MAX_AGE_MILLIS));
    }

    fn song(id: &str, album: Option<&str>, duration: Option<f64>, podcast: bool) -> Song {
        Song {
            album_id: album.map(str::to_string),
            duration,
            is_podcast: podcast.then_some(true),
            ..Song::new(id, id)
        }
    }

    #[test]
    fn chooses_crossfade_gapless_or_sequential() {
        use TrackTransition::*;
        let mut settings = AudioEffectsSettings::default();
        let one = song("1", Some("a"), Some(200.0), false);
        let same_album = song("2", Some("a"), Some(200.0), false);
        let other = song("3", Some("b"), Some(200.0), false);
        let between =
            |to: Option<&Song>, d: f64, s: &AudioEffectsSettings, in_order: bool, repeat: bool| {
                TrackTransition::between(&one, to, d, s, in_order, repeat)
            };
        assert_eq!(
            between(Some(&other), 200.0, &settings, true, false),
            Gapless
        );
        settings.crossfade = 6.0;
        assert_eq!(
            between(Some(&other), 200.0, &settings, true, false),
            Crossfade { seconds: 6.0 }
        );
        // Album tracks played in order keep their join.
        assert_eq!(
            between(Some(&same_album), 200.0, &settings, true, false),
            Gapless
        );
        assert_eq!(
            between(Some(&same_album), 200.0, &settings, false, false),
            Crossfade { seconds: 6.0 }
        );
        settings.crossfade_skips_albums = false;
        assert_eq!(
            between(Some(&same_album), 200.0, &settings, true, false),
            Crossfade { seconds: 6.0 }
        );
        // Short songs, repeat one and podcasts never crossfade.
        assert_eq!(between(Some(&other), 15.0, &settings, true, false), Gapless);
        assert_eq!(
            between(
                Some(&song("4", Some("b"), Some(10.0), false)),
                200.0,
                &settings,
                true,
                false
            ),
            Gapless
        );
        assert_eq!(between(Some(&one), 200.0, &settings, true, true), Gapless);
        assert_eq!(
            between(
                Some(&song("p", Some("a"), Some(200.0), true)),
                200.0,
                &settings,
                true,
                false
            ),
            Sequential
        );
        assert_eq!(between(None, 200.0, &settings, true, false), Sequential);
        settings.crossfade = 0.0;
        settings.gapless = false;
        assert_eq!(
            between(Some(&other), 200.0, &settings, true, false),
            Sequential
        );
    }

    #[test]
    fn equal_power_fade_keeps_constant_energy() {
        for step in 0..=10 {
            let (out, inc) = TrackTransition::volumes(step as f64 / 10.0);
            close(f64::from(out * out + inc * inc), 1.0, 1e-5);
        }
        assert_eq!(TrackTransition::volumes(-1.0).0, 1.0);
        assert_eq!(TrackTransition::volumes(2.0).1, 1.0);
    }

    #[test]
    fn automatic_next_follows_repeat_modes_without_changing_the_queue() {
        let mut queue = PlaybackQueue::new();
        queue.replace(
            vec![
                song("1", Some("a"), Some(200.0), false),
                song("2", Some("a"), Some(200.0), false),
            ],
            1,
            false,
        );
        assert_eq!(automatic_next(&queue), None);
        queue.repeat_mode = RepeatMode::All;
        assert_eq!(automatic_next(&queue).map(|s| s.id), Some("1".to_string()));
        queue.repeat_mode = RepeatMode::One;
        assert_eq!(automatic_next(&queue).map(|s| s.id), Some("2".to_string()));
        assert_eq!(queue.index(), 1);
    }

    #[test]
    fn settings_clamp_crossfade_and_default_to_gapless() {
        let decoded: AudioEffectsSettings = serde_json::from_str(r#"{"crossfade":40}"#).unwrap();
        assert_eq!(decoded.crossfade, 12.0);
        assert!(decoded.gapless);
        assert!(decoded.crossfade_skips_albums);
    }
}
