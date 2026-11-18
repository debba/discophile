//! Polling native service integration. Secrets and recovery material never enter plain app storage.
pub mod configuration;
mod playback;

use crate::{app::App, platform::secrets, tasks::update};
use configuration::{Configuration, ConflictChoice, Manifest, SyncBase};
use discophile_core::connect::services::{Backup, Catalog, PairedDevice, Poll, VaultInfo};
use discophile_core::connect::vault::{Envelope, VaultKeys, Watermark};
use discophile_core::connect::{ConnectClient, ConnectCredential, ConnectError, ServerInfo};
use discophile_core::l10n::t;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use uuid::Uuid;

const POLL_INTERVAL_MS: i64 = 3_000;
type Completion = Box<dyn FnOnce(&mut App, Value) + Send>;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub device_id: Uuid,
    pub device_secret: String,
    pub keys: VaultKeys,
    #[serde(default)]
    pub watermark: Watermark,
    #[serde(default)]
    pub state_revision: u64,
    #[serde(default)]
    pub seen: VecDeque<Uuid>,
    #[serde(default)]
    pub state_watermarks: BTreeMap<Uuid, Watermark>,
    #[serde(default)]
    pub selected: BTreeSet<String>,
    #[serde(default)]
    pub preferences: bool,
    #[serde(default)]
    pub automatic: bool,
    #[serde(default)]
    pub baseline: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub select_new_connections: bool,
    #[serde(default)]
    pub known_connections: BTreeSet<String>,
    #[serde(default)]
    pub sync_base: Option<SyncBase>,
    #[serde(default)]
    pub removed: BTreeSet<String>,
    #[serde(default)]
    pub replacement: Option<(String, String)>,
}

#[derive(Default)]
pub struct ServicesState {
    pub identity: Option<Identity>,
    pub vault: Option<VaultInfo>,
    pub devices: Vec<PairedDevice>,
    pub backups: Vec<Backup>,
    pub catalog: Option<Catalog>,
    pub selected: BTreeSet<String>,
    pub preferences: bool,
    pub automatic: bool,
    pub paired: bool,
    pub name: String,
    pub recovery_input: String,
    pub recovery_visible: bool,
    pub confirm_recovery_saved: bool,
    pub confirm_delete: bool,
    pub revoke_target: Option<Uuid>,
    pub restore_target: Option<(Uuid, Configuration)>,
    pub conflict: Option<Configuration>,
    pub pending_cloud: Option<Configuration>,
    pub cloud: Option<Configuration>,
    pub reviewing: bool,
    pub review_ready: bool,
    pub remove_target: Option<String>,
    pub replace_target: Option<String>,
    sync_requested: bool,
    conflict_choice: Option<(u64, ConflictChoice)>,
    pub busy: bool,
    background: bool,
    pending: VecDeque<(Value, Completion)>,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub playback_notice: Option<String>,
    pub states: BTreeMap<Uuid, playback::Presence>,
    pub remote_device: Option<Uuid>,
    pub independent_playback: bool,
    pub(super) forwarding: Option<(Uuid, Uuid, Uuid, i64)>,
    send_retries: BTreeMap<Uuid, u8>,
    retry_after: i64,
    last_presence: i64,
    pub(super) outgoing: VecDeque<Value>,
    pub(super) transfer: Option<playback::OutgoingTransfer>,
    pub(super) receiving: Option<playback::IncomingTransfer>,
    pub(super) transfer_request: Option<(Uuid, Uuid, i64)>,
    namespace: String,
    loaded: bool,
    last_poll: i64,
    last_backup: i64,
    paused_after_error: bool,
    registration_replaced: bool,
}

/// Background heartbeats must not disable controls or change the settings layout.
pub fn changing(app: &App) -> bool {
    let state = &app.connect.services;
    (state.busy && !state.background) || !state.pending.is_empty()
}

pub fn paid(app: &App, benefit: &str) -> bool {
    app.connect.account.as_ref().is_some_and(|a| {
        a.plan
            .entitlements
            .iter()
            .any(|e| e.id == benefit && e.available)
    })
}
pub fn connection_limit(app: &App) -> usize {
    app.connect
        .account
        .as_ref()
        .and_then(|a| {
            a.plan
                .entitlements
                .iter()
                .find(|e| e.id == "sync.connections")
        })
        .and_then(|e| e.limit)
        .map(|n| n.min(100) as usize)
        .unwrap_or(100)
}
pub fn account_sync(app: &App) -> bool {
    app.connect.server_info.as_ref().is_some_and(|info| info.capability_usable("account-sync"))
}

