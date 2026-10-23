//! Book covers. The Audiobookshelf cover route needs the bearer token, so covers are fetched with the
//! client rather than through the shared artwork cache. Desktop prefers a downloaded book's artwork
//! and keeps fetched covers in a disposable disk cache for offline browsing.

use std::collections::HashMap;

use egui::{ColorImage, TextureHandle, TextureOptions};

use crate::app::App;
use crate::tasks::update;

const MEMORY_LIMIT: usize = 300;
/// Failed covers are requested again after this delay.
const RETRY_MS: i64 = 60_000;

enum Entry {
    Loading,
    Ready(TextureHandle),
    Failed(i64),
}

#[derive(Default)]
pub struct Covers {
    entries: HashMap<String, (Entry, u64)>,
    counter: u64,
}

impl Covers {
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    fn evict(&mut self) {
        if self.entries.len() <= MEMORY_LIMIT {
            return;
        }
        let mut ages: Vec<(String, u64)> = self.entries.iter().map(|(k, (_, used))| (k.clone(), *used)).collect();
        ages.sort_by_key(|(_, used)| *used);
        for (key, _) in ages.into_iter().take(self.entries.len() - MEMORY_LIMIT) {
            self.entries.remove(&key);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
type Local = Option<std::path::PathBuf>;
#[cfg(target_arch = "wasm32")]
type Local = Option<()>;

#[cfg(not(target_arch = "wasm32"))]
fn local_artwork(app: &App, book: &str) -> Local {
    super::downloads::artwork_path(app, book)
}

#[cfg(target_arch = "wasm32")]
fn local_artwork(_app: &App, _book: &str) -> Local {
    None
}

#[cfg(not(target_arch = "wasm32"))]
fn available_offline(local: &Local, key: &str) -> bool {
    local.is_some() || disk_path(key).exists()
}

#[cfg(target_arch = "wasm32")]
fn available_offline(_local: &Local, _key: &str) -> bool {
    false
}

#[cfg(not(target_arch = "wasm32"))]
fn disk_path(key: &str) -> std::path::PathBuf {
    crate::platform::storage::cache_dir().join("audiobook-covers").join(discophile_core::md5_hex(key.as_bytes()))
}

/// The cover texture for a book of the connected account, starting a load the first time.
pub fn texture(app: &mut App, book: &str) -> Option<TextureHandle> {
    let account = app.audiobooks.account.as_ref()?.id();
    let key = format!("{account}:{book}");
    let now = discophile_core::now_millis();
    let covers = &mut app.audiobooks.covers;
    covers.counter += 1;
    let counter = covers.counter;
    if let Some((entry, used)) = covers.entries.get_mut(&key) {
        *used = counter;
        match entry {
            Entry::Ready(texture) => return Some(texture.clone()),
            Entry::Loading => return None,
            Entry::Failed(at) if now - *at < RETRY_MS => return None,
            Entry::Failed(_) => {}
        }
    }
    let client = app.audiobooks.client.clone();
    let local = local_artwork(app, book);
    if client.is_none() && !available_offline(&local, &key) {
        return None;
    }
    let covers = &mut app.audiobooks.covers;
    covers.entries.insert(key.clone(), (Entry::Loading, counter));
    covers.evict();
    let ctx = app.ctx().clone();
    let book = book.to_string();
    app.tasks.spawn(async move {
        let image = load(&key, &book, client, local).await;
        update(move |app| {
            let entry = match image {
                Some(image) => Entry::Ready(ctx.load_texture(format!("audiobook-cover:{key}"), image, TextureOptions::LINEAR)),
                None => Entry::Failed(discophile_core::now_millis()),
            };
            let covers = &mut app.audiobooks.covers;
            covers.counter += 1;
            let counter = covers.counter;
            covers.entries.insert(key, (entry, counter));
        })
    });
    None
}

#[cfg(not(target_arch = "wasm32"))]
async fn load(
    key: &str,
    book: &str,
    client: Option<discophile_core::audiobookshelf::AudiobookshelfClient>,
    local: Local,
) -> Option<ColorImage> {
    if let Some(path) = local
        && let Some(image) = tokio::fs::read(&path).await.ok().and_then(|b| crate::artwork::decode(&b))
    {
        return Some(image);
    }
    let file = disk_path(key);
    if let Some(image) = tokio::fs::read(&file).await.ok().and_then(|b| crate::artwork::decode(&b)) {
        return Some(image);
    }
    let bytes = client?.cover(book).await.ok()?;
    let image = crate::artwork::decode(&bytes)?;
    if let Some(parent) = file.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    let _ = tokio::fs::write(&file, &bytes).await;
    Some(image)
}

#[cfg(target_arch = "wasm32")]
async fn load(
    _key: &str,
    book: &str,
    client: Option<discophile_core::audiobookshelf::AudiobookshelfClient>,
    _local: Local,
) -> Option<ColorImage> {
    let bytes = client?.cover(book).await.ok()?;
    crate::artwork::decode(&bytes)
}
