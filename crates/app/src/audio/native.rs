//! Desktop audio engine.
//!
//! - One decoding thread per item (`decoder`): Symphonia over local files or progressive HTTP
//!   downloads (`stream`), resampled to the device rate with Rubato (`resample`).
//! - One engine thread mixes the current item, the prepared next item and crossfades (`mixer`),
//!   applies effects per item and writes into a lock-free ring (`ring`). It owns the cpal stream.
//! - The cpal callback only reads the ring and applies volume and pause ramps.
//!
//! Item boundaries are recorded as markers at ring indices, so `Advanced` and `Ended` are reported
//! when the samples are heard, not when they are decoded.

mod decoder;
mod mixer;
mod resample;
mod ring;
mod stream;

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use discophile_core::audio_effects::AudioEffectsSettings;

use self::decoder::Voice;
use self::mixer::{Marker, MarkerKind, Mixer, PcmSource};
use self::ring::{Consumer, Producer};
use super::{AudioDetails, AudioEvent, AudioOutput, ContentKind, MediaItem, OutputStatus, TransitionSettings};

/// Rendered audio kept ahead of the device.
const AHEAD_SECONDS: f64 = 0.3;
const RENDER_FRAMES: usize = 1024;
const DEVICE_RETRY: Duration = Duration::from_secs(3);
/// The device stream stops after this long without playback.
const IDLE_STOP: Duration = Duration::from_secs(5);
const FLUSH_TIMEOUT: Duration = Duration::from_millis(250);

enum Command {
    Load(MediaItem, f64, bool),
    SetNext(Option<MediaItem>),
    Play,
    Pause,
    Stop,
    Seek(f64),
    Rate(f32),
    Transitions(TransitionSettings),
    Effects(AudioEffectsSettings),
    Shutdown,
}

/// Read by the device callback without locks.
struct Control {
    volume: AtomicU32,
    paused: AtomicBool,
}

#[derive(Default)]
struct Published {
    status: OutputStatus,
    /// Commands the engine has applied when it wrote `status`.
    applied: u64,
}

struct Shared {
    published: Mutex<Published>,
    events: Mutex<Vec<AudioEvent>>,
    ctx: egui::Context,
}

/// The desktop `AudioOutput`.
pub struct NativeOutput {
    commands: Sender<Command>,
    shared: Arc<Shared>,
    control: Arc<Control>,
    sent: u64,
    /// Shown until the engine has applied the latest command.
    optimistic: OutputStatus,
    thread: Option<JoinHandle<()>>,
}

impl NativeOutput {
    /// Starts the engine thread. The output device is opened when something is loaded; if none is
    /// available, playback reports `Failed` and is retried on `play`.
    pub fn new(ctx: &egui::Context) -> Result<Self, String> {
        let (commands, receiver) = mpsc::channel();
        let shared = Arc::new(Shared { published: Mutex::default(), events: Mutex::default(), ctx: ctx.clone() });
        let control = Arc::new(Control { volume: AtomicU32::new(1.0f32.to_bits()), paused: AtomicBool::new(true) });
        // Built on the audio thread: `cpal::Stream` is not `Send` on macOS.
        let (engine_shared, engine_control) = (shared.clone(), control.clone());
        let thread = std::thread::Builder::new()
            .name("discophile-audio".into())
            .spawn(move || Engine::new(receiver, engine_shared, engine_control).run())
            .map_err(|e| format!("could not start the audio thread: {e}"))?;
        Ok(Self { commands, shared, control, sent: 0, optimistic: OutputStatus::default(), thread: Some(thread) })
    }

    fn send(&mut self, command: Command, status: OutputStatus) {
        self.optimistic = status;
        if self.commands.send(command).is_ok() {
            self.sent += 1;
        }
    }

    /// Changes the effects chain (equalizer, ReplayGain, volume limit). Also on the trait.
    pub fn set_effects_settings(&mut self, settings: AudioEffectsSettings) {
        let status = self.status();
        self.send(Command::Effects(settings), status);
    }
}