pub fn recover_account(app: &mut App) {
    let mut operation = json!({"action":"recover"});
    if let Some(identity) = app.connect.services.identity.as_ref() { operation["deviceSecret"] = json!(identity.device_secret); }
    run(app, operation, |app, value| {
        app.connect.services.vault = serde_json::from_value(value["vault"].clone()).ok();
        if let Some(code) = value["recoveryCode"].as_str() {
            app.connect.services.recovery_input = code.to_string();
            if app.connect.services.reviewing {
                if !prepare_review(app) {
                    app.connect.services.error = Some(t("The encrypted configuration could not be authenticated. Check your recovery code."));
                }
            } else {
                pair(app, false);
            }
        } else {
            app.connect.services.error = Some(t("Open a previously connected computer once to enable synchronization through your account."));
        }
    });
}
fn namespace(credential: &ConnectCredential) -> String {
    use sha2::{Digest, Sha256};
    let text = format!(
        "{}\n{}\n{}",
        credential.origin, credential.instance_id, credential.account_id
    );
    format!("connect-vault:{:x}", Sha256::digest(text))
}
pub fn save(app: &mut App) {
    let state = &mut app.connect.services;
    if let Some(identity) = state.identity.as_mut() {
        identity.selected = state.selected.clone();
        identity.preferences = state.preferences;
        identity.automatic = state.automatic;
        identity.name = state.name.clone();
        configuration::save_private(&state.namespace, identity);
    }
}

fn context(app: &App) -> Option<(ServerInfo, ConnectCredential)> {
    if app.connect.busy || app.connect.services.busy {
        return None;
    }
    let info = app.connect.server_info.clone()?;
    let credential = app.connect.credential.clone()?;
    if credential.needs_refresh(discophile_core::now_millis())
        || !credential.scopes.iter().any(|s| s == "connect:manage")
        || !info.capability_usable("devices")
    {
        return None;
    }
    Some((info, credential))
}

fn run(app: &mut App, mut operation: Value, done: impl FnOnce(&mut App, Value) + Send + 'static) {
    if app.connect.busy || app.connect.services.busy {
        app.connect.services.pending.push_back((operation, Box::new(done)));
        return;
    }
    let Some((info, credential)) = context(app) else {
        app.connect.services.error = Some(t(
            "Refresh your account or sign in again to allow Connect services.",
        ));
        return;
    };
    if operation.get("deviceId").is_none()
        && let Some(identity) = app.connect.services.identity.as_ref()
    {
        operation["deviceId"] = json!(identity.device_id);
    }
    let generation = app.connect.generation;
    let action = operation["action"].as_str().unwrap_or_default().to_string();
    let retry_operation = operation.clone();
    app.connect.services.busy = true;
    app.connect.services.background = matches!(action.as_str(), "poll" | "state" | "send" | "ack");
    app.tasks.spawn(async move {
        let result = ConnectClient::new()
            .service(&info, &credential, &operation)
            .await;
        update(move |app: &mut App| {
            if app.connect.generation != generation {
                return;
            }
            app.connect.services.busy = false;
            match result {
                Ok(value) => {
                    if let Ok(id) = serde_json::from_value(retry_operation["id"].clone()) { app.connect.services.send_retries.remove(&id); }
                    if action == "join" { app.connect.services.registration_replaced = false; }
                    app.connect.services.error = None;
                    done(app, value);
                }
                Err(error) => {
                    if action == "join" && matches!(&error, ConnectError::Service(code) if code == "revision_conflict") && account_sync(app) {
                        recover_account(app);
                        return;
                    }
                    if action == "join" && matches!(&error, ConnectError::Service(code) if code == "forbidden")
                        && account_sync(app) && !app.connect.services.registration_replaced
                    {
                        replace_registration(app);
                        return;
                    }
                    let transient = matches!(
                        error,
                        ConnectError::Network
                            | ConnectError::RateLimited { .. }
                            | ConnectError::ServerUnavailable(_)
                    );
                    if transient && action == "send" {
                        if let Ok(id) = serde_json::from_value::<Uuid>(retry_operation["id"].clone()) {
                            let attempts = app.connect.services.send_retries.entry(id).or_default();
                            if *attempts < 3 {
                                *attempts += 1;
                                let delay = match &error { ConnectError::RateLimited { retry_after } => retry_after.unwrap_or(3).max(1) as i64 * 1000, _ => 1_000 };
                                app.connect.services.retry_after = discophile_core::now_millis() + delay;
                                app.connect.services.outgoing.push_front(retry_operation);
                            } else { app.connect.services.send_retries.remove(&id); }
                        }
                    } else if let Ok(id) = serde_json::from_value::<Uuid>(retry_operation["id"].clone()) {
                        app.connect.services.send_retries.remove(&id);
                    }
                    if action == "write" && matches!(&error, ConnectError::Service(code) if code == "revision_conflict") {
                        app.connect.services.sync_requested = true;
                        app.connect.services.last_poll = 0;
                        return;
                    }
                    app.connect.services.error = Some(super::error_message(&error));
                    app.connect.services.paused_after_error = !transient
                        && matches!(action.as_str(), "rotate" | "poll" | "join");
                    if error == ConnectError::Unauthorized {
                        super::refresh_account(app);
                    }
                }
            }
        })
    });
}

