//! The Audiobookshelf connection: the account in app storage, its tokens in the OS keychain
//! (browser storage on the web), and the persist callback that saves rotated tokens.

use std::sync::Mutex;

use discophile_core::audiobookshelf::{AudiobookshelfAccount, AudiobookshelfClient, AudiobookshelfLogin, AudiobookshelfTokens};
use discophile_core::l10n::{audiobook_error, t};
use discophile_core::{MusicError, MusicResult};

use crate::app::App;
use crate::platform::{secrets, storage};
use crate::tasks::update;

const ACCOUNT_KEY: &str = "audiobooks/account";
const TOKENS_SECRET: &str = "audiobookshelf-tokens";

/// Serializes account and token writes between the UI thread and token renewal in background tasks.
static STORE: Mutex<()> = Mutex::new(());

/// The form behind "Connect Audiobookshelf" (Settings, the Add account sheet and the Audiobooks page).
#[derive(Default)]
pub struct ConnectForm {
    pub server: String,
    pub username: String,
    pub password: String,
    pub name: String,
    pub allow_http: bool,
    pub show_password: bool,
    pub busy: bool,
    pub error: Option<String>,
    /// Set once after a successful connection or alias change, consumed by `connect_form`.
    pub done: bool,
    /// The form was filled from the saved account.
    pub prefilled: bool,
    generation: u64,
}

impl ConnectForm {
    /// Fills the form from the saved connection the first time it is shown.
    pub fn prefill(&mut self, account: Option<&AudiobookshelfAccount>) {
        if self.prefilled {
            return;
        }
        self.prefilled = true;
        if let Some(account) = account {
            self.server = discophile_core::server_string(&account.server);
            self.username = account.username.clone();
            self.name = account.name.clone().unwrap_or_default();
            self.allow_http = account.server.scheme() == "http";
        }
    }

    /// Only the alias changed: no password, same server and user.
    pub fn editing_alias_only(&self, account: Option<&AudiobookshelfAccount>) -> bool {
        let Some(account) = account else { return false };
        self.password.is_empty()
            && self.server.trim().trim_end_matches('/') == discophile_core::server_string(&account.server)
            && self.username.trim() == account.username
    }

    pub fn reset(&mut self) {
        let generation = self.generation;
        *self = Self { generation: generation + 1, ..Default::default() };
    }
}

fn write(account: &AudiobookshelfAccount, tokens: &AudiobookshelfTokens) -> MusicResult<()> {
    let text = serde_json::to_string(tokens).map_err(|_| MusicError::InvalidResponse)?;
    secrets::set(TOKENS_SECRET, &text);
    storage::save(ACCOUNT_KEY, account);
    Ok(())
}

/// Saves a renamed or token-rotated connection, refusing connections replaced by a newer sign-in.
fn persist(account: &AudiobookshelfAccount, tokens: &AudiobookshelfTokens) -> MusicResult<()> {
    let _guard = STORE.lock().unwrap_or_else(|e| e.into_inner());
    let stored: Option<AudiobookshelfAccount> = storage::load(ACCOUNT_KEY);
    if stored.map(|a| a.connection_id) != Some(account.connection_id) {
        return Err(MusicError::Authentication);
    }
    write(account, tokens)
}

