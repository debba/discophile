use crate::{
    app::App,
    platform::{secrets, storage},
    theme::Appearance,
};
use discophile_core::audiobookshelf::AudiobookshelfAccount;
use discophile_core::l10n::{self, Language, t};
use discophile_core::{InternetRadioStation, ServerAccount, validate_server};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const PREFERENCES_ID: &str = "preferences";

#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Configuration {
    pub version: u32,
    pub connections: Vec<SharedConnection>,
    pub preferences: Option<SharedPreferences>,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum SharedConnection {
    Music {
        account: ServerAccount,
    },
    /// Password-based reauthentication creates independent sessions; refresh tokens are never copied.
    Audiobookshelf {
        account: AudiobookshelfAccount,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        password: Option<String>,
    },
    Radio {
        station: InternetRadioStation,
    },
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SharedPreferences {
    pub language: Option<Language>,
    pub appearance: Appearance,
}

impl SharedConnection {
    pub fn id(&self) -> String {
        match self {
            Self::Music { account } => account.id(),
            Self::Audiobookshelf { account, .. } => format!("abs:{}", account.id()),
            Self::Radio { station } => format!("radio:{}", station.id),
        }
    }
    pub fn label(&self) -> String {
        match self {
            Self::Music { account } => format!(
                "{} · {}",
                account.display_name(),
                account.credentials.server
            ),
            Self::Audiobookshelf { account, password } => if password.is_some() { account.display_name() } else {
                format!("{} · {}", account.display_name(), t("Sign in once to enable automatic synchronization"))
            },
            Self::Radio { station } => station.name.clone(),
        }
    }

    pub fn same_audiobookshelf_login(&self, other: &Self) -> bool {
        matches!((self, other), (Self::Audiobookshelf { account: a, .. }, Self::Audiobookshelf { account: b, .. })
            if a.server == b.server && a.username == b.username && a.user_id != b.user_id)
    }
}

pub fn choices(app: &App) -> Vec<SharedConnection> {
    let audiobook = app.audiobooks.account.clone();
    app.session
        .accounts
        .iter()
        .cloned()
        .map(|account| SharedConnection::Music { account })
        .chain(
            audiobook
                .into_iter()
                .map(|mut account| {
                    let password = crate::features::audiobooks::account::shared_password(&account);
                    // Session identity belongs to the local device and must not cause sync conflicts.
                    account.connection_id = uuid::Uuid::nil();
                    SharedConnection::Audiobookshelf { account, password }
                }),
        )
        .chain(
            app.radio
                .stations
                .iter()
                .cloned()
                .map(|station| SharedConnection::Radio { station }),
        )
        .collect()
}

pub fn snapshot(app: &App) -> Configuration {
    let state = &app.connect.services;
    let mut connections: Vec<_> = choices(app)
        .into_iter()
        .filter(|c| state.selected.contains(&c.id()))
        .collect();
    connections.sort_by_key(SharedConnection::id);
    Configuration {
        version: 1,
        connections,
        preferences: state.preferences.then_some(SharedPreferences {
            language: app.settings.language,
            appearance: app.settings.appearance,
        }),
    }
}

pub fn hash(configuration: &Configuration) -> String {
    fingerprint(configuration)
}

fn fingerprint(value: &impl Serialize) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(serde_json::to_vec(value).expect("configuration is serializable")))
}

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    connections: BTreeMap<String, String>,
    preferences: Option<String>,
}

