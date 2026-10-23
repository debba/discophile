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
                    if let Err(error) = replace(app, login) {
                        app.audiobooks.form.error = Some(audiobook_error(error));
                        return;
                    }
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
            "Use your Audiobookshelf server address and local username and password. Tokens are kept in this browser's storage. OpenID sign-in is not available yet.",
        )
    } else {
        t(
            "Use your Audiobookshelf server address and local username and password. Tokens are stored separately in this computer's keychain. Download books for offline listening from their detail page. OpenID sign-in is not available yet.",
        )
    }
}
