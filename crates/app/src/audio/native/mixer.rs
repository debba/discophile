//! Renders the current item, the prepared next item and a running crossfade into one stream of
//! device-rate samples, and marks where each item starts and ends so the engine can report it when
//! those samples are actually heard.

use std::sync::Arc;

use discophile_core::audio_effects::{AudioEffectsSettings, AudioTrackContext, EffectsProcessor, TrackTransition};

use crate::audio::transition::{Plan, plan};
use crate::audio::{AudioDetails, ContentKind, MediaItem, TransitionSettings};

/// Gapless items are prepared this long before the end of the current one.
pub const GAPLESS_LEAD: f64 = 30.0;

#[derive(Clone, Debug, PartialEq)]
pub enum SourceState {
    /// More data is coming but none is decoded yet.
    Starved,
    Finished,
    Failed(String),
}

/// The result of one read. `state` matters only when `frames` is zero.
#[derive(Clone, Debug, PartialEq)]
pub struct SourceRead {
    pub frames: usize,
    /// Item position of the first frame, in seconds.
    pub pos: f64,
    pub speed: f32,
    pub state: SourceState,
}

/// Decoded audio already at the device rate and channel count.
pub trait PcmSource: Send {
    fn read(&mut self, out: &mut [f32]) -> SourceRead;
    fn seek(&mut self, seconds: f64);
    fn set_speed(&mut self, speed: f32);
    fn duration(&self) -> Option<f64>;
    fn buffered_frames(&self) -> usize;
    fn details(&self) -> Option<AudioDetails>;
}

/// Creates the source for an item, starting at a position, at a speed.
pub type SourceFactory = Box<dyn FnMut(&MediaItem, f64, f32) -> Box<dyn PcmSource> + Send>;

#[derive(Clone, Debug, PartialEq)]
pub enum MarkerKind {
    Position,
    /// The item starts; `from` is the item it took over from automatically.
    Started {
        from: Option<String>,
    },
    Ended,
    Failed(String),
}

#[derive(Clone, Debug)]
pub struct Marker {
    /// Absolute sample index in the output ring.
    pub index: u64,
    pub item: Arc<MediaItem>,
    pub pos: f64,
    pub speed: f32,
    pub kind: MarkerKind,
}

struct Track {
    item: Arc<MediaItem>,
    source: Box<dyn PcmSource>,
    fx: Option<EffectsProcessor>,
    pos: f64,
}

impl Track {
    fn duration(&self) -> Option<f64> {
        self.source.duration().or(self.item.duration_hint).filter(|d| d.is_finite() && *d > 0.0)
    }
}

struct Fade {
    track: Track,
    total: usize,
    done: usize,
    /// The outgoing item ran out before the fade ended.
    drained: bool,
}

pub struct Mixer {
    rate: u32,
    channels: usize,
    factory: SourceFactory,
    current: Option<Track>,
    next: Option<Track>,
    next_item: Option<Arc<MediaItem>>,
    outgoing: Option<Fade>,
    /// Album of the item before the current one, for automatic ReplayGain.
    previous_album: Option<String>,
    /// Emitted with the current track's next frames.
    pending: Option<MarkerKind>,
    transitions: TransitionSettings,
    effects: Option<AudioEffectsSettings>,
    speed: f32,
    markers: Vec<Marker>,
    scratch: Vec<f32>,
    starved: bool,
}

impl Mixer {
    pub fn new(rate: u32, channels: usize, factory: SourceFactory) -> Self {
        Self {
            rate: rate.max(1),
            channels: channels.max(1),
            factory,
            current: None,
            next: None,
            next_item: None,
            outgoing: None,
            previous_album: None,
            pending: None,
            transitions: TransitionSettings::default(),
            effects: None,
            speed: 1.0,
            markers: Vec::new(),
            scratch: Vec::new(),
            starved: false,
        }
    }

    fn make_track(&mut self, item: Arc<MediaItem>, start_at: f64) -> Track {
        let speed = if item.kind == ContentKind::Radio { 1.0 } else { self.speed };
        let source = (self.factory)(&item, start_at, speed);
        let fx = self.effects.as_ref().map(|settings| EffectsProcessor::new(settings.clone(), f64::from(self.rate), self.channels));
        Track { item, source, fx, pos: start_at }
    }

