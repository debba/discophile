//! One audio transfer on the tokio runtime: streams the server response to `<ID>.part`, then renames
//! it to the record's final file name. Stream and artwork URLs carry credentials: never log them.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use discophile_core::offline_catalog::{OfflineRecord, is_audio_response};
use tokio::io::AsyncWriteExt;
use url::Url;

use super::{format, store};

/// Shared between the UI and a running transfer.
#[derive(Default)]
pub struct Progress {
    written: AtomicU64,
    total: AtomicU64,
    cancelled: AtomicBool,
}

impl Progress {
    pub fn fraction(&self) -> f32 {
        format::fraction(self.written.load(Ordering::Relaxed), self.total.load(Ordering::Relaxed))
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

pub struct Finished {
    pub path: PathBuf,
    pub bytes: i64,
    pub extension: String,
}

pub enum Outcome {
    Done(Finished),
    Cancelled,
    Failed,
}

pub async fn run(url: Url, record: OfflineRecord, progress: Arc<Progress>) -> Outcome {
    let part = store::part_path(&record);
    let outcome = write(url, &record, &part, &progress).await;
    if !matches!(outcome, Outcome::Done(_)) {
        let _ = tokio::fs::remove_file(&part).await;
    }
    outcome
}

async fn write(url: Url, record: &OfflineRecord, part: &PathBuf, progress: &Progress) -> Outcome {
    let failed = |reason: &str| {
        log::warn!("Download of a track failed: {reason}");
        Outcome::Failed
    };
    if let Some(parent) = part.parent()
        && let Err(error) = tokio::fs::create_dir_all(parent).await {
            return failed(&error.to_string());
        }
    let mut response = match discophile_core::http::server_client().get(url).send().await {
        Ok(response) => response,
        Err(error) => return failed(&error.without_url().to_string()),
    };
    let content_type = response.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).map(str::to_string);
    if !is_audio_response(response.status().as_u16(), content_type.as_deref()) {
        return failed(&format!("unexpected response {}", response.status()));
    }
    let extension = format::extension_for_content_type(content_type.as_deref(), &record.file_extension);
    let total = response.content_length().unwrap_or(0);
    progress.total.store(total, Ordering::Relaxed);
    let mut file = match tokio::fs::File::create(part).await {
        Ok(file) => file,
        Err(error) => return failed(&error.to_string()),
    };
    let mut written: u64 = 0;
    loop {
        if progress.is_cancelled() {
            return Outcome::Cancelled;
        }
        match response.chunk().await {
            Ok(Some(chunk)) => {
                if let Err(error) = file.write_all(&chunk).await {
                    return failed(&error.to_string());
                }
                written += chunk.len() as u64;
                progress.written.store(written, Ordering::Relaxed);
            }
            Ok(None) => break,
            Err(error) => return failed(&error.without_url().to_string()),
        }
    }
    if let Err(error) = file.flush().await {
        return failed(&error.to_string());
    }
    drop(file);
    if written == 0 || (total > 0 && written != total) {
        return failed("incomplete transfer");
    }
    if progress.is_cancelled() {
        return Outcome::Cancelled;
    }
    let mut finished = record.clone();
    finished.file_extension = extension;
    let path = store::file_path(&finished);
    if let Err(error) = tokio::fs::rename(part, &path).await {
        return failed(&error.to_string());
    }
    Outcome::Done(Finished { path, bytes: written.min(i64::MAX as u64) as i64, extension: finished.file_extension })
}

/// Saves a cover next to the downloads unless it is already there. Returns whether the file exists.
pub async fn save_artwork(url: Url, path: PathBuf) -> bool {
    if tokio::fs::try_exists(&path).await.unwrap_or(false) {
        return true;
    }
    let Ok(response) = discophile_core::http::server_client().get(url).send().await else { return false };
    if !response.status().is_success() {
        return false;
    }
    let Ok(bytes) = response.bytes().await else { return false };
    if bytes.is_empty() || bytes.len() > 10_000_000 || image::guess_format(&bytes).is_err() {
        return false;
    }
    if let Some(parent) = path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    let temporary = path.with_extension("jpg.tmp");
    tokio::fs::write(&temporary, &bytes).await.is_ok() && tokio::fs::rename(&temporary, &path).await.is_ok()
}

#[cfg(test)]
mod tests {
    use discophile_core::Song;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;

    /// Serves one canned HTTP response per connection.
    async fn serve(content_type: &'static str, body: Vec<u8>) -> Url {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut request = [0u8; 2048];
                let _ = socket.read(&mut request).await;
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = socket.write_all(head.as_bytes()).await;
                let _ = socket.write_all(&body).await;
            }
        });
        Url::parse(&format!("http://{address}/rest/stream")).unwrap()
    }

    #[tokio::test]
    async fn transfers_stream_to_the_final_file_and_reject_error_pages() {
        let dir = std::env::temp_dir().join(format!("discophile-downloads-{}", uuid::Uuid::new_v4()));
        // SAFETY: only this test reads or writes the data directory variable.
        unsafe { std::env::set_var("DISCOPHILE_DATA_DIR", &dir) };
        let record = OfflineRecord::new("account", &Song::new("1", "One"), "mp3", false);

        let progress = Arc::new(Progress::default());
        let url = serve("audio/flac", vec![7u8; 100_000]).await;
        let Outcome::Done(done) = run(url, record.clone(), progress.clone()).await else { panic!("transfer failed") };
        assert_eq!(done.extension, "flac");
        assert_eq!(done.bytes, 100_000);
        assert!(done.path.exists() && done.path.to_string_lossy().ends_with(".flac"));
        assert!(!store::part_path(&record).exists());
        assert_eq!(progress.fraction(), 1.0);

        let other = OfflineRecord::new("account", &Song::new("2", "Two"), "mp3", false);
        let url = serve("text/html", b"<html>error</html>".to_vec()).await;
        assert!(matches!(run(url, other.clone(), Arc::new(Progress::default())).await, Outcome::Failed));
        assert!(!store::part_path(&other).exists() && !store::file_path(&other).exists());

        let cancelled = Arc::new(Progress::default());
        cancelled.cancel();
        let url = serve("audio/mpeg", vec![1u8; 10]).await;
        assert!(matches!(run(url, other.clone(), cancelled).await, Outcome::Cancelled));
        let _ = std::fs::remove_dir_all(dir);
    }
}
