//! Credentials and tokens: the OS keychain on desktop (Secret Service, Keychain, Credential Manager).
//! Browsers have no keychain; the web build keeps secrets in this site's local storage and says so in Settings.

#[cfg(not(target_arch = "wasm32"))]
pub use native::{delete, get, set};

/// Every secret lives in one keychain item holding a JSON map, read once per run and cached. macOS asks for
/// permission per item whenever the binary's signature changes (every rebuild), so one item means at most
/// one prompt. Debug builds use a protected file instead; `DISCOPHILE_SECRETS=keychain|file` overrides.
#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::Mutex;

    const SERVICE: &str = "app.discophile.desktop";
    const BUNDLE: &str = "secrets";
    /// Where secrets go when the keychain is unavailable; merged back into the keychain once it works.
    const FALLBACK_FILE: &str = "secrets.json";
    /// Where secrets go when the file is chosen over the keychain (debug builds), kept apart from the
    /// fallback so a release build never absorbs or clears it.
    const LOCAL_FILE: &str = "secrets-local.json";

    struct Store {
        keychain: bool,
        file: &'static str,
        map: BTreeMap<String, String>,
        /// Names already looked up as separate keychain items (how earlier versions saved them).
        legacy_checked: BTreeSet<String>,
    }

    fn with_store<R>(f: impl FnOnce(&mut Store) -> R) -> R {
        static STORE: Mutex<Option<Store>> = Mutex::new(None);
        let mut guard = STORE.lock().unwrap_or_else(|e| e.into_inner());
        f(guard.get_or_insert_with(open))
    }

    fn open() -> Store {
        let keychain = match std::env::var("DISCOPHILE_SECRETS").as_deref() {
            Ok("file") => false,
            Ok("keychain") => true,
            _ => !cfg!(debug_assertions),
        };
        let store = |keychain, file, map| Store { keychain, file, map, legacy_checked: BTreeSet::new() };
        if !keychain {
            return store(false, LOCAL_FILE, super::fallback::load(LOCAL_FILE));
        }
        let fallback = super::fallback::load(FALLBACK_FILE);
        match keyring::Entry::new(SERVICE, BUNDLE).and_then(|e| e.get_password()) {
            Ok(text) => {
                let mut map: BTreeMap<String, String> = serde_json::from_str(&text).unwrap_or_default();
                let merge = !fallback.is_empty();
                for (name, value) in fallback {
                    map.entry(name).or_insert(value);
                }
                let mut store = store(true, FALLBACK_FILE, map);
                if merge {
                    persist(&mut store);
                }
                store
            }
            Err(keyring::Error::NoEntry) => store(true, FALLBACK_FILE, fallback),
            Err(error) => {
                log::warn!("Keychain read failed: {error}; using the protected fallback file");
                store(false, FALLBACK_FILE, fallback)
            }
        }
    }

    fn persist(store: &mut Store) {
        if store.keychain {
            let text = serde_json::to_string(&store.map).unwrap_or_default();
            match keyring::Entry::new(SERVICE, BUNDLE).and_then(|e| e.set_password(&text)) {
                Ok(()) => {
                    super::fallback::remove(FALLBACK_FILE);
                    return;
                }
                Err(error) => {
                    log::warn!("Keychain write failed: {error}; using the protected fallback file");
                    store.keychain = false;
                }
            }
        }
        super::fallback::save(store.file, &store.map);
    }

    pub fn get(name: &str) -> Option<String> {
        with_store(|store| {
            if let Some(value) = store.map.get(name) {
                return Some(value.clone());
            }
            if !store.keychain || !store.legacy_checked.insert(name.to_string()) {
                return None;
            }
            // Moves an item saved by an earlier version into the single item.
            let entry = keyring::Entry::new(SERVICE, name).ok()?;
            let value = entry.get_password().ok()?;
            store.map.insert(name.to_string(), value.clone());
            persist(store);
            let _ = entry.delete_credential();
            Some(value)
        })
    }

    pub fn set(name: &str, value: &str) {
        with_store(|store| {
            store.legacy_checked.insert(name.to_string());
            if store.map.get(name).map(String::as_str) != Some(value) {
                store.map.insert(name.to_string(), value.to_string());
                persist(store);
            }
        })
    }

    pub fn delete(name: &str) {
        with_store(|store| {
            if store.keychain && store.legacy_checked.insert(name.to_string()) {
                let _ = keyring::Entry::new(SERVICE, name).and_then(|e| e.delete_credential());
            }
            if store.map.remove(name).is_some() {
                persist(store);
            }
        })
    }
}

/// Files readable only by the current user, for systems without a keychain service (for example a bare
/// window manager without Secret Service) and for debug builds.
#[cfg(not(target_arch = "wasm32"))]
mod fallback {
    use std::collections::BTreeMap;

    fn path(file: &str) -> std::path::PathBuf {
        super::super::storage::data_dir().join(file)
    }
    pub fn load(file: &str) -> BTreeMap<String, String> {
        std::fs::read_to_string(path(file)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }
    pub fn save(file: &str, map: &BTreeMap<String, String>) {
        let path = path(file);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if std::fs::write(&path, serde_json::to_string(map).unwrap_or_default()).is_ok() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
        }
    }
    pub fn remove(file: &str) {
        let _ = std::fs::remove_file(path(file));
    }
}

#[cfg(target_arch = "wasm32")]
pub fn get(name: &str) -> Option<String> {
    super::storage::read(&format!("secrets/{name}")).and_then(|t| serde_json::from_str(&t).ok())
}

#[cfg(target_arch = "wasm32")]
pub fn set(name: &str, value: &str) {
    super::storage::write(&format!("secrets/{name}"), &serde_json::to_string(value).unwrap_or_default());
}

#[cfg(target_arch = "wasm32")]
pub fn delete(name: &str) {
    super::storage::remove(&format!("secrets/{name}"));
}