    /// Replaces everything with `item`. Returns the shared item used in markers.
    pub fn load(&mut self, item: impl Into<Arc<MediaItem>>, start_at: f64) -> Arc<MediaItem> {
        let item = item.into();
        self.outgoing = None;
        self.next = None;
        self.previous_album = None;
        let track = self.make_track(item.clone(), start_at.max(0.0));
        self.current = Some(track);
        self.pending = Some(MarkerKind::Started { from: None });
        self.starved = true;
        self.refresh_track_contexts();
        item
    }

    pub fn stop(&mut self) {
        self.current = None;
        self.next = None;
        self.next_item = None;
        self.outgoing = None;
        self.pending = None;
        self.markers.clear();
        self.starved = false;
    }

    pub fn set_next(&mut self, item: Option<MediaItem>) {
        if self.next.as_ref().is_some_and(|prepared| item.as_ref() != Some(prepared.item.as_ref())) {
            self.next = None;
        }
        if self.next_item.as_deref() != item.as_ref() {
            self.next_item = item.map(Arc::new);
        }
        self.refresh_track_contexts();
    }

    pub fn next_item(&self) -> Option<MediaItem> {
        self.next_item.as_deref().cloned()
    }

    pub fn current_item(&self) -> Option<Arc<MediaItem>> {
        self.current.as_ref().map(|t| t.item.clone())
    }

    /// Seeks the current item and ends a running crossfade.
    pub fn seek(&mut self, seconds: f64) {
        self.outgoing = None;
        if let Some(track) = &mut self.current {
            track.source.seek(seconds.max(0.0));
            track.pos = seconds.max(0.0);
            if let Some(fx) = &mut track.fx {
                fx.reset();
            }
            self.pending.get_or_insert(MarkerKind::Position);
            self.starved = true;
        }
    }

    /// Ends a crossfade at once: the incoming item continues alone.
    pub fn end_fade(&mut self) {
        self.outgoing = None;
    }

    pub fn set_speed(&mut self, speed: f32) {
        self.speed = speed;
        for track in self.tracks_mut() {
            if track.item.kind != ContentKind::Radio {
                track.source.set_speed(speed);
            }
        }
    }

    pub fn set_transitions(&mut self, transitions: TransitionSettings) {
        self.transitions = transitions;
    }

    pub fn set_effects(&mut self, settings: AudioEffectsSettings) {
        let (rate, channels) = (f64::from(self.rate), self.channels);
        self.effects = Some(settings.clone());
        for track in self.tracks_mut() {
            match &mut track.fx {
                Some(fx) => fx.update(settings.clone()),
                None => track.fx = Some(EffectsProcessor::new(settings.clone(), rate, channels)),
            }
        }
        self.refresh_track_contexts();
    }

    fn tracks_mut(&mut self) -> impl Iterator<Item = &mut Track> {
        self.current.iter_mut().chain(self.next.iter_mut()).chain(self.outgoing.iter_mut().map(|f| &mut f.track))
    }

    /// Music and podcasts get every effect, radio has no ReplayGain tags, audiobooks only the volume limit.
    fn refresh_track_contexts(&mut self) {
        let previous = self.previous_album.clone();
        let next_album = self.next_item.as_ref().and_then(|i| i.album_key.clone());
        let context = |item: &MediaItem, previous: Option<&String>, following: Option<&String>| {
            let album = item.album_key.as_ref();
            AudioTrackContext {
                replay_gain: item.replay_gain.clone(),
                album_context: album.is_some() && (previous == album || following == album),
                music: item.kind != ContentKind::Audiobook,
            }
        };
        if let Some(track) = &mut self.current {
            let ctx = context(&track.item, previous.as_ref(), next_album.as_ref());
            if let Some(fx) = &mut track.fx
                && fx.track() != &ctx
            {
                fx.set_track_context(ctx);
            }
        }
        let current_album = self.current.as_ref().and_then(|t| t.item.album_key.clone());
        if let Some(track) = &mut self.next {
            let ctx = context(&track.item, current_album.as_ref(), None);
            if let Some(fx) = &mut track.fx
                && fx.track() != &ctx
            {
                fx.set_track_context(ctx);
            }
        }
    }

