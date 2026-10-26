use crate::{
    app::App,
    features::connect::services as service,
    theme::{self, Font, Palette, icons},
    ui::{
        settings::{footer, group},
        widgets,
    },
};
use discophile_core::l10n::{t, tf};
use egui::{AtomExt, Response, Ui, vec2};

mod device_settings;

#[cfg(test)]
pub(in crate::ui::connect) mod tests;

pub fn player_devices(app: &mut App, ui: &mut Ui) -> Response {
    let remote = service::remote_playback(app);
    let label = remote.as_ref().map(|r| tf("Playback devices: %@", &[&r.name])).unwrap_or_else(|| t("Playback devices"));
    let button = widgets::icon_button_tinted(ui, icons::DEVICES, &label, 19.0, remote.map(|_| Palette::of_ui(ui).accent));
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
    let footer_space = if app.connect.services.paired && !handoff { 260.0 } else { 120.0 };
    let max_height = (ui.ctx().content_rect().height() - footer_space).clamp(widgets::CONTROL_LARGE, 360.0);
    egui::ScrollArea::vertical()
        .id_salt("playback-devices")
        .max_height(max_height)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            if !app.connect.services.paired {
                ui.add(egui::Label::new(t("Sign in to Connect on both computers to move playback between them.")).wrap());
            } else {
                let remote = service::remote_playback(app);
                ui.push_id("this-computer", |ui| {
                    device_card(ui, false, |ui| {
                        let status = if app.connect.services.independent_playback {
                            t("Independent playback")
                        } else if app.player.is_playing() {
                            t("Playing here")
                        } else if remote.is_some() {
                            String::new()
                        } else {
                            t("Ready to play")
                        };
                        device_heading(ui, &t("This computer"), &status, service::DeviceKind::current(), remote.is_none() && app.player.is_playing());
                        if let Some(source) = &remote {
                            if playback_action(ui, &t("Continue here"), &t("Play on this computer"), handoff && source.online && source.state.can_handoff && !service::transfer_pending(app)).clicked() {
                                service::request_transfer(app, source.id);
                            }
                            if playback_action(ui, &t("Listen independently here"), &t("Listen independently here"), !service::transfer_pending(app)).clicked() {
                                service::listen_independently(app);
                                ui.close();
                            }
                        }
                    });
                });
                let own = app.connect.services.identity.as_ref().map(|i| i.device_id);
                let peers: Vec<_> = app.connect.services.devices.iter().filter(|d| Some(d.id) != own).cloned().collect();
                if peers.is_empty() {
                    ui.label(theme::secondary(ui, t("No other computers connected"), Font::Callout));
                }
                for device in peers {
                    ui.push_id(device.id, |ui| {
                        let selected = remote.as_ref().is_some_and(|r| r.id == device.id);
                        let state = app.connect.services.states.get(&device.id).cloned();
                        let fresh = device.online && state.as_ref().is_some_and(|s| (-5_000..=30_000).contains(&discophile_core::now_millis().saturating_sub(s.captured_at)));
                        let status = if device.suspended {
                            t("Suspended by the current plan")
                        } else if !device.online {
                            t("Offline")
                        } else if fresh {
                            t(if state.as_ref().unwrap().playing { "Playing" } else { "Paused" })
                        } else {
                            t("Online")
                        };
                        device_card(ui, selected, |ui| {
                            device_heading(ui, &device.name, &status, service::device_kind(app, device.id, &device.name), selected);
                            if let Some(state) = &state {
                                ui.add(egui::Label::new(theme::secondary(ui, &state.title, Font::Callout)).truncate()).on_hover_text(&state.title);
                            }
                            if !handoff || selected {
                                return;
                            }
                            let available = device.online && !device.suspended && !service::changing(app) && !app.connect.busy;
                            if playback_action(
                                ui,
                                &t("Play on this device"),
                                &tf("Play on %@", &[&device.name]),
                                available && service::can_transfer(app),
                            ).clicked() {
                                service::transfer_to(app, device.id);
                            }
                            if playback_action(
                                ui,
                                &t("Continue here"),
                                &tf("Continue here from %@", &[&device.name]),
                                available && !service::transfer_pending(app) && state.is_some_and(|s| s.can_handoff),
                            ).clicked() {
                                service::request_transfer(app, device.id);
                            }
                        });
                    });
                }
            }
            if let Some(notice) = &app.connect.services.playback_notice {
                ui.add(egui::Label::new(theme::secondary(ui, notice, Font::Footnote)).wrap());
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
            playback_action(ui, &t("Device settings"), &t("Manage playback devices"), true)
        })
        .inner
        .clicked()
    {
        app.nav.push(crate::nav::Route::Settings(crate::nav::SettingsPage::Connect));
        ui.close();
    }
}