pub fn inspect(app: &mut App) {
    let Some((info, credential)) = context(app) else {
        return;
    };
    let generation = app.connect.generation;
    app.connect.services.busy = true;
    app.connect.services.background = false;
    app.tasks.spawn(async move {
        let client = ConnectClient::new();
        let result = client
            .service(&info, &credential, &json!({"action":"inspect"}))
            .await;
        let catalog = client.catalog(&info).await.ok();
        update(move |app: &mut App| {
            if app.connect.generation != generation {
                return;
            }
            app.connect.services.busy = false;
            app.connect.services.catalog = catalog;
            match result {
                Ok(value) => {
                    let vault = value
                        .get("vault")
                        .filter(|v| !v.is_null())
                        .and_then(|v| serde_json::from_value::<VaultInfo>(v.clone()).ok());
                    app.connect.services.vault = vault;
                    app.connect.services.error = None;
                    if app.connect.services.reviewing {
                        review(app);
                    } else if app.connect.services.vault.is_some()
                        && app.connect.services.identity.is_some()
                        && synchronization_started(app)
                    {
                        let pending_rotation = app.connect.services.vault.as_ref().zip(app.connect.services.identity.as_ref())
                            .is_some_and(|(v, i)| v.rotation_required && v.id == i.keys.vault_id && i.keys.epoch == v.epoch + 1);
                        if pending_rotation { rotate_key(app); return; }
                        let needs_key = app.connect.services.vault.as_ref().zip(app.connect.services.identity.as_ref())
                            .is_some_and(|(v, i)| v.id != i.keys.vault_id || v.epoch != i.keys.epoch);
                        if account_sync(app) && needs_key { recover_account(app); } else { pair(app, false); }
                    } else if app.connect.services.vault.is_none() {
                        app.connect.services.paired = false;
                        app.connect.services.cloud = None;
                    }
                }
                Err(error) => app.connect.services.error = Some(super::error_message(&error)),
            }
        })
    });
}

fn synchronization_started(app: &App) -> bool {
    app.connect.services.identity.as_ref().is_some_and(|i| i.sync_base.is_some() || !i.baseline.is_empty())
}

pub fn start_review(app: &mut App) {
    let state = &mut app.connect.services;
    state.reviewing = true;
    state.review_ready = false;
    state.selected.clear();
    state.preferences = false;
    inspect(app);
}

fn review(app: &mut App) {
    if app.connect.services.vault.is_none() {
        if synchronization_started(app) {
            app.connect.services.identity = None;
        }
        app.connect.services.cloud = None;
        app.connect.services.review_ready = true;
        return;
    }
    if prepare_review(app) {
        return;
    }
    if account_sync(app) {
        recover_account(app);
    }
}

pub fn prepare_review(app: &mut App) -> bool {
    let state = &mut app.connect.services;
    let Some(vault) = state.vault.as_ref() else { return false };
    let Some(instance) = app.connect.credential.as_ref().map(|c| c.instance_id.as_str()) else { return false };
    let keys = if state.recovery_input.trim().is_empty() {
        state.identity.as_ref().map(|identity| identity.keys.clone())
    } else {
        VaultKeys::recover(&state.recovery_input).ok()
    };
    if let Some(config) = keys.and_then(|keys| open_configuration(&keys, vault, instance)) {
        state.cloud = Some(config);
        state.review_ready = true;
        true
    } else {
        false
    }
}

pub fn confirm_review(app: &mut App) {
    if !app.connect.services.review_ready { return; }
    let create = app.connect.services.vault.is_none();
    if create && (account_sync(app) || app.connect.services.identity.is_none()) {
        prepare(app);
    }
    pair(app, create);
}

fn open_configuration(keys: &VaultKeys, vault: &VaultInfo, instance: &str) -> Option<Configuration> {
    let config = keys.open::<Configuration>(vault.envelope.as_ref()?, instance, "configuration").ok()?;
    config.validate().then_some(config)
}

/// The account refused this computer's device registration, usually because another device
/// revoked it. With account sync the keys come from the account, so register again once.
fn replace_registration(app: &mut App) {
    let Some(secret) = discophile_core::oauth::random_token() else { return };
    let state = &mut app.connect.services;
    let Some(identity) = state.identity.as_mut() else { return };
    identity.device_id = Uuid::new_v4();
    identity.device_secret = secret;
    identity.state_revision = 0;
    identity.seen.clear();
    identity.state_watermarks.clear();
    state.registration_replaced = true;
    state.paired = false;
    state.error = None;
    state.paused_after_error = false;
    save(app);
    inspect(app);
}

