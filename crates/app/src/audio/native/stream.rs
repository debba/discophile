//! Progressive download of a stream into memory, read by the decoder as a seekable file.
//!
//! The download fills sorted, non-overlapping byte segments. Reads block until their bytes arrive.
//! A read far outside the downloaded range restarts the download there with an HTTP `Range` request
//! when the server supports it; otherwise it waits for the single download to get there. Live radio
//! keeps only a window behind the read position. With range support, memory stays bounded for long
//! files (audiobooks): the download pauses well ahead of the reader and old data is dropped.

use std::io::{self, Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};
use std::time::Duration;

/// A read this far beyond the downloaded end waits instead of starting a new request.
const WAIT_AHEAD: u64 = 512 * 1024;
/// Live streams keep this much already-read data.
const LIVE_KEEP: usize = 1024 * 1024;
const MAX_SEGMENTS: usize = 16;

/// Memory bounds for seekable streams on servers that accept ranges.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// The download pauses this far ahead of the reader, and resumes at half of it.
    pub ahead: u64,
    /// Data kept behind the reader once the total exceeds `max_total`.
    pub keep_behind: u64,
    pub max_total: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self { ahead: 32 << 20, keep_behind: 8 << 20, max_total: 64 << 20 }
    }
}
pub const INTERRUPTED: &str = "read interrupted";

/// What the response told us about the resource.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HeaderInfo {
    pub total_len: Option<u64>,
    pub accepts_ranges: bool,
    pub content_type: Option<String>,
}

/// Starts delivering the resource's bytes from `offset` into `sink`, without blocking.
pub trait Fetcher: Send + Sync + 'static {
    fn fetch(&self, offset: u64, sink: Sink);
}

struct Segment {
    start: u64,
    data: Vec<u8>,
}

impl Segment {
    fn end(&self) -> u64 {
        self.start + self.data.len() as u64
    }
}

#[derive(Default)]
struct State {
    segments: Vec<Segment>,
    /// Start of the segment the running fetch appends to.
    active: Option<u64>,
    generation: u64,
    fetching: bool,
    headers: Option<HeaderInfo>,
    error: Option<String>,
    read_pos: u64,
}

impl State {
    fn segment_index(&self, start: u64) -> Option<usize> {
        self.segments.iter().position(|s| s.start == start)
    }

    fn covering(&self, pos: u64) -> Option<&Segment> {
        self.segments.iter().find(|s| s.start <= pos && pos < s.end())
    }

    fn total_len(&self) -> Option<u64> {
        self.headers.as_ref().and_then(|h| h.total_len)
    }

    fn accepts_ranges(&self) -> bool {
        self.headers.as_ref().is_some_and(|h| h.accepts_ranges && h.total_len.is_some())
    }

    /// Drops whole segments away from `pos`, then data far behind it, until under the limit.
    fn evict(&mut self, pos: u64, limits: &Limits) {
        let total = |segments: &[Segment]| segments.iter().map(|s| s.data.len() as u64).sum::<u64>();
        while total(&self.segments) > limits.max_total {
            let candidate = (0..self.segments.len())
                .filter(|&i| {
                    let s = &self.segments[i];
                    !(s.start <= pos && pos <= s.end()) && self.active != Some(s.start)
                })
                .max_by_key(|&i| self.segments[i].start.abs_diff(pos));
            match candidate {
                Some(index) => {
                    self.segments.remove(index);
                }
                None => break,
            }
        }
        if total(&self.segments) > limits.max_total
            && let Some(index) = self.segments.iter().position(|s| s.start <= pos && pos <= s.end())
        {
            let start = self.segments[index].start;
            let drop = pos.saturating_sub(limits.keep_behind).saturating_sub(start) as usize;
            if drop > 0 {
                let segment = &mut self.segments[index];
                segment.data.drain(..drop);
                segment.start += drop as u64;
                if self.active == Some(start) {
                    self.active = Some(segment.start);
                }
            }
        }
    }
}

pub struct Store {
    state: Mutex<State>,
    changed: Condvar,
    fetcher: Box<dyn Fetcher>,
    live: bool,
    limits: Limits,
}

/// Where a fetch delivers bytes. Stale sinks (after a restart or when the reader is gone) refuse data.
pub struct Sink {
    store: Weak<Store>,
    generation: u64,
}

