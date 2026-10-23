//! Music library accounts and the active client. Account metadata is stored as `accounts`; each
//! password or access token lives in the keychain under `account:<id>`.

use discophile_core::client::MusicClient;
use discophile_core::{Credentials, MusicError, MusicProvider, ServerAccount, l10n};
use serde::{Deserialize, Serialize};

use crate::app::App;
use crate::platform::{secrets, storage};
use crate::tasks::update;

#[derive(Default, Serialize, Deserialize)]
struct Stored {
    accounts: Vec<ServerAccount>,
    active: Option<String>,
}

#[derive(Default)]
pub struct Session {
    pub accounts: Vec<ServerAccount>,
    pub active: Option<String>,
    pub client: Option<MusicClient>,
    /// Account currently being connected or verified.
    pub busy: Option<String>,
    /// Bumped on every library switch so late results from the previous library are ignored.
    pub generation: u64,
}

impl Session {
    pub fn load() -> Self {
        let stored: Stored = storage::load("accounts").unwrap_or_default();
        let accounts: Vec<ServerAccount> = stored
            .accounts
            .into_iter()
            .filter_map(|mut account| {
                let password = secrets::get(&format!("account:{}", account.id()))?;
                account.credentials.password = password;
                Some(account)
            })
            .collect();
        let active = stored.active.filter(|id| accounts.iter().any(|a| &a.id() == id)).or_else(|| accounts.first().map(ServerAccount::id));
        let client = active.as_ref().and_then(|id| accounts.iter().find(|a| &a.id() == id)).map(|a| MusicClient::new(a.credentials.clone()));
        Self { accounts, active, client, busy: None, generation: 0 }
    }

    fn persist(&self) {
        let accounts = self
            .accounts
            .iter()
            .map(|account| {
                let mut copy = account.clone();
                copy.credentials.password = String::new();
                copy
            })
            .collect();
        storage::save("accounts", &Stored { accounts, active: self.active.clone() });
    }

    pub fn has_library(&self) -> bool {
        self.client.is_some()
    }

    /// The active library's namespace, or an empty string without one.
    pub fn account_key(&self) -> String {
        self.active.clone().unwrap_or_default()
    }

    pub fn active_account(&self) -> Option<&ServerAccount> {
        let id = self.active.as_ref()?;
        self.accounts.iter().find(|a| &a.id() == id)
    }

    pub fn provider(&self) -> Option<MusicProvider> {
        self.client.as_ref().map(|c| c.provider())
    }
}

/// Connects a new account (or replaces one with the same server and user) and makes it active.
/// `done` receives the error message on failure, `None` on success.
#[allow(clippy::too_many_arguments)]
pub fn add_account(
    app: &mut App,
    provider: MusicProvider,
    server: &str,
    username: &str,
    password: &str,
    allow_http: bool,
    alias: &str,
    done: impl FnOnce(&mut App, Option<String>) + crate::platform::MaybeSend + 'static,
) {
    let credentials = match Credentials::simple(server, username, password, allow_http, provider) {
        Ok(credentials) => credentials,
        Err(error) => return done(app, Some(l10n::music_error(error))),
    };
    let alias = alias.trim().to_string();
    let id = credentials.account_key();
    app.session.busy = Some(id.clone());
    app.tasks.spawn(async move {
        let result = MusicClient::connect(credentials).await;
        update(move |app: &mut App| {
            app.session.busy = None;
            match result {
                Ok(client) => {
                    let account = ServerAccount::new(client.credentials().clone(), &alias);
                    store_account(app, account);
                    switch_account(app, &id);
                    done(app, None);
                }
                Err(error) => done(app, Some(l10n::music_error(error))),
            }
        })
    });
}

fn store_account(app: &mut App, account: ServerAccount) {
    let id = account.id();
    secrets::set(&format!("account:{id}"), &account.credentials.password);
    if let Some(existing) = app.session.accounts.iter_mut().find(|a| a.id() == id) {
        *existing = account;
    } else {
        app.session.accounts.push(account);
    }
    app.session.persist();
}

/// Makes a saved account active. Playback stops and library caches reload.
pub fn switch_account(app: &mut App, id: &str) {
    let Some(account) = app.session.accounts.iter().find(|a| a.id() == id).cloned() else { return };
    let changed = app.session.active.as_deref() != Some(id);
    app.session.active = Some(id.to_string());
    app.session.client = Some(MusicClient::new(account.credentials));
    app.session.persist();
    if changed || app.session.generation == 0 {
        app.on_library_changed();
    }
}

/// Changes only the alias. Credentials, downloads and current playback are preserved.
pub fn rename_account(app: &mut App, id: &str, alias: &str) {
    if let Some(account) = app.session.accounts.iter_mut().find(|a| a.id() == id) {
        account.name = alias.trim().to_string();
        app.session.persist();
    }
}

/// Saves a new password (or API key) for an existing account after verifying it.
pub fn update_password(app: &mut App, id: &str, password: &str, done: impl FnOnce(&mut App, Option<String>) + crate::platform::MaybeSend + 'static) {
    let Some(account) = app.session.accounts.iter().find(|a| a.id() == id).cloned() else { return };
    let mut credentials = account.credentials.clone();
    credentials.password = password.to_string();
    credentials.authentication = Default::default();
    if credentials.provider == MusicProvider::Jellyfin {
        credentials.user_id = None;
    }
    let id = id.to_string();
    app.session.busy = Some(id.clone());
    app.tasks.spawn(async move {
        let result = MusicClient::connect(credentials).await;
        update(move |app: &mut App| {
            app.session.busy = None;
            match result {
                Ok(client) => {
                    store_account(app, ServerAccount::new(client.credentials().clone(), &account.name));
                    if app.session.active.as_deref() == Some(&id) {
                        app.session.client = Some(client);
                    }
                    done(app, None);
                }
                Err(error) => done(app, Some(l10n::music_error(error))),
            }
        })
    });
}

/// Removes the connection and its downloads. Media on the server is never deleted.
pub fn remove_account(app: &mut App, id: &str) {
    secrets::delete(&format!("account:{id}"));
    app.session.accounts.retain(|a| a.id() != id);
    crate::features::downloads::remove_account(app, id);
    storage::remove_group(&format!("library/{id}"));
    if app.session.active.as_deref() == Some(id) {
        app.session.active = None;
        app.session.client = None;
        if let Some(next) = app.session.accounts.first().map(ServerAccount::id) {
            switch_account(app, &next);
        } else {
            app.session.persist();
            app.on_library_changed();
        }
    } else {
        app.session.persist();
    }
}

/// Checks the saved connection. `done` receives the error message, or `None` when it works.
pub fn verify_account(app: &mut App, id: &str, done: impl FnOnce(&mut App, Option<String>) + crate::platform::MaybeSend + 'static) {
    let Some(account) = app.session.accounts.iter().find(|a| a.id() == id).cloned() else { return };
    app.session.busy = Some(id.to_string());
    app.tasks.spawn(async move {
        let result: Result<(), MusicError> = MusicClient::new(account.credentials).ping().await;
        update(move |app: &mut App| {
            app.session.busy = None;
            done(app, result.err().map(l10n::music_error));
        })
    });
}
