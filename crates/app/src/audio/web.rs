//! Browser audio output: one `HTMLAudioElement` per item (the next one is preloaded for a near-gapless
//! start or a crossfade), with an optional Web Audio graph for effects:
//!
//! element → MediaElementSource → per-item gain (ReplayGain, preamp, volume, fade)
//!         → shared input → equalizer biquads → limiter (DynamicsCompressor) → speakers
//!
//! The graph is only used when an effect needs it (an active equalizer or a boost). Routing an
//! element through Web Audio requires CORS headers on the media server; when a CORS load fails the
//! item is reloaded without the graph and later items skip it. Attenuation (volume limit, negative
//! ReplayGain) works without the graph through the element volume. Browsers only start audio after
//! a user gesture: a blocked `play()` leaves the output paused instead of reporting a failure.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use discophile_core::audio_effects::{AudioEffectsSettings, FilterKind, ParametricEqualizer};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{
    AudioContext, BiquadFilterNode, BiquadFilterType, DynamicsCompressorNode, GainNode, HtmlAudioElement, MediaElementAudioSourceNode,
};

use super::transition::{Plan, plan};
use super::{AudioDetails, AudioEvent, AudioOutput, AudioSource, ContentKind, MediaItem, OutputStatus, TransitionSettings};

const GAPLESS_LEAD: f64 = 30.0;
const TICK_MS: i32 = 100;
/// `HAVE_FUTURE_DATA`.
const READY: u16 = 3;
const EVENTS: [&str; 9] = ["ended", "error", "waiting", "playing", "canplay", "loadedmetadata", "pause", "stalled", "seeked"];

struct DeckGraph {
    _source: MediaElementAudioSourceNode,
    gain: GainNode,
}

struct Deck {
    item: MediaItem,
    element: HtmlAudioElement,
    graph: Option<DeckGraph>,
    listeners: Vec<(&'static str, Closure<dyn FnMut(web_sys::Event)>)>,
    /// Gain from ReplayGain, preamp and volume limit (linear).
    item_gain: f64,
    /// Crossfade gain (linear).
    fade_gain: f64,
    cors: bool,
}

impl Drop for Deck {
    fn drop(&mut self) {
        let _ = self.element.pause();
        for (name, listener) in &self.listeners {
            let _ = self.element.remove_event_listener_with_callback(name, listener.as_ref().unchecked_ref());
        }
        self.element.remove_attribute("src").ok();
        self.element.load();
        if let Some(graph) = &self.graph {
            let _ = graph.gain.disconnect();
        }
    }
}

struct Graph {
    context: AudioContext,
    input: GainNode,
    filters: Vec<BiquadFilterNode>,
    limiter: DynamicsCompressorNode,
}

impl Graph {
    fn new() -> Result<Self, JsValue> {
        let context = AudioContext::new()?;
        let input = context.create_gain()?;
        let limiter = context.create_dynamics_compressor()?;
        limiter.threshold().set_value(-0.3);
        limiter.knee().set_value(0.0);
        limiter.ratio().set_value(20.0);
        limiter.attack().set_value(0.001);
        limiter.release().set_value(0.15);
        limiter.connect_with_audio_node(&context.destination())?;
        input.connect_with_audio_node(&limiter)?;
        Ok(Self { context, input, filters: Vec::new(), limiter })
    }

