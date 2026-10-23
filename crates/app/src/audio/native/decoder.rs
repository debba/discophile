//! One decoding thread per item: opens the file or stream, decodes with Symphonia, maps channels,
//! resamples to the device rate and queues a couple of seconds ahead of the mixer.

use std::collections::VecDeque;
use std::fs::File;
use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, CodecParameters, Decoder, DecoderOptions};
use symphonia::core::errors::{Error as SymphoniaError, SeekErrorKind};
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::{MediaSource, MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::Time;

use super::mixer::{PcmSource, SourceRead, SourceState};
use super::resample::{Resample, clamp_speed, map_channels};
use super::stream::{HttpFetcher, INTERRUPTED, Store, StoreReader};
use crate::audio::{AudioDetails, AudioSource, ContentKind, MediaItem};

/// Decoded audio kept ahead of the mixer, in seconds.
const QUEUE_SECONDS: f64 = 2.0;
const HEADER_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_PLAYLIST_BYTES: usize = 64 * 1024;

struct Chunk {
    samples: Vec<f32>,
    read: usize,
    pos: f64,
    speed: f32,
}

#[derive(Default)]
struct State {
    chunks: VecDeque<Chunk>,
    buffered: usize,
    finished: bool,
    error: Option<String>,
    details: Option<AudioDetails>,
    duration: Option<f64>,
    seek: Option<f64>,
}

struct Shared {
    state: Mutex<State>,
    changed: Condvar,
    speed: AtomicU32,
    cancel: AtomicBool,
    interrupt: Arc<AtomicBool>,
    waiting: Arc<AtomicBool>,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn speed(&self) -> f32 {
        f32::from_bits(self.speed.load(Ordering::Relaxed))
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

/// A decoding item. Dropping it stops the thread and the download.
pub struct Voice {
    shared: Arc<Shared>,
    rate: u32,
    channels: usize,
}

impl Voice {
    pub fn spawn(item: &MediaItem, start_at: f64, out_rate: u32, out_channels: usize, speed: f32) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            changed: Condvar::new(),
            speed: AtomicU32::new(clamp_speed(speed).to_bits()),
            cancel: AtomicBool::new(false),
            interrupt: Arc::default(),
            waiting: Arc::default(),
        });
        let thread_shared = shared.clone();
        let item = item.clone();
        let spawned = std::thread::Builder::new().name("discophile-decode".into()).spawn(move || {
            let shared = thread_shared;
            let mut decoder = DecodeThread::new(shared.clone(), out_rate, out_channels);
            if let Err(message) = decoder.run(&item, start_at)
                && !shared.cancelled() {
                    log::warn!("Decoding {} failed: {message}", item.id);
                    shared.lock().error = Some(message);
                    shared.changed.notify_all();
                }
        });
        if let Err(error) = spawned {
            shared.lock().error = Some(format!("could not start decoding: {error}"));
        }
        Self { shared, rate: out_rate.max(1), channels: out_channels.max(1) }
    }
}

impl Drop for Voice {
    fn drop(&mut self) {
        self.shared.cancel.store(true, Ordering::Relaxed);
        self.shared.interrupt.store(true, Ordering::Relaxed);
        self.shared.changed.notify_all();
    }
}

impl PcmSource for Voice {
    fn read(&mut self, out: &mut [f32]) -> SourceRead {
        let mut state = self.shared.lock();
        let Some(first) = state.chunks.front() else {
            let speed = self.shared.speed();
            let state = match (&state.error, state.finished) {
                (Some(error), _) => SourceState::Failed(error.clone()),
                (None, true) => SourceState::Finished,
                (None, false) => SourceState::Starved,
            };
            return SourceRead { frames: 0, pos: 0.0, speed, state };
        };
        let pos = first.pos + (first.read / self.channels) as f64 / f64::from(self.rate) * f64::from(first.speed);
        let speed = first.speed;
        let mut written = 0;
        while written < out.len() {
            let Some(chunk) = state.chunks.front_mut() else { break };
            let count = (out.len() - written).min(chunk.samples.len() - chunk.read);
            out[written..written + count].copy_from_slice(&chunk.samples[chunk.read..chunk.read + count]);
            chunk.read += count;
            written += count;
            if chunk.read >= chunk.samples.len() {
                state.chunks.pop_front();
            }
        }
        let frames = written / self.channels;
        state.buffered = state.buffered.saturating_sub(frames);
        drop(state);
        self.shared.changed.notify_all();
        SourceRead { frames, pos, speed, state: SourceState::Starved }
    }

