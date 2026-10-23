//! Artwork textures: memory LRU, a disposable disk cache on desktop, and the downloaded artwork that
//! offline items keep next to their audio.

use std::collections::HashMap;

use egui::{ColorImage, TextureHandle, TextureOptions};
use url::Url;

use crate::tasks::{Tasks, update};

const MEMORY_LIMIT: usize = 400;
const MAX_BYTES: usize = 10_000_000;
/// Decoded artwork is capped at this edge length; grids and the player never need more.
const MAX_EDGE: u32 = 640;

enum Entry {
    Loading,
    Ready(TextureHandle),
    Failed,
}

#[derive(Default)]
pub struct ArtworkCache {
    entries: HashMap<String, (Entry, u64)>,
    frame: u64,
}

impl ArtworkCache {
    pub fn begin_frame(&mut self) {
        self.frame += 1;
        if self.entries.len() > MEMORY_LIMIT {
            let mut ages: Vec<(String, u64)> = self.entries.iter().map(|(k, (_, used))| (k.clone(), *used)).collect();
            ages.sort_by_key(|(_, used)| *used);
            for (key, _) in ages.into_iter().take(self.entries.len() - MEMORY_LIMIT) {
                self.entries.remove(&key);
            }
        }
    }

    /// The texture for `key`, starting a load from `url` the first time it is requested.
    /// `local` names a downloaded artwork file that is preferred over the network (desktop only).
    pub fn texture(&mut self, tasks: &Tasks, key: &str, url: Option<Url>, local: Option<std::path::PathBuf>) -> Option<TextureHandle> {
        let frame = self.frame;
        if let Some((entry, used)) = self.entries.get_mut(key) {
            *used = frame;
            return match entry {
                Entry::Ready(texture) => Some(texture.clone()),
                _ => None,
            };
        }
        if url.is_none() && local.is_none() {
            return None;
        }
        self.entries.insert(key.to_string(), (Entry::Loading, frame));
        let key = key.to_string();
        let ctx = tasks.ctx().clone();
        tasks.spawn(async move {
            let image = load(&key, url, local).await;
            update(move |app| {
                let entry = match image {
                    Some(image) => Entry::Ready(ctx.load_texture(format!("artwork:{key}"), image, TextureOptions::LINEAR)),
                    None => Entry::Failed,
                };
                let frame = app.artwork.frame;
                app.artwork.entries.insert(key, (entry, frame));
            })
        });
        None
    }

    /// Forgets everything, for example after switching libraries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Lets a failed item be requested again (after reconnecting).
    pub fn retry_failed(&mut self) {
        self.entries.retain(|_, (entry, _)| !matches!(entry, Entry::Failed));
    }
}

async fn load(key: &str, url: Option<Url>, local: Option<std::path::PathBuf>) -> Option<ColorImage> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(path) = local
            && let Ok(bytes) = tokio::fs::read(&path).await
                && let Some(image) = decode(&bytes) {
                    return Some(image);
                }
        let file = crate::platform::storage::cache_dir().join("artwork").join(discophile_core::md5_hex(key.as_bytes()));
        if let Ok(bytes) = tokio::fs::read(&file).await
            && let Some(image) = decode(&bytes) {
                return Some(image);
            }
        let bytes = fetch(url?).await?;
        let image = decode(&bytes)?;
        if let Some(parent) = file.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        let _ = tokio::fs::write(&file, &bytes).await;
        prune_disk_cache(file.parent().map(|p| p.to_path_buf()));
        Some(image)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (key, local);
        decode(&fetch(url?).await?)
    }
}

async fn fetch(url: Url) -> Option<Vec<u8>> {
    let response = discophile_core::http::server_client().get(url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let bytes = response.bytes().await.ok()?;
    (bytes.len() < MAX_BYTES).then(|| bytes.to_vec())
}

pub fn decode(bytes: &[u8]) -> Option<ColorImage> {
    let image = image::load_from_memory(bytes).ok()?;
    let image = if image.width() > MAX_EDGE || image.height() > MAX_EDGE {
        image.resize(MAX_EDGE, MAX_EDGE, image::imageops::FilterType::Triangle)
    } else {
        image
    };
    let rgba = image.to_rgba8();
    Some(ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw()))
}

/// Keeps the disposable disk cache bounded, oldest files first.
#[cfg(not(target_arch = "wasm32"))]
fn prune_disk_cache(dir: Option<std::path::PathBuf>) {
    const LIMIT: usize = 2000;
    let Some(dir) = dir else { return };
    let Ok(entries) = std::fs::read_dir(&dir) else { return };
    let mut files: Vec<(std::time::SystemTime, std::path::PathBuf)> = entries
        .filter_map(Result::ok)
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    if files.len() <= LIMIT {
        return;
    }
    files.sort();
    for (_, path) in files.iter().take(files.len() - LIMIT) {
        let _ = std::fs::remove_file(path);
    }
}

/// Rasterizes the app icon SVG (the vinyl mark) once at the requested size.
pub fn app_icon(ctx: &egui::Context, size: u32) -> TextureHandle {
    let id = egui::Id::unique(("app-icon", size));
    if let Some(texture) = ctx.data(|d| d.get_temp::<TextureHandle>(id)) {
        return texture;
    }
    let image = render_svg(include_bytes!("../assets/AppIcon.svg"), size).unwrap_or_else(|| ColorImage::filled([1, 1], egui::Color32::TRANSPARENT));
    let texture = ctx.load_texture("app-icon", image, TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, texture.clone()));
    texture
}

pub fn render_svg(svg: &[u8], size: u32) -> Option<ColorImage> {
    let tree = resvg::usvg::Tree::from_data(svg, &resvg::usvg::Options::default()).ok()?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size)?;
    let scale = size as f32 / tree.size().width().max(tree.size().height());
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    Some(ColorImage::from_rgba_premultiplied([size as usize, size as usize], pixmap.data()))
}
