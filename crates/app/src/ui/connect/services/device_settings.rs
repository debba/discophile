use super::*;
use discophile_core::connect::{RemoteCommand, services::PairedDevice};
use uuid::Uuid;

pub(super) fn show(app: &mut App, ui: &mut Ui, busy: bool) {
    widgets::section_heading(ui, &t("Paired playback devices"), None);
    let own = app.connect.services.identity.as_ref().map(|i| i.device_id);
    let selected = service::remote_playback(app).map(|r| r.id);
    for device in app.connect.services.devices.clone() {
        ui.push_id(device.id, |ui| {
            device_card(ui, selected == Some(device.id), |ui| {
                let local = own == Some(device.id);
                let state = app.connect.services.states.get(&device.id).cloned();
                let age = state.as_ref().map(|s| discophile_core::now_millis().saturating_sub(s.captured_at));
                let fresh = age.is_some_and(|age| (-5_000..=30_000).contains(&age));
                let status = if local { t("This computer") }
                    else if device.suspended { t("Suspended by the current plan") }
                    else if !device.online { t("Offline") }
                    else if fresh && state.as_ref().is_some_and(|s| s.playing || s.can_handoff || s.duration.is_some()) {
                        t(if state.as_ref().unwrap().playing { "Playing" } else { "Paused" })
                    } else { t("Online") };
                ui.horizontal_top(|ui| {
                    let width = (ui.available_width() - 52.0).max(80.0);
                    ui.allocate_ui_with_layout(vec2(width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.set_width(width);
                        device_heading(ui, &device.name, &status, service::device_kind(app, device.id, &device.name), selected == Some(device.id));
                        if let Some(state) = &state {
                            ui.add(egui::Label::new(theme::secondary(ui, &state.title, Font::Callout)).truncate()).on_hover_text(&state.title);
                        }
                    });
                    options(app, ui, &device, busy);
                });
                let online = device.online && !device.suspended;
                let remote = !local && service::paid(app, "playback.remote");
                if remote && online && let Some(state) = state {
                    let enabled = !busy && !app.connect.busy && fresh;
                    ui.add_enabled_ui(enabled, |ui| {
                        ui.add_space(widgets::space::S);
                        if ui.available_width() >= 340.0 {
                            ui.horizontal_top(|ui| {
                                ui.allocate_ui_with_layout(vec2(148.0, 62.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                                    ui.set_min_width(148.0);
                                    ui.add_space(9.0);
                                    transport_controls(app, ui, device.id, &device.name, state.playing);
                                });
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                                    let width = ui.available_width().min(240.0);
                                    ui.allocate_ui_with_layout(vec2(width, 62.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                                        ui.set_width(width);
                                        volume(app, ui, device.id, &device.name, state.volume);
                                    });
                                });
                            });
                        } else {
                            transport_controls(app, ui, device.id, &device.name, state.playing);
                            volume(app, ui, device.id, &device.name, state.volume);
                        }
                        let mut position = state.position;
                        if state.playing { position += age.unwrap_or(0).max(0) as f64 / 1000.0 * state.rate as f64; }
                        if let Some(duration) = state.duration.filter(|d| d.is_finite() && *d > 0.0) {
                            ui.push_id("position", |ui| {
                                if let Some(position) = slider(ui, &t("Position"), &tf("Playback position on %@", &[&device.name]), position.min(duration), 0.0..=duration, |v| {
                                    format!("{} / {}", discophile_core::l10n::time(v), discophile_core::l10n::time(duration))
                                }) {
                                    send(app, device.id, RemoteCommand::Seek { position });
                                }
                            });
                        }
                    });
                }
                if !local && online && service::paid(app, "playback.handoff") {
                    ui.add_enabled_ui(!busy && !app.connect.busy, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            if selected != Some(device.id) && transfer_action(ui, icons::DEVICES, &t("Play on this device"), &tf("Move playback to %@", &[&device.name]), service::can_transfer(app)).clicked() {
                                service::transfer_to(app, device.id);
                            }
                            let can_receive = fresh && app.connect.services.states.get(&device.id).is_some_and(|s| s.can_handoff) && !service::transfer_pending(app);
                            if transfer_action(ui, icons::ARROW_LEFT, &t("Continue here"), &tf("Continue here from %@", &[&device.name]), can_receive).clicked() {
                                service::request_transfer(app, device.id);
                            }
                        });
                    });
                }
                revocation(app, ui, device.id, busy);
            });
        });
        ui.add_space(widgets::space::S);
    }
    footer(ui, &t("Players must be running and online. Your music server must be reachable from the destination. Connect does not relay audio."));
}

fn transport_controls(app: &mut App, ui: &mut Ui, id: Uuid, name: &str, playing: bool) {
    ui.horizontal(|ui| {
        if transport(ui, icons::SKIP_BACK, &tf("Previous on %@", &[&name]), false).clicked() {
            send(app, id, RemoteCommand::Previous);
        }
        let (glyph, key, command) = if playing {
            (icons::PAUSE, "Pause on %@", RemoteCommand::Pause)
        } else {
            (icons::PLAY, "Play on %@", RemoteCommand::Play)
        };
        if transport(ui, glyph, &tf(key, &[&name]), true).clicked() {
            send(app, id, command);
        }
        if transport(ui, icons::SKIP_FORWARD, &tf("Next on %@", &[&name]), false).clicked() {
            send(app, id, RemoteCommand::Next);
        }
    });
}