    fn seek(&mut self, seconds: f64) {
        let mut state = self.shared.lock();
        state.chunks.clear();
        state.buffered = 0;
        state.finished = false;
        state.seek = Some(seconds.max(0.0));
        self.shared.interrupt.store(true, Ordering::Relaxed);
        drop(state);
        self.shared.changed.notify_all();
    }

    fn set_speed(&mut self, speed: f32) {
        self.shared.speed.store(clamp_speed(speed).to_bits(), Ordering::Relaxed);
    }

    fn duration(&self) -> Option<f64> {
        self.shared.lock().duration
    }

    fn buffered_frames(&self) -> usize {
        self.shared.lock().buffered
    }

    fn details(&self) -> Option<AudioDetails> {
        self.shared.lock().details.clone()
    }
}

struct Opened {
    source: Box<dyn MediaSource>,
    hint: Hint,
    streamed: bool,
    byte_len: Option<u64>,
}

struct DecodeThread {
    shared: Arc<Shared>,
    out_rate: u32,
    out_channels: usize,
    pos: f64,
}

enum Step {
    Continue,
    End,
}

fn is_interrupted(error: &SymphoniaError) -> bool {
    matches!(error, SymphoniaError::IoError(e) if e.to_string() == INTERRUPTED)
}

impl DecodeThread {
    fn new(shared: Arc<Shared>, out_rate: u32, out_channels: usize) -> Self {
        Self { shared, out_rate: out_rate.max(1), out_channels: out_channels.max(1), pos: 0.0 }
    }

