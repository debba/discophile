use crate::{
    app::App,
    player::{self, Mode},
};
use discophile_core::connect::services::Message;
use discophile_core::connect::{ConnectMessage, PlaybackContent, PlaybackSnapshot, RemoteCommand};
use discophile_core::l10n::{t, tf};
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
    #[serde(default)]
    pub output_device: Option<Uuid>,
    #[serde(default = "normal_rate")]
    pub rate: f32,
}
fn normal_rate() -> f32 {
    1.0
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
    presence: Presence,
}
pub struct IncomingTransfer {
    from: Uuid,
    message: Uuid,
    until: i64,
    content: String,
    playing: bool,
    generation: u64,
    rate: f32,
    completion: Option<(Uuid, i64)>,
    confirm_source_pause: bool,
}

#[derive(Clone)]
pub struct RemotePlayback {
    pub id: Uuid,
    pub name: String,
    pub state: Presence,
    pub online: bool,
}

pub fn remote_playback(app: &App) -> Option<RemotePlayback> {
    let id = app.connect.services.remote_device?;
    if !app.connect.services.paired || !super::paid(app, "playback.remote") {
        return None;
    }
    let device = app.connect.services.devices.iter().find(|d| d.id == id && Some(d.id) != own_id(app))?;
    let mut state = app.connect.services.states.get(&id)?.clone();
    let age = now_millis().saturating_sub(state.captured_at);
    let online = device.online && !device.suspended && (-5_000..=30_000).contains(&age);
    if online && state.playing {
        state.position += age.max(0) as f64 / 1000.0 * state.rate as f64;
    }
    if let Some(duration) = state.duration {
        state.position = state.position.min(duration);
    }
    Some(RemotePlayback {
        id,
        name: device.name.clone(),
        state,
        online,
    })
}

pub fn route_command(app: &mut App, command: RemoteCommand) -> bool {
    let Some(remote) = remote_playback(app) else { return false };
    if remote.online {
        let valid = match command {
            RemoteCommand::Seek { position } => position.is_finite() && position >= 0.0,
            RemoteCommand::SetVolume { volume } => volume.is_finite() && (0.0..=1.0).contains(&volume),
            _ => true,
        };
        if valid
            && send_command(app, remote.id, command.clone()).is_some()
            && let Some(state) = app.connect.services.states.get_mut(&remote.id)
        {
            state.position = remote.state.position;
            state.captured_at = now_millis();
            match command {
                RemoteCommand::Play => state.playing = true,
                RemoteCommand::Pause => state.playing = false,
                RemoteCommand::Seek { position } => state.position = position,
                RemoteCommand::SetVolume { volume } => state.volume = volume,
                _ => {}
            }
        }
    } else {
        app.connect.services.playback_notice = Some(tf("%@ is unavailable. Choose another playback device.", &[&remote.name]));
    }
    true
}

pub fn take_local_control(app: &mut App) {
    if let Some(id) = app.connect.services.remote_device.take() {
        send_command(app, id, RemoteCommand::Pause);
    }
}

pub fn listen_independently(app: &mut App) {
    app.connect.services.remote_device = None;
    app.connect.services.independent_playback = true;
    let notice = t("Other devices keep playing. Choose what to listen to here.");
    app.toast(&notice);
    app.connect.services.playback_notice = Some(notice);
}

fn observe_output(app: &mut App) {
    if app.player.is_playing() {
        app.connect.services.remote_device = None;
        return;
    }
    if let Some(id) = app.connect.services.remote_device {
        if !app.connect.services.devices.iter().any(|d| d.id == id && !d.suspended) {
            app.connect.services.remote_device = None;
        } else if let Some(state) = app.connect.services.states.get(&id)
            && (0..=30_000).contains(&(now_millis() - state.captured_at))
            && let Some(target) = state.output_device
        {
            if Some(target) == own_id(app) {
                app.connect.services.remote_device = None;
            } else if app.connect.services.devices.iter().any(|d| d.id == target && d.online && !d.suspended) {
                app.connect.services.remote_device = Some(target);
            }
        }
    }
    // A local pause must not silently switch an independent listener back to remote controls.
    if app.connect.services.remote_device.is_none() && !app.connect.services.independent_playback && !transfer_pending(app) {
        app.connect.services.remote_device = app
            .connect
            .services
            .devices
            .iter()
            .filter(|d| Some(d.id) != own_id(app) && d.online && !d.suspended)
            .filter_map(|d| {
                app.connect
                    .services
                    .states
                    .get(&d.id)
                    .filter(|s| s.playing && (0..=30_000).contains(&(now_millis() - s.captured_at)))
                    .map(|s| (d.id, s.captured_at))
            })
            .max_by_key(|(_, captured)| *captured)
            .map(|(id, _)| id);
    }
    if let Some((_, target, _, _)) = app.connect.services.forwarding
        && app.connect.services.remote_device == Some(target)
    {
        app.connect.services.forwarding = None;
    }
}