impl Drop for NativeOutput {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl AudioOutput for NativeOutput {
    fn load(&mut self, item: MediaItem, start_at: f64, autoplay: bool) {
        self.control.paused.store(!autoplay, Ordering::Relaxed);
        let status = OutputStatus {
            item_id: Some(item.id.clone()),
            position: start_at.max(0.0),
            duration: item.duration_hint,
            playing: autoplay,
            buffering: autoplay,
            details: None,
        };
        self.send(Command::Load(item, start_at, autoplay), status);
    }

    fn set_next(&mut self, item: Option<MediaItem>) {
        let status = self.status();
        self.send(Command::SetNext(item), status);
    }

    fn play(&mut self) {
        let mut status = self.status();
        status.playing = status.item_id.is_some();
        self.send(Command::Play, status);
    }

    fn pause(&mut self) {
        self.control.paused.store(true, Ordering::Relaxed);
        let mut status = self.status();
        status.playing = false;
        status.buffering = false;
        self.send(Command::Pause, status);
    }

    fn stop(&mut self) {
        self.control.paused.store(true, Ordering::Relaxed);
        self.send(Command::Stop, OutputStatus::default());
    }

    fn seek(&mut self, seconds: f64) {
        let mut status = self.status();
        status.position = seconds.max(0.0);
        self.send(Command::Seek(seconds), status);
    }

    fn set_volume(&mut self, volume: f32) {
        let volume = if volume.is_finite() { volume.clamp(0.0, 1.0) } else { 1.0 };
        self.control.volume.store(volume.to_bits(), Ordering::Relaxed);
    }

    fn set_rate(&mut self, rate: f32) {
        let status = self.status();
        self.send(Command::Rate(rate), status);
    }

    fn set_transitions(&mut self, settings: TransitionSettings) {
        let status = self.status();
        self.send(Command::Transitions(settings), status);
    }

    fn set_effects(&mut self, settings: &AudioEffectsSettings) {
        self.set_effects_settings(settings.clone());
    }

    fn status(&self) -> OutputStatus {
        let published = self.shared.published.lock().unwrap_or_else(|e| e.into_inner());
        if published.applied >= self.sent { published.status.clone() } else { self.optimistic.clone() }
    }

