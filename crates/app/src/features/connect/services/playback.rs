use crate::{
    app::App,
    player::{self, Mode},
};
use discophile_core::connect::services::Message;
use discophile_core::connect::{ConnectMessage, PlaybackContent, PlaybackSnapshot, RemoteCommand};
use discophile_core::l10n::t;
use discophile_core::now_millis;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Presence {
    pub title: String,
    pub playing: bool,
    pub position: f64,
    pub captured_at: i64,
    pub can_handoff: bool,
    pub volume: f32,
    pub duration: Option<f64>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Body {
    version: u32,
    created_at: i64,
    reply_to: Option<Uuid>,
    message: ConnectMessage,
}
pub struct OutgoingTransfer {
    target: Uuid,
    message: Uuid,
    until: i64,
    generation: u64,
    was_playing: bool,
    signature: String,
}
pub struct IncomingTransfer {
    from: Uuid,
    message: Uuid,
    until: i64,
    content: String,
    playing: bool,
    generation: u64,
    rate: f32,
}

fn snapshot_content(snapshot: &PlaybackSnapshot) -> String {
    match &snapshot.content {
        PlaybackContent::Music {
            account_key,
            queue,
            index,
            ..
        } => format!(
            "{account_key}:{}",
            queue.get(*index).map(|s| s.id.as_str()).unwrap_or_default()
        ),
        PlaybackContent::Audiobook { account_key, item_id, .. } => format!("book:{account_key}:{item_id}"),
        PlaybackContent::Radio { station } => format!("radio:{:?}", Some(station.id)),
    }
}

fn own_id(app: &App) -> Option<Uuid> {
    app.connect.services.identity.as_ref().map(|i| i.device_id)
}
fn current_content(app: &App) -> String {
    match app.player.mode {
        Mode::Music => format!(
            "{}:{}",
            app.session.account_key(),
            app.player.current().map(|s| s.id).unwrap_or_default()
        ),
        Mode::Radio => format!("radio:{:?}", app.player.radio.as_ref().map(|s| s.id)),
        Mode::Audiobook => app.audiobooks.playback.as_ref().map(|p| format!("book:{}:{}", p.account, p.book.id)).unwrap_or_default(),
        _ => String::new(),
    }
}

fn transfer_signature(app: &App) -> String {
    let queue: Vec<_> = app.player.queue.songs().iter().map(|song| song.id.as_str()).collect();
    json!([current_content(app), queue, app.player.queue.index(), app.player.queue.repeat_mode, app.player.rate]).to_string()
}
fn snapshot(app: &App) -> Option<PlaybackSnapshot> {
    let content = match app.player.mode {
        Mode::Music => {
            // The transfer is rejected rather than silently dropping the end of a large queue.
            if app.player.queue.songs().len() > 200 {
                return None;
            }
            PlaybackContent::Music {
                account_key: app.session.account_key(),
                provider: app.session.provider()?,
                queue: app.player.queue.songs().to_vec(),
                index: app.player.queue.index(),
                repeat_mode: app.player.queue.repeat_mode,
            }
        }
        Mode::Radio => PlaybackContent::Radio {
            station: app.player.radio.clone()?,
        },
        Mode::Audiobook => {
            let playback = app.audiobooks.playback.as_ref()?;
            PlaybackContent::Audiobook {
                account_key: playback.account.clone(),
                item_id: playback.book.id.clone(),
                title: playback.book.title().to_string(),
            }
        }
        _ => return None,
    };
    let position = if app.player.mode == Mode::Audiobook {
        app.audiobooks.playback.as_ref()?.position
    } else {
        app.player.position()
    };
    Some(PlaybackSnapshot {
        content,
        position,
        playing: app.player.is_playing(),
        volume: app.player.volume,
        rate: app.player.rate,
        captured_at: now_millis(),
    })
}

fn enqueue(
    app: &mut App,
    target: Uuid,
    kind: &str,
    message: ConnectMessage,
    reply_to: Option<Uuid>,
) -> Option<Uuid> {
    if app.connect.services.outgoing.len() >= 30
        || !super::paid(
            app,
            if kind == "handoff" {
                "playback.handoff"
            } else {
                "playback.remote"
            },
        )
    {
        return None;
    }
    let identity = app.connect.services.identity.as_ref()?;
    if app
        .connect
        .services
        .vault
        .as_ref()
        .is_none_or(|v| v.rotation_required || v.epoch != identity.keys.epoch)
    {
        return None;
    }
    let instance = &app.connect.credential.as_ref()?.instance_id;
    let id = Uuid::new_v4();
    let body = Body {
        version: 1,
        created_at: now_millis(),
        reply_to,
        message,
    };
    let envelope = identity
        .keys
        .seal(
            instance,
            identity.device_id,
            &format!("message:{target}:{id}:{kind}"),
            1,
            &body,
        )
        .ok()?;
    app.connect.services.outgoing.push_back(
        json!({"action":"send", "id":id, "targetId":target, "kind":kind, "envelope":envelope}),
    );
    Some(id)
}

pub fn send_command(app: &mut App, target: Uuid, command: RemoteCommand) {
    enqueue(
        app,
        target,
        "remote",
        ConnectMessage::Command { target, command },
        None,
    );
}
pub fn request_transfer(app: &mut App, source: Uuid) {
    if transfer_pending(app) { return; }
    let Some(target) = own_id(app) else { return };
    if let Some(message) = enqueue(
        app,
        source,
        "handoff",
        ConnectMessage::TransferRequest { target },
        None,
    ) {
        app.connect.services.transfer_request = Some((source, message, now_millis() + 30_000));
        app.connect.services.playback_notice =
            Some(t("Asking the other computer to continue playback here…"));
    }
}

pub fn transfer_pending(app: &App) -> bool {
    app.connect.services.transfer.is_some() || app.connect.services.receiving.is_some() || app.connect.services.transfer_request.is_some()
}

pub fn can_transfer(app: &App) -> bool {
    super::paid(app, "playback.handoff") && app.connect.services.paired
        && !transfer_pending(app) && snapshot(app).is_some()
}

fn offer(app: &mut App, target: Uuid, reply_to: Option<Uuid>) -> bool {
    if app.connect.services.transfer.is_some() { return false; }
    let Some(own) = own_id(app) else { return false };
    let Some(snapshot) = snapshot(app) else { return false };
    let playing = snapshot.playing;
    let Some(id) = enqueue(app, target, "handoff", ConnectMessage::Transfer { from: own, snapshot }, reply_to) else { return false };
    app.connect.services.transfer = Some(OutgoingTransfer {
        target, message: id, until: now_millis() + 30_000,
        generation: app.player.control_generation, was_playing: playing, signature: transfer_signature(app),
    });
    true
}

pub fn transfer_to(app: &mut App, target: Uuid) {
    if !transfer_pending(app) && app.connect.services.devices.iter().any(|d| d.id == target && Some(d.id) != own_id(app) && d.online && !d.suspended)
        && offer(app, target, None) {
        app.connect.services.playback_notice = Some(t("Moving playback to the selected computer…"));
    }
}

pub fn queue_presence(app: &mut App) {
    let title = crate::ui::player_bar::actions::now_playing_text(app)
        .map(|(title, artist)| format!("{title} · {artist}"))
        .unwrap_or_else(|| t("Nothing playing"));
    let (position, duration) = crate::ui::player_bar::actions::timeline(app);
    let presence = Presence {
        title,
        playing: app.player.is_playing(),
        position,
        captured_at: now_millis(),
        can_handoff: snapshot(app).is_some(),
        volume: app.player.volume,
        duration,
    };
    let Some(identity) = app.connect.services.identity.as_mut() else {
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
    identity.state_revision += 1;
    let envelope = identity.keys.seal(
        instance,
        identity.device_id,
        &format!("state:{}", identity.device_id),
        identity.state_revision,
        &presence,
    );
    if let Ok(envelope) = envelope {
        // One pending heartbeat is enough; controls have precedence over obsolete presence.
        app.connect
            .services
            .outgoing
            .retain(|o| o["action"] != "state");
        app.connect
            .services
            .outgoing
            .push_back(json!({"action":"state", "envelope":envelope}));
        super::save(app);
    }
}

pub fn receive_poll(app: &mut App, messages: &[Message]) {
    let Some(instance) = app
        .connect
        .credential
        .as_ref()
        .map(|c| c.instance_id.clone())
    else {
        return;
    };
    let Some(own) = own_id(app) else { return };
    let devices = app.connect.services.devices.clone();
    for device in devices {
        if !device.online || device.id == own {
            continue;
        }
        if let Some(envelope) = device.state {
            let identity = app.connect.services.identity.as_mut().unwrap();
            if envelope.sender_id != device.id || envelope.epoch != identity.keys.epoch {
                continue;
            }
            if let Ok(presence) = identity.keys.open::<Presence>(
                &envelope,
                &instance,
                &format!("state:{}", device.id),
            ) {
                if (now_millis() - presence.captured_at).abs() > 60_000
                    || !presence.position.is_finite()
                {
                    continue;
                }
                if identity
                    .state_watermarks
                    .entry(device.id)
                    .or_default()
                    .observe(&envelope)
                    .is_ok()
                {
                    app.connect.services.states.insert(device.id, presence);
                }
            }
        }
    }
    let mut acknowledged = Vec::new();
    for message in messages {
        acknowledged.push(message.id);
        let identity = app.connect.services.identity.as_mut().unwrap();
        if identity.seen.contains(&message.id)
            || message.envelope.sender_id != message.sender_id
            || message.envelope.epoch != identity.keys.epoch
            || !app
                .connect
                .services
                .devices
                .iter()
                .any(|d| d.id == message.sender_id && !d.suspended)
        {
            continue;
        }
        let object = format!("message:{own}:{}:{}", message.id, message.kind);
        let Ok(body) = identity
            .keys
            .open::<Body>(&message.envelope, &instance, &object)
        else {
            continue;
        };
        if body.version != 1
            || now_millis() - body.created_at > 60_000
            || body.created_at - now_millis() > 5_000
        {
            continue;
        }
        identity.seen.push_back(message.id);
        // At most 3600 accepted operations can fall in a command's 60-second life across two
        // fixed rate-limit windows (1800/account/minute). Keep every potentially valid ID.
        while identity.seen.len() > 4096 {
            identity.seen.pop_front();
        }
        // Persist before applying a command so redelivery after an uncertain ACK cannot repeat it.
        super::save(app);
        apply(app, message, body);
    }
    if !acknowledged.is_empty() {
        app.connect
            .services
            .outgoing
            .push_back(json!({"action":"ack", "ids":acknowledged}));
    }
    super::save(app);
}

fn apply(app: &mut App, envelope: &Message, body: Body) {
    let Some(own) = own_id(app) else { return };
    match body.message {
        ConnectMessage::Command { target, command }
            if envelope.kind == "remote"
                && target == own
                && super::paid(app, "playback.remote") =>
        {
            match command {
                RemoteCommand::Play => player::play(app),
                RemoteCommand::Pause => player::pause(app),
                RemoteCommand::Next => player::next(app),
                RemoteCommand::Previous => player::previous(app),
                RemoteCommand::Seek { position }
                    if position.is_finite() && position >= 0.0 && position <= 31_536_000.0 =>
                {
                    player::seek(app, position)
                }
                RemoteCommand::SetVolume { volume }
                    if volume.is_finite() && (0.0..=1.0).contains(&volume) =>
                {
                    player::set_volume(app, volume)
                }
                _ => {}
            }
        }
        ConnectMessage::TransferRequest { target }
            if envelope.kind == "handoff"
                && target == envelope.sender_id
                && super::paid(app, "playback.handoff") =>
        {
            if app.connect.services.transfer.is_some() {
                return;
            }
            if !offer(app, target, Some(envelope.id)) {
                enqueue(
                    app,
                    target,
                    "handoff",
                    ConnectMessage::TransferRejected {
                        device_id: own,
                        reason: "unsupported_content".into(),
                    },
                    Some(envelope.id),
                );
            }
        }
        ConnectMessage::Transfer { from, snapshot }
            if envelope.kind == "handoff"
                && from == envelope.sender_id
                && super::paid(app, "playback.handoff") =>
        {
            let expected = app.connect.services.transfer_request.as_ref().is_some_and(
                |(source, id, until)| {
                    *source == from && Some(*id) == body.reply_to && *until >= now_millis()
                },
            );
            let direct = body.reply_to.is_none() && app.connect.services.receiving.is_none()
                && app.connect.services.transfer.is_none() && app.connect.services.transfer_request.is_none();
            if !expected && !direct {
                return;
            }
            app.connect.services.transfer_request = None;
            let playing = snapshot.playing;
            let content = snapshot_content(&snapshot);
            if accept(app, &snapshot) {
                app.connect.services.receiving = Some(IncomingTransfer {
                    from,
                    message: envelope.id,
                    until: now_millis() + 20_000,
                    content,
                    playing,
                    generation: app.player.control_generation,
                    rate: snapshot.rate,
                });
            } else {
                enqueue(
                    app,
                    from,
                    "handoff",
                    ConnectMessage::TransferRejected {
                        device_id: own,
                        reason: "library_unavailable".into(),
                    },
                    Some(envelope.id),
                );
                app.connect.services.error = Some(t(
                    "This computer cannot play the requested library or queue. Playback continues on the other computer.",
                ));
            }
        }
        ConnectMessage::TransferComplete { device_id }
            if envelope.kind == "handoff" && device_id == envelope.sender_id =>
        {
            let complete = app
                .connect
                .services
                .transfer
                .as_ref()
                .is_some_and(|pending| {
                    pending.target == device_id
                        && Some(pending.message) == body.reply_to
                        && pending.until >= now_millis()
                        && pending.generation == app.player.control_generation
                        && pending.was_playing == app.player.is_playing()
                        && pending.signature == transfer_signature(app)
                });
            if complete {
                player::pause(app);
                app.connect.services.transfer = None;
                app.connect.services.playback_notice = Some(t("Playback moved to the other computer."));
            }
        }
        ConnectMessage::TransferRejected { device_id, .. }
            if envelope.kind == "handoff" && device_id == envelope.sender_id =>
        {
            if app.connect.services.transfer_request.as_ref().is_some_and(|(source, id, _)| *source == device_id && Some(*id) == body.reply_to) {
                app.connect.services.transfer_request = None;
            }
            if app
                .connect
                .services
                .transfer
                .as_ref()
                .is_some_and(|p| p.target == device_id && Some(p.message) == body.reply_to)
            {
                app.connect.services.transfer = None;
            }
            app.connect.services.playback_notice = Some(t(
                "Playback transfer was not completed. The source computer keeps playing.",
            ));
        }
        _ => {}
    }
}

fn accept(app: &mut App, snapshot: &PlaybackSnapshot) -> bool {
    if !snapshot.position.is_finite()
        || !(0.0..=31_536_000.0).contains(&snapshot.position)
        || !snapshot.rate.is_finite()
        || !(0.5..=4.0).contains(&snapshot.rate)
        || (now_millis() - snapshot.captured_at).abs() > 60_000
    {
        return false;
    }
    let position = snapshot.position_at(now_millis());
    match &snapshot.content {
        PlaybackContent::Music {
            account_key,
            provider,
            queue,
            index,
            repeat_mode,
        } => {
            let Some(account) = app
                .session
                .accounts
                .iter()
                .find(|a| a.id() == *account_key && a.credentials.provider == *provider)
            else {
                return false;
            };
            if queue.is_empty()
                || queue.len() > 200
                || *index >= queue.len()
                || queue.iter().any(|s| s.id.is_empty() || s.id.len() > 1000)
            {
                return false;
            }
            let id = account.id();
            crate::session::switch_account(app, &id);
            player::play_transferred(
                app,
                queue.clone(),
                *index,
                *repeat_mode,
                position,
                snapshot.playing,
            );
        }
        PlaybackContent::Radio { station } => {
            if discophile_core::InternetRadioStation::new(
                Some(station.id),
                &station.name,
                station.stream_url.as_str(),
                station.allow_http,
            )
            .is_err()
            {
                return false;
            }
            player::play_radio(app, station.clone());
            if !snapshot.playing {
                player::pause(app);
            }
        }
        PlaybackContent::Audiobook {
            account_key,
            item_id,
            ..
        } => {
            if app
                .audiobooks
                .account
                .as_ref()
                .is_none_or(|a| a.id() != *account_key)
                || app.audiobooks.client.is_none()
            {
                return false;
            }
            if !crate::features::audiobooks::start_transferred(app, item_id, position, snapshot.playing) { return false; }
        }
    }
    if app.player.mode == Mode::Audiobook && current_content(app) == snapshot_content(snapshot) && !snapshot.playing {
        player::pause(app);
    }
    player::set_rate(app, snapshot.rate);
    // Volume belongs to this computer and is intentionally not copied from the sender.
    true
}

pub fn tick(app: &mut App) {
    let now = now_millis();
    if app
        .connect
        .services
        .transfer
        .as_ref()
        .is_some_and(|p| p.until < now)
    {
        app.connect.services.transfer = None;
        app.connect.services.playback_notice = Some(t(
            "Playback transfer timed out. The source computer keeps playing.",
        ));
    }
    if app
        .connect
        .services
        .transfer_request
        .as_ref()
        .is_some_and(|(_, _, until)| *until < now)
    {
        app.connect.services.transfer_request = None;
        app.connect.services.playback_notice = Some(t("Playback transfer timed out. The source computer keeps playing."));
    }
    let Some(pending) = app.connect.services.receiving.as_ref() else {
        return;
    };
    let actual = app.player.output.status();
    let same_content = current_content(app) == pending.content;
    let ready = same_content
        && app.audiobooks.starting.is_none()
        && (app.player.mode != Mode::Audiobook || app.audiobooks.playback.as_ref().is_some_and(|p| !p.loading))
        && actual.item_id.is_some()
        && actual.item_id == app.player.status.item_id
        && !actual.buffering
        && (actual.playing || !pending.playing)
        && pending.generation == app.player.control_generation;
    let failed = pending.until < now || app.player.error.is_some();
    if !ready && !failed {
        return;
    }
    let pending = app.connect.services.receiving.take().unwrap();
    if failed && pending.generation == app.player.control_generation {
        crate::features::audiobooks::cancel_pending_start(app);
        player::stop(app);
    }
    let Some(own) = own_id(app) else { return };
    let message = if ready {
        ConnectMessage::TransferComplete { device_id: own }
    } else {
        ConnectMessage::TransferRejected {
            device_id: own,
            reason: "playback_failed".into(),
        }
    };
    enqueue(app, pending.from, "handoff", message, Some(pending.message));
}

/// A known audiobook was prepared asynchronously without an intervening user action.
pub fn audiobook_prepared(app: &mut App) {
    if let Some(pending) = app.connect.services.receiving.as_ref() {
        if current_content(app) == pending.content {
            let playing = pending.playing;
            let rate = pending.rate;
            if !playing {
                player::pause(app);
            }
            player::set_rate(app, rate);
            if let Some(pending) = app.connect.services.receiving.as_mut() {
                pending.generation = app.player.control_generation;
            }
        }
    }
}

pub fn audiobook_start_allowed(app: &App) -> bool {
    app.connect
        .services
        .receiving
        .as_ref()
        .is_none_or(|pending| {
            pending.generation == app.player.control_generation && pending.until >= now_millis()
        })
}

pub fn audiobook_failed(app: &mut App) {
    if let Some(pending) = app.connect.services.receiving.as_mut() { pending.until = 0; }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{AudioSource, ContentKind, MediaItem};
    use crate::features::connect::services::Identity;
    use discophile_core::connect::services::{PairedDevice, VaultInfo};
    use discophile_core::connect::vault::VaultKeys;
    use discophile_core::connect::{
        Account, AccountInfo, ConnectCredential, Entitlement, InstanceOrigin, Plan, PlanSource,
        Tier, TokenSet,
    };
    use discophile_core::{Credentials, MusicProvider, ServerAccount, Song};

    fn app(id: Uuid, peer: Uuid, keys: &VaultKeys) -> App {
        let mut app = App::connect_test_app();
        let credentials = Credentials::simple(
            "http://127.0.0.1:9",
            "test",
            "fixture",
            true,
            MusicProvider::Subsonic,
        )
        .unwrap();
        let account = ServerAccount::new(credentials.clone(), "Fixture");
        app.session.accounts.push(account.clone());
        app.session.active = Some(account.id());
        app.session.client = Some(discophile_core::client::MusicClient::new(credentials));
        app.session.generation = 1;
        app.connect.credential = Some(ConnectCredential::new(
            InstanceOrigin::official(),
            "test-instance".into(),
            "test-owner".into(),
            "discophile-desktop".into(),
            TokenSet {
                access_token: "test".into(),
                refresh_token: "test".into(),
                scopes: vec![],
                expires_at: i64::MAX,
            },
        ));
        app.connect.account = Some(AccountInfo {
            instance_id: "test-instance".into(),
            account: Account {
                id: "test-owner".into(),
                name: "Test".into(),
                email: None,
                email_verified: true,
                avatar_url: None,
            },
            plan: Plan {
                tier: Tier::Connect,
                name: "Connect".into(),
                source: PlanSource::Manual,
                expires_at: None,
                entitlements: ["playback.remote", "playback.handoff"]
                    .into_iter()
                    .map(|id| Entitlement {
                        id: id.into(),
                        limit: None,
                        available: true,
                    })
                    .collect(),
            },
        });
        let state = &mut app.connect.services;
        state.namespace = format!("test-{}", Uuid::new_v4());
        state.identity = Some(Identity {
            device_id: id,
            device_secret: "test".into(),
            keys: keys.clone(),
            watermark: Default::default(),
            state_revision: 0,
            seen: Default::default(),
            state_watermarks: Default::default(),
            selected: Default::default(),
            preferences: false,
            automatic: false,
            baseline: String::new(),
            name: "Test".into(),
            select_new_connections: false,
            known_connections: Default::default(),
        });
        state.vault = Some(VaultInfo {
            id: keys.vault_id,
            epoch: keys.epoch,
            revision: 1,
            rotation_required: false,
            connection_count: 1,
            envelope: None,
        });
        state.devices = [id, peer]
            .into_iter()
            .map(|id| PairedDevice {
                id,
                name: "Computer".into(),
                online: true,
                suspended: false,
                state: None,
            })
            .collect();
        app
    }
    fn start(app: &mut App) {
        app.player.queue.replace(
            vec![Song::new("song-1", "One"), Song::new("song-2", "Two")],
            0,
            false,
        );
        app.player.mode = Mode::Music;
        app.player.output.load(
            MediaItem {
                id: "song-1".into(),
                source: AudioSource::Url("http://127.0.0.1:9/fixture".into()),
                kind: ContentKind::Music,
                duration_hint: Some(300.0),
                replay_gain: None,
                album_key: None,
                server_format: None,
            },
            42.0,
            true,
        );
        app.player.status = app.player.output.status();
    }
    fn sent(app: &mut App) -> serde_json::Value {
        let index = app
            .connect
            .services
            .outgoing
            .iter()
            .position(|v| v["action"] == "send")
            .expect("a message was sent");
        app.connect.services.outgoing.remove(index).unwrap()
    }
    fn deliver(app: &mut App, wire: serde_json::Value) {
        let envelope: discophile_core::connect::vault::Envelope =
            serde_json::from_value(wire["envelope"].clone()).unwrap();
        let message = Message {
            id: serde_json::from_value(wire["id"].clone()).unwrap(),
            sender_id: envelope.sender_id,
            kind: wire["kind"].as_str().unwrap().into(),
            envelope,
            expires_at: chrono::Utc::now().to_rfc3339(),
        };
        receive_poll(app, &[message]);
    }

    #[test]
    fn sending_playback_to_a_device_waits_for_confirmation_and_preserves_pause() {
        for playing in [true, false] {
            let keys = VaultKeys::new().unwrap();
            let source = Uuid::new_v4(); let target = Uuid::new_v4();
            let mut a = app(source, target, &keys); let mut b = app(target, source, &keys);
            a.connect.services.paired = true;
            start(&mut a);
            if !playing { player::pause(&mut a); }
            assert!(can_transfer(&a));
            transfer_to(&mut a, target);
            assert_eq!(a.player.is_playing(), playing);
            deliver(&mut b, sent(&mut a));
            assert_eq!(b.player.is_playing(), playing);
            tick(&mut b);
            deliver(&mut a, sent(&mut b));
            assert!(!a.player.is_playing());
            assert!(a.connect.services.transfer.is_none());
            assert_eq!(b.player.queue.songs().len(), 2);
        }
    }

    #[test]
    fn audiobook_handoff_fetches_an_unbrowsed_book_and_keeps_it_paused() {
        use discophile_core::audiobookshelf::{AudiobookshelfAccount, AudiobookshelfClient, AudiobookshelfTokens};
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let mut routes = Vec::new();
            for _ in 0..4 {
                let (mut socket, _) = listener.accept().unwrap();
                socket.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
                let mut data = Vec::new();
                loop {
                    let mut chunk = [0; 4096];
                    let n = socket.read(&mut chunk).unwrap(); assert!(n > 0);
                    data.extend_from_slice(&chunk[..n]);
                    if data.windows(4).any(|w| w == b"\r\n\r\n") { break; }
                }
                let request = String::from_utf8_lossy(&data);
                let route = request.lines().next().unwrap().split(' ').nth(1).unwrap().split('?').next().unwrap();
                let body = match route {
                    "/api/items/book" => r#"{"id":"book","libraryId":"library","mediaType":"book","media":{"metadata":{"title":"Synthetic book"},"duration":180}}"#,
                    "/api/me/progress/book" => "null",
                    "/api/items/book/play" => r#"{"id":"session","libraryItemId":"book","duration":180,"currentTime":0,"audioTracks":[{"startOffset":0,"duration":180,"contentUrl":"/s/item/book/file.mp3","mimeType":"audio/mpeg"}]}"#,
                    "/api/me" => "{}",
                    _ => panic!("unexpected fixture route: {route}"),
                };
                routes.push(route.to_string());
                let status = if route == "/api/me/progress/book" { "404 Not Found" } else { "200 OK" };
                let response = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                socket.write_all(response.as_bytes()).unwrap();
            }
            routes
        });
        let keys = VaultKeys::new().unwrap(); let source = Uuid::new_v4(); let target = Uuid::new_v4();
        let mut a = app(source, target, &keys); let mut b = app(target, source, &keys);
        let (tasks, updates) = crate::tasks::Tasks::new(egui::Context::default()); b.tasks = tasks;
        let account = AudiobookshelfAccount::new(url::Url::parse(&origin).unwrap(), "fixture-user", "fixture", None);
        b.audiobooks.client = Some(AudiobookshelfClient::new(account.clone(), AudiobookshelfTokens { access_token: "fixture-token".into(), refresh_token: None }, |_, _| Ok(())));
        b.audiobooks.account = Some(account.clone());
        assert!(crate::features::audiobooks::library::known_book(&b, "book").is_none());
        let snapshot = PlaybackSnapshot { content: PlaybackContent::Audiobook { account_key: account.id(), item_id: "book".into(), title: "Synthetic book".into() },
            position: 75.0, playing: false, volume: 0.5, rate: 1.5, captured_at: now_millis() };
        enqueue(&mut a, target, "handoff", ConnectMessage::Transfer { from: source, snapshot }, None);
        deliver(&mut b, sent(&mut a));
        for phase in 0..3 {
            let update = updates.recv_timeout(std::time::Duration::from_secs(5)).unwrap_or_else(|_| panic!("update phase {phase}; starting: {:?}; player error: {:?}; sync error: {:?}", b.audiobooks.starting, b.player.error, b.audiobooks.sync.error));
            update(&mut b);
        }
        b.player.status = b.player.output.status();
        tick(&mut b);
        assert_eq!(b.player.mode, Mode::Audiobook);
        assert!(!b.player.is_playing());
        assert_eq!(b.audiobooks.playback.as_ref().unwrap().position, 75.0);
        assert_eq!(b.player.rate, 1.5);
        assert!(b.connect.services.receiving.is_none());
        assert_eq!(sent(&mut b)["kind"], "handoff");
        assert!(server.join().unwrap().contains(&"/api/items/book".to_string()));
    }

    #[test]
    fn handoff_keeps_source_playing_until_destination_acknowledges() {
        let keys = VaultKeys::new().unwrap();
        let source = Uuid::new_v4();
        let target = Uuid::new_v4();
        let mut a = app(source, target, &keys);
        let mut b = app(target, source, &keys);
        start(&mut a);
        request_transfer(&mut b, source);
        deliver(&mut a, sent(&mut b));
        assert!(a.player.is_playing(), "a request must not pause the source");
        deliver(&mut b, sent(&mut a));
        assert!(
            a.player.is_playing(),
            "starting the destination must not pause the source"
        );
        assert_eq!(b.player.queue.songs().len(), 2);
        assert!(b.player.position() >= 42.0);
        tick(&mut b);
        deliver(&mut a, sent(&mut b));
        assert!(!a.player.is_playing());
        assert!(b.player.output.status().playing);
    }

    #[test]
    fn rejection_timeout_and_user_intervention_do_not_pause_the_source() {
        let keys = VaultKeys::new().unwrap();
        let source = Uuid::new_v4();
        let target = Uuid::new_v4();
        let mut a = app(source, target, &keys);
        let mut b = app(target, source, &keys);
        start(&mut a);
        b.session.accounts.clear();
        request_transfer(&mut b, source);
        deliver(&mut a, sent(&mut b));
        deliver(&mut b, sent(&mut a));
        deliver(&mut a, sent(&mut b));
        assert!(a.player.is_playing());
        b.session.accounts = a.session.accounts.clone();
        request_transfer(&mut b, source);
        deliver(&mut a, sent(&mut b));
        deliver(&mut b, sent(&mut a));
        tick(&mut b);
        player::seek(&mut a, 100.0);
        deliver(&mut a, sent(&mut b));
        assert!(
            a.player.is_playing(),
            "an old acknowledgement must not interrupt a new user action"
        );
        a.connect.services.transfer.as_mut().unwrap().until = 0;
        tick(&mut a);
        assert!(a.player.is_playing());
    }

    #[test]
    fn commands_reject_redelivery_wrong_target_and_invalid_ranges() {
        let keys = VaultKeys::new().unwrap();
        let sender = Uuid::new_v4();
        let target = Uuid::new_v4();
        let mut a = app(sender, target, &keys);
        let mut b = app(target, sender, &keys);
        start(&mut b);
        send_command(&mut a, target, RemoteCommand::Seek { position: 90.0 });
        let message = sent(&mut a);
        deliver(&mut b, message.clone());
        let generation = b.player.control_generation;
        deliver(&mut b, message);
        assert_eq!(b.player.control_generation, generation);
        assert_eq!(b.player.position(), 90.0);
        send_command(&mut a, target, RemoteCommand::Seek { position: -10.0 });
        deliver(&mut b, sent(&mut a));
        assert_eq!(b.player.position(), 90.0);
        let id = Uuid::new_v4();
        enqueue(
            &mut a,
            target,
            "remote",
            ConnectMessage::Command {
                target: id,
                command: RemoteCommand::Pause,
            },
            None,
        );
        deliver(&mut b, sent(&mut a));
        assert!(b.player.is_playing());
    }
}