fn device_name(app: &App, id: Uuid) -> String {
    app.connect
        .services
        .devices
        .iter()
        .find(|d| d.id == id)
        .map(|d| d.name.clone())
        .unwrap_or_else(|| t("Playback devices"))
}

pub fn transfer_status(app: &App) -> Option<String> {
    if let Some(pending) = &app.connect.services.transfer {
        return Some(tf("Moving playback to %@…", &[&device_name(app, pending.target)]));
    }
    if let Some((_, target, _, _)) = app.connect.services.forwarding {
        return Some(tf("Moving playback to %@…", &[&device_name(app, target)]));
    }
    if let Some(pending) = &app.connect.services.receiving {
        return Some(tf("Receiving playback from %@…", &[&device_name(app, pending.from)]));
    }
    if let Some((source, _, _)) = app.connect.services.transfer_request {
        return Some(tf("Receiving playback from %@…", &[&device_name(app, source)]));
    }
    None
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
    let operation = json!({"action":"send", "id":id, "targetId":target, "kind":kind, "envelope":envelope});
    if kind == "handoff" { app.connect.services.outgoing.push_front(operation); }
    else { app.connect.services.outgoing.push_back(operation); }
    Some(id)
}

pub fn send_command(app: &mut App, target: Uuid, command: RemoteCommand) -> Option<Uuid> {
    enqueue(
        app,
        target,
        "remote",
        ConnectMessage::Command { target, command },
        None,
    )
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
    app.connect.services.transfer.is_some() || app.connect.services.receiving.is_some() || app.connect.services.transfer_request.is_some() || app.connect.services.forwarding.is_some()
}

pub fn can_transfer(app: &App) -> bool {
    super::paid(app, "playback.handoff") && app.connect.services.paired
        && !transfer_pending(app) && (remote_playback(app).is_some_and(|r| r.online && r.state.can_handoff) || snapshot(app).is_some())
}

fn offer(app: &mut App, target: Uuid, reply_to: Option<Uuid>) -> bool {
    if app.connect.services.transfer.is_some() { return false; }
    let Some(own) = own_id(app) else { return false };
    let Some(snapshot) = snapshot(app) else { return false };
    let playing = snapshot.playing;
    let Some(id) = enqueue(app, target, "handoff", ConnectMessage::Transfer { from: own, snapshot, confirm_source_pause: true }, reply_to) else { return false };
    app.connect.services.transfer = Some(OutgoingTransfer {
        target, message: id, until: now_millis() + 30_000,
        generation: app.player.control_generation, was_playing: playing, signature: transfer_signature(app), presence: local_presence(app),
    });
    true
}

pub fn transfer_to(app: &mut App, target: Uuid) {
    if let Some(remote) = remote_playback(app) {
        if !transfer_pending(app) && remote.online && remote.id != target
            && app.connect.services.devices.iter().any(|d| d.id == target && d.online && !d.suspended)
            && let Some(message) = enqueue(app, remote.id, "handoff", ConnectMessage::TransferRequest { target }, None) {
                app.connect.services.forwarding = Some((remote.id, target, message, now_millis() + 30_000));
        }
        return;
    }
    if !transfer_pending(app) && app.connect.services.devices.iter().any(|d| d.id == target && Some(d.id) != own_id(app) && d.online && !d.suspended)
        && offer(app, target, None) {
        app.connect.services.playback_notice = Some(t("Moving playback to the selected computer…"));
    }
}

fn local_presence(app: &App) -> Presence {
    let title = crate::ui::player_bar::actions::now_playing_text(app)
        .map(|(title, artist)| format!("{title} · {artist}"))
        .unwrap_or_else(|| t("Nothing playing"));
    let (position, duration) = crate::ui::player_bar::actions::timeline(app);
    Presence {
        title,
        playing: app.player.is_playing(),
        position,
        captured_at: now_millis(),
        can_handoff: snapshot(app).is_some(),
        volume: app.player.volume,
        duration,
        output_device: app.connect.services.remote_device,
        rate: if app.player.mode == Mode::Audiobook || app.player.current().is_some_and(|s| s.is_podcast()) { app.player.rate } else { 1.0 },
    }
}