pub fn prepare(app: &mut App) {
    let Ok(keys) = VaultKeys::new() else { return };
    let Some(secret) = discophile_core::oauth::random_token() else {
        return;
    };
    app.connect.services.identity = Some(Identity {
        device_id: Uuid::new_v4(),
        device_secret: secret,
        keys,
        watermark: Default::default(),
        state_revision: 0,
        seen: Default::default(),
        state_watermarks: Default::default(),
        selected: app.connect.services.selected.clone(),
        preferences: app.connect.services.preferences,
        automatic: false,
        baseline: String::new(),
        name: app.connect.services.name.clone(),
        select_new_connections: account_sync(app),
        known_connections: configuration::choices(app).iter().map(|c| c.id()).collect(),
        sync_base: None,
        removed: Default::default(),
        replacement: None,
    });
    app.connect.services.recovery_visible = !account_sync(app);
    save(app);
}

pub fn pair(app: &mut App, create: bool) {
    if app.connect.services.busy {
        return;
    }
    let Some(credential) = app.connect.credential.as_ref() else {
        return;
    };
    let instance = credential.instance_id.clone();
    let mut identity = if let Some(identity) = app.connect.services.identity.clone() {
        identity
    } else {
        let keys = if create {
            VaultKeys::new()
        } else {
            VaultKeys::recover(&app.connect.services.recovery_input)
        };
        let keys = match keys {
            Ok(keys) => keys,
            Err(_) => {
                app.connect.services.error = Some(t(
                    "The recovery code is invalid. Copy it from a trusted computer.",
                ));
                return;
            }
        };
        Identity {
            device_id: Uuid::new_v4(),
            device_secret: discophile_core::oauth::random_token().unwrap_or_default(),
            keys,
            watermark: Default::default(),
            state_revision: 0,
            seen: Default::default(),
            state_watermarks: Default::default(),
            selected: Default::default(),
            preferences: app.connect.services.preferences,
            automatic: false,
            baseline: String::new(),
            name: app.connect.services.name.clone(),
            select_new_connections: account_sync(app),
            known_connections: Default::default(),
            sync_base: None,
            removed: Default::default(),
            replacement: None,
        }
    };
    if identity.device_secret.len() != 43 {
        app.connect.services.error = Some(t("Unable to generate a device identity."));
        return;
    }
    let vault = app.connect.services.vault.clone();
    let activating = !synchronization_started(app);
    let review_base = Manifest::of(app.connect.services.cloud.as_ref().unwrap_or(&Configuration::default()));
    let requested = configuration::snapshot(app);
    if !app.connect.services.recovery_input.trim().is_empty() {
        match VaultKeys::recover(&app.connect.services.recovery_input) {
            Ok(keys) => identity.keys = keys,
            Err(_) => {
                app.connect.services.error = Some(t(
                    "The recovery code is invalid. Copy it from a trusted computer.",
                ));
                return;
            }
        }
    }
    let mut initial = None;
    if let Some(vault) = &vault {
        if identity.keys.epoch != vault.epoch || identity.keys.vault_id != vault.id {
            app.connect.services.error =
                Some(t("Use the latest recovery code from a trusted computer."));
            return;
        }
        if let Some(envelope) = &vault.envelope
            && (open_configuration(&identity.keys, vault, &instance).is_none() || identity.watermark.observe(envelope).is_err())
        {
            app.connect.services.error = Some(t(
                "The encrypted configuration could not be authenticated. Check your recovery code.",
            ));
            return;
        }
    } else {
        if !create || (!account_sync(app) && !app.connect.services.confirm_recovery_saved) {
            return;
        }
        let configuration = &requested;
        if !configuration.validate() || configuration.connections.len() > connection_limit(app) {
            return;
        }
        initial = identity
            .keys
            .seal(
                &instance,
                identity.device_id,
                "configuration",
                1,
                configuration,
            )
            .ok();
        if initial.is_none() {
            return;
        }
    }
    if activating {
        identity.known_connections = configuration::choices(app).iter().map(|c| c.id()).collect();
        identity.select_new_connections = true;
    }
    let mut operation = json!({"action":"join", "deviceId":identity.device_id, "deviceSecret":identity.device_secret,
        "name":app.connect.services.name.trim(), "vaultId":identity.keys.vault_id, "epoch":identity.keys.epoch,
        "connectionCount":configuration::snapshot(app).connections.len() });
    if let Some(envelope) = initial {
        operation["envelope"] = json!(envelope);
    }
    if account_sync(app) && vault.as_ref().is_none_or(|v| !v.rotation_required) {
        operation["recoveryCode"] = json!(identity.keys.recovery_code());
    }
    // Persist the only copy before the first network write, including uncertain network outcomes.
    app.connect.services.identity = Some(identity);
    save(app);
    run(app, operation, move |app, value| {
        app.connect.services.vault = serde_json::from_value(value["vault"].clone()).ok();
        app.connect.services.paired = true;
        app.connect.services.paused_after_error = false;
        app.connect.services.recovery_input.clear();
        app.connect.services.reviewing = false;
        app.connect.services.review_ready = false;
        if activating {
            app.connect.services.automatic = true;
            if let Some(identity) = app.connect.services.identity.as_mut() {
                identity.sync_base = Some(SyncBase { local: Manifest::default(), cloud: review_base });
            }
        }
        synchronize(app, true, ConflictChoice::Review);
        save(app);
        refresh(app);
    });
}