fn make_client(account: AudiobookshelfAccount, tokens: AudiobookshelfTokens) -> AudiobookshelfClient {
    AudiobookshelfClient::new(account, tokens, persist)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn shared_password(account: &AudiobookshelfAccount) -> Option<String> {
    secrets::get(&format!("audiobookshelf-password:{}", account.id())).filter(|s| !s.is_empty())
}

pub(crate) fn save_password(account: &AudiobookshelfAccount, password: &str) {
    if !password.is_empty() { secrets::set(&format!("audiobookshelf-password:{}", account.id()), password); }
}

/// Reauthenticate on each computer so refresh-token rotation remains device-specific.
#[cfg(not(target_arch = "wasm32"))]
pub fn restore_shared(app: &mut App, account: AudiobookshelfAccount, password: String) {
    if app.audiobooks.form.busy || app.audiobooks.account.as_ref().is_some_and(|a| a.id() != account.id()) { return; }
    app.audiobooks.form.generation += 1;
    let generation = app.audiobooks.form.generation;
    let connect_generation = app.connect.generation;
    app.audiobooks.form.busy = true;
    app.tasks.spawn(async move {
        let result = AudiobookshelfClient::connect(account.server.as_str(), &account.username, &password,
            account.server.scheme() == "http", account.name.as_deref().unwrap_or_default()).await;
        update(move |app: &mut App| {
            if app.audiobooks.form.generation != generation { return; }
            app.audiobooks.form.busy = false;
            if app.connect.generation != connect_generation { return; }
            match result {
                Ok(login) if login.account.id() == account.id() => {
                    if replace(app, login).is_ok() {
                        save_password(&account, &password);
                        app.connect.services.notice = Some(t("Audiobookshelf connected on this computer."));
                    }
                }
                _ => app.connect.services.error = Some(t("Audiobookshelf could not sign in automatically. Sign in again to update the shared connection.")),
            }
        })
    });
}

/// Reads the saved connection at launch.
pub fn load(app: &mut App) {
    let state = &mut app.audiobooks;
    let Some(account) = storage::load::<AudiobookshelfAccount>(ACCOUNT_KEY) else { return };
    match secrets::get(TOKENS_SECRET).and_then(|text| serde_json::from_str::<AudiobookshelfTokens>(&text).ok()) {
        Some(tokens) => state.client = Some(make_client(account.clone(), tokens)),
        None => state.error = Some(audiobook_error(MusicError::Authentication)),
    }
    state.account = Some(account);
}

/// Signs in, checks library access and replaces the saved connection.
pub fn connect(app: &mut App) {
    let account = app.audiobooks.account.clone();
    if app.audiobooks.form.busy {
        return;
    }
    if app.audiobooks.form.editing_alias_only(account.as_ref()) {
        let name = app.audiobooks.form.name.clone();
        match rename(app, &name) {
            Ok(()) => {
                let form = &mut app.audiobooks.form;
                form.done = true;
                form.password.clear();
                form.error = None;
            }
            Err(error) => app.audiobooks.form.error = Some(audiobook_error(error)),
        }
        return;
    }
    let form = &mut app.audiobooks.form;
    form.busy = true;
    form.error = None;
    let generation = form.generation;
    let (server, username, password, allow_http, name) =
        (form.server.clone(), form.username.clone(), form.password.clone(), form.allow_http, form.name.clone());
    app.tasks.spawn(async move {
        let result = AudiobookshelfClient::connect(&server, &username, &password, allow_http, &name).await;
        update(move |app| {
            if app.audiobooks.form.generation != generation {
                return;
            }
            app.audiobooks.form.busy = false;
            match result {
                Ok(login) => {
                    let connected_account = login.account.clone();
                    if let Err(error) = replace(app, login) {
                        app.audiobooks.form.error = Some(audiobook_error(error));
                        return;
                    }
                    save_password(&connected_account, &password);
                    let form = &mut app.audiobooks.form;
                    form.password.clear();
                    form.done = true;
                    form.error = None;
                }
                Err(error) => app.audiobooks.form.error = Some(audiobook_error(error)),
            }
        })
    });
}

/// Installs a verified login as the saved connection.
fn replace(app: &mut App, login: AudiobookshelfLogin) -> MusicResult<()> {
    if app.player.mode == crate::player::Mode::Audiobook {
        crate::player::stop(app);
    }
    {
        let _guard = STORE.lock().unwrap_or_else(|e| e.into_inner());
        write(&login.account, &login.tokens)?;
    }
    let previous = app.audiobooks.account.as_ref().map(AudiobookshelfAccount::id);
    let state = &mut app.audiobooks;
    state.client = Some(make_client(login.account.clone(), login.tokens));
    state.account = Some(login.account.clone());
    state.error = None;
    state.sync.error = None;
    state.detail = None;
    state.browse.reset();
    if previous.as_deref() != Some(login.account.id().as_str()) {
        state.covers.clear();
    }
    super::downloads::load(app);
    super::sync::retry_pending(app, true);
    Ok(())
}

/// Changes the alias without signing in again.
pub fn rename(app: &mut App, name: &str) -> MusicResult<()> {
    let Some(account) = app.audiobooks.account.clone() else { return Err(MusicError::MissingCredentials) };
    let updated = match &app.audiobooks.client {
        Some(client) => client.rename(name)?,
        None => {
            let mut next = account;
            next.name = Some(name.trim().to_string());
            let _guard = STORE.lock().unwrap_or_else(|e| e.into_inner());
            storage::save(ACCOUNT_KEY, &next);
            next
        }
    };
    app.audiobooks.account = Some(updated);
    Ok(())
}

/// Removes the connection, its tokens and its downloads; revokes the session in the background.
pub fn disconnect(app: &mut App) {
    if app.player.mode == crate::player::Mode::Audiobook {
        crate::player::stop(app);
    }
    let client = app.audiobooks.client.take();
    let account = app.audiobooks.account.take();
    {
        let _guard = STORE.lock().unwrap_or_else(|e| e.into_inner());
        secrets::delete(TOKENS_SECRET);
        storage::remove(ACCOUNT_KEY);
    }
    if let Some(account) = account {
        secrets::delete(&format!("audiobookshelf-password:{}", account.id()));
        super::downloads::remove_account(app, &account.id());
    }
    let state = &mut app.audiobooks;
    state.error = None;
    state.sync.error = None;
    state.detail = None;
    state.browse.reset();
    state.covers.clear();
    state.form.reset();
    if let Some(client) = client {
        // Local removal never waits for the server.
        app.tasks.spawn(async move {
            let _ = client.logout().await;
            update(|_| {})
        });
    }
    if matches!(app.nav.current, crate::nav::Route::Audiobook(_)) {
        app.nav.back();
    }
}

/// Text for the connection footnote, adapted to where tokens are kept.
pub fn storage_note() -> String {
    if crate::platform::IS_WEB {
        t(
            "Use your Audiobookshelf server address and local username and password. Credentials are kept in this browser's storage. OpenID sign-in is not available yet.",
        )
    } else {
        t(
            "Use your Audiobookshelf server address and local username and password. Credentials are saved in this computer's keychain. Selecting this connection in Connect also synchronizes the password so your other computers can sign in. Download books for offline listening from their detail page. OpenID sign-in is not available yet.",
        )
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn shared_password_creates_an_independent_verified_session() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let server_url = url::Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let server = std::thread::spawn(move || {
            for path in ["/login", "/api/libraries"] {
                let (mut socket, _) = listener.accept().unwrap();
                socket.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
                let mut request = Vec::new();
                loop {
                    let mut chunk = [0; 4096]; let count = socket.read(&mut chunk).unwrap(); assert!(count > 0);
                    request.extend_from_slice(&chunk[..count]);
                    if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&request[..end]);
                        let length: usize = head.lines().filter_map(|line| line.split_once(':')).find(|(name, _)| name.eq_ignore_ascii_case("content-length")).map(|(_, value)| value.trim().parse().unwrap()).unwrap_or(0);
                        if request.len() >= end + 4 + length { break; }
                    }
                }
                let text = String::from_utf8_lossy(&request);
                assert_eq!(text.lines().next().unwrap().split(' ').nth(1).unwrap(), path);
                let body = if path == "/login" {
                    assert!(text.contains("fixture-password"));
                    r#"{"user":{"id":"fixture-user","username":"fixture","accessToken":"independent-access","refreshToken":"independent-refresh"}}"#
                } else {
                    assert!(text.to_ascii_lowercase().contains("authorization: bearer independent-access"));
                    r#"{"libraries":[{"id":"books","name":"Books","mediaType":"book"}]}"#
                };
                write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        let shared = AudiobookshelfAccount::new(server_url, "fixture-user", "fixture", Some("Shared books"));
        let mut app = App::connect_test_app();
        let (tasks, updates) = crate::tasks::Tasks::new(egui::Context::default()); app.tasks = tasks;
        restore_shared(&mut app, shared.clone(), "fixture-password".into());
        updates.recv_timeout(std::time::Duration::from_secs(5)).unwrap()(&mut app);
        let local = app.audiobooks.account.as_ref().expect("automatic sign-in succeeded");
        assert_eq!(local.id(), shared.id());
        assert_ne!(local.connection_id, shared.connection_id);
        assert!(app.audiobooks.client.is_some());
        assert_eq!(shared_password(local).as_deref(), Some("fixture-password"));
        let tokens: AudiobookshelfTokens = serde_json::from_str(&secrets::get(TOKENS_SECRET).unwrap()).unwrap();
        assert_eq!(tokens.refresh_token.as_deref(), Some("independent-refresh"));
        server.join().unwrap();
        secrets::delete(TOKENS_SECRET);
        secrets::delete(&format!("audiobookshelf-password:{}", shared.id()));
        storage::remove(ACCOUNT_KEY);
    }
}