pub fn queue_presence(app: &mut App) {
    let presence = local_presence(app);
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
                    || !presence.position.is_finite() || presence.position < 0.0 || !presence.volume.is_finite() || !(0.0..=1.0).contains(&presence.volume)
                    || presence.duration.is_some_and(|d| !d.is_finite() || d < 0.0)
                    || !presence.rate.is_finite() || !(0.5..=4.0).contains(&presence.rate)
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
    observe_output(app);
}

fn apply(app: &mut App, envelope: &Message, body: Body) {
    let Some(own) = own_id(app) else { return };
    match body.message {
        ConnectMessage::Command { target, command }
            if envelope.kind == "remote"
                && target == own
                && super::paid(app, "playback.remote") =>
        {
            app.connect.services.remote_device = None;
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
                && target != own
                && app.connect.services.devices.iter().any(|d| d.id == target && d.online && !d.suspended)
                && super::paid(app, "playback.handoff") =>
        {
            if app.connect.services.transfer.is_some() {
                return;
            }
            if !offer(app, target, (target == envelope.sender_id).then_some(envelope.id)) {
                enqueue(
                    app,
                    envelope.sender_id,
                    "handoff",
                    ConnectMessage::TransferRejected {
                        device_id: own,
                        reason: "unsupported_content".into(),
                    },
                    Some(envelope.id),
                );
            }
        }
        ConnectMessage::Transfer { from, snapshot, confirm_source_pause }
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
                    completion: None,
                    confirm_source_pause,
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
            if app.connect.services.receiving.as_ref().is_some_and(|p| p.from == device_id
                && p.completion.is_some_and(|(id, _)| Some(id) == body.reply_to)) {
                app.connect.services.receiving = None;
                app.connect.services.playback_notice = Some(t("Playing on this computer."));
                return;
            }
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
                let pending = app.connect.services.transfer.take().unwrap();
                player::pause(app);
                let mut state = pending.presence;
                state.captured_at = now_millis();
                state.position = app.player.position();
                app.connect.services.states.insert(device_id, state);
                app.connect.services.remote_device = Some(device_id);
                app.connect.services.independent_playback = false;
                enqueue(app, device_id, "handoff", ConnectMessage::TransferComplete { device_id: own }, Some(envelope.id));
                queue_presence(app);
                app.connect.services.playback_notice = Some(t("Playback moved to the other computer."));
            } else if app.connect.services.transfer.as_ref().is_some_and(|p| p.target == device_id && Some(p.message) == body.reply_to) {
                app.connect.services.transfer = None;
                enqueue(app, device_id, "handoff", ConnectMessage::TransferRejected { device_id: own, reason: "source_changed".into() }, Some(envelope.id));
            }
        }
        ConnectMessage::TransferRejected { device_id, .. }
            if envelope.kind == "handoff" && device_id == envelope.sender_id =>
        {
            let receiving = app.connect.services.receiving.as_ref().is_some_and(|p| p.from == device_id && p.completion.is_some_and(|(id, _)| Some(id) == body.reply_to));
            let correlated = receiving || app.connect.services.transfer_request.as_ref().is_some_and(|(source, id, _)| *source == device_id && Some(*id) == body.reply_to)
                || app.connect.services.transfer.as_ref().is_some_and(|p| p.target == device_id && Some(p.message) == body.reply_to)
                || app.connect.services.forwarding.as_ref().is_some_and(|(source, _, id, _)| *source == device_id && Some(*id) == body.reply_to);
            if !correlated { return; }
            if app.connect.services.receiving.as_ref().is_some_and(|p| p.from == device_id && p.completion.is_some_and(|(id, _)| Some(id) == body.reply_to)) {
                let pending = app.connect.services.receiving.take().unwrap();
                if pending.generation == app.player.control_generation { player::stop(app); }
            }
            if app.connect.services.forwarding.as_ref().is_some_and(|(source, _, id, _)| *source == device_id && Some(*id) == body.reply_to) {
                app.connect.services.forwarding = None;
            }
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
            app.connect.services.playback_notice = Some(t(if receiving { "The transfer could not be confirmed. Playback was paused on this computer." }
                else { "Playback transfer was not completed. The source computer keeps playing." }));
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
    // Incoming handoff controls the local output; it must not route back to its source.
    app.connect.services.remote_device = None;
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
    app.connect.services.independent_playback = false;
    // Volume belongs to this computer and is intentionally not copied from the sender.
    true
}

pub fn tick(app: &mut App) {
    let now = now_millis();
    if app.player.is_playing() { app.connect.services.remote_device = None; }
    if app.connect.services.forwarding.is_some_and(|(_, _, _, until)| until < now) {
        app.connect.services.forwarding = None;
        app.connect.services.playback_notice = Some(t("Playback transfer timed out. The source computer keeps playing."));
    }
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
    if let Some((_, sent_at)) = pending.completion {
        // Older clients confirm by publishing that their physical output has paused.
        if app.connect.services.states.get(&pending.from).is_some_and(|s| !s.playing && s.captured_at >= sent_at) {
            app.connect.services.receiving = None;
            app.connect.services.playback_notice = Some(t("Playing on this computer."));
            return;
        }
    }
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
    let failed = pending.until < now || app.player.error.is_some() || !same_content || pending.generation != app.player.control_generation;
    if !ready && !failed {
        return;
    }
    if ready && !failed {
        if pending.completion.is_none() {
            let from = pending.from; let reply = pending.message;
            let Some(own) = own_id(app) else { return };
            if let Some(id) = enqueue(app, from, "handoff", ConnectMessage::TransferComplete { device_id: own }, Some(reply)) {
                app.connect.services.receiving.as_mut().unwrap().completion = Some((id, now));
                if !app.connect.services.receiving.as_ref().unwrap().confirm_source_pause {
                    app.connect.services.receiving = None;
                    app.connect.services.playback_notice = Some(t("Playing on this computer."));
                }
                queue_presence(app);
            }
        }
        return;
    }
    let pending = app.connect.services.receiving.take().unwrap();
    if failed && pending.generation == app.player.control_generation {
        crate::features::audiobooks::cancel_pending_start(app);
        player::stop(app);
        app.connect.services.playback_notice = Some(t("The transfer could not be confirmed. Playback was paused on this computer."));
    }
    let Some(own) = own_id(app) else { return };
    let message = ConnectMessage::TransferRejected { device_id: own, reason: "playback_failed".into() };
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
        state.paired = true;
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

    fn publish_to(source: &mut App, controller: &mut App) {
        queue_presence(source);
        let index = source.connect.services.outgoing.iter().position(|v| v["action"] == "state").unwrap();
        let wire = source.connect.services.outgoing.remove(index).unwrap();
        controller.connect.services.devices.iter_mut().find(|d| Some(d.id) == own_id(source)).unwrap().state =
            Some(serde_json::from_value(wire["envelope"].clone()).unwrap());
        receive_poll(controller, &[]);
    }

    fn move_to(source: &mut App, destination: &mut App) {
        transfer_to(source, own_id(destination).unwrap());
        deliver(destination, sent(source));
        tick(destination);
        deliver(source, sent(destination));
        deliver(destination, sent(source));
    }

    #[test]
    fn confirmed_handoff_routes_controls_and_can_return_to_the_source() {
        let keys = VaultKeys::new().unwrap(); let a_id = Uuid::new_v4(); let b_id = Uuid::new_v4();
        let mut a = app(a_id, b_id, &keys); let mut b = app(b_id, a_id, &keys);
        start(&mut a);
        move_to(&mut a, &mut b);
        assert!(!a.player.output.status().playing && b.player.output.status().playing);
        assert!(b.connect.services.receiving.is_none());
        assert_eq!(remote_playback(&a).unwrap().id, b_id);
        player::toggle(&mut a);
        assert!(!remote_playback(&a).unwrap().state.playing);
        deliver(&mut b, sent(&mut a));
        assert!(!b.player.output.status().playing);
        player::toggle(&mut a); deliver(&mut b, sent(&mut a));
        assert!(b.player.output.status().playing && !a.player.output.status().playing);
        let local_volume = a.player.volume;
        player::set_volume(&mut a, 0.3); deliver(&mut b, sent(&mut a));
        assert_eq!(a.player.volume, local_volume);
        assert_eq!(b.player.volume, 0.3);
        crate::platform::media_controls::apply(&mut a, crate::platform::media_controls::MediaCommand::SetPosition(90.0));
        deliver(&mut b, sent(&mut a));
        assert_eq!(b.player.position(), 90.0);
        request_transfer(&mut a, b_id);
        deliver(&mut b, sent(&mut a)); deliver(&mut a, sent(&mut b));
        tick(&mut a); deliver(&mut b, sent(&mut a)); deliver(&mut a, sent(&mut b));
        assert!(a.player.output.status().playing && !b.player.output.status().playing);
        assert!(a.connect.services.remote_device.is_none());
        assert_eq!(remote_playback(&b).unwrap().id, a_id);
    }

    #[test]
    fn independent_listening_keeps_different_queues_and_controls_local_after_presence_updates() {
        let keys = VaultKeys::new().unwrap(); let a_id = Uuid::new_v4(); let b_id = Uuid::new_v4();
        let mut a = app(a_id, b_id, &keys); let mut b = app(b_id, a_id, &keys);
        start(&mut b);
        publish_to(&mut b, &mut a);
        assert_eq!(remote_playback(&a).unwrap().id, b_id);

        listen_independently(&mut a);
        player::play_songs(&mut a, vec![Song::new("independent", "Another album")], 0, false);
        assert!(a.player.output.status().playing && b.player.output.status().playing);
        assert_eq!(a.player.queue.current().unwrap().id, "independent");
        assert_eq!(b.player.queue.current().unwrap().id, "song-1");
        assert!(!a.connect.services.outgoing.iter().any(|v| v["action"] == "send"));
        publish_to(&mut b, &mut a);
        player::pause(&mut a);
        publish_to(&mut b, &mut a);
        assert!(remote_playback(&a).is_none(), "pausing locally must not select another output again");
        player::toggle(&mut a);
        player::set_volume(&mut a, 0.2);
        assert!(a.player.output.status().playing && b.player.output.status().playing);
        assert_ne!(a.player.volume, b.player.volume);
        assert!(!a.connect.services.outgoing.iter().any(|v| v["action"] == "send"));

        request_transfer(&mut a, b_id);
        deliver(&mut b, sent(&mut a)); deliver(&mut a, sent(&mut b));
        tick(&mut a); deliver(&mut b, sent(&mut a)); deliver(&mut a, sent(&mut b));
        assert!(a.player.output.status().playing && !b.player.output.status().playing);
        assert_eq!(a.player.queue.current().unwrap().id, "song-1");
        assert!(!a.connect.services.independent_playback);
    }

    #[test]
    fn an_unconfirmed_destination_rolls_back_instead_of_playing_over_the_source() {
        let keys = VaultKeys::new().unwrap(); let source = Uuid::new_v4(); let target = Uuid::new_v4();
        let mut a = app(source, target, &keys); let mut b = app(target, source, &keys);
        start(&mut a); transfer_to(&mut a, target); deliver(&mut b, sent(&mut a)); tick(&mut b);
        let _lost_confirmation = sent(&mut b);
        assert!(b.connect.services.receiving.as_ref().unwrap().completion.is_some());
        b.connect.services.receiving.as_mut().unwrap().until = 0;
        tick(&mut b);
        assert!(!b.player.output.status().playing);
        assert!(a.player.output.status().playing);
        assert!(b.connect.services.receiving.is_none());
    }

    #[test]
    fn source_keeps_playing_while_the_destination_audio_is_still_buffering() {
        use crate::audio::{AudioOutput, NullOutput, OutputStatus, AudioEvent, TransitionSettings};
        use discophile_core::audio_effects::AudioEffectsSettings;
        use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
        struct Delayed { inner: NullOutput, ready: Arc<AtomicBool> }
        impl AudioOutput for Delayed {
            fn load(&mut self, item: MediaItem, position: f64, autoplay: bool) { self.inner.load(item, position, autoplay); }
            fn set_next(&mut self, item: Option<MediaItem>) { self.inner.set_next(item); }
            fn play(&mut self) { self.inner.play(); }
            fn pause(&mut self) { self.inner.pause(); }
            fn stop(&mut self) { self.inner.stop(); }
            fn seek(&mut self, position: f64) { self.inner.seek(position); }
            fn set_volume(&mut self, volume: f32) { self.inner.set_volume(volume); }
            fn set_rate(&mut self, rate: f32) { self.inner.set_rate(rate); }
            fn set_transitions(&mut self, settings: TransitionSettings) { self.inner.set_transitions(settings); }
            fn set_effects(&mut self, settings: &AudioEffectsSettings) { self.inner.set_effects(settings); }
            fn status(&self) -> OutputStatus { let mut status = self.inner.status(); if !self.ready.load(Ordering::Relaxed) { status.playing = false; status.buffering = true; } status }
            fn poll_events(&mut self) -> Vec<AudioEvent> { self.inner.poll_events() }
        }
        let keys = VaultKeys::new().unwrap(); let source = Uuid::new_v4(); let target = Uuid::new_v4();
        let mut a = app(source, target, &keys); let mut b = app(target, source, &keys);
        let ready = Arc::new(AtomicBool::new(false));
        b.player.output = Box::new(Delayed { inner: NullOutput::default(), ready: ready.clone() });
        start(&mut a); transfer_to(&mut a, target); deliver(&mut b, sent(&mut a));
        tick(&mut b);
        assert!(a.player.output.status().playing);
        assert!(!b.connect.services.outgoing.iter().any(|v| v["action"] == "send"));
        ready.store(true, Ordering::Relaxed); tick(&mut b);
        deliver(&mut a, sent(&mut b)); deliver(&mut b, sent(&mut a));
        assert!(!a.player.output.status().playing && b.player.output.status().playing);
    }

    #[test]
    fn a_controller_moves_the_actual_remote_queue_to_a_third_device() {
        let keys = VaultKeys::new().unwrap(); let a_id = Uuid::new_v4(); let b_id = Uuid::new_v4(); let c_id = Uuid::new_v4();
        let mut a = app(a_id, b_id, &keys); let mut b = app(b_id, a_id, &keys); let mut c = app(c_id, a_id, &keys);
        for (app, missing) in [(&mut a, c_id), (&mut b, c_id), (&mut c, b_id)] {
            app.connect.services.devices.push(PairedDevice { id: missing, name: "Third computer".into(), online: true, suspended: false, state: None });
        }
        start(&mut a); publish_to(&mut a, &mut b);
        assert_eq!(remote_playback(&b).unwrap().id, a_id);
        transfer_to(&mut b, c_id);
        deliver(&mut a, sent(&mut b)); deliver(&mut c, sent(&mut a)); tick(&mut c);
        deliver(&mut a, sent(&mut c)); deliver(&mut c, sent(&mut a));
        publish_to(&mut a, &mut b); publish_to(&mut c, &mut b);
        assert!(c.player.output.status().playing);
        assert!(!a.player.output.status().playing && !b.player.output.status().playing);
        assert_eq!(c.player.queue.songs()[0].id, "song-1");
        assert_eq!(remote_playback(&b).unwrap().id, c_id);
        assert!(b.connect.services.forwarding.is_none());
    }

    #[test]
    fn offline_remote_controls_never_restart_the_local_output() {
        let keys = VaultKeys::new().unwrap(); let source = Uuid::new_v4(); let target = Uuid::new_v4();
        let mut a = app(source, target, &keys); let mut b = app(target, source, &keys);
        start(&mut b); publish_to(&mut b, &mut a);
        assert_eq!(remote_playback(&a).unwrap().id, target);
        a.connect.services.states.get_mut(&target).unwrap().captured_at -= 60_000;
        assert!(!remote_playback(&a).unwrap().online);
        player::play(&mut a); player::next(&mut a); player::set_volume(&mut a, 0.1);
        assert!(!a.player.output.status().playing);
        assert_eq!(a.player.volume, 1.0);
        assert!(!a.connect.services.outgoing.iter().any(|v| v["action"] == "send"));
        a.connect.services.devices.retain(|d| d.id != target); receive_poll(&mut a, &[]);
        assert!(remote_playback(&a).is_none());
    }

    #[test]
    fn legacy_offers_do_not_wait_for_a_confirmation_the_old_source_cannot_send() {
        let keys = VaultKeys::new().unwrap(); let source = Uuid::new_v4(); let target = Uuid::new_v4();
        let mut a = app(source, target, &keys); let mut b = app(target, source, &keys);
        start(&mut a);
        let snapshot = snapshot(&a).unwrap();
        enqueue(&mut a, target, "handoff", ConnectMessage::Transfer { from: source, snapshot, confirm_source_pause: false }, None);
        deliver(&mut b, sent(&mut a)); tick(&mut b);
        assert!(b.connect.services.receiving.is_none());
        assert!(b.player.output.status().playing);
        assert_eq!(sent(&mut b)["kind"], "handoff");
    }

    fn ready_services(app: &mut App) {
        let namespace = super::super::namespace(app.connect.credential.as_ref().unwrap());
        let state = &mut app.connect.services;
        state.loaded = true;
        state.namespace = namespace;
        state.paired = true;
        state.last_poll = now_millis();
        state.last_backup = now_millis();
    }

    #[test]
    fn idle_devices_rearm_polling_after_network_and_input_frames() {
        let keys = VaultKeys::new().unwrap();
        let mut app = app(Uuid::new_v4(), Uuid::new_v4(), &keys);
        ready_services(&mut app);
        let ctx = app.ctx().clone();
        for busy in [false, true, false] {
            app.connect.services.busy = busy;
            for frame in 0..5 {
                let output = ctx.run_ui(egui::RawInput::default(), |_| super::super::tick(&mut app));
                if frame > 1 {
                    assert!(output.viewport_output[&egui::ViewportId::ROOT].repaint_delay <= std::time::Duration::from_secs(3),
                        "idle devices must keep polling even after an intervening frame");
                }
                output.drop_without_applying_deltas();
            }
        }
        assert!(!app.player.is_playing());
    }

    #[test]
    fn background_poll_keeps_controls_available_and_accepts_handoff_to_idle_device() {
        let keys = VaultKeys::new().unwrap();
        let source = Uuid::new_v4();
        let target = Uuid::new_v4();
        let mut a = app(source, target, &keys);
        let mut b = app(target, source, &keys);
        ready_services(&mut a);
        ready_services(&mut b);
        a.connect.services.busy = true;
        a.connect.services.background = true;
        assert!(!super::super::changing(&a));
        assert!(b.player.current().is_none());
        queue_presence(&mut b);
        let state = b.connect.services.outgoing.pop_front().unwrap();
        a.connect.services.devices.iter_mut().find(|d| d.id == target).unwrap().state =
            Some(serde_json::from_value(state["envelope"].clone()).unwrap());
        receive_poll(&mut a, &[]);
        assert!(!a.connect.services.states[&target].playing);
        assert!(!a.connect.services.states[&target].can_handoff);
        start(&mut a);
        transfer_to(&mut a, target);
        deliver(&mut b, sent(&mut a));
        tick(&mut b);
        deliver(&mut a, sent(&mut b));
        assert!(b.player.is_playing());
        assert!(!a.player.is_playing());
        assert!(a.connect.services.transfer.is_none());
    }

    #[test]
    fn user_operations_wait_for_background_poll_instead_of_failing() {
        use discophile_core::connect::ServerInfo;
        use std::io::{Read, Write};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = InstanceOrigin::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
            let mut request = Vec::new();
            let body_start = loop {
                let mut chunk = [0; 4096];
                let n = socket.read(&mut chunk).unwrap();
                assert!(n > 0);
                request.extend_from_slice(&chunk[..n]);
                if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    break end + 4;
                }
            };
            let headers = String::from_utf8_lossy(&request[..body_start]);
            let length: usize = headers.lines().find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length").then(|| value.trim().parse().unwrap())
            }).unwrap();
            while request.len() < body_start + length {
                let mut chunk = [0; 4096];
                let n = socket.read(&mut chunk).unwrap();
                assert!(n > 0);
                request.extend_from_slice(&chunk[..n]);
            }
            let operation: serde_json::Value = serde_json::from_slice(&request[body_start..body_start + length]).unwrap();
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").unwrap();
            operation
        });
        let keys = VaultKeys::new().unwrap();
        let mut app = app(Uuid::new_v4(), Uuid::new_v4(), &keys);
        let credential = app.connect.credential.as_mut().unwrap();
        credential.origin = origin.clone();
        credential.scopes = vec!["connect:manage".into()];
        app.connect.server_info = Some(ServerInfo {
            origin: origin.clone(), instance_id: "test-instance".into(), name: None,
            issuer: origin.to_string(), authorization_endpoint: origin.url("/oauth/authorize"),
            token_endpoint: origin.url("/oauth/token"), revocation_endpoint: origin.url("/oauth/revoke"),
            resource: origin.url("/api/v1").to_string(),
            scopes_supported: vec!["account:read".into(), "offline_access".into(), "connect:manage".into()],
            native_clients: vec!["discophile-desktop".into()],
            capabilities: [("account".into(), 1), ("devices".into(), 1)].into_iter().collect(),
            dashboard: None,
        });
        let (tasks, updates) = crate::tasks::Tasks::new(egui::Context::default());
        app.tasks = tasks;
        ready_services(&mut app);
        app.connect.services.busy = true;
        app.connect.services.background = true;
        app.connect.services.name = "Renamed while polling".into();
        super::super::rename(&mut app);
        assert!(app.connect.services.error.is_none());
        assert!(super::super::changing(&app));
        assert_eq!(app.connect.services.pending.len(), 1);
        assert_eq!(app.connect.services.pending[0].0["action"], "join");
        app.connect.services.busy = false;
        super::super::tick(&mut app);
        assert!(app.connect.services.busy);
        assert!(app.connect.services.pending.is_empty());
        updates.recv_timeout(std::time::Duration::from_secs(5)).expect("queued rename completes")(&mut app);
        assert!(app.connect.services.error.is_none(), "{:?}", app.connect.services.error);
        assert!(!super::super::changing(&app));
        assert_eq!(server.join().unwrap()["name"], "Renamed while polling");
    }

    #[test]
    fn transient_send_failures_retry_the_identical_encrypted_message() {
        use std::io::{Read, Write};
        use discophile_core::connect::ServerInfo;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = InstanceOrigin::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let server = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for status in ["503 Service Unavailable", "200 OK"] {
                let (mut socket, _) = listener.accept().unwrap();
                socket.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
                let mut request = Vec::new();
                let start = loop {
                    let mut bytes = [0; 4096]; let count = socket.read(&mut bytes).unwrap(); assert!(count > 0);
                    request.extend_from_slice(&bytes[..count]);
                    if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") { break end + 4; }
                };
                let length: usize = String::from_utf8_lossy(&request[..start]).lines().find_map(|line| {
                    let (name, value) = line.split_once(':')?; name.eq_ignore_ascii_case("content-length").then(|| value.trim().parse().unwrap())
                }).unwrap();
                while request.len() < start + length {
                    let mut bytes = [0; 4096]; let count = socket.read(&mut bytes).unwrap(); assert!(count > 0); request.extend_from_slice(&bytes[..count]);
                }
                requests.push(serde_json::from_slice::<serde_json::Value>(&request[start..start + length]).unwrap());
                write!(socket, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}").unwrap();
            }
            requests
        });
        let keys = VaultKeys::new().unwrap(); let source = Uuid::new_v4(); let target = Uuid::new_v4();
        let mut app = app(source, target, &keys);
        app.connect.credential.as_mut().unwrap().origin = origin.clone();
        app.connect.credential.as_mut().unwrap().scopes = vec!["connect:manage".into()];
        app.connect.server_info = Some(ServerInfo {
            origin: origin.clone(), instance_id: "test-instance".into(), name: None, issuer: origin.to_string(),
            authorization_endpoint: origin.url("/oauth/authorize"), token_endpoint: origin.url("/oauth/token"),
            revocation_endpoint: origin.url("/oauth/revoke"), resource: origin.url("/api/v1").to_string(),
            scopes_supported: vec!["account:read".into(), "offline_access".into(), "connect:manage".into()],
            native_clients: vec!["discophile-desktop".into()], capabilities: [("account".into(), 1), ("devices".into(), 1)].into_iter().collect(), dashboard: None,
        });
        ready_services(&mut app);
        let (tasks, updates) = crate::tasks::Tasks::new(egui::Context::default()); app.tasks = tasks;
        send_command(&mut app, target, RemoteCommand::Pause);
        let mut operation = sent(&mut app);
        operation["deviceId"] = json!(source);
        super::super::run(&mut app, operation.clone(), |_, _| {});
        updates.recv_timeout(std::time::Duration::from_secs(5)).unwrap()(&mut app);
        assert_eq!(app.connect.services.outgoing.front().unwrap(), &operation);
        assert!(app.connect.services.retry_after > now_millis());
        app.connect.services.retry_after = 0;
        super::super::tick(&mut app);
        updates.recv_timeout(std::time::Duration::from_secs(5)).unwrap()(&mut app);
        let requests = server.join().unwrap();
        assert_eq!(requests, vec![operation.clone(), operation]);
        assert!(app.connect.services.send_retries.is_empty());
        assert!(app.connect.services.error.is_none());
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
            deliver(&mut b, sent(&mut a));
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
        enqueue(&mut a, target, "handoff", ConnectMessage::Transfer { from: source, snapshot, confirm_source_pause: true }, None);
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
        assert!(b.connect.services.receiving.as_ref().unwrap().completion.is_some());
        let completed = sent(&mut b);
        enqueue(&mut a, target, "handoff", ConnectMessage::TransferComplete { device_id: source }, Some(serde_json::from_value(completed["id"].clone()).unwrap()));
        deliver(&mut b, sent(&mut a));
        assert!(b.connect.services.receiving.is_none());
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
        deliver(&mut b, sent(&mut a));
        assert!(b.connect.services.receiving.is_none());
        assert_eq!(remote_playback(&a).unwrap().id, target);
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
        deliver(&mut b, sent(&mut a));
        assert!(!b.player.is_playing(), "the cancelled destination must not keep playing over the source");
        assert!(a.connect.services.transfer.is_none());
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