pub fn import_recovery(app: &mut App) {
    let Ok(keys) = VaultKeys::recover(&app.connect.services.recovery_input) else {
        app.connect.services.error = Some(t(
            "The recovery code is invalid. Copy it from a trusted computer.",
        ));
        return;
    };
    let Some(vault) = app.connect.services.vault.as_ref() else {
        return;
    };
    if keys.vault_id != vault.id || keys.epoch != vault.epoch {
        app.connect.services.error =
            Some(t("Use the latest recovery code from a trusted computer."));
        return;
    }
    let Some(instance) = app
        .connect
        .credential
        .as_ref()
        .map(|c| c.instance_id.clone())
    else {
        return;
    };
    if vault.envelope.as_ref().is_none_or(|e| {
        keys.open::<Configuration>(e, &instance, "configuration")
            .is_err()
    }) {
        app.connect.services.error = Some(t(
            "The encrypted configuration could not be authenticated. Check your recovery code.",
        ));
        return;
    }
    if let Some(identity) = app.connect.services.identity.as_mut() {
        identity.keys = keys;
    }
    app.connect.services.recovery_input.clear();
    save(app);
    refresh(app);
}

pub fn refresh(app: &mut App) {
    app.connect.services.paused_after_error = false;
    app.connect.services.last_poll = 0;
    if !app.connect.services.paired {
        inspect(app);
    }
}

pub fn synchronize_now(app: &mut App) {
    app.connect.services.sync_requested = true;
    refresh(app);
}

pub fn remove_shared_connection(app: &mut App, id: String) {
    if app.connect.services.cloud.as_ref().is_none_or(|cloud| !cloud.connections.iter().any(|entry| entry.id() == id)) {
        return;
    }
    if let Some(identity) = app.connect.services.identity.as_mut() {
        identity.removed.insert(id);
    }
    app.connect.services.remove_target = None;
    save(app);
    synchronize_now(app);
}

pub fn replace_shared_connection(app: &mut App, cloud_id: String, local_id: String) {
    let state = &mut app.connect.services;
    let Some(identity) = state.identity.as_mut() else { return };
    identity.replacement = Some((cloud_id, local_id.clone()));
    state.selected.insert(local_id);
    state.replace_target = None;
    save(app);
    synchronize_now(app);
}

pub fn remove_shared_preferences(app: &mut App) {
    if let Some(identity) = app.connect.services.identity.as_mut() {
        identity.removed.insert(configuration::PREFERENCES_ID.into());
        app.connect.services.preferences = false;
    }
    app.connect.services.remove_target = None;
    save(app);
    synchronize_now(app);
}

fn accept_configuration(app: &mut App, config: &Configuration) {
    configuration::import(app, config);
    let local = configuration::snapshot(app);
    let known = app.connect.services.identity.as_ref().filter(|i| i.sync_base.is_none())
        .map(|_| configuration::choices(app).iter().map(|entry| entry.id()).collect::<BTreeSet<_>>());
    let state = &mut app.connect.services;
    if let Some(identity) = state.identity.as_mut() {
        identity.select_new_connections = true;
        if let Some(known) = known { identity.known_connections.extend(known); }
        identity.baseline = configuration::hash(&local);
        identity.sync_base = Some(SyncBase { local: Manifest::of(&local), cloud: Manifest::of(config) });
        identity.known_connections.extend(config.connections.iter().map(|entry| entry.id()));
        identity.removed.clear();
        identity.replacement = None;
    }
    state.conflict = None;
    state.pending_cloud = None;
    state.conflict_choice = None;
}

fn synchronize(app: &mut App, force_import: bool, choice: ConflictChoice) {
    let state = &app.connect.services;
    let Some(vault) = state.vault.as_ref() else { return };
    let Some(identity) = state.identity.as_ref() else { return };
    let Some(instance) = app.connect.credential.as_ref().map(|c| c.instance_id.as_str()) else { return };
    let Some(config) = open_configuration(&identity.keys, vault, instance) else {
        app.connect.services.error = Some(t("The encrypted configuration could not be authenticated. Check your recovery code."));
        return;
    };
    let Some(envelope) = vault.envelope.as_ref() else { return };
    let mut watermark = identity.watermark.clone();
    if watermark.observe(envelope).is_err() {
        app.connect.services.error = Some(t("Connect returned an older or conflicting encrypted configuration."));
        return;
    }
    let local = configuration::snapshot(app);
    let manifest = Manifest::of(&config);
    let legacy_base = || {
        if identity.baseline == configuration::hash(&local) {
            SyncBase { local: Manifest::of(&local), cloud: manifest.clone() }
        } else {
            // Legacy installations saved a single local digest. Review different copies of the same source.
            SyncBase::default()
        }
    };
    let base = identity.sync_base.clone().unwrap_or_else(legacy_base);
    let first_import = identity.sync_base.is_none();
    let mut removed = identity.removed.clone();
    if let Some((previous, replacement)) = &identity.replacement
        && local.connections.iter().any(|entry| entry.id() == *replacement)
    {
        removed.insert(previous.clone());
    }
    let pending_removals = !removed.is_empty();
    let cloud_changed = manifest != base.cloud;
    let merged = configuration::merge(&local, &config, &base, &removed, state.preferences, choice);
    let state = &mut app.connect.services;
    state.identity.as_mut().unwrap().watermark = watermark;
    state.cloud = Some(config.clone());
    if !state.automatic && !state.sync_requested {
        if cloud_changed {
            state.pending_cloud = Some(config);
        }
        return;
    }
    state.sync_requested = false;
    match merged {
        Err(()) => state.conflict = Some(config),
        Ok(next) if !configuration::equivalent(&config, &next) => {
            write_configuration(app, next, false);
        }
        Ok(_) => {
            if force_import || first_import || cloud_changed || pending_removals || state.conflict.is_some() {
                accept_configuration(app, &config);
            }
        }
    }
}

