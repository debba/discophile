//! Files of the download manager: `data_dir()/downloads/<account>/` holds `catalog.json`, the audio
//! files named by `OfflineRecord::filename`, transfers in progress (`*.part`) and downloaded artwork.

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};

use discophile_core::offline_catalog::{OfflineRecord, artwork_filename};

const CATALOG: &str = "catalog.json";

pub fn root() -> PathBuf {
    crate::platform::storage::data_dir().join("downloads")
}

/// A directory name for an account key (normally an MD5 hex digest).
pub fn directory_name(account: &str) -> String {
    let name: String = account.chars().map(|c| if c.is_ascii_alphanumeric() || "-_".contains(c) { c } else { '_' }).collect();
    if name.is_empty() { "_".into() } else { name }
}

pub fn account_dir(account: &str) -> PathBuf {
    root().join(directory_name(account))
}

pub fn file_path(record: &OfflineRecord) -> PathBuf {
    account_dir(&record.account).join(record.filename())
}

pub fn part_path(record: &OfflineRecord) -> PathBuf {
    account_dir(&record.account).join(format!("{}.part", record.id.to_string().to_uppercase()))
}

pub fn artwork_path(account: &str, cover: &str) -> PathBuf {
    account_dir(account).join(artwork_filename(account, cover))
}

/// What was found on disk at launch.
#[derive(Default)]
pub struct Loaded {
    pub records: Vec<OfflineRecord>,
    pub artwork: HashSet<PathBuf>,
    /// A catalog existed but could not be decoded.
    pub unreadable: bool,
}

/// Reads every account's catalog, lists downloaded artwork and deletes interrupted `*.part` files
/// (their transfers restart from the beginning).
pub fn load_all() -> Loaded {
    let mut loaded = Loaded::default();
    let Ok(entries) = std::fs::read_dir(root()) else { return loaded };
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let dir_name = entry.file_name().to_string_lossy().to_string();
        match std::fs::read_to_string(dir.join(CATALOG)) {
            Ok(text) => match serde_json::from_str::<Vec<OfflineRecord>>(&text) {
                Ok(records) => loaded.records.extend(records.into_iter().filter(|r| directory_name(&r.account) == dir_name)),
                Err(error) => {
                    log::warn!("Ignoring unreadable download catalog {dir_name}: {error}");
                    loaded.unreadable = true;
                }
            },
            Err(error) if error.kind() != io::ErrorKind::NotFound => loaded.unreadable = true,
            Err(_) => {}
        }
        let Ok(files) = std::fs::read_dir(&dir) else { continue };
        for file in files.flatten() {
            let name = file.file_name().to_string_lossy().to_string();
            if name.ends_with(".part") {
                let _ = std::fs::remove_file(file.path());
            } else if name.starts_with("art-") {
                loaded.artwork.insert(file.path());
            }
        }
    }
    loaded
}

/// Writes one account's catalog atomically.
pub fn save(account: &str, records: &[&OfflineRecord]) -> io::Result<()> {
    let dir = account_dir(account);
    std::fs::create_dir_all(&dir)?;
    let text = serde_json::to_string(records).map_err(io::Error::other)?;
    let temporary = dir.join("catalog.json.tmp");
    std::fs::write(&temporary, text)?;
    std::fs::rename(&temporary, dir.join(CATALOG))
}

pub fn remove_file(path: &Path) {
    if let Err(error) = std::fs::remove_file(path)
        && error.kind() != io::ErrorKind::NotFound {
            log::warn!("Unable to remove a downloaded file: {error}");
        }
}

/// Deletes an account's directory with its catalog, audio and artwork.
pub fn remove_account_dir(account: &str) {
    let dir = account_dir(account);
    if dir.exists()
        && let Err(error) = std::fs::remove_dir_all(&dir) {
            log::warn!("Unable to remove downloads of a library: {error}");
        }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directory_names_cannot_escape_the_downloads_folder() {
        assert_eq!(directory_name("0cc175b9c0f1b6a831c399e269772661"), "0cc175b9c0f1b6a831c399e269772661");
        assert_eq!(directory_name("../x"), "___x");
        assert_eq!(directory_name(""), "_");
    }
}