    fn run(&mut self, item: &MediaItem, start_at: f64) -> Result<(), String> {
        let opened = open(item, &self.shared)?;
        let live = item.kind == ContentKind::Radio;
        let mss = MediaSourceStream::new(opened.source, MediaSourceStreamOptions { buffer_len: 256 * 1024 });
        let format_options = FormatOptions { enable_gapless: true, ..Default::default() };
        let probed = symphonia::default::get_probe()
            .format(&opened.hint, mss, &format_options, &MetadataOptions::default())
            .map_err(|e| if is_interrupted(&e) { INTERRUPTED.to_string() } else { format!("unsupported or damaged audio: {e}") })?;
        let mut format = probed.format;
        let (track_id, params) = default_track(format.as_ref()).ok_or("no audio track")?;
        let mut decoder = make_decoder(&params)?;
        let in_rate = params.sample_rate.unwrap_or(44_100);
        let duration = if live { None } else { duration_of(&params) };
        let codec = symphonia::default::get_codecs().get_codec(params.codec).map(|d| codec_name(d.short_name));
        let details = AudioDetails {
            codec: codec.or_else(|| item.server_format.as_ref().map(|f| f.to_uppercase())),
            sample_rate: params.sample_rate,
            bit_depth: params.bits_per_sample.or(params.bits_per_coded_sample),
            channels: params.channels.map(|c| c.count() as u32),
            bit_rate: match (opened.byte_len, duration) {
                (Some(bytes), Some(seconds)) if seconds > 0.0 => Some((bytes as f64 * 8.0 / seconds / 1000.0).round() as u32),
                _ => None,
            },
            output_device: None,
            output_sample_rate: Some(self.out_rate),
            streamed: opened.streamed,
        };
        {
            let mut state = self.shared.lock();
            state.details = Some(details);
            state.duration = duration;
        }
        let mut resample = Resample::new(in_rate, self.out_rate, self.out_channels, self.shared.speed());
        let mut skip_frames = 0u64;
        let mut at_end = false;
        if start_at > 0.05 && !live {
            self.shared.lock().seek.get_or_insert(start_at);
        }
        let mut sample_buffer: Option<SampleBuffer<f32>> = None;
        let mut mapped = Vec::new();
        let mut resampled = Vec::new();
        let mut decode_errors = 0;
        let mut speed = self.shared.speed();
        let mut reported_format = false;
        loop {
            if self.shared.cancelled() {
                return Ok(());
            }
            let seek = {
                let mut state = self.shared.lock();
                let seek = state.seek.take();
                if seek.is_some() {
                    self.shared.interrupt.store(false, Ordering::Relaxed);
                    state.finished = false;
                }
                seek
            };
            if let Some(target) = seek {
                resample.reset();
                at_end = false;
                skip_frames = 0;
                if live {
                    continue;
                }
                let time = Time::from(target);
                // Accurate MP3 seeks parse every frame up to the target, which over HTTP means waiting
                // for the download; coarse seeks jump by byte estimate and still land within a frame.
                let mode = if opened.streamed { SeekMode::Coarse } else { SeekMode::Accurate };
                match format.seek(mode, SeekTo::Time { time, track_id: Some(track_id) }) {
                    Ok(seeked) => {
                        decoder.reset();
                        let seconds = |ts: u64| match params.time_base {
                            Some(base) => {
                                let t = base.calc_time(ts);
                                t.seconds as f64 + t.frac
                            }
                            None => ts as f64 / f64::from(in_rate),
                        };
                        let early = seeked.required_ts.saturating_sub(seeked.actual_ts);
                        let late = seeked.actual_ts.saturating_sub(seeked.required_ts);
                        skip_frames = (seconds(early) * f64::from(in_rate)).round() as u64;
                        self.pos = target + seconds(late);
                    }
                    Err(SymphoniaError::SeekError(SeekErrorKind::OutOfRange)) => {
                        self.pos = duration.unwrap_or(target);
                        at_end = true;
                    }
                    Err(e) if is_interrupted(&e) => {
                        // A newer seek or cancellation arrived; the next loop handles it.
                        self.shared.lock().seek.get_or_insert(target);
                        continue;
                    }
                    Err(SymphoniaError::SeekError(SeekErrorKind::Unseekable | SeekErrorKind::ForwardOnly)) => {}
                    Err(e) => return Err(format!("seek failed: {e}")),
                }
                continue;
            }
            if at_end {
                // Stay alive so a seek back still works.
                let state = self.shared.lock();
                if state.seek.is_none() && !self.shared.cancelled() {
                    let _ = self.shared.changed.wait_timeout(state, Duration::from_millis(200));
                }
                continue;
            }
            let wanted = self.shared.speed();
            if wanted != speed {
                speed = wanted;
                resample.set_speed(speed);
            }
            let step = (|| -> Result<Step, String> {
                let packet = match format.next_packet() {
                    Ok(packet) => packet,
                    Err(e) if is_interrupted(&e) => return Ok(Step::Continue),
                    Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(Step::End),
                    Err(SymphoniaError::ResetRequired) => {
                        // A new logical stream (chained Ogg radio).
                        let (_, params) = default_track(format.as_ref()).ok_or("no audio track")?;
                        decoder = make_decoder(&params)?;
                        return Ok(Step::Continue);
                    }
                    Err(e) => return Err(e.to_string()),
                };
                if packet.track_id() != track_id {
                    return Ok(Step::Continue);
                }
                let decoded = match decoder.decode(&packet) {
                    Ok(decoded) => decoded,
                    Err(SymphoniaError::DecodeError(message)) => {
                        decode_errors += 1;
                        if decode_errors > 100 {
                            return Err(format!("too many decoding errors: {message}"));
                        }
                        return Ok(Step::Continue);
                    }
                    Err(e) if is_interrupted(&e) => return Ok(Step::Continue),
                    Err(e) => return Err(e.to_string()),
                };
                decode_errors = 0;
                let spec = *decoded.spec();
                let frames = decoded.frames();
                if !reported_format {
                    // Containers such as MP4 may only reveal the layout once decoding starts.
                    reported_format = true;
                    if let Some(details) = &mut self.shared.lock().details {
                        details.channels = details.channels.or(Some(spec.channels.count() as u32));
                        details.sample_rate = details.sample_rate.or(Some(spec.rate));
                    }
                }
                if frames == 0 {
                    return Ok(Step::Continue);
                }
                let needed = decoded.capacity().max(frames) * spec.channels.count();
                if sample_buffer.as_ref().is_none_or(|b| b.capacity() < needed) {
                    sample_buffer = Some(SampleBuffer::new(decoded.capacity().max(frames) as u64, spec));
                }
                let buffer = sample_buffer.as_mut().expect("allocated");
                buffer.copy_interleaved_ref(decoded);
                let source_channels = spec.channels.count().max(1);
                let mut samples = buffer.samples();
                if skip_frames > 0 {
                    let skip = skip_frames.min(frames as u64) as usize;
                    skip_frames -= skip as u64;
                    samples = &samples[skip * source_channels..];
                }
                if spec.rate != resample.in_rate() {
                    resampled.clear();
                    resample.finish(&mut resampled);
                    resample = Resample::new(spec.rate, self.out_rate, self.out_channels, speed);
                    self.push(&mut resampled, speed);
                }
                mapped.clear();
                map_channels(samples, source_channels, self.out_channels, &mut mapped);
                resampled.clear();
                resample.push(&mapped, &mut resampled);
                self.push(&mut resampled, speed);
                Ok(Step::Continue)
            })()?;
            if let Step::End = step {
                resampled.clear();
                resample.finish(&mut resampled);
                self.push(&mut resampled, speed);
                let mut state = self.shared.lock();
                if state.seek.is_none() {
                    state.finished = true;
                }
                drop(state);
                self.shared.changed.notify_all();
                at_end = true;
            }
        }
    }

