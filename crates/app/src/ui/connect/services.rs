use crate::{
    app::App,
    features::connect::services as service,
    theme::{self, Font, Palette, icons},
    ui::{
        settings::{footer, group},
        widgets,
    },
};
use discophile_core::connect::RemoteCommand;
use discophile_core::l10n::{t, tf};
use egui::{Response, Ui, vec2};

#[cfg(test)]
mod tests;

pub fn player_devices(app: &mut App, ui: &mut Ui) -> Response {
    let button = widgets::icon_button(ui, icons::DEVICES, &t("Playback devices"), 19.0);
    if button.clicked() {
        button.request_focus();
    }
    let popup_id = egui::Popup::default_response_id(&button);
    let was_open = egui::Popup::is_id_open(ui.ctx(), popup_id);
    let width = widgets::ACCOUNT_MENU_WIDTH.min(ui.ctx().content_rect().width() - 32.0);
    egui::Popup::menu(&button)
        .accessible_name(t("Playback devices"))
        .align(egui::emath::RectAlign::TOP_END)
        .gap(8.0)
        .width(width)
        .layout(egui::Layout::top_down(egui::Align::Min))
        .frame(widgets::account_menu_frame(ui))
        .show(|ui| {
            ui.set_width(width - 24.0);
            ui.spacing_mut().item_spacing.y = widgets::space::S;
            playback_devices_contents(app, ui);
        });
    let open = egui::Popup::is_id_open(ui.ctx(), popup_id);
    if was_open && !open && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        button.request_focus();
    }
    ui.ctx().accesskit_node_builder(button.id, |node| {
        node.set_has_popup(egui::accesskit::HasPopup::Menu);
        node.set_expanded(open);
    });
    button
}

fn playback_devices_contents(app: &mut App, ui: &mut Ui) {
    let handoff = service::paid(app, "playback.handoff");
    ui.label(theme::text(t("Playback devices"), Font::Headline));
    let max_height = (ui.ctx().content_rect().height() - 260.0).clamp(widgets::CONTROL_LARGE, 360.0);
    egui::ScrollArea::vertical()
        .id_salt("playback-devices")
        .max_height(max_height)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            if !app.connect.services.paired {
                ui.add(egui::Label::new(t("Sign in to Connect on both computers to move playback between them.")).wrap());
            } else {
                let own = app.connect.services.identity.as_ref().map(|i| i.device_id);
                let peers: Vec<_> = app.connect.services.devices.iter().filter(|d| Some(d.id) != own).cloned().collect();
                if peers.is_empty() {
                    ui.label(t("No other computers connected"));
                }
                for device in peers {
                    ui.push_id(device.id, |ui| {
                        ui.separator();
                        ui.add(egui::Label::new(theme::text(&device.name, Font::Headline)).wrap());
                        let status = if device.suspended {
                            t("Suspended by the current plan")
                        } else if device.online {
                            t("Online")
                        } else {
                            t("Offline")
                        };
                        ui.add(egui::Label::new(theme::secondary(ui, status, Font::Footnote)).wrap());
                        let state = app.connect.services.states.get(&device.id).cloned();
                        if let Some(state) = &state {
                            ui.add(egui::Label::new(theme::secondary(ui, &state.title, Font::Callout)).wrap());
                        }
                        if !handoff {
                            return;
                        }
                        let available = device.online && !device.suspended && !service::changing(app) && !app.connect.busy;
                        if playback_action(
                            ui,
                            &t("Play on this device"),
                            &tf("Play on %@", &[&device.name]),
                            available && service::can_transfer(app),
                        )
                        .clicked()
                        {
                            service::transfer_to(app, device.id);
                        }
                        if playback_action(
                            ui,
                            &t("Continue here"),
                            &tf("Continue here from %@", &[&device.name]),
                            available && !service::transfer_pending(app) && state.is_some_and(|s| s.can_handoff),
                        )
                        .clicked()
                        {
                            service::request_transfer(app, device.id);
                        }
                    });
                }
            }
            if let Some(notice) = &app.connect.services.playback_notice {
                ui.add(egui::Label::new(notice).wrap());
            }
            if let Some(error) = &app.connect.services.error {
                super::error_text(ui, error);
            }
        });
    if app.connect.services.paired && !handoff {
        ui.separator();
        ui.add(egui::Label::new(theme::secondary(ui, t("Playback handoff is included in the Connect plan."), Font::Callout)).wrap());
        if ui
            .push_id("explore-connect", |ui| playback_action(ui, &t("Explore Connect"), &t("Explore Connect"), true))
            .inner
            .clicked()
        {
            app.ctx().open_url(egui::OpenUrl::new_tab(app.connect.origin.url("/plans").as_str()));
            ui.close();
        }
    }
    ui.separator();
    if ui
        .push_id("manage-playback-devices", |ui| {
            playback_action(ui, &t("Manage playback devices"), &t("Manage playback devices"), true)
        })
        .inner
        .clicked()
    {
        app.nav.push(crate::nav::Route::Settings(crate::nav::SettingsPage::Connect));
        ui.close();
    }
}

