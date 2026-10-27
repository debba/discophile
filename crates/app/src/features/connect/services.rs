//! Polling native service integration. Secrets and recovery material never enter plain app storage.
pub mod configuration;
mod playback;

use crate::{app::App, platform::secrets, tasks::update};
use configuration::Configuration;
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
            pair(app, false);
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
                    app.connect.services.error = Some(super::error_message(&error));
                    app.connect.services.paused_after_error = !transient
                        && matches!(action.as_str(), "write" | "rotate" | "poll" | "join");
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
                    if account_sync(app) && app.connect.services.vault.is_none() {
                        if app.connect.services.identity.is_none() {
                            let limit = connection_limit(app);
                            app.connect.services.selected = configuration::choices(app).into_iter().take(limit).map(|c| c.id()).collect();
                            app.connect.services.preferences = true;
                        }
                        // Without a vault nothing on the account is sealed with stored keys, and keys
                        // of a deleted vault can never create a new one: start from a fresh identity.
                        prepare(app);
                        pair(app, true);
                    } else if app.connect.services.vault.is_some()
                        && app.connect.services.identity.is_some()
                    {
                        let pending_rotation = app.connect.services.vault.as_ref().zip(app.connect.services.identity.as_ref())
                            .is_some_and(|(v, i)| v.rotation_required && v.id == i.keys.vault_id && i.keys.epoch == v.epoch + 1);
                        if pending_rotation { publish(app, true); return; }
                        let needs_key = app.connect.services.vault.as_ref().zip(app.connect.services.identity.as_ref())
                            .is_some_and(|(v, i)| v.id != i.keys.vault_id || v.epoch != i.keys.epoch);
                        if account_sync(app) && needs_key { recover_account(app); } else { pair(app, false); }
                    } else if account_sync(app) {
                        recover_account(app);
                    }
                }
                Err(error) => app.connect.services.error = Some(super::error_message(&error)),
            }
        })
    });
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
        }
    };
    if identity.device_secret.len() != 43 {
        app.connect.services.error = Some(t("Unable to generate a device identity."));
        return;
    }
    let vault = app.connect.services.vault.clone();
    let old_revision = identity.watermark.revision;
    let old_baseline = identity.baseline.clone();
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
    let mut imported = None;
    let mut initial = None;
    if let Some(vault) = &vault {
        if identity.keys.epoch != vault.epoch || identity.keys.vault_id != vault.id {
            app.connect.services.error =
                Some(t("Use the latest recovery code from a trusted computer."));
            return;
        }
        if let Some(envelope) = &vault.envelope {
            let opened = identity
                .keys
                .open::<Configuration>(envelope, &instance, "configuration");
            match opened {
                Ok(config) if config.validate() && identity.watermark.observe(envelope).is_ok() => {
                    imported = Some(config)
                }
                _ => {
                    app.connect.services.error = Some(t(
                        "The encrypted configuration could not be authenticated. Check your recovery code.",
                    ));
                    return;
                }
            }
        }
    } else {
        if !create || (!account_sync(app) && !app.connect.services.confirm_recovery_saved) {
            return;
        }
        let configuration = configuration::snapshot(app);
        initial = identity
            .keys
            .seal(
                &instance,
                identity.device_id,
                "configuration",
                1,
                &configuration,
            )
            .ok();
        if initial.is_none() {
            return;
        }
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
        let local = configuration::snapshot(app);
        let local_changed = !old_baseline.is_empty() && configuration::hash(&local) != old_baseline;
        let remote_changed = app
            .connect
            .services
            .vault
            .as_ref()
            .is_some_and(|v| v.revision > old_revision);
        let mut keep_baseline = false;
        if let Some(config) = imported {
            if !old_baseline.is_empty() && !app.connect.services.automatic {
                app.connect.services.pending_cloud = Some(config);
                keep_baseline = true;
            } else if local_changed && remote_changed && local != config {
                app.connect.services.conflict = Some(config);
                keep_baseline = true;
            } else if !local_changed {
                configuration::import(app, &config);
            } else {
                keep_baseline = true;
            }
        }
        if !keep_baseline {
            let baseline = configuration::hash(&configuration::snapshot(app));
            if let Some(identity) = app.connect.services.identity.as_mut() {
                identity.baseline = baseline;
            }
        }
        if old_baseline.is_empty() {
            app.connect.services.automatic = true;
            let known = configuration::choices(app).iter().map(|c| c.id()).collect();
            if let Some(identity) = app.connect.services.identity.as_mut() { identity.known_connections = known; }
        }
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

pub fn publish(app: &mut App, rotate: bool) {
    let Some(vault) = app.connect.services.vault.clone() else {
        return;
    };
    let local_config = configuration::snapshot(app);
    let mut config = local_config.clone();
    // Opting this computer out of preferences does not erase the other computers' shared preferences.
    if !app.connect.services.preferences {
        if let (Some(envelope), Some(identity), Some(credential)) = (vault.envelope.as_ref(), app.connect.services.identity.as_ref(), app.connect.credential.as_ref()) {
            if let Ok(remote) = identity.keys.open::<Configuration>(envelope, &credential.instance_id, "configuration") {
                config.preferences = remote.preferences;
            }
        }
    }
    if config.connections.len() > connection_limit(app) {
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
                identity.baseline = configuration::hash(&local_config);
            }
            app.connect.services.conflict = None;
            app.connect.services.pending_cloud = None;
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
            backup(app);
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
    if configuration::import(app, &config) {
        let baseline = configuration::hash(&configuration::snapshot(app));
        if let Some(identity) = app.connect.services.identity.as_mut() {
            identity.baseline = baseline;
        }
        save(app);
        app.connect.services.notice = Some(t(
            "Configuration restored locally. Publish it to update the other devices.",
        ));
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
                publish(app, true);
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
    let Some(config) = app.connect.services.conflict.take() else {
        return;
    };
    configuration::import(app, &config);
    let baseline = configuration::hash(&configuration::snapshot(app));
    if let Some(identity) = app.connect.services.identity.as_mut() {
        identity.baseline = baseline;
    }
    save(app);
}

pub fn resume_automatic(app: &mut App) {
    let Some(remote) = app.connect.services.pending_cloud.take() else { return };
    let local = configuration::snapshot(app);
    let changed = app.connect.services.identity.as_ref().is_some_and(|i| !i.baseline.is_empty() && i.baseline != configuration::hash(&local));
    app.connect.services.conflict = Some(remote);
    if !changed { use_cloud(app); }
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
        let Some(instance) = app
            .connect
            .credential
            .as_ref()
            .map(|c| c.instance_id.clone())
        else {
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
                publish(app, true);
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
        if let Some(envelope) = poll.vault.envelope.as_ref() {
            let config = identity
                .keys
                .open::<Configuration>(envelope, &instance, "configuration");
            let old_revision = identity.watermark.revision;
            let baseline = identity.baseline.clone();
            let local = configuration::snapshot(app);
            if let Ok(remote) = config
                && remote.validate()
            {
                let identity = app.connect.services.identity.as_mut().unwrap();
                if identity.watermark.observe(envelope).is_err() {
                    app.connect.services.error = Some(t(
                        "Connect returned an older or conflicting encrypted configuration.",
                    ));
                    return;
                }
                if envelope.revision > old_revision && !app.connect.services.automatic {
                    app.connect.services.pending_cloud = Some(remote);
                } else if envelope.revision > old_revision && app.connect.services.automatic {
                    if !baseline.is_empty()
                        && configuration::hash(&local) != baseline
                        && local != remote
                    {
                        app.connect.services.conflict = Some(remote);
                    } else {
                        configuration::import(app, &remote);
                        let baseline = configuration::hash(&configuration::snapshot(app));
                        app.connect.services.identity.as_mut().unwrap().baseline = baseline;
                    }
                }
            } else {
                app.connect.services.error = Some(t(
                    "The encrypted configuration could not be authenticated. Check your recovery code.",
                ));
                return;
            }
        }
        save(app);
        playback::receive_poll(app, &poll.messages);
        if app.connect.services.automatic
            && app.connect.services.conflict.is_none()
            && !app.connect.services.busy
        {
            let current = configuration::hash(&configuration::snapshot(app));
            if app
                .connect
                .services
                .identity
                .as_ref()
                .is_some_and(|i| !i.baseline.is_empty() && i.baseline != current)
            {
                publish(app, false);
            }
        }
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
                if new && identity.select_new_connections && state.selected.len() < limit {
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