impl Sink {
    /// Reports the response headers. Returns false when the fetch should stop.
    pub fn headers(&self, info: HeaderInfo) -> bool {
        let Some(store) = self.store.upgrade() else { return false };
        let mut state = store.lock();
        if state.generation != self.generation {
            return false;
        }
        match &mut state.headers {
            None => state.headers = Some(info),
            Some(known) => {
                known.total_len = known.total_len.or(info.total_len);
                known.accepts_ranges = info.accepts_ranges;
            }
        }
        store.changed.notify_all();
        true
    }

    /// Appends bytes. Returns false when the fetch should stop.
    pub fn push(&self, data: &[u8]) -> bool {
        let Some(store) = self.store.upgrade() else { return false };
        let restart = {
            let mut state = store.lock();
            if state.generation != self.generation {
                return false;
            }
            let Some(index) = state.active.and_then(|start| state.segment_index(start)) else { return false };
            state.segments[index].data.extend_from_slice(data);
            // Joining the following segment: continue after it with a new request.
            let mut restart = None;
            if index + 1 < state.segments.len() && state.segments[index].end() >= state.segments[index + 1].start {
                let next = state.segments.remove(index + 1);
                let overlap = (state.segments[index].end() - next.start) as usize;
                if overlap < next.data.len() {
                    state.segments[index].data.extend_from_slice(&next.data[overlap..]);
                }
                let end = state.segments[index].end();
                if state.total_len().is_some_and(|total| end >= total) {
                    state.fetching = false;
                    state.generation += 1;
                    state.active = None;
                } else {
                    restart = Some(end);
                }
            } else if !store.live && state.accepts_ranges() && state.segments[index].end() > state.read_pos + store.limits.ahead {
                // Far enough ahead: pause; the reader resumes the download when it gets closer.
                state.fetching = false;
                state.generation += 1;
                state.active = None;
                store.changed.notify_all();
                return false;
            }
            store.changed.notify_all();
            restart
        };
        match restart {
            Some(end) => {
                store.start_fetch(end);
                false
            }
            None => true,
        }
    }

    /// The response ended normally.
    pub fn finish(&self) {
        let Some(store) = self.store.upgrade() else { return };
        let mut state = store.lock();
        if state.generation != self.generation {
            return;
        }
        state.fetching = false;
        // A response that ends without a known length defines it.
        let end = state.active.and_then(|start| state.segment_index(start)).map(|i| state.segments[i].end());
        if let (Some(end), Some(headers)) = (end, state.headers.as_mut())
            && headers.total_len.is_none()
        {
            headers.total_len = Some(end);
        }
        store.changed.notify_all();
    }

    pub fn fail(&self, message: String) {
        let Some(store) = self.store.upgrade() else { return };
        let mut state = store.lock();
        if state.generation != self.generation {
            return;
        }
        state.fetching = false;
        state.error = Some(message);
        store.changed.notify_all();
    }
}

impl Store {
    /// Starts downloading from the beginning.
    pub fn open(fetcher: Box<dyn Fetcher>, live: bool) -> Arc<Self> {
        Self::with_limits(fetcher, live, Limits::default())
    }

    pub fn with_limits(fetcher: Box<dyn Fetcher>, live: bool, limits: Limits) -> Arc<Self> {
        let store = Arc::new(Self { state: Mutex::new(State::default()), changed: Condvar::new(), fetcher, live, limits });
        store.start_fetch(0);
        store
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn start_fetch(self: &Arc<Self>, offset: u64) {
        let sink = {
            let mut state = self.lock();
            state.generation += 1;
            state.error = None;
            state.fetching = true;
            // Continue a segment that ends here, or open a new one.
            let start = match state.segments.iter().find(|s| s.end() == offset && !s.data.is_empty()) {
                Some(segment) => segment.start,
                None => {
                    if state.segments.len() >= MAX_SEGMENTS {
                        // Drop the segment farthest from the new position.
                        if let Some(far) = (0..state.segments.len()).max_by_key(|&i| state.segments[i].start.abs_diff(offset)) {
                            state.segments.remove(far);
                        }
                    }
                    if let Some(existing) = state.segment_index(offset) {
                        state.segments[existing].data.clear();
                    } else {
                        let at = state.segments.iter().position(|s| s.start > offset).unwrap_or(state.segments.len());
                        state.segments.insert(at, Segment { start: offset, data: Vec::new() });
                    }
                    offset
                }
            };
            state.active = Some(start);
            Sink { store: Arc::downgrade(self), generation: state.generation }
        };
        self.fetcher.fetch(offset, sink);
    }

    /// Waits for the first response headers.
    pub fn wait_headers(&self, interrupt: &AtomicBool, timeout: Duration) -> io::Result<HeaderInfo> {
        let deadline = std::time::Instant::now() + timeout;
        let mut state = self.lock();
        loop {
            if let Some(headers) = &state.headers {
                return Ok(headers.clone());
            }
            if let Some(error) = &state.error {
                return Err(io::Error::other(error.clone()));
            }
            if interrupt.load(Ordering::Relaxed) {
                return Err(io::Error::other(INTERRUPTED));
            }
            let now = std::time::Instant::now();
            if now >= deadline {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "the server did not answer in time"));
            }
            state = self.changed.wait_timeout(state, Duration::from_millis(100)).map(|r| r.0).unwrap_or_else(|e| e.into_inner().0);
        }
    }

    pub fn total_len(&self) -> Option<u64> {
        self.lock().total_len()
    }
}