    /// Duration and details of the item, if one of the tracks plays it.
    pub fn info_for(&self, item: &Arc<MediaItem>) -> Option<(Option<f64>, Option<AudioDetails>)> {
        let tracks = self.current.iter().chain(self.outgoing.iter().map(|f| &f.track)).chain(self.next.iter());
        tracks.into_iter().find(|t| Arc::ptr_eq(&t.item, item)).map(|t| (t.duration(), t.source.details()))
    }

    /// The current item is waiting for data.
    pub fn is_starved(&self) -> bool {
        self.starved && self.current.is_some()
    }

    pub fn has_work(&self) -> bool {
        self.current.is_some()
    }

    pub fn take_markers(&mut self) -> Vec<Marker> {
        std::mem::take(&mut self.markers)
    }

    fn prepare_next_if_due(&mut self) {
        if self.next.is_some() {
            return;
        }
        let (Some(current), Some(item)) = (&self.current, &self.next_item) else { return };
        let lead = match plan(&current.item, current.duration(), item, &self.transitions) {
            Plan::Sequential => return,
            Plan::Gapless => GAPLESS_LEAD,
            Plan::Crossfade(fade) => (fade + 15.0).max(GAPLESS_LEAD),
        };
        let Some(duration) = current.duration() else { return };
        if duration - current.pos <= lead {
            let item = item.clone();
            let track = self.make_track(item, 0.0);
            self.next = Some(track);
            self.refresh_track_contexts();
        }
    }

    /// Frames until the crossfade should begin, when one is planned and the next item is ready.
    fn frames_until_crossfade(&self) -> Option<usize> {
        let current = self.current.as_ref()?;
        let next = self.next.as_ref()?;
        let Plan::Crossfade(fade) = plan(&current.item, current.duration(), &next.item, &self.transitions) else { return None };
        let start = current.duration()? - fade;
        // Too late for a full fade (for example after a seek close to the end): join gaplessly.
        if current.pos > start + fade / 2.0 {
            return None;
        }
        // The epsilon keeps float noise from adding a frame.
        let frames = ((start - current.pos) / f64::from(self.speed_of(current)) * f64::from(self.rate) - 1e-6).ceil();
        Some(frames.max(0.0) as usize)
    }

    fn speed_of(&self, track: &Track) -> f32 {
        if track.item.kind == ContentKind::Radio { 1.0 } else { self.speed.max(0.01) }
    }

    fn begin_crossfade(&mut self) -> bool {
        let Some(next) = self.next.as_ref() else { return false };
        let Some(current) = self.current.as_ref() else { return false };
        let Plan::Crossfade(fade) = plan(&current.item, current.duration(), &next.item, &self.transitions) else { return false };
        if next.source.buffered_frames() == 0 {
            return false;
        }
        let incoming = self.next.take().expect("checked");
        let outgoing = self.current.take().expect("checked");
        self.next_item = None;
        self.previous_album = outgoing.item.album_key.clone();
        self.pending = Some(MarkerKind::Started { from: Some(outgoing.item.id.clone()) });
        let total = (fade * f64::from(self.rate)).round().max(1.0) as usize;
        self.outgoing = Some(Fade { track: outgoing, total, done: 0, drained: false });
        self.current = Some(incoming);
        self.refresh_track_contexts();
        true
    }

    /// The current item ended: continue with the next one, or mark the end.
    fn advance(&mut self, index: u64) {
        let Some(ended) = self.current.take() else { return };
        self.outgoing = None;
        match self.next_item.take() {
            Some(item) => {
                let track = match self.next.take() {
                    Some(track) if Arc::ptr_eq(&track.item, &item) => track,
                    _ => self.make_track(item, 0.0),
                };
                self.previous_album = ended.item.album_key.clone();
                self.pending = Some(MarkerKind::Started { from: Some(ended.item.id.clone()) });
                self.current = Some(track);
                self.refresh_track_contexts();
            }
            None => {
                self.next = None;
                let speed = self.speed_of(&ended);
                self.markers.push(Marker { index, item: ended.item, pos: ended.pos, speed, kind: MarkerKind::Ended });
            }
        }
    }

    fn fail(&mut self, index: u64, message: String) {
        let Some(failed) = self.current.take() else { return };
        let speed = self.speed_of(&failed);
        if let Some(kind @ MarkerKind::Started { from: Some(_) }) = self.pending.take() {
            self.markers.push(Marker { index, item: failed.item.clone(), pos: failed.pos, speed, kind });
        }
        self.markers.push(Marker { index, item: failed.item, pos: failed.pos, speed, kind: MarkerKind::Failed(message) });
        self.outgoing = None;
        self.next = None;
        self.next_item = None;
    }