fn playback_action(ui: &mut Ui, label: &str, accessible_name: &str, enabled: bool) -> Response {
    let palette = Palette::of_ui(ui);
    // Keep focus on the same device and action when presence or the live roster changes.
    let id = ui.scope_id().with(label);
    let response = ui
        .scope_builder(egui::UiBuilder::new().scope_id(id), |ui| {
            ui.add_enabled(
                enabled,
                egui::Button::new(theme::text(label, Font::Callout).color(palette.text))
                    .wrap()
                    .min_size(vec2(ui.available_width(), widgets::CONTROL_LARGE))
                    .corner_radius(8),
            )
        })
        .inner;
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::MenuItem, ui.is_enabled() && enabled, accessible_name));
    if response.has_focus() {
        response.scroll_to_me(Some(egui::Align::Center));
        if widgets::keyboard_navigation(ui.ctx()) {
            ui.painter()
                .rect_stroke(response.rect.shrink(1.0), 8, egui::Stroke::new(2.0, palette.accent), egui::StrokeKind::Inside);
        }
    }
    response
}

pub fn settings(app: &mut App, ui: &mut Ui) {
    widgets::section_heading(ui, &t("Plans and pricing"), None);
    group(ui, |ui| {
        if let Some(catalog) = app.connect.services.catalog.clone() {
            for tier in &catalog.tiers {
                ui.label(theme::text(&tier.name, Font::Headline));
                let number = |cents: u32| {
                    let value = format!("{:.2}", cents as f64 / 100.0);
                    if app.settings.effective_language() == discophile_core::l10n::Language::Italian { value.replace('.', ",") } else { value }
                };
                let price = if tier.monthly_cents == 0 {
                    t("Free, with no expiry")
                } else {
                    tf(
                        "€%@ per month or €%@ per year",
                        &[
                            &number(tier.monthly_cents),
                            &number(tier.yearly_cents),
                        ],
                    )
                };
                ui.add(egui::Label::new(theme::text(price, Font::Body)).wrap());
                egui::CollapsingHeader::new(t("Included in your plan")).id_salt(("connect-plan", &tier.id)).show(ui, |ui| {
                    for benefit in &tier.entitlements {
                        if let Some(kind) = benefit.kind() {
                            ui.add(egui::Label::new(super::desktop::entitlement_label(kind, benefit.limit)).wrap());
                            if !benefit.available { ui.label(theme::secondary(ui, t("Planned · not available yet"), Font::Footnote)); }
                        }
                    }
                });
                ui.add_space(8.0);
            }
        }
        let url = app.connect.origin.url("/plans");
        if ui
            .button(t("Compare plans and request activation"))
            .clicked()
        {
            ui.ctx().open_url(egui::OpenUrl::new_tab(url.as_str()));
        }
        ui.add(egui::Label::new(theme::secondary(ui, t("No payments or automatic renewals. Activation requests are sent to info@discophile.app and plans are assigned manually."), Font::Callout)).wrap());
    });
    if app.connect.credential.is_none() {
        return;
    }
    let offered = app
        .connect
        .server_info
        .as_ref()
        .is_some_and(|i| i.capability_usable("devices"));
    let scope = app
        .connect
        .credential
        .as_ref()
        .is_some_and(|c| c.scopes.iter().any(|s| s == "connect:manage"));
    if !offered || !scope {
        footer(
            ui,
            &t("Refresh your account or sign in again to allow Connect services."),
        );
        return;
    }
    widgets::section_heading(ui, &t("Encrypted synchronization"), None);
    let busy = service::changing(app) || app.connect.busy;
    group(ui, |ui| {
        ui.label(t("Computer name"));
        ui.add(
            egui::TextEdit::singleline(&mut app.connect.services.name)
                .char_limit(100)
                .desired_width(300.0),
        );
        if busy {
            ui.spinner();
        }
        if let Some(error) = &app.connect.services.error {
            super::error_text(ui, error);
        }
        if let Some(notice) = &app.connect.services.notice {
            ui.add(egui::Label::new(theme::secondary(ui, notice, Font::Callout)).wrap());
        }
        if let Some(notice) = &app.connect.services.playback_notice {
            ui.add(egui::Label::new(theme::secondary(ui, notice, Font::Callout)).wrap());
        }
        ui.add_enabled_ui(!busy, |ui| {
            if app.connect.services.paired && ui.button(t("Save computer name")).clicked() { service::rename(app); }
            if !app.connect.services.paired {
                if service::account_sync(app) {
                    ui.add(egui::Label::new(t("Sign in to the same Connect account on your computers. Your selected connections and preferences synchronize without exchanging a code.")).wrap());
                    if ui.button(t("Connect this computer")).clicked() { service::refresh(app); }
                    egui::CollapsingHeader::new(t("Recover an older configuration")).show(ui, |ui| {
                        ui.add(egui::TextEdit::singleline(&mut app.connect.services.recovery_input).password(true).hint_text("dc1_…").desired_width(f32::INFINITY));
                        if ui.button(t("Import recovery code")).clicked() { service::pair(app, false); }
                    });
                    return;
                }
                let existing = app.connect.services.vault.is_some();
                if existing {
                    ui.add(egui::Label::new(t("Enter the recovery code from a trusted computer. Pairing copies the shared connections to this computer; local connections and downloads are kept.")).wrap());
                    ui.add(egui::TextEdit::singleline(&mut app.connect.services.recovery_input).password(true).hint_text("dc1_…").desired_width(f32::INFINITY));
                    ui.checkbox(&mut app.connect.services.preferences, t("Also synchronize language and appearance"));
                    if ui.button(t("Pair this computer")).clicked() { service::pair(app, false); }
                } else if app.connect.services.identity.is_none() {
                    ui.add(egui::Label::new(t("Create a private recovery code before synchronizing. Connect cannot recover your encrypted data if all trusted computers and the code are lost.")).wrap());
                    if ui.button(t("Create recovery code")).clicked() { service::prepare(app); }
                }
                if !existing && app.connect.services.identity.is_some() {
                    recovery(app, ui);
                    ui.checkbox(&mut app.connect.services.confirm_recovery_saved, t("I saved my recovery code somewhere safe"));
                    if ui.add_enabled(app.connect.services.confirm_recovery_saved, egui::Button::new(t("Create encrypted vault and pair"))).clicked() { service::pair(app, true); }
                }
                if ui.button(t("Refresh Connect services")).clicked() { service::refresh(app); }
                if app.connect.services.identity.is_some() && ui.button(t("Forget this registration and pair again")).clicked() { service::forget_registration(app); }
                return;
            }
            let identity_epoch = app.connect.services.identity.as_ref().map(|i| i.keys.epoch).unwrap_or_default();
            let vault = app.connect.services.vault.clone();
            if vault.as_ref().is_some_and(|v| v.rotation_required) && ui.button(t("Rotate encryption key")).clicked() { service::publish(app, true); }
            if vault.as_ref().is_some_and(|v| v.epoch > identity_epoch) {
                ui.add(egui::TextEdit::singleline(&mut app.connect.services.recovery_input).password(true).hint_text("dc1_…").desired_width(f32::INFINITY));
                if ui.button(t("Import updated recovery code")).clicked() { service::import_recovery(app); }
            }
            let old = (app.connect.services.automatic, app.connect.services.preferences, app.connect.services.selected.clone());
            ui.checkbox(&mut app.connect.services.automatic, t("Automatically synchronize selected connections"));
            if !old.0 && app.connect.services.automatic { service::resume_automatic(app); }
            ui.checkbox(&mut app.connect.services.preferences, t("Also synchronize language and appearance"));
            ui.add_space(8.0);
            ui.label(theme::text(t("Connections to share"), Font::Headline));
            let limit = service::connection_limit(app);
            for choice in service::configuration::choices(app) {
                let id = choice.id(); let mut selected = app.connect.services.selected.contains(&id);
                if ui.add_enabled(selected || app.connect.services.selected.len() < limit, egui::Checkbox::new(&mut selected, choice.label())).changed() {
                    if selected { app.connect.services.selected.insert(id); } else { app.connect.services.selected.remove(&id); }
                }
            }
            if old != (app.connect.services.automatic, app.connect.services.preferences, app.connect.services.selected.clone()) { service::save(app); }
            ui.add(egui::Label::new(theme::secondary(ui, t("Only language and appearance are shared. Volume, audio effects, downloads, local paths and AI credentials stay on this computer. Selected Audiobookshelf connections sign in automatically when a saved password is available."), Font::Callout)).wrap());
            if app.connect.services.conflict.is_some() {
                ui.add(egui::Label::new(t("Both this computer and another device changed the configuration. Choose which shared configuration to keep; local connections and downloads will be preserved.")).wrap());
                if ui.button(t("Use cloud configuration")).clicked() { service::use_cloud(app); }
                if ui.button(t("Publish this computer's configuration")).clicked() { service::publish(app, false); }
            } else {
                ui.horizontal_wrapped(|ui| {
                    if ui.button(t("Synchronize now")).clicked() { service::publish(app, false); }
                    if ui.button(t("Refresh Connect services")).clicked() { service::refresh(app); }
                });
            }
            egui::CollapsingHeader::new(t("Advanced recovery")).show(ui, |ui| recovery(app, ui));
            ui.horizontal_wrapped(|ui| {
                if ui.button(t("Export encrypted configuration")).clicked() { service::configuration::export_encrypted(app); }
                if service::paid(app, "backup.configuration") && ui.button(t("Back up configuration now")).clicked() { service::backup(app); }
            });
        });
    });
    footer(ui, &t(if service::account_sync(app) {
        "Connect protects your synchronization key and restores it when you sign in. The Connect server can recover the key for your account."
    } else if crate::platform::secrets::using_keychain() {
        "Recovery keys are saved in this computer's system keychain."
    } else {
        "Recovery keys are saved in this computer's protected secrets file."
    }));
    if !app.connect.services.paired {
        return;
    }
    devices(app, ui, busy);
    backups(app, ui, busy);
    group(ui, |ui| {
        ui.checkbox(
            &mut app.connect.services.confirm_delete,
            t("Delete my Connect cloud configuration, devices and backups; keep local copies"),
        );
        if ui
            .add_enabled(
                !busy && app.connect.services.confirm_delete,
                egui::Button::new(t("Delete Connect cloud data")),
            )
            .clicked()
        {
            service::delete(app);
        }
    });
}