/// A blocking `Read + Seek` view of a [`Store`].
pub struct StoreReader {
    store: Arc<Store>,
    pos: u64,
    interrupt: Arc<AtomicBool>,
    waiting: Arc<AtomicBool>,
    retried: bool,
}

impl StoreReader {
    pub fn new(store: Arc<Store>, interrupt: Arc<AtomicBool>, waiting: Arc<AtomicBool>) -> Self {
        Self { store, pos: 0, interrupt, waiting, retried: false }
    }

    #[cfg(test)]
    pub fn store(&self) -> &Arc<Store> {
        &self.store
    }
}

enum Next {
    Wait,
    Restart,
}

impl Read for StoreReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let store = self.store.clone();
        let mut state = store.lock();
        loop {
            if self.interrupt.load(Ordering::Relaxed) {
                self.waiting.store(false, Ordering::Relaxed);
                return Err(io::Error::other(INTERRUPTED));
            }
            if let Some(segment) = state.covering(self.pos) {
                let offset = (self.pos - segment.start) as usize;
                let count = buf.len().min(segment.data.len() - offset);
                buf[..count].copy_from_slice(&segment.data[offset..offset + count]);
                self.pos += count as u64;
                if store.live && offset > 2 * LIVE_KEEP {
                    let start = segment.start;
                    if let Some(index) = state.segment_index(start) {
                        let drop = offset - LIVE_KEEP;
                        let segment = &mut state.segments[index];
                        segment.data.drain(..drop);
                        segment.start += drop as u64;
                        if state.active == Some(start) {
                            state.active = Some(start + drop as u64);
                        }
                    }
                }
                self.waiting.store(false, Ordering::Relaxed);
                self.retried = false;
                state.read_pos = self.pos;
                let mut resume = None;
                if !store.live && state.accepts_ranges() {
                    state.evict(self.pos, &store.limits);
                    let end = state.covering(self.pos.saturating_sub(1)).or_else(|| state.covering(self.pos)).map(Segment::end);
                    if let Some(end) = end
                        && !state.fetching
                        && state.error.is_none()
                        && state.total_len().is_some_and(|total| end < total)
                        && end - self.pos < store.limits.ahead / 2
                    {
                        resume = Some(end);
                    }
                }
                drop(state);
                if let Some(end) = resume {
                    store.start_fetch(end);
                }
                return Ok(count);
            }
            if state.total_len().is_some_and(|total| self.pos >= total) {
                self.waiting.store(false, Ordering::Relaxed);
                return Ok(0);
            }
            let accepts_ranges = !store.live && state.headers.as_ref().is_some_and(|h| h.accepts_ranges);
            let active_end = state.active.and_then(|start| state.segment_index(start)).map(|i| state.segments[i].end());
            let next = if state.fetching {
                match active_end {
                    Some(end) if self.pos >= end && self.pos - end <= WAIT_AHEAD => Next::Wait,
                    _ if state.headers.is_none() => Next::Wait,
                    _ if accepts_ranges => Next::Restart,
                    Some(end) if self.pos >= end => Next::Wait,
                    _ => return Err(io::Error::new(io::ErrorKind::Unsupported, "this stream cannot seek back")),
                }
            } else if let Some(error) = &state.error {
                if accepts_ranges && !self.retried {
                    // One retry from the read position after a dropped connection.
                    self.retried = true;
                    Next::Restart
                } else {
                    self.waiting.store(false, Ordering::Relaxed);
                    return Err(io::Error::other(error.clone()));
                }
            } else if accepts_ranges {
                Next::Restart
            } else if active_end.is_none_or(|end| self.pos >= end) {
                // The only download finished before reaching this position.
                return Ok(0);
            } else {
                return Err(io::Error::new(io::ErrorKind::Unsupported, "this part of the stream is no longer available"));
            };
            match next {
                Next::Wait => {
                    self.waiting.store(true, Ordering::Relaxed);
                    state = store.changed.wait_timeout(state, Duration::from_millis(100)).map(|r| r.0).unwrap_or_else(|e| e.into_inner().0);
                }
                Next::Restart => {
                    drop(state);
                    store.start_fetch(self.pos);
                    state = store.lock();
                }
            }
        }
    }
}

