//! Credentials and tokens: the OS keychain on desktop (Secret Service, Keychain, Credential Manager).
//! Browsers have no keychain; the web build keeps secrets in this site's local storage and says so in Settings.

#[cfg(not(target_arch = "wasm32"))]
const SERVICE: &str = "app.discophile.desktop";

#[cfg(not(target_arch = "wasm32"))]
pub fn get(name: &str) -> Option<String> {
    match keyring::Entry::new(SERVICE, name).and_then(|e| e.get_password()) {
        Ok(value) => Some(value),
        Err(keyring::Error::NoEntry) => None,
        Err(error) => {
            log::warn!("Keychain read failed for {name}: {error}; using the protected fallback file");
            fallback::get(name)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn set(name: &str, value: &str) {
    if let Err(error) = keyring::Entry::new(SERVICE, name).and_then(|e| e.set_password(value)) {
        log::warn!("Keychain write failed for {name}: {error}; using the protected fallback file");
        fallback::set(name, value);
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn delete(name: &str) {
    let _ = keyring::Entry::new(SERVICE, name).and_then(|e| e.delete_credential());
    fallback::delete(name);
}

/// Used only when no keychain service is available (for example a bare window manager without
/// Secret Service). The file is readable only by the current user.
#[cfg(not(target_arch = "wasm32"))]
mod fallback {
    use std::collections::BTreeMap;

    fn path() -> std::path::PathBuf {
        super::super::storage::data_dir().join("secrets.json")
    }
    fn load() -> BTreeMap<String, String> {
        std::fs::read_to_string(path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }
    fn store(map: &BTreeMap<String, String>) {
        let path = path();
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
    pub fn get(name: &str) -> Option<String> {
        load().get(name).cloned()
    }
    pub fn set(name: &str, value: &str) {
        let mut map = load();
        map.insert(name.into(), value.into());
        store(&map);
    }
    pub fn delete(name: &str) {
        let mut map = load();
        if map.remove(name).is_some() {
            store(&map);
        }
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