    /// Rebuilds the equalizer between the shared input and the limiter.
    fn set_equalizer(&mut self, equalizer: Option<&ParametricEqualizer>) {
        let _ = self.input.disconnect();
        for filter in self.filters.drain(..) {
            let _ = filter.disconnect();
        }
        let nyquist = f64::from(self.context.sample_rate()) * 0.49;
        for filter in equalizer.map(|e| e.filters.as_slice()).unwrap_or_default() {
            if filter.gain == 0.0 || filter.frequency >= nyquist {
                continue;
            }
            let Ok(node) = self.context.create_biquad_filter() else { continue };
            node.set_type(match filter.kind {
                FilterKind::Peaking => BiquadFilterType::Peaking,
                FilterKind::LowShelf => BiquadFilterType::Lowshelf,
                FilterKind::HighShelf => BiquadFilterType::Highshelf,
            });
            node.frequency().set_value(filter.frequency as f32);
            node.q().set_value(filter.q as f32);
            node.gain().set_value(filter.gain as f32);
            self.filters.push(node);
        }
        let mut previous: &web_sys::AudioNode = &self.input;
        for filter in &self.filters {
            let _ = previous.connect_with_audio_node(filter);
            previous = filter;
        }
        let _ = previous.connect_with_audio_node(&self.limiter);
    }
}

struct Fade {
    deck: Deck,
    start_ms: f64,
    seconds: f64,
}

struct State {
    ctx: egui::Context,
    me: Weak<RefCell<State>>,
    current: Option<Deck>,
    next: Option<Deck>,
    next_item: Option<MediaItem>,
    outgoing: Option<Fade>,
    previous_album: Option<String>,
    events: Vec<AudioEvent>,
    /// The person wants playback (as opposed to paused).
    wants_play: bool,
    ended: bool,
    failed: bool,
    volume: f32,
    rate: f32,
    transitions: TransitionSettings,
    effects: Option<AudioEffectsSettings>,
    graph: Option<Graph>,
    /// The browser or the media server refused Web Audio routing.
    graph_unavailable: bool,
}

pub struct WebOutput {
    state: Rc<RefCell<State>>,
    timer: Option<(i32, Closure<dyn FnMut()>)>,
}

impl WebOutput {
    pub fn new(ctx: &egui::Context) -> Result<Self, String> {
        let window = web_sys::window().ok_or("no window")?;
        let state = Rc::new_cyclic(|me| {
            RefCell::new(State {
                ctx: ctx.clone(),
                me: me.clone(),
                current: None,
                next: None,
                next_item: None,
                outgoing: None,
                previous_album: None,
                events: Vec::new(),
                wants_play: false,
                ended: false,
                failed: false,
                volume: 1.0,
                rate: 1.0,
                transitions: TransitionSettings::default(),
                effects: None,
                graph: None,
                graph_unavailable: false,
            })
        });
        let weak = Rc::downgrade(&state);
        let tick = Closure::<dyn FnMut()>::new(move || {
            if let Some(state) = weak.upgrade()
                && let Ok(mut state) = state.try_borrow_mut()
            {
                state.tick();
            }
        });
        let id = window
            .set_interval_with_callback_and_timeout_and_arguments_0(tick.as_ref().unchecked_ref(), TICK_MS)
            .map_err(|e| format!("{e:?}"))?;
        Ok(Self { state, timer: Some((id, tick)) })
    }

    fn with<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        f(&mut self.state.borrow_mut())
    }
}

impl Drop for WebOutput {
    fn drop(&mut self) {
        if let (Some(window), Some((id, _))) = (web_sys::window(), &self.timer) {
            window.clear_interval_with_handle(*id);
        }
        let mut state = self.state.borrow_mut();
        state.current = None;
        state.next = None;
        state.outgoing = None;
    }
}

fn now_ms() -> f64 {
    js_sys::Date::now()
}

fn db_to_linear(db: f64) -> f64 {
    10f64.powf(db.min(24.0) / 20.0)
}

impl State {
    fn repaint(&self) {
        self.ctx.request_repaint();
    }

    fn equalizer_for(&self, item: &MediaItem) -> Option<ParametricEqualizer> {
        if item.kind == ContentKind::Audiobook {
            return None;
        }
        self.effects.as_ref()?.active_equalizer()
    }

    fn album_context(&self, item: &MediaItem, following: Option<&MediaItem>) -> bool {
        let album = item.album_key.as_ref();
        album.is_some() && (self.previous_album.as_ref() == album || following.and_then(|f| f.album_key.as_ref()) == album)
    }