    /// Queues converted samples, waiting while the queue is full. Drops them if a seek arrives.
    fn push(&mut self, samples: &mut Vec<f32>, speed: f32) {
        if samples.is_empty() {
            return;
        }
        let frames = samples.len() / self.out_channels;
        let limit = (QUEUE_SECONDS * f64::from(self.out_rate)) as usize;
        let mut state = self.shared.lock();
        loop {
            if self.shared.cancelled() || state.seek.is_some() {
                samples.clear();
                return;
            }
            if state.buffered < limit {
                break;
            }
            state = self.shared.changed.wait_timeout(state, Duration::from_millis(100)).map(|r| r.0).unwrap_or_else(|e| e.into_inner().0);
        }
        state.chunks.push_back(Chunk { samples: std::mem::take(samples), read: 0, pos: self.pos, speed });
        state.buffered += frames;
        drop(state);
        self.pos += frames as f64 / f64::from(self.out_rate) * f64::from(speed);
    }
}

fn default_track(format: &dyn FormatReader) -> Option<(u32, CodecParameters)> {
    let track = format
        .default_track()
        .filter(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .or_else(|| format.tracks().iter().find(|t| t.codec_params.codec != CODEC_TYPE_NULL))?;
    Some((track.id, track.codec_params.clone()))
}

fn make_decoder(params: &CodecParameters) -> Result<Box<dyn Decoder>, String> {
    symphonia::default::get_codecs().make(params, &DecoderOptions::default()).map_err(|e| format!("unsupported codec: {e}"))
}

fn duration_of(params: &CodecParameters) -> Option<f64> {
    let frames = params.n_frames?;
    let seconds = match params.time_base {
        Some(base) => {
            let t = base.calc_time(frames);
            t.seconds as f64 + t.frac
        }
        None => frames as f64 / f64::from(params.sample_rate?),
    };
    (seconds.is_finite() && seconds > 0.0).then_some(seconds)
}

fn codec_name(short: &str) -> String {
    match short {
        "vorbis" => "Vorbis".into(),
        "opus" => "Opus".into(),
        name if name.starts_with("pcm") => "PCM".into(),
        name => name.to_uppercase(),
    }
}

fn extension(path: &str) -> Option<String> {
    let name = path.rsplit('/').next()?;
    let (_, ext) = name.rsplit_once('.')?;
    (!ext.is_empty() && ext.len() <= 5).then(|| ext.to_ascii_lowercase())
}

fn open(item: &MediaItem, shared: &Shared) -> Result<Opened, String> {
    match &item.source {
        AudioSource::File(path) => open_file(path, item),
        AudioSource::Url(url) => {
            if let Some(path) = url.strip_prefix("file://") {
                let path = url::Url::parse(url).ok().and_then(|u| u.to_file_path().ok()).unwrap_or_else(|| path.into());
                return open_file(&path, item);
            }
            open_url(url, item, shared, 0)
        }
    }
}

fn open_file(path: &std::path::Path, item: &MediaItem) -> Result<Opened, String> {
    let file = File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let byte_len = file.metadata().ok().map(|m| m.len());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(&ext.to_ascii_lowercase());
    } else if let Some(format) = &item.server_format {
        hint.with_extension(&format.to_ascii_lowercase());
    }
    Ok(Opened { source: Box::new(file), hint, streamed: false, byte_len })
}