impl Manifest {
    pub fn of(config: &Configuration) -> Self {
        Self {
            connections: config.connections.iter().map(|entry| (entry.id(), entry.fingerprint())).collect(),
            preferences: config.preferences.as_ref().map(fingerprint),
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct SyncBase {
    pub local: Manifest,
    pub cloud: Manifest,
}

#[derive(Clone, Copy, Default)]
pub enum ConflictChoice {
    #[default]
    Review,
    Local,
    Cloud,
}

impl SharedConnection {
    fn fingerprint(&self) -> String {
        // Older clients can serialize a local Audiobookshelf session ID into the shared record.
        if let Self::Audiobookshelf { account, password } = self {
            let mut account = account.clone();
            account.connection_id = uuid::Uuid::nil();
            account.name = account.name.filter(|name| !name.is_empty());
            return fingerprint(&Self::Audiobookshelf { account, password: password.clone() });
        }
        fingerprint(self)
    }
}

/// Replay only local edits over the current cloud copy. A missing local session is never a deletion.
pub fn merge(
    local: &Configuration,
    cloud: &Configuration,
    base: &SyncBase,
    removed: &BTreeSet<String>,
    sync_preferences: bool,
    choice: ConflictChoice,
) -> Result<Configuration, ()> {
    let mut entries: BTreeMap<_, _> = cloud.connections.iter().map(|entry| (entry.id(), entry)).collect();
    for entry in &local.connections {
        let id = entry.id();
        let current = entry.fingerprint();
        if removed.contains(&id) || base.local.connections.get(&id) == Some(&current) {
            continue;
        }
        let remote = entries.get(&id).map(|entry| entry.fingerprint());
        let remote_changed = remote.as_ref() != base.cloud.connections.get(&id);
        if remote.as_ref() == Some(&current) {
            continue;
        }
        if remote_changed {
            match choice {
                ConflictChoice::Review => return Err(()),
                ConflictChoice::Cloud => continue,
                ConflictChoice::Local => {}
            }
        }
        entries.insert(id, entry);
    }
    for id in removed {
        if let Some(entry) = entries.get(id)
            && base.cloud.connections.get(id) != Some(&entry.fingerprint())
        {
            match choice {
                ConflictChoice::Review => return Err(()),
                ConflictChoice::Cloud => continue,
                ConflictChoice::Local => {}
            }
        }
        entries.remove(id);
    }
    let mut preferences = cloud.preferences.clone();
    if removed.contains(PREFERENCES_ID) {
        let remote = cloud.preferences.as_ref().map(fingerprint);
        if remote.is_none() || remote == base.cloud.preferences || matches!(choice, ConflictChoice::Local) {
            preferences = None;
        } else if matches!(choice, ConflictChoice::Review) {
            return Err(());
        }
    } else if sync_preferences {
        let local_preferences = local.preferences.as_ref().map(fingerprint);
        let cloud_preferences = cloud.preferences.as_ref().map(fingerprint);
        if local_preferences != base.local.preferences && local_preferences != cloud_preferences {
            if cloud_preferences == base.cloud.preferences || matches!(choice, ConflictChoice::Local) {
                preferences = local.preferences.clone();
            } else if matches!(choice, ConflictChoice::Review) {
                return Err(());
            }
        }
    }
    Ok(Configuration { version: 1, connections: entries.into_values().cloned().collect(), preferences })
}

pub fn equivalent(left: &Configuration, right: &Configuration) -> bool {
    let left = Manifest::of(left);
    let right = Manifest::of(right);
    left.connections == right.connections && left.preferences == right.preferences
}

impl Configuration {
    pub fn validate(&self) -> bool {
        if self.version != 1 || self.connections.len() > 100 {
            return false;
        }
        let mut ids = BTreeSet::new();
        self.connections.iter().all(|connection| {
            if !ids.insert(connection.id()) {
                return false;
            }
            match connection {
                SharedConnection::Music { account } => {
                    let c = &account.credentials;
                    validate_server(c.server.as_str(), c.server.scheme() == "http").is_ok()
                        && !c.username.is_empty()
                        && c.username.len() <= 1000
                        && !c.password.is_empty()
                        && c.password.len() <= 16_384
                }
                SharedConnection::Audiobookshelf { account, password } => {
                    validate_server(account.server.as_str(), account.server.scheme() == "http")
                        .is_ok() && !account.user_id.is_empty() && !account.username.is_empty()
                        && password.as_ref().is_none_or(|s| !s.is_empty() && s.len() <= 16_384)
                }
                SharedConnection::Radio { station } => InternetRadioStation::new(
                    Some(station.id),
                    &station.name,
                    station.stream_url.as_str(),
                    station.allow_http,
                )
                .is_ok(),
            }
        })
    }
}

/// Import merges additions/updates; removing a cloud record never deletes local copies or downloads.
pub fn import(app: &mut App, configuration: &Configuration) -> bool {
    if !configuration.validate() {
        return false;
    }
    for connection in &configuration.connections {
        match connection {
            SharedConnection::Music { account } => {
                let id = account.id();
                let changed = !app.session.accounts.contains(account);
                if changed {
                    crate::session::store_account(app, account.clone());
                }
                if app.session.active.is_none() {
                    crate::session::switch_account(app, &id);
                } else if changed && app.session.active.as_deref() == Some(&id) {
                    app.session.client = Some(discophile_core::client::MusicClient::new(
                        account.credentials.clone(),
                    ));
                }
            }
            SharedConnection::Radio { station } => {
                if !app.radio.stations.contains(station) {
                    let mut form = crate::features::radio::StationForm::editing(station);
                    crate::features::radio::save_form(app, &mut form);
                }
            }
            SharedConnection::Audiobookshelf { account, password } => {
                if app.audiobooks.account.as_ref().is_some_and(|local| local.id() != account.id()) {
                    continue;
                }
                let connected = app.audiobooks.account.as_ref().is_some_and(|a| a.id() == account.id()) && app.audiobooks.client.is_some();
                if connected {
                    if let Some(password) = password { crate::features::audiobooks::account::save_password(account, password); }
                    if app.audiobooks.account.as_ref().is_some_and(|a| a.name != account.name) {
                        let _ = crate::features::audiobooks::account::rename(app, account.name.as_deref().unwrap_or_default());
                    }
                } else {
                    let form = &mut app.audiobooks.form;
                    form.server = account.server.to_string();
                    form.username = account.username.clone();
                    form.name = account.name.clone().unwrap_or_default();
                    form.allow_http = account.server.scheme() == "http";
                    form.prefilled = true;
                    if let Some(password) = password.clone().or_else(|| crate::features::audiobooks::account::shared_password(account)) {
                        crate::features::audiobooks::account::restore_shared(app, account.clone(), password);
                    } else {
                        crate::features::audiobooks::account::remember_shared(app, account.clone());
                        app.connect.services.notice = Some(t("Sign in once to Audiobookshelf on the source computer to enable automatic synchronization."));
                    }
                }
            }
        }
    }
    if let Some(account) = app.audiobooks.account.as_ref()
        && storage::load::<AudiobookshelfAccount>("connect/audiobookshelf-template").is_some_and(|legacy| legacy.id() == account.id())
    {
        storage::remove("connect/audiobookshelf-template");
    }
    if app.connect.services.preferences
        && let Some(preferences) = &configuration.preferences
    {
        if (app.settings.language, app.settings.appearance) != (preferences.language, preferences.appearance) {
            app.settings.language = preferences.language;
            app.settings.appearance = preferences.appearance;
            app.settings.save();
            l10n::set_language(app.settings.effective_language());
        }
    }
    let max = super::connection_limit(app);
    app.connect.services.selected = configuration
        .connections
        .iter()
        .take(max)
        .map(SharedConnection::id)
        .collect();
    true
}

pub fn export_encrypted(app: &mut App) {
    let Some(vault) = app.connect.services.vault.as_ref() else {
        return;
    };
    let Some(envelope) = vault.envelope.as_ref() else {
        return;
    };
    let text = serde_json::to_vec_pretty(envelope).unwrap_or_default();
    app.tasks.spawn(async move {
        if let Some(path) = rfd::AsyncFileDialog::new()
            .set_file_name("discophile-encrypted-configuration.json")
            .save_file()
            .await
        {
            let _ = path.write(&text).await;
        }
        crate::tasks::update(|_: &mut App| {})
    });
}

pub fn save_private(name: &str, identity: &super::Identity) {
    if let Ok(text) = serde_json::to_string(identity) {
        secrets::set(name, &text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use discophile_core::{Credentials, MusicProvider};
    fn account(user: &str) -> ServerAccount {
        ServerAccount::new(Credentials::simple("https://music.example.invalid", user, "fixture", false, MusicProvider::Subsonic).unwrap(), user)
    }
    fn shared(account: ServerAccount) -> Configuration {
        Configuration { version: 1, connections: vec![SharedConnection::Music { account }], preferences: Some(SharedPreferences { language: None, appearance: Appearance::Dark }) }
    }
    #[test]
    fn audiobook_connections_share_passwords_without_sharing_session_identity_or_tokens() {
        let mut app = App::connect_test_app();
        let mut account = AudiobookshelfAccount::new(url::Url::parse(&format!("https://{}.example.invalid", uuid::Uuid::new_v4())).unwrap(), "fixture-user", "fixture", None);
        let secret = format!("audiobookshelf-password:{}", account.id());
        crate::features::audiobooks::account::save_password(&account, "fixture-password");
        app.audiobooks.account = Some(account.clone());
        let first = choices(&app);
        account.connection_id = uuid::Uuid::new_v4();
        app.audiobooks.account = Some(account);
        assert!(first == choices(&app), "independent sign-ins must not create synchronization conflicts");
        let serialized = serde_json::to_value(&first).unwrap();
        assert_eq!(serialized[0]["password"], "fixture-password");
        assert_eq!(serialized[0]["account"]["connectionID"], uuid::Uuid::nil().to_string());
        assert!(serialized[0].get("tokens").is_none());
        secrets::delete(&secret);
        assert!(serde_json::to_value(choices(&app)).unwrap()[0].get("password").is_none());
    }

    #[test]
    fn import_preserves_local_connections_and_device_specific_settings() {
        let mut app = App::connect_test_app();
        app.settings.appearance = Appearance::Light;
        app.settings.volume = 0.25;
        let local = account("local"); let remote = account("remote");
        app.session.accounts.push(local.clone()); app.session.active = Some(local.id());
        assert!(import(&mut app, &shared(remote.clone())));
        assert_eq!(app.session.accounts.len(), 2);
        assert_eq!(app.session.active, Some(local.id()));
        assert_eq!(app.settings.volume, 0.25);
        assert_eq!(app.settings.appearance, Appearance::Light);
        assert!(app.connect.services.selected.contains(&remote.id()));
        assert!(import(&mut app, &Configuration { version: 1, connections: vec![], preferences: None }));
        assert_eq!(app.session.accounts.len(), 2, "cloud removal must preserve local connections and downloads");
        assert!(app.connect.services.selected.is_empty());
    }
    #[test]
    fn synchronized_records_are_canonical_and_invalid_records_are_rejected() {
        let mut app = App::connect_test_app();
        let a = account("a"); let b = account("b");
        app.connect.services.selected = [a.id(), b.id()].into_iter().collect();
        app.session.accounts = vec![a.clone(), b.clone()];
        let first = hash(&snapshot(&app));
        app.session.accounts.reverse();
        assert_eq!(hash(&snapshot(&app)), first);
        let mut invalid = shared(a.clone());
        invalid.connections.push(SharedConnection::Music { account: a });
        assert!(!invalid.validate(), "duplicate connection identifiers are ambiguous");
        let mut invalid_account = b;
        invalid_account.credentials.server = url::Url::parse("file:///private").unwrap();
        assert!(!shared(invalid_account).validate());
    }

    fn config(accounts: &[&ServerAccount]) -> Configuration {
        Configuration {
            version: 1,
            connections: accounts.iter().map(|account| SharedConnection::Music { account: (*account).clone() }).collect(),
            preferences: None,
        }
    }

    #[test]
    fn first_merge_keeps_cloud_connections_and_only_adds_selected_local_connections() {
        let existing = account("cloud");
        let selected = account("selected");
        let cloud = config(&[&existing]);
        let base = SyncBase { local: Manifest::default(), cloud: Manifest::of(&cloud) };
        let local = config(&[&selected]);
        let merged = merge(&local, &cloud, &base, &BTreeSet::new(), false, ConflictChoice::Review).unwrap();
        assert!(equivalent(&merged, &config(&[&existing, &selected])));
        assert!(equivalent(&merged, &config(&[&selected, &existing])));
        let merged_again = merge(&merged, &merged, &base, &BTreeSet::new(), false, ConflictChoice::Review).unwrap();
        assert!(equivalent(&merged, &merged_again));
    }

    #[test]
    fn separate_edits_merge_and_conflicting_edits_require_a_choice() {
        let a = account("a");
        let b = account("b");
        let previous = config(&[&a, &b]);
        let base = SyncBase { local: Manifest::of(&previous), cloud: Manifest::of(&previous) };
        let mut local_a = a.clone();
        local_a.name = "Local A".into();
        let mut cloud_b = b.clone();
        cloud_b.name = "Cloud B".into();
        let local = config(&[&local_a, &b]);
        let remote = config(&[&a, &cloud_b]);
        let merged = merge(&local, &remote, &base, &BTreeSet::new(), false, ConflictChoice::Review).unwrap();
        assert!(equivalent(&merged, &config(&[&local_a, &cloud_b])));

        let mut cloud_a = a.clone();
        cloud_a.name = "Cloud A".into();
        let remote = config(&[&cloud_a, &cloud_b]);
        assert!(merge(&local, &remote, &base, &BTreeSet::new(), false, ConflictChoice::Review).is_err());
        let resolved = merge(&local, &remote, &base, &BTreeSet::new(), false, ConflictChoice::Local).unwrap();
        assert!(equivalent(&resolved, &config(&[&local_a, &cloud_b])));
    }

    #[test]
    fn removal_requires_an_explicit_action_and_other_devices_do_not_republish_it() {
        let a = account("a");
        let cloud = shared(a.clone());
        let base = SyncBase { local: Manifest::of(&cloud), cloud: Manifest::of(&cloud) };
        let offline = config(&[]);
        let merged = merge(&offline, &cloud, &base, &BTreeSet::new(), false, ConflictChoice::Review).unwrap();
        assert!(equivalent(&merged, &cloud));
        let removed = [a.id(), PREFERENCES_ID.into()].into_iter().collect();
        let merged = merge(&cloud, &cloud, &base, &removed, true, ConflictChoice::Review).unwrap();
        assert!(equivalent(&merged, &offline));
        let from_other_device = merge(&cloud, &merged, &base, &BTreeSet::new(), true, ConflictChoice::Review).unwrap();
        assert!(equivalent(&from_other_device, &offline));
    }

    #[test]
    fn remote_audiobookshelf_session_ids_do_not_create_revisions() {
        let account = AudiobookshelfAccount::new(url::Url::parse("https://books.example.invalid").unwrap(), "user", "reader", None);
        let remote = Configuration {
            version: 1,
            connections: vec![SharedConnection::Audiobookshelf { account, password: Some("fixture".into()) }],
            preferences: None,
        };
        let mut local = remote.clone();
        if let SharedConnection::Audiobookshelf { account, .. } = &mut local.connections[0] {
            account.connection_id = uuid::Uuid::nil();
            account.name = Some(String::new());
        }
        assert!(equivalent(&local, &remote));
    }
}