    /// Linear gain for an item: volume limit, equalizer preamp and ReplayGain.
    fn item_gain(&self, item: &MediaItem, album_context: bool) -> f64 {
        let Some(settings) = &self.effects else { return 1.0 };
        let mut db = settings.volume_limit;
        if let Some(eq) = self.equalizer_for(item) {
            db += eq.preamp;
        }
        if item.kind != ContentKind::Audiobook
            && let Some(replay) = settings.replay_gain_decibels(item.replay_gain.as_ref(), album_context)
        {
            db += replay;
        }
        db_to_linear(db)
    }

    /// Effects that attenuation through the element volume cannot do.
    fn needs_graph(&self, item: &MediaItem) -> bool {
        !self.graph_unavailable && (self.equalizer_for(item).is_some() || self.item_gain(item, true) > 1.0 + 1e-6)
    }

    fn ensure_graph(&mut self) -> bool {
        if self.graph.is_none() && !self.graph_unavailable {
            match Graph::new() {
                Ok(graph) => self.graph = Some(graph),
                Err(error) => {
                    log::warn!("Web Audio unavailable, playing without effects: {error:?}");
                    self.graph_unavailable = true;
                }
            }
        }
        self.graph.is_some()
    }

    fn make_deck(&mut self, item: MediaItem, allow_graph: bool) -> Result<Deck, String> {
        let AudioSource::Url(url) = &item.source;
        let element = HtmlAudioElement::new().map_err(|e| format!("{e:?}"))?;
        element.set_preload("auto");
        let use_graph = allow_graph && self.needs_graph(&item) && self.ensure_graph();
        if use_graph {
            element.set_cross_origin(Some("anonymous"));
        }
        element.set_src(url);
        let rate = if item.kind == ContentKind::Radio { 1.0 } else { f64::from(self.rate) };
        element.set_default_playback_rate(rate);
        element.set_playback_rate(rate);
        let mut listeners = Vec::new();
        for name in EVENTS {
            let weak = self.me.clone();
            let listener = Closure::<dyn FnMut(web_sys::Event)>::new(move |_event: web_sys::Event| {
                if let Some(state) = weak.upgrade()
                    && let Ok(mut state) = state.try_borrow_mut()
                {
                    state.tick();
                    state.repaint();
                }
            });
            let _ = element.add_event_listener_with_callback(name, listener.as_ref().unchecked_ref());
            listeners.push((name, listener));
        }
        let graph = if use_graph {
            let graph = self.graph.as_ref().expect("ensured");
            let nodes = (|| -> Result<DeckGraph, JsValue> {
                let source = graph.context.create_media_element_source(&element)?;
                let gain = graph.context.create_gain()?;
                source.connect_with_audio_node(&gain)?;
                gain.connect_with_audio_node(&graph.input)?;
                Ok(DeckGraph { _source: source, gain })
            })();
            match nodes {
                Ok(nodes) => Some(nodes),
                Err(error) => {
                    log::warn!("Web Audio routing failed: {error:?}");
                    None
                }
            }
        } else {
            None
        };
        let cors = graph.is_some();
        let following = self.next_item.clone();
        let album = self.album_context(&item, following.as_ref());
        let item_gain = self.item_gain(&item, album);
        let deck = Deck { item, element, graph, listeners, item_gain, fade_gain: 1.0, cors };
        Ok(deck)
    }

    fn apply_volume(&self, deck: &Deck) {
        let volume = f64::from(self.volume) * deck.fade_gain;
        match &deck.graph {
            Some(graph) => {
                deck.element.set_volume(1.0);
                graph.gain.gain().set_value((volume * deck.item_gain) as f32);
            }
            None => deck.element.set_volume((volume * deck.item_gain).clamp(0.0, 1.0)),
        }
    }

    fn apply_volumes(&self) {
        for deck in self.current.iter().chain(self.next.iter()).chain(self.outgoing.iter().map(|f| &f.deck)) {
            self.apply_volume(deck);
        }
    }