fn is_hls(content_type: &str, ext: Option<&str>, body: Option<&str>) -> bool {
    content_type.contains("vnd.apple.mpegurl")
        || content_type.contains("application/x-mpegurl")
        || ext == Some("m3u8")
        || body.is_some_and(|b| b.contains("#EXT-X-"))
}

fn is_playlist(content_type: &str, ext: Option<&str>) -> bool {
    matches!(ext, Some("m3u" | "m3u8" | "pls")) || ["mpegurl", "scpls", "x-pls"].iter().any(|t| content_type.contains(t))
}

/// The first stream URL listed in an M3U or PLS playlist.
pub fn playlist_entry(text: &str) -> Option<String> {
    text.lines().map(str::trim).find_map(|line| {
        let value = if line.to_ascii_lowercase().starts_with("file") {
            line.split_once('=').map(|(_, v)| v.trim())?
        } else if line.starts_with('#') || line.is_empty() {
            return None;
        } else {
            line
        };
        (value.starts_with("http://") || value.starts_with("https://")).then(|| value.to_string())
    })
}

fn open_url(url: &str, item: &MediaItem, shared: &Shared, depth: usize) -> Result<Opened, String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("unsupported URL scheme".into());
    }
    let live = item.kind == ContentKind::Radio;
    let store = Store::open(Box::new(HttpFetcher::new(url.to_string())), live);
    let headers = store.wait_headers(&shared.interrupt, HEADER_TIMEOUT).map_err(|e| e.to_string())?;
    let content_type = headers.content_type.clone().unwrap_or_default().to_ascii_lowercase();
    let path = url::Url::parse(url).map(|u| u.path().to_string()).unwrap_or_default();
    let ext = extension(&path);
    let mut reader = StoreReader::new(store.clone(), shared.interrupt.clone(), shared.waiting.clone());
    if is_hls(&content_type, ext.as_deref(), None) && !is_playlist_body_plain(&mut reader)? {
        return Err("HLS streams are not supported by the desktop player".into());
    }
    if is_playlist(&content_type, ext.as_deref()) || content_type.starts_with("text/") {
        let mut text = Vec::new();
        (&mut reader).take(MAX_PLAYLIST_BYTES as u64).read_to_end(&mut text).map_err(|e| e.to_string())?;
        let text = String::from_utf8_lossy(&text);
        if is_hls(&content_type, ext.as_deref(), Some(&text)) {
            return Err("HLS streams are not supported by the desktop player".into());
        }
        let entry = playlist_entry(&text).ok_or("the playlist lists no stream")?;
        if depth >= 3 {
            return Err("too many nested playlists".into());
        }
        return open_url(&entry, item, shared, depth + 1);
    }
    let mut hint = Hint::new();
    if let Some(mime) = content_type.split(';').next().filter(|m| !m.is_empty()) {
        hint.mime_type(mime);
    }
    if let Some(ext) = ext.clone().or_else(|| item.server_format.as_ref().map(|f| f.to_ascii_lowercase())) {
        hint.with_extension(&ext);
    }
    Ok(Opened { source: Box::new(reader), hint, streamed: true, byte_len: headers.total_len })
}