fn volume(app: &mut App, ui: &mut Ui, id: Uuid, name: &str, volume: f32) {
    ui.push_id("volume", |ui| {
        if let Some(volume) = slider(ui, &t("Volume"), &tf("Volume on %@", &[&name]), volume as f64 * 100.0, 0.0..=100.0, |v| format!("{v:.0}%")) {
            send(app, id, RemoteCommand::SetVolume { volume: volume as f32 / 100.0 });
        }
    });
}

fn transport(ui: &mut Ui, icon: &str, label: &str, primary: bool) -> Response {
    let palette = Palette::of_ui(ui);
    let mut button = egui::Button::new(theme::icon(icon, 20.0).color(if primary { egui::Color32::WHITE } else { palette.text }))
        .min_size(vec2(44.0, 44.0)).corner_radius(22).frame(primary);
    if primary { button = button.fill(palette.action); }
    let response = ui.add(button);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label));
    focus(ui, &response, 22);
    response.on_hover_text(label)
}

fn transfer_action(ui: &mut Ui, icon: &str, label: &str, accessible: &str, enabled: bool) -> Response {
    let response = ui.add_enabled(enabled, egui::Button::new((theme::icon(icon, 17.0), theme::text(label, Font::Callout)))
        .min_size(vec2(0.0, 44.0)).corner_radius(8).wrap());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled && ui.is_enabled(), accessible));
    focus(ui, &response, 8);
    response
}

fn slider(ui: &mut Ui, label: &str, accessible: &str, value: f64, range: std::ops::RangeInclusive<f64>, format: impl Fn(f64) -> String) -> Option<f64> {
    let width = ui.available_width();
    let (_, row) = ui.allocate_space(vec2(width, 18.0));
    let output = crate::ui::player_bar::controls::large_slider(ui, value, range, width, false, 5.0, accessible);
    let palette = Palette::of_ui(ui);
    ui.painter().text(row.left_center(), egui::Align2::LEFT_CENTER, label, Font::Callout.id(), palette.secondary);
    ui.painter().text(row.right_center(), egui::Align2::RIGHT_CENTER, format(output.shown), Font::Callout.id(), palette.text);
    focus(ui, &output.response, 6);
    output.changed
}

fn focus(ui: &mut Ui, response: &Response, radius: u8) {
    if response.has_focus() && widgets::keyboard_navigation(ui.ctx()) {
        response.scroll_to_me(Some(egui::Align::Center));
    }
    widgets::focus_ring(ui, response, radius);
}

fn send(app: &mut App, id: Uuid, command: RemoteCommand) {
    if service::send_command(app, id, command.clone()).is_none() { return; }
    if let Some(state) = app.connect.services.states.get_mut(&id) {
        let now = discophile_core::now_millis();
        if state.playing { state.position += (now - state.captured_at).max(0) as f64 / 1000.0 * state.rate as f64; }
        state.captured_at = now;
        match command {
            RemoteCommand::Play => state.playing = true,
            RemoteCommand::Pause => state.playing = false,
            RemoteCommand::Seek { position } => state.position = position,
            RemoteCommand::SetVolume { volume } => state.volume = volume,
            _ => {}
        }
    }
}

fn options(app: &mut App, ui: &mut Ui, device: &PairedDevice, busy: bool) {
    let label = tf("Device options: %@", &[&device.name]);
    let button = transport(ui, icons::DOTS_THREE, &label, false);
    if button.clicked() { button.request_focus(); }
    let id = egui::Popup::default_response_id(&button);
    let was_open = egui::Popup::is_id_open(ui.ctx(), id);
    egui::Popup::menu(&button).accessible_name(&label).width(264.0).frame(widgets::account_menu_frame(ui)).show(|ui| {
        ui.set_width(240.0);
        if ui.add_enabled_ui(!busy, |ui| widgets::account_menu_action(ui, icons::SIGN_OUT, &t("Revoke device access"), None, None)).inner.clicked() {
            app.connect.services.revoke_target = Some(device.id);
            ui.close();
        }
    });
    let open = egui::Popup::is_id_open(ui.ctx(), id);
    if was_open && !open && ui.input(|input| input.key_pressed(egui::Key::Escape)) { button.request_focus(); }
    ui.ctx().accesskit_node_builder(button.id, |node| {
        node.set_has_popup(egui::accesskit::HasPopup::Menu);
        node.set_expanded(open);
    });
}

fn revocation(app: &mut App, ui: &mut Ui, id: Uuid, busy: bool) {
    if app.connect.services.revoke_target != Some(id) { return; }
    ui.separator();
    ui.add(egui::Label::new(t(if service::account_sync(app) {
        "Revoking signs this device out and updates encryption for your remaining computers."
    } else {
        "Revoking blocks this device immediately and changes the recovery code. Remaining computers will need the new code."
    })).wrap());
    ui.horizontal_wrapped(|ui| {
        if ui.add_enabled(!busy, egui::Button::new(t("Confirm device revocation")).min_size(vec2(0.0, 44.0))).clicked() {
            service::revoke(app, id);
        }
        if ui.add(egui::Button::new(t("Cancel")).min_size(vec2(0.0, 44.0))).clicked() {
            app.connect.services.revoke_target = None;
        }
    });
}

#[cfg(test)]
mod tests;