    fn refresh_effects(&mut self) {
        let following = self.next_item.clone();
        let current_album = self.current.as_ref().map(|d| self.album_context(&d.item, following.as_ref()));
        let current_gain = self.current.as_ref().zip(current_album).map(|(d, album)| self.item_gain(&d.item, album));
        if let (Some(deck), Some(gain)) = (&mut self.current, current_gain) {
            deck.item_gain = gain;
        }
        let equalizer = self.current.as_ref().and_then(|d| self.equalizer_for(&d.item));
        if let Some(graph) = &mut self.graph {
            graph.set_equalizer(equalizer.as_ref());
        }
        self.apply_volumes();
    }

    fn start(&mut self, deck: &Deck) {
        if let Some(graph) = &self.graph {
            let _ = graph.context.resume();
        }
        self.apply_volume(deck);
        let Ok(promise) = deck.element.play() else { return };
        let weak = self.me.clone();
        wasm_bindgen_futures::spawn_local(async move {
            if let Err(error) = wasm_bindgen_futures::JsFuture::from(promise).await {
                let name = js_sys::Reflect::get(&error, &"name".into()).ok().and_then(|n| n.as_string()).unwrap_or_default();
                if let Some(state) = weak.upgrade()
                    && let Ok(mut state) = state.try_borrow_mut()
                    && name == "NotAllowedError"
                {
                    // Autoplay policy: wait for the person to press play.
                    log::info!("The browser blocked playback until the page is clicked");
                    state.wants_play = false;
                    state.repaint();
                }
            }
        });
    }

    fn load(&mut self, item: MediaItem, start_at: f64, autoplay: bool) {
        self.current = None;
        self.next = None;
        self.outgoing = None;
        self.previous_album = None;
        self.ended = false;
        self.failed = false;
        self.wants_play = autoplay;
        let start_at = if item.kind == ContentKind::Radio { 0.0 } else { start_at.max(0.0) };
        match self.make_deck(item.clone(), true) {
            Ok(deck) => {
                if start_at > 0.0 {
                    deck.element.set_current_time(start_at);
                }
                let equalizer = self.equalizer_for(&deck.item);
                if let Some(graph) = &mut self.graph {
                    graph.set_equalizer(equalizer.as_ref());
                }
                if autoplay {
                    self.start(&deck);
                } else {
                    self.apply_volume(&deck);
                }
                self.current = Some(deck);
            }
            Err(message) => {
                self.failed = true;
                self.events.push(AudioEvent::Failed { item_id: item.id, message });
                self.repaint();
            }
        }
    }

    /// Moves to the next item: by itself at the end, or into a crossfade.
    fn advance(&mut self, crossfade: Option<f64>) -> bool {
        let Some(item) = self.next_item.take() else { return false };
        let deck = match self.next.take() {
            Some(deck) if deck.item == item => Ok(deck),
            _ => self.make_deck(item.clone(), true),
        };
        let Some(old) = self.current.take() else { return false };
        let from = old.item.id.clone();
        self.previous_album = old.item.album_key.clone();
        match crossfade {
            Some(seconds) => self.outgoing = Some(Fade { deck: old, start_ms: now_ms(), seconds }),
            None => {
                self.outgoing = None;
                drop(old);
            }
        }
        match deck {
            Ok(mut deck) => {
                if crossfade.is_some() {
                    deck.fade_gain = 0.0;
                }
                self.ended = false;
                self.start(&deck);
                self.current = Some(deck);
                self.refresh_effects();
                self.events.push(AudioEvent::Advanced { from, to: item.id });
            }
            Err(message) => {
                self.events.push(AudioEvent::Advanced { from, to: item.id.clone() });
                self.events.push(AudioEvent::Failed { item_id: item.id, message });
                self.failed = true;
                self.wants_play = false;
            }
        }
        self.repaint();
        true
    }

    fn end_fade(&mut self) {
        if self.outgoing.take().is_some()
            && let Some(deck) = &mut self.current
        {
            deck.fade_gain = 1.0;
        }
        self.apply_volumes();
    }