/// An `.m3u8` URL may still be a plain M3U radio playlist; HLS ones contain `#EXT-X-` tags.
fn is_playlist_body_plain(reader: &mut StoreReader) -> Result<bool, String> {
    use std::io::{Seek, SeekFrom};
    let mut text = Vec::new();
    reader.take(MAX_PLAYLIST_BYTES as u64).read_to_end(&mut text).map_err(|e| e.to_string())?;
    reader.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    Ok(!String::from_utf8_lossy(&text).contains("#EXT-X-"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playlist_parsing() {
        assert_eq!(playlist_entry("#EXTM3U\n#EXTINF:-1,Radio\nhttp://a/stream\n"), Some("http://a/stream".into()));
        assert_eq!(
            playlist_entry("[playlist]\nNumberOfEntries=1\nFile1=https://b/live.mp3\nTitle1=B\n"),
            Some("https://b/live.mp3".into())
        );
        assert_eq!(playlist_entry("#EXTM3U\n"), None);
    }

    #[test]
    fn hls_detection() {
        assert!(is_hls("application/vnd.apple.mpegurl", None, None));
        assert!(is_hls("", Some("m3u8"), None));
        assert!(is_hls("audio/x-mpegurl", None, Some("#EXTM3U\n#EXT-X-TARGETDURATION:6\n")));
        assert!(!is_hls("audio/x-mpegurl", Some("m3u"), Some("#EXTM3U\nhttp://a\n")));
        assert!(is_playlist("audio/x-scpls", None));
        assert!(!is_playlist("audio/mpeg", Some("mp3")));
    }

    #[test]
    fn decodes_a_wav_file_to_the_device_rate() {
        // 0.5 s of 22.05 kHz mono 16-bit silence-with-a-ramp, resampled to 48 kHz stereo.
        let rate = 22_050u32;
        let frames = rate / 2;
        let mut wav = Vec::new();
        let data_len = frames * 2;
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_len).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&rate.to_le_bytes());
        wav.extend_from_slice(&(rate * 2).to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_len.to_le_bytes());
        for i in 0..frames {
            wav.extend_from_slice(&(((i % 100) as i16) * 100).to_le_bytes());
        }
        let path = std::env::temp_dir().join(format!("discophile-test-{}.wav", std::process::id()));
        std::fs::write(&path, &wav).unwrap();
        let item = MediaItem {
            id: "wav".into(),
            source: AudioSource::File(path.clone()),
            kind: ContentKind::Music,
            duration_hint: None,
            replay_gain: None,
            album_key: None,
            server_format: None,
        };
        let mut voice = Voice::spawn(&item, 0.0, 48_000, 2, 1.0);
        let mut total = 0;
        let mut buf = vec![0.0f32; 4096];
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            let read = voice.read(&mut buf);
            total += read.frames;
            if read.frames == 0 {
                match read.state {
                    SourceState::Finished => break,
                    SourceState::Failed(e) => panic!("{e}"),
                    SourceState::Starved => std::thread::sleep(Duration::from_millis(5)),
                }
            }
            assert!(std::time::Instant::now() < deadline);
        }
        assert_eq!(total, 24_000);
        let details = voice.details().unwrap();
        assert_eq!(details.sample_rate, Some(rate));
        assert_eq!(details.channels, Some(1));
        assert_eq!(details.bit_depth, Some(16));
        assert!(!details.streamed);
        assert!((voice.duration().unwrap() - 0.5).abs() < 1e-6);
        // Seeking after the end still works.
        voice.seek(0.25);
        let mut after = 0;
        loop {
            let read = voice.read(&mut buf);
            after += read.frames;
            if read.frames == 0 {
                match read.state {
                    SourceState::Finished => break,
                    SourceState::Failed(e) => panic!("{e}"),
                    SourceState::Starved => std::thread::sleep(Duration::from_millis(5)),
                }
            }
            assert!(std::time::Instant::now() < deadline);
        }
        assert!((after as i64 - 12_000).abs() <= 2, "{after}");
        let _ = std::fs::remove_file(path);
    }
}