    fn poll_events(&mut self) -> Vec<AudioEvent> {
        std::mem::take(&mut *self.shared.events.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

struct Device {
    stream: cpal::Stream,
    producer: Producer,
    rate: u32,
    channels: usize,
    name: String,
    lost: Arc<AtomicBool>,
    running: bool,
}

/// What is heard now: the last marker the device passed.
struct Audible {
    item: Arc<MediaItem>,
    pos: f64,
    /// Ring index where `pos` applies.
    index: u64,
    speed: f32,
    ended: bool,
    failed: bool,
}

struct Engine {
    commands: Receiver<Command>,
    shared: Arc<Shared>,
    control: Arc<Control>,
    applied: u64,
    device: Option<Device>,
    mixer: Option<Mixer>,
    retry_at: Option<Instant>,
    device_error_reported: bool,
    paused: bool,
    speed: f32,
    transitions: TransitionSettings,
    effects: Option<AudioEffectsSettings>,
    next: Option<MediaItem>,
    audible: Option<Audible>,
    markers: VecDeque<Marker>,
    flushed_at: Option<Instant>,
    idle_since: Option<Instant>,
    scratch: Vec<f32>,
    /// Duration and details of the audible item, kept after the mixer moved past it.
    info_cache: Option<(Arc<MediaItem>, Option<f64>, Option<AudioDetails>)>,
}

impl Engine {
    fn new(commands: Receiver<Command>, shared: Arc<Shared>, control: Arc<Control>) -> Self {
        Self {
            commands,
            shared,
            control,
            applied: 0,
            device: None,
            mixer: None,
            retry_at: None,
            device_error_reported: false,
            paused: true,
            speed: 1.0,
            transitions: TransitionSettings::default(),
            effects: None,
            next: None,
            audible: None,
            markers: VecDeque::new(),
            flushed_at: None,
            idle_since: None,
            scratch: Vec::new(),
            info_cache: None,
        }
    }

    fn run(mut self) {
        loop {
            let wait = if self.is_active() { Duration::from_millis(5) } else { Duration::from_millis(50) };
            match self.commands.recv_timeout(wait) {
                Ok(command) => {
                    if !self.handle(command) {
                        return;
                    }
                    while let Ok(command) = self.commands.try_recv() {
                        if !self.handle(command) {
                            return;
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            self.check_device();
            self.advance_timeline();
            self.fill();
            self.advance_timeline();
            self.manage_stream();
            self.publish();
        }
    }

    fn is_active(&self) -> bool {
        !self.paused && self.audible.as_ref().is_some_and(|a| !a.ended && !a.failed)
    }

    fn emit(&self, event: AudioEvent) {
        self.shared.events.lock().unwrap_or_else(|e| e.into_inner()).push(event);
        self.shared.ctx.request_repaint();
    }

    fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
        self.control.paused.store(paused, Ordering::Relaxed);
    }

    fn write_index(&self) -> u64 {
        self.device.as_ref().map_or(0, |d| d.producer.write_index())
    }

    fn flush(&mut self) {
        self.markers.clear();
        if let Some(device) = &mut self.device {
            device.producer.flush();
            if !device.running {
                device.producer.force_flush();
            }
            self.flushed_at = Some(Instant::now());
        }
    }

    /// Returns false on shutdown.
    fn handle(&mut self, command: Command) -> bool {
        self.applied += 1;
        match command {
            Command::Shutdown => return false,
            Command::Load(item, start_at, autoplay) => self.load(Arc::new(item), start_at.max(0.0), autoplay),
            Command::SetNext(item) => {
                self.next = item.clone();
                if let Some(mixer) = &mut self.mixer {
                    mixer.set_next(item);
                }
            }
            Command::Play => self.play(),
            Command::Pause => {
                self.set_paused(true);
                if let Some(mixer) = &mut self.mixer {
                    mixer.end_fade();
                }
            }
            Command::Stop => {
                self.set_paused(true);
                self.flush();
                if let Some(mixer) = &mut self.mixer {
                    mixer.stop();
                }
                self.audible = None;
                self.next = None;
            }
            Command::Seek(seconds) => self.seek(seconds),
            Command::Rate(rate) => {
                self.speed = resample::clamp_speed(rate);
                if let Some(mixer) = &mut self.mixer {
                    mixer.set_speed(self.speed);
                }
            }
            Command::Transitions(settings) => {
                self.transitions = settings;
                if let Some(mixer) = &mut self.mixer {
                    mixer.set_transitions(settings);
                }
            }
            Command::Effects(settings) => {
                if let Some(mixer) = &mut self.mixer {
                    mixer.set_effects(settings.clone());
                }
                self.effects = Some(settings);
            }
        }
        true
    }

    fn speed_for(&self, item: &MediaItem) -> f32 {
        if item.kind == ContentKind::Radio { 1.0 } else { self.speed }
    }

    fn load(&mut self, item: Arc<MediaItem>, start_at: f64, autoplay: bool) {
        self.set_paused(!autoplay);
        self.flush();
        let start_at = if item.kind == ContentKind::Radio { 0.0 } else { start_at };
        if autoplay {
            self.retry_at = None;
            self.device_error_reported = false;
        }
        let speed = self.speed_for(&item);
        if let Some(mixer) = &mut self.mixer {
            mixer.load(item.clone(), start_at);
        }
        self.audible = Some(Audible { item, pos: start_at, index: self.write_index(), speed, ended: false, failed: false });
        self.check_device();
    }

    fn play(&mut self) {
        let Some(audible) = &self.audible else { return };
        if audible.ended || audible.failed || self.mixer.as_ref().is_some_and(|m| !m.has_work()) {
            let item = audible.item.clone();
            let start = if audible.ended { 0.0 } else { audible.pos };
            self.load(item, start, true);
            if let Some(mixer) = &mut self.mixer {
                mixer.set_next(self.next.clone());
            }
            return;
        }
        self.retry_at = None;
        self.device_error_reported = false;
        self.set_paused(false);
        self.check_device();
    }

    fn seek(&mut self, seconds: f64) {
        let Some(audible) = &self.audible else { return };
        if audible.item.kind == ContentKind::Radio || !seconds.is_finite() {
            return;
        }
        let item = audible.item.clone();
        let duration = self.duration_of(&item);
        let target = match duration {
            Some(duration) => seconds.clamp(0.0, duration),
            None => seconds.max(0.0),
        };
        self.flush();
        if let Some(mixer) = &mut self.mixer {
            let committed = mixer.current_item();
            if committed.as_ref().is_some_and(|c| Arc::ptr_eq(c, &item)) {
                mixer.seek(target);
            } else {
                // The mixer already moved on to the next item (or ended) but that was not heard yet.
                let follow = committed.map(|c| (*c).clone()).or_else(|| mixer.next_item());
                mixer.load(item.clone(), target);
                mixer.set_next(follow);
            }
        }
        let speed = self.speed_for(&item);
        self.audible = Some(Audible { item, pos: target, index: self.write_index(), speed, ended: false, failed: false });
    }

    fn duration_of(&self, item: &Arc<MediaItem>) -> Option<f64> {
        if item.kind == ContentKind::Radio {
            return None;
        }
        let cached = self.info_cache.as_ref().filter(|(i, _, _)| Arc::ptr_eq(i, item)).and_then(|(_, d, _)| *d);
        self.mixer.as_ref().and_then(|m| m.info_for(item)).and_then(|(d, _)| d).or(cached).or(item.duration_hint)
    }

    fn refresh_info_cache(&mut self) {
        let Some(audible) = &self.audible else { return };
        let Some((duration, details)) = self.mixer.as_ref().and_then(|m| m.info_for(&audible.item)) else { return };
        let previous = self.info_cache.take().filter(|(i, _, _)| Arc::ptr_eq(i, &audible.item));
        let (old_duration, old_details) = previous.map(|(_, d, x)| (d, x)).unwrap_or_default();
        self.info_cache = Some((audible.item.clone(), duration.or(old_duration), details.or(old_details)));
    }

    /// Opens the device when needed, and recovers from a lost one.
    fn check_device(&mut self) {
        if self.device.as_ref().is_some_and(|d| d.lost.load(Ordering::Relaxed)) {
            log::warn!("Audio output device lost");
            let position = self.position();
            self.device = None;
            self.mixer = None;
            self.markers.clear();
            if let Some(audible) = &mut self.audible {
                audible.pos = position;
                audible.index = 0;
            }
            // Like unplugging headphones on a phone: pause, then use the new default device.
            self.set_paused(true);
            self.retry_at = None;
        }
        let needed = self.audible.as_ref().is_some_and(|a| !a.ended && !a.failed);
        if self.device.is_some() || !needed {
            return;
        }
        if self.retry_at.is_some_and(|at| Instant::now() < at) {
            return;
        }
        match open_device(self.control.clone()) {
            Ok(device) => {
                log::info!("Audio output: {} at {} Hz, {} channels", device.name, device.rate, device.channels);
                let (rate, channels) = (device.rate, device.channels);
                let factory: mixer::SourceFactory = Box::new(move |item: &MediaItem, start: f64, speed: f32| {
                    Box::new(Voice::spawn(item, start, rate, channels, speed)) as Box<dyn PcmSource>
                });
                let mut mixer = Mixer::new(rate, channels, factory);
                mixer.set_speed(self.speed);
                mixer.set_transitions(self.transitions);
                if let Some(effects) = &self.effects {
                    mixer.set_effects(effects.clone());
                }
                let index = device.producer.write_index();
                if let Some(audible) = &mut self.audible {
                    mixer.load(audible.item.clone(), audible.pos);
                    mixer.set_next(self.next.clone());
                    audible.index = index;
                }
                self.device = Some(device);
                self.mixer = Some(mixer);
                self.device_error_reported = false;
                self.retry_at = None;
            }
            Err(message) => {
                log::warn!("No audio output: {message}");
                self.retry_at = Some(Instant::now() + DEVICE_RETRY);
                if !self.device_error_reported {
                    self.device_error_reported = true;
                    if let Some(audible) = &self.audible
                        && !self.paused
                    {
                        self.emit(AudioEvent::Failed {
                            item_id: audible.item.id.clone(),
                            message: format!("No audio output device is available ({message})"),
                        });
                    }
                    self.set_paused(true);
                }
            }
        }
    }

    /// Reports markers the device has played past.
    fn advance_timeline(&mut self) {
        let Some(device) = &self.device else { return };
        let read = device.producer.read_index();
        while self.markers.front().is_some_and(|m| m.index <= read) {
            let marker = self.markers.pop_front().expect("checked");
            let mut audible = Audible {
                item: marker.item.clone(),
                pos: marker.pos,
                index: marker.index,
                speed: marker.speed,
                ended: false,
                failed: false,
            };
            match marker.kind {
                MarkerKind::Position | MarkerKind::Started { from: None } => {}
                MarkerKind::Started { from: Some(from) } => {
                    self.emit(AudioEvent::Advanced { from, to: marker.item.id.clone() });
                }
                MarkerKind::Ended => {
                    audible.ended = true;
                    self.set_paused(true);
                    self.emit(AudioEvent::Ended { item_id: marker.item.id.clone() });
                }
                MarkerKind::Failed(message) => {
                    audible.failed = true;
                    self.set_paused(true);
                    self.emit(AudioEvent::Failed { item_id: marker.item.id.clone(), message });
                }
            }
            self.audible = Some(audible);
        }
    }

    fn fill(&mut self) {
        let (Some(device), Some(mixer)) = (&mut self.device, &mut self.mixer) else { return };
        if !device.producer.flush_acknowledged() {
            if !device.running || self.flushed_at.is_some_and(|at| at.elapsed() > FLUSH_TIMEOUT) {
                device.producer.force_flush();
            } else {
                return;
            }
        }
        device.producer.settle();
        let channels = device.channels;
        let target = ((AHEAD_SECONDS * f64::from(device.rate)) as usize * channels).min(device.producer.capacity() / 2);
        while device.producer.buffered() < target {
            let frames = RENDER_FRAMES.min(device.producer.free() / channels);
            if frames == 0 {
                break;
            }
            self.scratch.clear();
            self.scratch.resize(frames * channels, 0.0);
            let rendered = mixer.render(&mut self.scratch, device.producer.write_index());
            self.markers.extend(mixer.take_markers());
            if rendered == 0 {
                break;
            }
            for sample in &mut self.scratch[..rendered * channels] {
                *sample = sample.clamp(-1.0, 1.0);
            }
            device.producer.push(&self.scratch[..rendered * channels]);
        }
        self.markers.extend(mixer.take_markers());
    }

    /// Stops the device stream after a while without playback, and restarts it when needed.
    fn manage_stream(&mut self) {
        let active = self.is_active();
        let Some(device) = &mut self.device else { return };
        if active {
            self.idle_since = None;
            if !device.running {
                if let Err(error) = device.stream.play() {
                    log::warn!("Audio stream did not start: {error}");
                }
                device.running = true;
            }
        } else {
            let since = *self.idle_since.get_or_insert_with(Instant::now);
            if device.running && since.elapsed() > IDLE_STOP && device.stream.pause().is_ok() {
                device.running = false;
            }
        }
    }

    fn position(&self) -> f64 {
        let Some(audible) = &self.audible else { return 0.0 };
        let Some(device) = &self.device else { return audible.pos };
        if audible.ended {
            return self.duration_of(&audible.item).unwrap_or(audible.pos);
        }
        let read = device.producer.read_index();
        let frames = read.saturating_sub(audible.index) as f64 / device.channels as f64;
        audible.pos + frames / f64::from(device.rate) * f64::from(audible.speed)
    }

    fn publish(&mut self) {
        self.refresh_info_cache();
        let status = match &self.audible {
            None => OutputStatus::default(),
            Some(audible) => {
                let live = !audible.ended && !audible.failed;
                let playing = !self.paused && live;
                let cached = self.info_cache.as_ref().filter(|(i, _, _)| Arc::ptr_eq(i, &audible.item));
                let starved = match (&self.device, &self.mixer) {
                    (Some(device), Some(mixer)) => {
                        mixer.is_starved() && device.producer.buffered() < device.rate as usize * device.channels / 10
                    }
                    _ => true,
                };
                let details = cached.and_then(|(_, _, d)| d.clone()).map(|mut details| {
                    if let Some(device) = &self.device {
                        details.output_device = Some(device.name.clone());
                        details.output_sample_rate = Some(device.rate);
                    }
                    details
                });
                let mut position = self.position();
                let duration = self.duration_of(&audible.item);
                if let Some(duration) = duration {
                    position = position.min(duration);
                }
                OutputStatus { item_id: Some(audible.item.id.clone()), position, duration, playing, buffering: playing && starved, details }
            }
        };
        let mut published = self.shared.published.lock().unwrap_or_else(|e| e.into_inner());
        published.status = status;
        published.applied = self.applied;
    }
}

fn open_device(control: Arc<Control>) -> Result<Device, String> {
    let host = cpal::default_host();
    // ALSA's default can point at card 0 instead of the desktop's selected output.
    #[cfg(target_os = "linux")]
    for backend in ["pipewire", "pulse"] {
        if let Ok(mut devices) = host.output_devices()
            && let Some(device) = devices.find(|d| d.name().is_ok_and(|name| name == backend))
        {
            match open_output_device(device, control.clone()) {
                Ok(output) => return Ok(output),
                Err(error) => log::warn!("Audio backend {backend} unavailable: {error}"),
            }
        }
    }
    let device = host.default_output_device().ok_or("no output device")?;
    open_output_device(device, control)
}

fn open_output_device(device: cpal::Device, control: Arc<Control>) -> Result<Device, String> {
    let name = device.name().unwrap_or_else(|_| "Default output".into());
    let supported = device.default_output_config().map_err(|e| e.to_string())?;
    let rate = supported.sample_rate().0;
    let channels = usize::from(supported.channels()).max(1);
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let (producer, consumer) = ring::ring(rate as usize * channels);
    let lost = Arc::new(AtomicBool::new(false));
    let build = |consumer| -> Result<cpal::Stream, String> {
        use cpal::SampleFormat as F;
        let (c, l) = (control.clone(), lost.clone());
        match format {
            F::F32 => build_stream::<f32>(&device, &config, consumer, c, l, channels, rate),
            F::F64 => build_stream::<f64>(&device, &config, consumer, c, l, channels, rate),
            F::I16 => build_stream::<i16>(&device, &config, consumer, c, l, channels, rate),
            F::I32 => build_stream::<i32>(&device, &config, consumer, c, l, channels, rate),
            F::U16 => build_stream::<u16>(&device, &config, consumer, c, l, channels, rate),
            F::I8 => build_stream::<i8>(&device, &config, consumer, c, l, channels, rate),
            F::U8 => build_stream::<u8>(&device, &config, consumer, c, l, channels, rate),
            other => Err(format!("unsupported device sample format {other}")),
        }
    };
    let stream = build(consumer)?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(Device { stream, producer, rate, channels, name, lost, running: true })
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut consumer: Consumer,
    control: Arc<Control>,
    lost: Arc<AtomicBool>,
    channels: usize,
    rate: u32,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32> + Send + 'static,
{
    let mut scratch = vec![0.0f32; 16_384];
    let mut gain = 0.0f32;
    // About 5 ms to follow volume and pause changes without clicks.
    let smoothing = 1.0 - (-1.0 / (0.005 * rate as f32)).exp();
    let data = move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
        let len = data.len();
        if scratch.len() < len {
            scratch.resize(len, 0.0);
        }
        let paused = control.paused.load(Ordering::Relaxed);
        let volume = f32::from_bits(control.volume.load(Ordering::Relaxed));
        let target = if paused { 0.0 } else { volume };
        if paused && gain < 1e-4 {
            gain = 0.0;
            consumer.skip_flushed();
            data.iter_mut().for_each(|s| *s = T::EQUILIBRIUM);
            return;
        }
        let read = consumer.pop(&mut scratch[..len]);
        scratch[read..len].iter_mut().for_each(|s| *s = 0.0);
        for frame in scratch[..len].chunks_exact_mut(channels) {
            gain += (target - gain) * smoothing;
            frame.iter_mut().for_each(|s| *s *= gain);
        }
        for (out, sample) in data.iter_mut().zip(&scratch[..len]) {
            *out = T::from_sample(*sample);
        }
    };
    let error = move |error: cpal::StreamError| {
        log::warn!("Audio stream error: {error}");
        if matches!(error, cpal::StreamError::DeviceNotAvailable) {
            lost.store(true, Ordering::Relaxed);
        }
    };
    device.build_output_stream(config, data, error, None).map_err(|e| e.to_string())
}