    fn tick(&mut self) {
        // Failures of the current item; a CORS load is retried once without Web Audio.
        if let Some(deck) = &self.current
            && let Some(error) = deck.element.error()
            && !self.failed
        {
            if deck.cors {
                log::warn!("Media server refused CORS; playing without effects");
                self.graph_unavailable = true;
                let (item, position) = (deck.item.clone(), deck.element.current_time());
                let play = self.wants_play;
                let next = self.next_item.clone();
                self.load(item, position, play);
                self.next_item = next;
                return;
            }
            self.failed = true;
            self.wants_play = false;
            let message = match error.code() {
                1 => "playback was aborted".to_string(),
                2 => "a network error stopped playback".to_string(),
                3 => "the audio could not be decoded".to_string(),
                4 => "this format or address is not supported by the browser".to_string(),
                _ => error.message(),
            };
            let item_id = deck.item.id.clone();
            self.events.push(AudioEvent::Failed { item_id, message });
            self.repaint();
            return;
        }
        if self.next.as_ref().is_some_and(|d| d.element.error().is_some()) {
            // Retried as a fresh element when the current item ends.
            self.next = None;
        }
        // A running crossfade.
        if let Some(fade) = &self.outgoing {
            let progress = (now_ms() - fade.start_ms) / (fade.seconds * 1000.0);
            if progress >= 1.0 || fade.deck.element.ended() {
                self.end_fade();
            } else {
                let (out_gain, in_gain) = discophile_core::audio_effects::TrackTransition::volumes(progress);
                if let Some(fade) = &mut self.outgoing {
                    fade.deck.fade_gain = f64::from(out_gain);
                }
                if let Some(deck) = &mut self.current {
                    deck.fade_gain = f64::from(in_gain);
                }
                self.apply_volumes();
            }
        }
        let Some(deck) = &self.current else { return };
        if deck.element.ended() && !self.ended {
            if !self.advance(None) {
                self.ended = true;
                self.wants_play = false;
                let item_id = self.current.as_ref().map(|d| d.item.id.clone()).unwrap_or_default();
                self.events.push(AudioEvent::Ended { item_id });
                self.repaint();
            }
            return;
        }
        // Prepare the next item, and start a due crossfade.
        let (Some(next), true) = (self.next_item.clone(), self.wants_play) else { return };
        if self.outgoing.is_some() {
            return;
        }
        let duration = Some(deck.element.duration()).filter(|d| d.is_finite() && *d > 0.0).or(deck.item.duration_hint);
        let Some(duration) = duration else { return };
        let remaining = duration - deck.element.current_time();
        let plan = plan(&deck.item, Some(duration), &next, &self.transitions);
        let lead = match plan {
            Plan::Sequential => return,
            Plan::Gapless => GAPLESS_LEAD,
            Plan::Crossfade(fade) => (fade + 15.0).max(GAPLESS_LEAD),
        };
        if self.next.is_none() && remaining <= lead {
            match self.make_deck(next.clone(), true) {
                Ok(deck) => self.next = Some(deck),
                Err(error) => log::warn!("Could not prepare the next item: {error}"),
            }
        }
        if let Plan::Crossfade(fade) = plan
            && remaining <= fade
            && remaining > fade / 2.0
            && self.next.as_ref().is_some_and(|d| d.element.ready_state() >= READY)
        {
            self.advance(Some(fade));
        }
    }
}

impl AudioOutput for WebOutput {
    fn load(&mut self, item: MediaItem, start_at: f64, autoplay: bool) {
        self.with(|s| s.load(item, start_at, autoplay));
    }

    fn set_next(&mut self, item: Option<MediaItem>) {
        self.with(|s| {
            if s.next.as_ref().is_some_and(|d| Some(&d.item) != item.as_ref()) {
                s.next = None;
            }
            s.next_item = item;
            s.refresh_effects();
        });
    }