fn recovery(app: &mut App, ui: &mut Ui) {
    ui.checkbox(
        &mut app.connect.services.recovery_visible,
        t("Show private recovery code"),
    );
    if app.connect.services.recovery_visible
        && let Some(identity) = app.connect.services.identity.as_ref()
    {
        let mut code = identity.keys.recovery_code();
        ui.add(
            egui::TextEdit::multiline(&mut code)
                .interactive(false)
                .desired_width(f32::INFINITY)
                .desired_rows(3),
        );
        ui.add(egui::Label::new(t("Anyone with this code can decrypt your shared configuration. Transfer it privately; never email it to Discophile.")).wrap());
        if ui.button(t("Copy private recovery code")).clicked() {
            ui.ctx().copy_text(code);
        }
    }
}
fn devices(app: &mut App, ui: &mut Ui, busy: bool) {
    widgets::section_heading(ui, &t("Paired playback devices"), None);
    let devices = app.connect.services.devices.clone();
    let own = app.connect.services.identity.as_ref().map(|i| i.device_id);
    let paid = service::paid(app, "playback.remote");
    group(ui, |ui| {
        for device in devices {
            ui.label(theme::text(&device.name, Font::Headline));
            ui.label(if Some(device.id) == own {
                t("This computer")
            } else if device.suspended {
                t("Suspended by the current plan")
            } else if device.online {
                t("Online")
            } else {
                t("Offline")
            });
            if let Some(state) = app.connect.services.states.get(&device.id) {
                ui.add(egui::Label::new(&state.title).wrap());
            }
            if paid && Some(device.id) != own && device.online {
                ui.add_enabled_ui(!busy, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        if ui.add_enabled(service::can_transfer(app), egui::Button::new(t("Play on this device"))).clicked() {
                            service::transfer_to(app, device.id);
                        }
                        for (label, command) in [
                            (t("Play"), RemoteCommand::Play),
                            (t("Pause"), RemoteCommand::Pause),
                            (t("Previous"), RemoteCommand::Previous),
                            (t("Next"), RemoteCommand::Next),
                        ] {
                            if ui.button(label).clicked() {
                                service::send_command(app, device.id, command);
                            }
                        }
                        if app
                            .connect
                            .services
                            .states
                            .get(&device.id)
                            .is_some_and(|s| s.can_handoff)
                            && ui.button(t("Continue here")).clicked()
                        {
                            service::request_transfer(app, device.id);
                        }
                    });
                    if let Some(state) = app.connect.services.states.get(&device.id).cloned() {
                        let (mut position, mut volume) = app.connect.services.remote_edits.get(&device.id).copied().unwrap_or((state.position, state.volume));
                        ui.horizontal_wrapped(|ui| {
                            ui.add(egui::Slider::new(&mut volume, 0.0..=1.0).text(t("Volume")));
                            if ui.button(t("Set device volume")).clicked() { service::send_command(app, device.id, RemoteCommand::SetVolume { volume }); }
                        });
                        if let Some(duration) = state.duration.filter(|d| d.is_finite() && *d > 0.0) {
                            ui.horizontal_wrapped(|ui| {
                                ui.add(egui::DragValue::new(&mut position).range(0.0..=duration).suffix(" s"));
                                if ui.button(t("Seek on device")).clicked() { service::send_command(app, device.id, RemoteCommand::Seek { position }); }
                            });
                        }
                        app.connect.services.remote_edits.insert(device.id, (position, volume));
                    }
                });
            }
            if ui
                .add_enabled(!busy, egui::Button::new(t("Revoke device access")))
                .clicked()
            {
                app.connect.services.revoke_target = Some(device.id);
            }
            if app.connect.services.revoke_target == Some(device.id) {
                ui.add(egui::Label::new(t(if service::account_sync(app) {
                    "Revoking signs this device out and updates encryption for your remaining computers."
                } else {
                    "Revoking blocks this device immediately and changes the recovery code. Remaining computers will need the new code."
                })).wrap());
                if ui
                    .add_enabled(!busy, egui::Button::new(t("Confirm device revocation")))
                    .clicked()
                {
                    service::revoke(app, device.id);
                }
                if ui.button(t("Cancel")).clicked() {
                    app.connect.services.revoke_target = None;
                }
            }
            ui.separator();
        }
    });
    footer(
        ui,
        &t(
            "Players must be running and online. Your music server must be reachable from the destination. Connect does not relay audio.",
        ),
    );
}
fn backups(app: &mut App, ui: &mut Ui, busy: bool) {
    widgets::section_heading(ui, &t("Configuration backups"), None);
    let backups = app.connect.services.backups.clone();
    group(ui, |ui| {
        ui.add(egui::Label::new(t("Up to 64 encrypted snapshots are retained for 30 days. Audio files are not included. Restoring merges connections into this computer and preserves downloads.")).wrap());
        if backups.is_empty() {
            ui.label(t("No configuration backups yet"));
        }
        for backup in backups {
            ui.horizontal_wrapped(|ui| {
                ui.label(tf(
                    "Revision %lld · %@",
                    &[&backup.revision.to_string(), &backup.created_at],
                ));
                if ui
                    .add_enabled(!busy, egui::Button::new(t("Review restore")))
                    .clicked()
                {
                    service::restore(app, backup.id);
                }
            });
        }
        if let Some((_, config)) = app.connect.services.restore_target.as_ref() {
            ui.label(tf(
                "Restore %lld shared connections to this computer?",
                &[&config.connections.len().to_string()],
            ));
            if ui.button(t("Restore configuration locally")).clicked() {
                service::apply_restore(app);
            }
            if ui.button(t("Cancel")).clicked() {
                app.connect.services.restore_target = None;
            }
        }
    });
}
