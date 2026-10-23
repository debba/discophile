//! Small JSON documents: files in the application data directory on desktop, `localStorage` on the web.
//! Keys may contain `/` to group documents, for example `history/<account>`.

use serde::{Serialize, de::DeserializeOwned};

pub fn load<T: DeserializeOwned>(key: &str) -> Option<T> {
    let text = read(key)?;
    match serde_json::from_str(&text) {
        Ok(value) => Some(value),
        Err(error) => {
            log::warn!("Ignoring unreadable {key}: {error}");
            None
        }
    }
}

pub fn save<T: Serialize>(key: &str, value: &T) {
    match serde_json::to_string(value) {
        Ok(text) => write(key, &text),
        Err(error) => log::error!("Unable to encode {key}: {error}"),
    }
}

fn sanitize(key: &str) -> String {
    key.split('/')
        .map(|part| part.chars().map(|c| if c.is_ascii_alphanumeric() || "-_.".contains(c) { c } else { '_' }).collect::<String>())
        .filter(|part| !part.is_empty() && part != "." && part != "..")
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::path::PathBuf;
    use std::sync::OnceLock;

    pub fn dirs() -> Option<&'static directories::ProjectDirs> {
        static DIRS: OnceLock<Option<directories::ProjectDirs>> = OnceLock::new();
        DIRS.get_or_init(|| directories::ProjectDirs::from("app", "Discophile", "Discophile")).as_ref()
    }

    /// Persistent application data (settings, history, downloads catalog).
    pub fn data_dir() -> PathBuf {
        std::env::var_os("DISCOPHILE_DATA_DIR")
            .map(PathBuf::from)
            .or_else(|| dirs().map(|d| d.data_dir().to_path_buf()))
            .unwrap_or_else(|| PathBuf::from(".discophile"))
    }

    /// Disposable caches (artwork, AutoEq index).
    pub fn cache_dir() -> PathBuf {
        std::env::var_os("DISCOPHILE_CACHE_DIR")
            .map(PathBuf::from)
            .or_else(|| dirs().map(|d| d.cache_dir().to_path_buf()))
            .unwrap_or_else(|| data_dir().join("cache"))
    }

    pub fn path(key: &str) -> PathBuf {
        data_dir().join("state").join(format!("{}.json", super::sanitize(key)))
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::{cache_dir, data_dir};

#[cfg(not(target_arch = "wasm32"))]
pub fn read(key: &str) -> Option<String> {
    std::fs::read_to_string(native::path(key)).ok()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn write(key: &str, text: &str) {
    let path = native::path(key);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let temporary = path.with_extension("json.tmp");
    let result = std::fs::write(&temporary, text).and_then(|_| std::fs::rename(&temporary, &path));
    if let Err(error) = result {
        log::error!("Unable to save {key}: {error}");
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn remove(key: &str) {
    let _ = std::fs::remove_file(native::path(key));
}

/// Removes every document whose key starts with `prefix/`.
#[cfg(not(target_arch = "wasm32"))]
pub fn remove_group(prefix: &str) {
    let dir = data_dir().join("state").join(sanitize(prefix));
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(target_arch = "wasm32")]
fn local() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

#[cfg(target_arch = "wasm32")]
pub fn read(key: &str) -> Option<String> {
    local()?.get_item(&format!("discophile:{}", sanitize(key))).ok()?
}

#[cfg(target_arch = "wasm32")]
pub fn write(key: &str, text: &str) {
    if let Some(storage) = local() {
        if storage.set_item(&format!("discophile:{}", sanitize(key)), text).is_err() {
            log::error!("Browser storage is full; {key} was not saved");
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub fn remove(key: &str) {
    if let Some(storage) = local() {
        let _ = storage.remove_item(&format!("discophile:{}", sanitize(key)));
    }
}

#[cfg(target_arch = "wasm32")]
pub fn remove_group(prefix: &str) {
    let Some(storage) = local() else { return };
    let prefix = format!("discophile:{}/", sanitize(prefix));
    let keys: Vec<String> = (0..storage.length().unwrap_or(0))
        .filter_map(|i| storage.key(i).ok().flatten())
        .filter(|k| k.starts_with(&prefix))
        .collect();
    for key in keys {
        let _ = storage.remove_item(&key);
    }
}