    fn play(&mut self) {
        self.with(|s| {
            if s.failed {
                if let Some(deck) = &s.current {
                    let (item, position, next) = (deck.item.clone(), deck.element.current_time(), s.next_item.clone());
                    s.load(item, position, true);
                    s.next_item = next;
                }
                return;
            }
            let Some(deck) = s.current.take() else { return };
            if s.ended {
                deck.element.set_current_time(0.0);
                s.ended = false;
            }
            s.wants_play = true;
            s.start(&deck);
            if let Some(fade) = &s.outgoing {
                let _ = fade.deck.element.play();
            }
            s.current = Some(deck);
        });
    }

    fn pause(&mut self) {
        self.with(|s| {
            s.wants_play = false;
            s.end_fade();
            if let Some(deck) = &s.current {
                let _ = deck.element.pause();
            }
        });
    }

    fn stop(&mut self) {
        self.with(|s| {
            s.wants_play = false;
            s.current = None;
            s.next = None;
            s.next_item = None;
            s.outgoing = None;
            s.ended = false;
            s.failed = false;
        });
    }

    fn seek(&mut self, seconds: f64) {
        self.with(|s| {
            s.end_fade();
            let Some(deck) = &s.current else { return };
            if deck.item.kind == ContentKind::Radio || !seconds.is_finite() {
                return;
            }
            deck.element.set_current_time(seconds.max(0.0));
            if s.ended {
                s.ended = false;
            }
        });
    }

    fn set_volume(&mut self, volume: f32) {
        self.with(|s| {
            s.volume = if volume.is_finite() { volume.clamp(0.0, 1.0) } else { 1.0 };
            s.apply_volumes();
        });
    }

    fn set_rate(&mut self, rate: f32) {
        self.with(|s| {
            s.rate = if rate.is_finite() { rate.clamp(0.5, 4.0) } else { 1.0 };
            for deck in s.current.iter().chain(s.next.iter()) {
                if deck.item.kind != ContentKind::Radio {
                    deck.element.set_default_playback_rate(f64::from(s.rate));
                    deck.element.set_playback_rate(f64::from(s.rate));
                }
            }
        });
    }

    fn set_transitions(&mut self, settings: TransitionSettings) {
        self.with(|s| s.transitions = settings);
    }

    fn set_effects(&mut self, settings: &AudioEffectsSettings) {
        self.with(|s| {
            if s.effects.as_ref() == Some(settings) {
                return;
            }
            s.effects = Some(settings.clone());
            // A deck without Web Audio cannot gain an equalizer: reload it at its position.
            let reload = s
                .current
                .as_ref()
                .filter(|d| d.graph.is_none() && s.needs_graph(&d.item))
                .map(|d| (d.item.clone(), d.element.current_time()));
            match reload {
                Some((item, position)) => {
                    let (play, next) = (s.wants_play, s.next_item.clone());
                    s.load(item, position, play);
                    s.next_item = next;
                }
                None => s.refresh_effects(),
            }
            s.next = None;
        });
    }

    fn status(&self) -> OutputStatus {
        let state = self.state.borrow();
        let Some(deck) = &state.current else { return OutputStatus::default() };
        let element = &deck.element;
        let live = deck.item.kind == ContentKind::Radio;
        let duration = if live { None } else { Some(element.duration()).filter(|d| d.is_finite() && *d > 0.0).or(deck.item.duration_hint) };
        let playing = state.wants_play && !state.ended && !state.failed;
        OutputStatus {
            item_id: Some(deck.item.id.clone()),
            position: if state.ended { duration.unwrap_or_else(|| element.current_time()) } else { element.current_time() },
            duration,
            playing,
            buffering: playing && (element.ready_state() < READY || element.paused()),
            details: Some(AudioDetails {
                codec: deck.item.server_format.as_ref().map(|f| f.to_uppercase()),
                output_device: Some("Browser".into()),
                output_sample_rate: state.graph.as_ref().filter(|_| deck.graph.is_some()).map(|g| g.context.sample_rate() as u32),
                streamed: true,
                ..Default::default()
            }),
        }
    }

    fn poll_events(&mut self) -> Vec<AudioEvent> {
        let mut state = self.state.borrow_mut();
        state.tick();
        std::mem::take(&mut state.events)
    }
}