    /// Fills `out` (interleaved, device channels) from the start and returns the frames written.
    /// `base` is the ring index of `out[0]`. Stops early when the current item is waiting for data.
    pub fn render(&mut self, out: &mut [f32], base: u64) -> usize {
        let channels = self.channels;
        let total = out.len() / channels;
        let mut done = 0;
        self.prepare_next_if_due();
        while done < total {
            if self.current.is_none() {
                break;
            }
            let mut want = total - done;
            if self.outgoing.is_none()
                && let Some(left) = self.frames_until_crossfade()
            {
                if left == 0 {
                    if self.begin_crossfade() {
                        continue;
                    }
                } else {
                    want = want.min(left);
                }
            }
            let index = base + (done * channels) as u64;
            let rate = f64::from(self.rate);
            let track = self.current.as_mut().expect("checked");
            let slice = &mut out[done * channels..(done + want) * channels];
            let read = track.source.read(slice);
            if read.frames > 0 {
                let kind = self.pending.take().unwrap_or(MarkerKind::Position);
                self.markers.push(Marker { index, item: track.item.clone(), pos: read.pos, speed: read.speed, kind });
                track.pos = read.pos + read.frames as f64 / rate * f64::from(read.speed);
                let written = &mut slice[..read.frames * channels];
                if let Some(fx) = &mut track.fx {
                    fx.process(written);
                }
                self.mix_outgoing(index, done * channels, read.frames, out);
                done += read.frames;
                self.starved = false;
                self.prepare_next_if_due();
                continue;
            }
            match read.state {
                SourceState::Starved => {
                    self.starved = true;
                    break;
                }
                SourceState::Finished => self.advance(index),
                SourceState::Failed(message) => self.fail(index, message),
            }
        }
        done
    }