impl Seek for StoreReader {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let target = match pos {
            SeekFrom::Start(offset) => Some(offset),
            SeekFrom::Current(delta) => self.pos.checked_add_signed(delta),
            SeekFrom::End(delta) => {
                let total = self.store.total_len().ok_or_else(|| io::Error::new(io::ErrorKind::Unsupported, "unknown stream length"))?;
                total.checked_add_signed(delta)
            }
        };
        self.pos = target.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "seek before the start"))?;
        Ok(self.pos)
    }
}

impl symphonia::core::io::MediaSource for StoreReader {
    fn is_seekable(&self) -> bool {
        !self.store.live
    }

    fn byte_len(&self) -> Option<u64> {
        self.store.total_len()
    }
}

/// Fetches over HTTP on the shared tokio runtime.
pub struct HttpFetcher {
    client: reqwest::Client,
    url: String,
}

impl HttpFetcher {
    pub fn new(url: String) -> Self {
        Self { client: http_client(), url }
    }
}

pub fn http_client() -> reqwest::Client {
    use std::sync::OnceLock;
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .no_gzip()
                .redirect(reqwest::redirect::Policy::limited(5))
                .connect_timeout(Duration::from_secs(15))
                .read_timeout(Duration::from_secs(30))
                .user_agent(concat!("Discophile/", env!("CARGO_PKG_VERSION")))
                .build()
                .unwrap_or_default()
        })
        .clone()
}

impl Fetcher for HttpFetcher {
    fn fetch(&self, offset: u64, sink: Sink) {
        let mut request = self.client.get(&self.url);
        if offset > 0 {
            request = request.header(reqwest::header::RANGE, format!("bytes={offset}-"));
        }
        crate::platform::runtime().spawn(async move {
            let mut response = match request.send().await {
                Ok(response) => response,
                Err(error) => return sink.fail(describe(&error)),
            };
            let status = response.status();
            if !status.is_success() {
                return sink.fail(format!("HTTP {}", status.as_u16()));
            }
            let partial = status == reqwest::StatusCode::PARTIAL_CONTENT;
            let headers = response.headers();
            let text = |name: reqwest::header::HeaderName| headers.get(name).and_then(|v| v.to_str().ok()).map(str::to_string);
            let total_len = if partial {
                text(reqwest::header::CONTENT_RANGE).and_then(|range| range.rsplit('/').next().and_then(|t| t.trim().parse().ok()))
            } else {
                response.content_length().filter(|len| *len > 0)
            };
            let accepts_ranges = partial || text(reqwest::header::ACCEPT_RANGES).is_some_and(|v| v.eq_ignore_ascii_case("bytes"));
            let content_type = text(reqwest::header::CONTENT_TYPE);
            if !sink.headers(HeaderInfo { total_len, accepts_ranges, content_type }) {
                return;
            }
            // A server that ignored the range sends the whole body again.
            let mut skip = if offset > 0 && !partial { offset } else { 0 };
            loop {
                match response.chunk().await {
                    Ok(Some(bytes)) => {
                        let mut data = &bytes[..];
                        if skip > 0 {
                            let dropped = skip.min(data.len() as u64);
                            skip -= dropped;
                            data = &data[dropped as usize..];
                        }
                        if !data.is_empty() && !sink.push(data) {
                            return;
                        }
                    }
                    Ok(None) => return sink.finish(),
                    Err(error) => return sink.fail(describe(&error)),
                }
            }
        });
    }
}