fn device_card(ui: &mut Ui, selected: bool, contents: impl FnOnce(&mut Ui)) {
    let palette = Palette::of_ui(ui);
    egui::Frame::new()
        .fill(if selected { palette.surface_raised } else { egui::Color32::TRANSPARENT })
        .stroke(egui::Stroke::new(1.0, palette.separator))
        .corner_radius(10)
        .inner_margin(10)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = widgets::space::XS;
            contents(ui);
        });
}

fn device_heading(ui: &mut Ui, name: &str, status: &str, kind: service::DeviceKind, selected: bool) {
    let palette = Palette::of_ui(ui);
    let (glyph, kind_label) = match kind {
        service::DeviceKind::Linux => (icons::LINUX_LOGO, "Linux".to_owned()),
        service::DeviceKind::Mac => (icons::APPLE_LOGO, "macOS".to_owned()),
        service::DeviceKind::Windows => (icons::WINDOWS_LOGO, "Windows".to_owned()),
        service::DeviceKind::Smartphone => (icons::DEVICE_MOBILE, t("Smartphone")),
        service::DeviceKind::Unknown => (icons::DESKTOP, t("Computer")),
    };
    ui.horizontal_top(|ui| {
        ui.add_space(2.0);
        let icon = ui.label(theme::icon(glyph, 20.0)
            .color(if selected { palette.accent } else { palette.secondary })).on_hover_text(&kind_label);
        icon.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, if selected { t("Selected playback device") } else { kind_label.clone() }));
        ui.ctx().accesskit_node_builder(icon.id, |node| node.set_description(&kind_label));
        ui.add_space(2.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.add(egui::Label::new(theme::text(name, Font::Headline)).truncate()).on_hover_text(name);
            if !status.is_empty() {
                ui.add(egui::Label::new(theme::text(status, Font::Footnote)
                    .color(if selected { palette.accent } else { palette.secondary })).wrap());
            }
        });
    });
}

fn playback_action(ui: &mut Ui, label: &str, accessible_name: &str, enabled: bool) -> Response {
    let palette = Palette::of_ui(ui);
    let (icon, quiet) = if label == t("Listen independently here") {
        (icons::HEADPHONES, true)
    } else if label == t("Device settings") {
        (icons::GEAR_SIX, true)
    } else if label == t("Continue here") {
        (icons::ARROW_LEFT, false)
    } else {
        (icons::PLAY, false)
    };
    // Keep focus on the same device and action when presence or the live roster changes.
    let id = ui.scope_id().with(label);
    let response = ui
        .scope_builder(egui::UiBuilder::new().scope_id(id), |ui| {
            ui.add_enabled(
                enabled,
                egui::Button::new((
                    theme::icon(icon, 17.0).color(palette.secondary).atom_size(vec2(20.0, 20.0)),
                    theme::text(label, Font::Callout).color(palette.text).atom_shrink(true),
                ))
                    .gap(widgets::space::S)
                    .frame_when_inactive(!quiet)
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
                    if app.connect.services.error.is_some() && app.connect.services.identity.is_some()
                        && ui.button(t("Forget this registration and pair again")).clicked()
                    {
                        service::forget_registration(app);
                    }
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
    device_settings::show(app, ui, busy);
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