pub fn rotate_key(app: &mut App) {
    if app.connect.services.busy {
        return;
    }
    let Some(vault) = app.connect.services.vault.as_ref() else { return };
    let Some(identity) = app.connect.services.identity.as_ref() else { return };
    let Some(instance) = app.connect.credential.as_ref().map(|c| c.instance_id.as_str()) else { return };
    if let Some(config) = open_configuration(&identity.keys, vault, instance) {
        write_configuration(app, config, true);
    }
}

fn write_configuration(app: &mut App, config: Configuration, rotate: bool) {
    let Some(vault) = app.connect.services.vault.clone() else {
        return;
    };
    if !config.validate() || (!rotate && config.connections.len() > connection_limit(app)) {
        app.connect.services.error = Some(t(
            "Choose fewer connections for this plan, or request Connect.",
        ));
        return;
    }
    let Some(mut identity) = app.connect.services.identity.clone() else {
        return;
    };
    if rotate && identity.keys.epoch == vault.epoch {
        app.connect.services.outgoing.clear();
        if identity.keys.rotate().is_err() {
            return;
        }
        // Keep the next key even if the response is lost; recovery can retry the same epoch.
        app.connect.services.identity = Some(identity.clone());
        save(app);
    }
    let Some(instance) = app
        .connect
        .credential
        .as_ref()
        .map(|c| c.instance_id.clone())
    else {
        return;
    };
    let Ok(envelope) = identity.keys.seal(
        &instance,
        identity.device_id,
        "configuration",
        vault.revision + 1,
        &config,
    ) else {
        app.connect.services.error =
            Some(t("The selected configuration is too large to synchronize."));
        return;
    };
    let next = envelope.clone();
    let local_at_send = configuration::snapshot(app);
    let removed_at_send = identity.removed.clone();
    let replacement_at_send = identity.replacement.clone();
    let mut operation = json!({"action":if rotate {"rotate"} else {"write"}, "expectedRevision":vault.revision, "connectionCount":config.connections.len(), "envelope":envelope});
    if account_sync(app) {
        operation["recoveryCode"] = json!(identity.keys.recovery_code());
    }
    run(
        app,
        operation,
        move |app, _| {
            if let Some(vault) = app.connect.services.vault.as_mut() {
                vault.revision = next.revision;
                vault.epoch = next.epoch;
                vault.rotation_required = false;
                vault.envelope = Some(next.clone());
            }
            if let Some(identity) = app.connect.services.identity.as_mut() {
                let _ = identity.watermark.observe(&next);
            }
            app.connect.services.cloud = Some(config.clone());
            if !rotate && configuration::snapshot(app) == local_at_send
                && app.connect.services.identity.as_ref().is_some_and(|i| i.removed == removed_at_send && i.replacement == replacement_at_send)
            {
                accept_configuration(app, &config);
            } else if !rotate {
                // Local changes arrived while the write was in flight. Preserve them for the next poll.
                if let Some(identity) = app.connect.services.identity.as_mut() {
                    identity.sync_base = Some(SyncBase { local: Manifest::of(&local_at_send), cloud: Manifest::of(&config) });
                    identity.removed.retain(|id| !removed_at_send.contains(id));
                    if identity.replacement == replacement_at_send && config.connections.iter().any(|entry| replacement_at_send.as_ref().is_some_and(|(_, id)| entry.id() == *id)) {
                        identity.replacement = None;
                    }
                }
                app.connect.services.sync_requested = true;
            }
            app.connect.services.paused_after_error = false;
            if rotate {
                app.connect.services.paired = true;
                app.connect.services.recovery_visible = !account_sync(app);
                app.connect.services.notice = Some(t(if account_sync(app) {
                    "Encryption updated. Your other computers will reconnect through your account."
                } else {
                    "The key was rotated. Give the new recovery code to each remaining trusted computer."
                }));
            }
            save(app);
        },
    );
}

