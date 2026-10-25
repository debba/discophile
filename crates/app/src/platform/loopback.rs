//! The loopback listener of a browser sign-in (ChatGPT, Discophile Connect). It runs on its own thread,
//! reads requests one at a time with a short timeout, and closes anything that is not the exact OAuth
//! redirect without a reply.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use discophile_core::oauth;
use url::Url;

/// How waiting ended.
pub enum Wait<T> {
    /// `accept` returned a final answer; the confirmation page was sent.
    Done(T),
    TimedOut,
    Stopped,
    Failed,
}

/// Waits for `GET <redirect path>?…` with the exact `Host` header. `accept` validates the callback URL and
/// returns `None` to ignore it and keep waiting.
pub fn wait<T>(
    listener: TcpListener,
    redirect: &Url,
    timeout: Duration,
    cancel: &AtomicBool,
    mut accept: impl FnMut(&Url) -> Option<T>,
) -> Wait<T> {
    if listener.set_nonblocking(true).is_err() {
        return Wait::Failed;
    }
    let deadline = Instant::now() + timeout;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Wait::Stopped;
        }
        if Instant::now() >= deadline {
            return Wait::TimedOut;
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                let Some(raw) = read_request(&mut stream) else { continue };
                let Some(url) = oauth::parse_loopback_request(&raw, redirect, redirect.path()) else { continue };
                let Some(end) = accept(&url) else { continue };
                let _ = stream.write_all(oauth::loopback_response().as_bytes());
                let _ = stream.flush();
                return Wait::Done(end);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(100)),
            Err(_) => return Wait::Failed,
        }
    }
}

fn read_request(stream: &mut std::net::TcpStream) -> Option<Vec<u8>> {
    stream.set_nonblocking(false).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let mut raw = Vec::new();
    let mut buffer = [0u8; 4096];
    while !oauth::loopback_request_complete(&raw) {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => return None,
            Ok(n) => raw.extend_from_slice(&buffer[..n]),
        }
        if raw.len() > oauth::LOOPBACK_MAX_REQUEST_BYTES {
            return None;
        }
    }
    Some(raw)
}