fn describe(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        "the connection timed out".into()
    } else if error.is_connect() {
        "could not connect to the server".into()
    } else {
        error.to_string()
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::io::Read;

    /// Serves a byte vector in small pieces from a thread, optionally honouring ranges.
    pub struct MemoryFetcher {
        pub data: Arc<Vec<u8>>,
        pub ranges: bool,
        pub known_length: bool,
        pub piece: usize,
        pub delay: Duration,
        pub requests: Arc<Mutex<Vec<u64>>>,
    }

    impl MemoryFetcher {
        pub fn new(data: Vec<u8>, ranges: bool) -> Self {
            Self {
                data: Arc::new(data),
                ranges,
                known_length: true,
                piece: 1000,
                delay: Duration::from_micros(200),
                requests: Arc::default(),
            }
        }
    }

    impl Fetcher for MemoryFetcher {
        fn fetch(&self, offset: u64, sink: Sink) {
            self.requests.lock().unwrap().push(offset);
            let (data, ranges, piece, delay, known) = (self.data.clone(), self.ranges, self.piece, self.delay, self.known_length);
            std::thread::spawn(move || {
                let start = if ranges { offset as usize } else { 0 };
                let total = known.then_some(data.len() as u64);
                if !sink.headers(HeaderInfo { total_len: total, accepts_ranges: ranges, content_type: None }) {
                    return;
                }
                let mut skip = if ranges { 0 } else { offset as usize };
                let mut at = start;
                while at < data.len() {
                    let end = (at + piece).min(data.len());
                    let mut slice = &data[at..end];
                    let dropped = skip.min(slice.len());
                    skip -= dropped;
                    slice = &slice[dropped..];
                    if !slice.is_empty() && !sink.push(slice) {
                        return;
                    }
                    at = end;
                    std::thread::sleep(delay);
                }
                sink.finish();
            });
        }
    }

    fn bytes(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i % 251) as u8).collect()
    }

    fn reader(fetcher: MemoryFetcher, live: bool) -> StoreReader {
        let store = Store::open(Box::new(fetcher), live);
        StoreReader::new(store, Arc::default(), Arc::default())
    }

    #[test]
    fn reads_whole_stream_while_downloading() {
        let data = bytes(50_000);
        let mut reader = reader(MemoryFetcher::new(data.clone(), false), false);
        let mut out = Vec::new();
        reader.read_to_end(&mut out).unwrap();
        assert_eq!(out, data);
    }

    #[test]
    fn far_seek_uses_range_request() {
        let data = bytes(5_000_000);
        let mut fetcher = MemoryFetcher::new(data.clone(), true);
        fetcher.delay = Duration::from_millis(2);
        let requests = fetcher.requests.clone();
        let mut reader = reader(fetcher, false);
        let mut buf = [0u8; 100];
        reader.read_exact(&mut buf).unwrap();
        reader.seek(SeekFrom::Start(4_000_000)).unwrap();
        reader.read_exact(&mut buf).unwrap();
        assert_eq!(&buf[..], &data[4_000_000..4_000_100]);
        assert!(requests.lock().unwrap().contains(&4_000_000));
        // Back to the beginning: already downloaded, no new request needed.
        let count = requests.lock().unwrap().len();
        reader.seek(SeekFrom::Start(10)).unwrap();
        reader.read_exact(&mut buf).unwrap();
        assert_eq!(&buf[..], &data[10..110]);
        assert_eq!(requests.lock().unwrap().len(), count);
    }

    #[test]
    fn seek_to_end_and_back_reads_correct_bytes() {
        let data = bytes(2_000_000);
        let mut reader = reader(MemoryFetcher::new(data.clone(), true), false);
        reader.seek(SeekFrom::End(-1000)).unwrap_or_else(|_| {
            // Length unknown until headers: read one byte first.
            let mut one = [0u8; 1];
            reader.read_exact(&mut one).unwrap();
            reader.seek(SeekFrom::End(-1000)).unwrap()
        });
        let mut tail = Vec::new();
        reader.read_to_end(&mut tail).unwrap();
        assert_eq!(tail, &data[data.len() - 1000..]);
        reader.seek(SeekFrom::Start(0)).unwrap();
        let mut all = Vec::new();
        reader.read_to_end(&mut all).unwrap();
        assert_eq!(all, data);
    }

    #[test]
    fn without_ranges_forward_seek_waits_for_download() {
        let data = bytes(3_000_000);
        let fetcher = MemoryFetcher::new(data.clone(), false);
        let requests = fetcher.requests.clone();
        let mut reader = reader(fetcher, false);
        reader.seek(SeekFrom::Start(2_500_000)).unwrap();
        let mut buf = [0u8; 64];
        reader.read_exact(&mut buf).unwrap();
        assert_eq!(&buf[..], &data[2_500_000..2_500_064]);
        assert_eq!(requests.lock().unwrap().len(), 1);
    }

    #[test]
    fn unknown_length_ends_at_download_end() {
        let data = bytes(10_000);
        let mut fetcher = MemoryFetcher::new(data.clone(), false);
        fetcher.known_length = false;
        let mut reader = reader(fetcher, false);
        let mut out = Vec::new();
        reader.read_to_end(&mut out).unwrap();
        assert_eq!(out.len(), data.len());
        assert_eq!(reader.store().total_len(), Some(10_000));
    }

    #[test]
    fn interrupt_unblocks_waiting_read() {
        let data = bytes(1_000);
        let mut fetcher = MemoryFetcher::new(data, false);
        fetcher.delay = Duration::from_secs(5);
        fetcher.piece = 10;
        let store = Store::open(Box::new(fetcher), false);
        let interrupt = Arc::new(AtomicBool::new(false));
        let mut reader = StoreReader::new(store, interrupt.clone(), Arc::default());
        let flag = interrupt.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            flag.store(true, Ordering::Relaxed);
        });
        let mut buf = [0u8; 100];
        let mut first = [0u8; 10];
        reader.read_exact(&mut first).unwrap();
        let error = reader.read(&mut buf).unwrap_err();
        assert_eq!(error.to_string(), INTERRUPTED);
    }

    #[test]
    fn long_streams_stay_within_memory_limits() {
        let data = bytes(3_000_000);
        let mut fetcher = MemoryFetcher::new(data.clone(), true);
        fetcher.piece = 16 * 1024;
        let requests = fetcher.requests.clone();
        let limits = Limits { ahead: 256 * 1024, keep_behind: 64 * 1024, max_total: 512 * 1024 };
        let store = Store::with_limits(Box::new(fetcher), false, limits);
        let mut reader = StoreReader::new(store.clone(), Arc::default(), Arc::default());
        let mut out = Vec::new();
        let mut buf = [0u8; 4096];
        // A slow reader: the download stops about `ahead` bytes in front of it.
        reader.read_exact(&mut buf).unwrap();
        out.extend_from_slice(&buf);
        std::thread::sleep(Duration::from_millis(300));
        let held: usize = store.lock().segments.iter().map(|s| s.data.len()).sum();
        assert!(held <= 256 * 1024 + 4096 + 2 * 16 * 1024, "downloaded {held} bytes ahead");
        let mut peak = 0;
        loop {
            let n = reader.read(&mut buf).unwrap();
            if n == 0 {
                break;
            }
            out.extend_from_slice(&buf[..n]);
            let held: usize = store.lock().segments.iter().map(|s| s.data.len()).sum();
            peak = peak.max(held);
        }
        assert_eq!(out, data);
        assert!(peak < 1_000_000, "held {peak} bytes");
        // Paused once ahead of the slow start, then resumed.
        assert_eq!(requests.lock().unwrap().len(), 2);
        // Seeking back to evicted data downloads it again.
        reader.seek(SeekFrom::Start(100)).unwrap();
        reader.read_exact(&mut buf).unwrap();
        assert_eq!(&buf[..], &data[100..4196]);
        assert_eq!(requests.lock().unwrap().last(), Some(&100));
    }

    #[test]
    fn live_streams_drop_old_data() {
        let data = bytes(6 * LIVE_KEEP);
        let mut fetcher = MemoryFetcher::new(data.clone(), false);
        fetcher.known_length = false;
        fetcher.piece = 64 * 1024;
        let store = Store::open(Box::new(fetcher), true);
        let mut reader = StoreReader::new(store.clone(), Arc::default(), Arc::default());
        let mut out = Vec::new();
        reader.read_to_end(&mut out).unwrap();
        assert_eq!(out, data);
        let kept: usize = store.lock().segments.iter().map(|s| s.data.len()).sum();
        assert!(kept <= 3 * LIVE_KEEP, "kept {kept}");
        reader.seek(SeekFrom::Start(0)).unwrap();
        assert!(reader.read(&mut [0u8; 10]).is_err());
    }
}