pub fn backup(app: &mut App) {
    if !paid(app, "backup.configuration") {
        return;
    }
    run(app, json!({"action":"backup"}), |app, _| {
        app.connect.services.last_backup = discophile_core::now_millis();
        app.connect.services.notice = Some(t("Encrypted configuration backup saved."));
    });
}
pub fn restore(app: &mut App, id: Uuid) {
    run(
        app,
        json!({"action":"restore", "id":id}),
        move |app, value| {
            let envelope = serde_json::from_value::<Envelope>(value["envelope"].clone());
            let Some(identity) = app.connect.services.identity.as_ref() else {
                return;
            };
            let Some(instance) = app
                .connect
                .credential
                .as_ref()
                .map(|c| c.instance_id.as_str())
            else {
                return;
            };
            if let Ok(envelope) = envelope
                && let Ok(config) =
                    identity
                        .keys
                        .open::<Configuration>(&envelope, instance, "configuration")
                && config.validate()
            {
                app.connect.services.restore_target = Some((id, config));
            } else {
                app.connect.services.error = Some(t(
                    "This backup could not be decrypted with your recovery keys.",
                ));
            }
        },
    );
}
pub fn apply_restore(app: &mut App) {
    let Some((_, config)) = app.connect.services.restore_target.take() else {
        return;
    };
    let selected = app.connect.services.selected.clone();
    if configuration::import(app, &config) {
        app.connect.services.selected = selected;
        let local = configuration::snapshot(app);
        if let Some(identity) = app.connect.services.identity.as_mut() {
            identity.baseline = configuration::hash(&local);
            identity.known_connections.extend(config.connections.iter().map(|entry| entry.id()));
            if let Some(base) = identity.sync_base.as_mut() {
                base.local = Manifest::of(&local);
            }
        }
        save(app);
        app.connect.services.notice = Some(t("Configuration restored on this computer."));
    }
}
pub fn revoke(app: &mut App, target: Uuid) {
    run(
        app,
        json!({"action":"revoke", "targetId":target}),
        move |app, _| {
            app.connect.services.revoke_target = None;
            if app
                .connect
                .services
                .identity
                .as_ref()
                .is_some_and(|i| i.device_id == target)
            {
                app.connect.services.paired = false;
                app.connect.services.automatic = false;
                secrets::delete(&app.connect.services.namespace);
                app.connect.services.identity = None;
            } else if let Some(vault) = app.connect.services.vault.as_mut() {
                vault.rotation_required = true;
                rotate_key(app);
            }
        },
    );
}
pub fn delete(app: &mut App) {
    run(app, json!({"action":"delete"}), |app, _| {
        secrets::delete(&app.connect.services.namespace);
        app.connect.services = ServicesState {
            notice: Some(t(
                "Connect cloud data deleted. Your local music and downloads are unchanged.",
            )),
            ..Default::default()
        };
    });
}
pub fn use_cloud(app: &mut App) {
    resolve_conflict(app, ConflictChoice::Cloud);
}

pub fn use_local(app: &mut App) {
    resolve_conflict(app, ConflictChoice::Local);
}

fn resolve_conflict(app: &mut App, choice: ConflictChoice) {
    if let Some(vault) = app.connect.services.vault.as_ref() {
        app.connect.services.conflict_choice = Some((vault.revision, choice));
        synchronize_now(app);
    }
}

pub fn resume_automatic(app: &mut App) {
    app.connect.services.automatic = true;
    save(app);
    synchronize_now(app);
}

pub fn rename(app: &mut App) {
    let Some(identity) = app.connect.services.identity.as_ref() else { return };
    let operation = json!({"action":"join", "deviceId":identity.device_id, "deviceSecret":identity.device_secret,
        "name":app.connect.services.name.trim(), "vaultId":identity.keys.vault_id, "epoch":identity.keys.epoch, "connectionCount":0});
    run(app, operation, |app, _| { save(app); refresh(app); });
}

pub fn forget_registration(app: &mut App) {
    secrets::delete(&app.connect.services.namespace);
    let state = &mut app.connect.services;
    state.identity = None;
    state.paired = false;
    state.automatic = false;
    state.outgoing.clear();
    state.error = None;
    state.paused_after_error = false;
}