    /// Mixes the outgoing item of a crossfade into `frames` frames of `out` starting at `offset`.
    fn mix_outgoing(&mut self, _index: u64, offset: usize, frames: usize, out: &mut [f32]) {
        let channels = self.channels;
        let Some(fade) = &mut self.outgoing else { return };
        let len = frames * channels;
        self.scratch.clear();
        self.scratch.resize(len, 0.0);
        if !fade.drained {
            let read = fade.track.source.read(&mut self.scratch);
            if read.frames > 0 {
                if let Some(fx) = &mut fade.track.fx {
                    fx.process(&mut self.scratch[..read.frames * channels]);
                }
            } else if read.state != SourceState::Starved {
                fade.drained = true;
            }
        }
        let target = &mut out[offset..offset + len];
        for (frame, (incoming, outgoing)) in target.chunks_exact_mut(channels).zip(self.scratch.chunks_exact(channels)).enumerate() {
            let progress = (fade.done + frame) as f64 / fade.total as f64;
            let (gain_out, gain_in) = TrackTransition::volumes(progress);
            for (sample, old) in incoming.iter_mut().zip(outgoing) {
                *sample = *sample * gain_in + old * gain_out;
            }
        }
        fade.done += frames;
        if fade.done >= fade.total {
            self.outgoing = None;
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::audio::AudioSource;
    use std::sync::Mutex;

    /// A source producing `frames` frames of a constant value, `chunk` frames per read at most.
    pub struct ToneSource {
        pub value: f32,
        pub frames: usize,
        pub at: usize,
        pub rate: u32,
        pub channels: usize,
        pub delay_reads: usize,
        pub fail: bool,
    }

    impl PcmSource for ToneSource {
        fn read(&mut self, out: &mut [f32]) -> SourceRead {
            let pos = self.at as f64 / f64::from(self.rate);
            if self.fail {
                return SourceRead { frames: 0, pos, speed: 1.0, state: SourceState::Failed("broken".into()) };
            }
            if self.delay_reads > 0 {
                self.delay_reads -= 1;
                return SourceRead { frames: 0, pos, speed: 1.0, state: SourceState::Starved };
            }
            let frames = (out.len() / self.channels).min(self.frames - self.at);
            out[..frames * self.channels].iter_mut().for_each(|s| *s = self.value);
            self.at += frames;
            let state = if frames == 0 { SourceState::Finished } else { SourceState::Starved };
            SourceRead { frames, pos, speed: 1.0, state }
        }
        fn seek(&mut self, seconds: f64) {
            self.at = ((seconds * f64::from(self.rate)) as usize).min(self.frames);
        }
        fn set_speed(&mut self, _speed: f32) {}
        fn duration(&self) -> Option<f64> {
            Some(self.frames as f64 / f64::from(self.rate))
        }
        fn buffered_frames(&self) -> usize {
            if self.delay_reads > 0 { 0 } else { self.frames - self.at }
        }
        fn details(&self) -> Option<AudioDetails> {
            None
        }
    }

    pub const RATE: u32 = 100;

    pub fn item(id: &str, seconds: f64) -> MediaItem {
        MediaItem {
            id: id.into(),
            source: AudioSource::Url(format!("memory://{id}")),
            kind: ContentKind::Music,
            duration_hint: Some(seconds),
            replay_gain: None,
            album_key: None,
            server_format: None,
        }
    }

    /// Items named `<value>` play that constant value; `fail` makes them fail.
    pub fn mixer(created: Arc<Mutex<Vec<String>>>) -> Mixer {
        Mixer::new(
            RATE,
            1,
            Box::new(move |item: &MediaItem, start: f64, _speed: f32| {
                created.lock().unwrap().push(item.id.clone());
                let value = item.id.trim_end_matches(|c: char| c.is_alphabetic()).parse().unwrap_or(0.0);
                let frames = (item.duration_hint.unwrap_or(1.0) * f64::from(RATE)) as usize;
                Box::new(ToneSource {
                    value,
                    frames,
                    at: (start * f64::from(RATE)) as usize,
                    rate: RATE,
                    channels: 1,
                    delay_reads: 0,
                    fail: item.id.ends_with("fail"),
                }) as Box<dyn PcmSource>
            }),
        )
    }

    fn render_all(mixer: &mut Mixer, frames: usize) -> (Vec<f32>, Vec<Marker>) {
        let mut out = vec![0.0; frames];
        let mut written = 0;
        let mut markers = Vec::new();
        while written < frames {
            let n = mixer.render(&mut out[written..(written + 64).min(frames)], written as u64);
            markers.extend(mixer.take_markers());
            if n == 0 {
                break;
            }
            written += n;
        }
        out.truncate(written);
        markers.extend(mixer.take_markers());
        (out, markers)
    }

    fn settings(gapless: bool, crossfade: Option<f32>, keep_album_gapless: bool) -> TransitionSettings {
        TransitionSettings { gapless, crossfade, keep_album_gapless }
    }

    #[test]
    fn gapless_splice_is_sample_exact() {
        let created = Arc::new(Mutex::new(Vec::new()));
        let mut mixer = mixer(created.clone());
        mixer.load(item("1", 2.0), 0.0);
        mixer.set_next(Some(item("2", 1.0)));
        let (out, markers) = render_all(&mut mixer, 1000);
        assert_eq!(out.len(), 300);
        assert!(out[..200].iter().all(|s| *s == 1.0));
        assert!(out[200..].iter().all(|s| *s == 2.0));
        // Prepared ahead of time: both items exist before the first one ends.
        assert_eq!(created.lock().unwrap().as_slice(), ["1", "2"]);
        let started: Vec<_> = markers.iter().filter(|m| matches!(m.kind, MarkerKind::Started { .. })).collect();
        assert_eq!(started.len(), 2);
        assert_eq!(started[1].index, 200);
        assert_eq!(started[1].kind, MarkerKind::Started { from: Some("1".into()) });
        let ended = markers.iter().find(|m| m.kind == MarkerKind::Ended).unwrap();
        assert_eq!((ended.index, ended.item.id.as_str()), (300, "2"));
    }

    #[test]
    fn repeat_one_prepares_a_second_instance() {
        let created = Arc::new(Mutex::new(Vec::new()));
        let mut mixer = mixer(created.clone());
        mixer.load(item("3", 1.0), 0.0);
        mixer.set_next(Some(item("3", 1.0)));
        let (out, markers) = render_all(&mut mixer, 1000);
        assert_eq!(out.len(), 200);
        assert_eq!(created.lock().unwrap().len(), 2);
        assert!(markers.iter().any(|m| m.index == 100 && m.kind == MarkerKind::Started { from: Some("3".into()) }));
    }

    #[test]
    fn crossfade_overlaps_with_equal_power() {
        let created = Arc::new(Mutex::new(Vec::new()));
        let mut mixer = mixer(created);
        mixer.set_transitions(settings(true, Some(2.0), true));
        mixer.load(item("1", 10.0), 0.0);
        mixer.set_next(Some(item("3", 10.0)));
        let (out, markers) = render_all(&mut mixer, 5000);
        // 10 s + 10 s - 2 s of overlap.
        assert_eq!(out.len(), 1800);
        assert!(out[..800].iter().all(|s| *s == 1.0));
        let start = markers.iter().find(|m| m.kind == MarkerKind::Started { from: Some("1".into()) }).unwrap();
        assert_eq!(start.index, 800);
        // Halfway: both at cos(π/4).
        let mid = out[900];
        let expected = 1.0 * std::f32::consts::FRAC_1_SQRT_2 + 3.0 * std::f32::consts::FRAC_1_SQRT_2;
        assert!((mid - expected).abs() < 0.05, "{mid}");
        assert!(out[1000..].iter().all(|s| *s == 3.0));
    }

    #[test]
    fn album_tracks_stay_gapless_with_crossfade_on() {
        let created = Arc::new(Mutex::new(Vec::new()));
        let mut mixer = mixer(created);
        mixer.set_transitions(settings(true, Some(2.0), true));
        let (mut a, mut b) = (item("1", 10.0), item("2", 10.0));
        a.album_key = Some("x".into());
        b.album_key = Some("x".into());
        mixer.load(a, 0.0);
        mixer.set_next(Some(b));
        let (out, _) = render_all(&mut mixer, 5000);
        assert_eq!(out.len(), 2000);
    }

    #[test]
    fn ended_without_next() {
        let mut mixer = mixer(Arc::default());
        mixer.load(item("1", 1.0), 0.5);
        let (out, markers) = render_all(&mut mixer, 1000);
        assert_eq!(out.len(), 50);
        assert_eq!(markers.first().unwrap().pos, 0.5);
        assert_eq!(markers.last().unwrap().kind, MarkerKind::Ended);
        assert!(!mixer.has_work());
    }

    #[test]
    fn failing_next_reports_advance_then_failure() {
        let mut mixer = mixer(Arc::default());
        mixer.load(item("1", 1.0), 0.0);
        mixer.set_next(Some(item("2fail", 1.0)));
        let (out, markers) = render_all(&mut mixer, 1000);
        assert_eq!(out.len(), 100);
        let kinds: Vec<_> = markers.iter().filter(|m| m.kind != MarkerKind::Position).map(|m| m.kind.clone()).collect();
        assert_eq!(
            kinds,
            [MarkerKind::Started { from: None }, MarkerKind::Started { from: Some("1".into()) }, MarkerKind::Failed("broken".into())]
        );
    }

    #[test]
    fn changing_next_discards_prepared_item() {
        let created = Arc::new(Mutex::new(Vec::new()));
        let mut mixer = mixer(created.clone());
        mixer.load(item("1", 2.0), 0.0);
        mixer.set_next(Some(item("2", 1.0)));
        let mut out = vec![0.0; 10];
        mixer.render(&mut out, 0);
        assert_eq!(created.lock().unwrap().len(), 2);
        mixer.set_next(Some(item("4", 1.0)));
        let (rest, _) = render_all(&mut mixer, 1000);
        assert!(rest[..190].iter().all(|s| *s == 1.0));
        assert!(rest[190..].iter().all(|s| *s == 4.0));
    }

    #[test]
    fn starved_source_stops_rendering() {
        let mut mixer = Mixer::new(
            RATE,
            1,
            Box::new(|_: &MediaItem, _, _| {
                Box::new(ToneSource { value: 1.0, frames: 50, at: 0, rate: RATE, channels: 1, delay_reads: 2, fail: false })
                    as Box<dyn PcmSource>
            }),
        );
        mixer.load(item("1", 0.5), 0.0);
        let mut out = vec![0.0; 10];
        assert_eq!(mixer.render(&mut out, 0), 0);
        assert!(mixer.is_starved());
        assert_eq!(mixer.render(&mut out, 0), 0);
        assert_eq!(mixer.render(&mut out, 0), 10);
        assert!(!mixer.is_starved());
    }
}