fn poll(app: &mut App) {
    run(app, json!({"action":"poll"}), |app, value| {
        let Ok(poll) = serde_json::from_value::<Poll>(value) else {
            app.connect.services.error = Some(t("Connect returned an invalid services response."));
            return;
        };
        if poll.devices.len() > 100 || poll.backups.len() > 64 || poll.messages.len() > 50 {
            return;
        }
        if let Some(account) = app.connect.account.as_mut() {
            account.plan = poll.plan;
        }
        let Some(identity) = app.connect.services.identity.as_ref() else {
            return;
        };
        if poll.vault.id != identity.keys.vault_id || poll.vault.epoch < identity.watermark.epoch {
            app.connect.services.error = Some(t(
                "The encrypted configuration could not be authenticated. Check your recovery code.",
            ));
            return;
        }
        app.connect.services.vault = Some(poll.vault.clone());
        app.connect.services.devices = poll.devices;
        app.connect.services.backups = poll.backups;
        if poll.vault.rotation_required {
            if account_sync(app) && poll.vault.epoch == identity.keys.epoch {
                rotate_key(app);
                return;
            }
            app.connect.services.error = Some(t(
                "A device was revoked. Rotate the encryption key before synchronizing again.",
            ));
            return;
        }
        if poll.vault.epoch > identity.keys.epoch {
            if account_sync(app) {
                recover_account(app);
                return;
            }
            app.connect.services.error =
                Some(t("Use the latest recovery code from a trusted computer."));
            return;
        }
        let choice = app.connect.services.conflict_choice.filter(|(revision, _)| *revision == poll.vault.revision)
            .map(|(_, choice)| choice).unwrap_or_default();
        synchronize(app, false, choice);
        save(app);
        playback::receive_poll(app, &poll.messages);
    });
}

pub fn tick(app: &mut App) {
    let Some(credential) = app.connect.credential.as_ref() else {
        return;
    };
    // Any intervening frame (including a network completion) consumes egui's repaint timer.
    // Rearm it even while idle or busy so discovery and incoming controls never depend on audio.
    let interval = if playback::transfer_pending(app) { 1_000 } else { POLL_INTERVAL_MS };
    app.ctx().request_repaint_after(std::time::Duration::from_millis(interval as u64));
    if app.connect.account.is_none() {
        return;
    }
    if credential.needs_refresh(discophile_core::now_millis()) {
        if !app.connect.busy {
            super::refresh_account(app);
        }
        return;
    }
    let ns = namespace(credential);
    if !app.connect.services.loaded || app.connect.services.namespace != ns {
        let stored =
            secrets::get(&ns).and_then(|text| serde_json::from_str::<Identity>(&text).ok());
        let state = &mut app.connect.services;
        state.loaded = true;
        state.namespace = ns;
        state.name = t("This computer");
        if let Some(identity) = stored {
            state.selected = identity.selected.clone();
            state.preferences = identity.preferences;
            state.automatic = identity.automatic;
            state.name = identity.name.clone();
            state.identity = Some(identity);
        }
        inspect(app);
        return;
    }
    playback::tick(app);
    if app.connect.services.busy || app.connect.busy {
        return;
    }
    let retry_wait = app.connect.services.retry_after - discophile_core::now_millis();
    if retry_wait > 0 {
        app.ctx().request_repaint_after(std::time::Duration::from_millis(retry_wait as u64));
        return;
    }
    if let Some((operation, done)) = app.connect.services.pending.pop_front() {
        run(app, operation, done);
        return;
    }
    if let Some(operation) = app.connect.services.outgoing.pop_front() {
        run(app, operation, |_, _| {});
        return;
    }
    if !app.connect.services.paired || app.connect.services.paused_after_error {
        return;
    }
    if app.connect.services.automatic {
        let choices = configuration::choices(app);
        let limit = connection_limit(app);
        let state = &mut app.connect.services;
        let mut changed = false;
        if let Some(identity) = state.identity.as_mut() {
            for choice in choices {
                let id = choice.id();
                let new = identity.known_connections.insert(id.clone());
                changed |= new;
                let needs_review = state.cloud.as_ref().is_some_and(|cloud| {
                    cloud.connections.iter().any(|entry| choice.same_audiobookshelf_login(entry))
                });
                if new && !needs_review && identity.select_new_connections && state.selected.len() < limit {
                    state.selected.insert(id);
                }
            }
        }
        if changed { save(app); }
    }
    let now = discophile_core::now_millis();
    let remaining = interval - (now - app.connect.services.last_poll);
    if remaining > 0 {
        app.ctx().request_repaint_after(std::time::Duration::from_millis(remaining as u64));
        return;
    }
    app.connect.services.last_poll = now;
    poll(app);
    if paid(app, "playback.remote") && now - app.connect.services.last_presence >= POLL_INTERVAL_MS {
        app.connect.services.last_presence = now;
        playback::queue_presence(app);
    }
    if now - app.connect.services.last_backup > 86_400_000
        && paid(app, "backup.configuration")
        && app.connect.services.outgoing.len() < 10
    {
        app.connect.services.last_backup = now;
        app.connect
            .services
            .outgoing
            .push_back(json!({"action":"backup"}));
    }
}

pub use playback::{audiobook_failed, audiobook_prepared, audiobook_start_allowed};
pub use playback::{can_transfer, request_transfer, send_command, transfer_pending, transfer_to};
pub use playback::{DeviceKind, PresenceMedia, RemotePlayback, device_kind, listen_independently, remote_playback, route_command, take_local_control, transfer_status};
