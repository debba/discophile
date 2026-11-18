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
                synchronization_review(app, ui);
                return;
            }
            let identity_epoch = app.connect.services.identity.as_ref().map(|i| i.keys.epoch).unwrap_or_default();
            let vault = app.connect.services.vault.clone();
            if vault.as_ref().is_some_and(|v| v.rotation_required) && ui.button(t("Rotate encryption key")).clicked() { service::rotate_key(app); }
            if vault.as_ref().is_some_and(|v| v.epoch > identity_epoch) {
                ui.add(egui::TextEdit::singleline(&mut app.connect.services.recovery_input).password(true).hint_text("dc1_…").desired_width(f32::INFINITY));
                if ui.button(t("Import updated recovery code")).clicked() { service::import_recovery(app); }
            }
            if !app.connect.services.automatic && ui.button(t("Resume automatic synchronization")).clicked() {
                service::resume_automatic(app);
            }
            if ui.checkbox(&mut app.connect.services.preferences, t("Also synchronize language and appearance")).changed() {
                service::save(app);
                service::synchronize_now(app);
            }
            if app.connect.services.conflict.is_some() {
                ui.add(egui::Label::new(t("The same data changed here and on another device. Choose which version to keep for the conflicting changes.")).wrap());
                ui.horizontal_wrapped(|ui| {
                    if ui.button(t("Use cloud changes")).clicked() { service::use_cloud(app); }
                    if ui.button(t("Use this computer’s changes")).clicked() { service::use_local(app); }
                });
            } else if app.connect.services.pending_cloud.is_some() {
                ui.label(t("Changes from Connect are ready to synchronize."));
            }
            if ui.button(t("Synchronize now")).clicked() { service::synchronize_now(app); }
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
    shared_data(app, ui, busy);
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

fn synchronization_review(app: &mut App, ui: &mut Ui) {
    if !app.connect.services.reviewing {
        ui.add(egui::Label::new(t("Your libraries and downloads work independently of Connect. Review what to share when you are ready.")).wrap());
        if ui.button(t("Review synchronization")).clicked() {
            service::start_review(app);
        }
        return;
    }
    if !app.connect.services.review_ready {
        if ui.button(t("Refresh Connect services")).clicked() {
            service::refresh(app);
        }
        if app.connect.services.vault.is_some() {
            egui::CollapsingHeader::new(t("Recover an older configuration")).show(ui, |ui| {
                ui.add(egui::TextEdit::singleline(&mut app.connect.services.recovery_input).password(true).hint_text("dc1_…").desired_width(f32::INFINITY));
                if ui.button(t("Import recovery code")).clicked() && !service::prepare_review(app) {
                    app.connect.services.error = Some(t("The recovery code is invalid. Copy it from a trusted computer."));
                }
            });
        }
        return;
    }
    ui.add(egui::Label::new(t("Choose existing connections to share. New connections will be shared with every device on this account, within your plan limit.")).wrap());
    let cloud_ids: std::collections::BTreeSet<_> = app.connect.services.cloud.iter()
        .flat_map(|config| config.connections.iter().map(|entry| entry.id())).collect();
    if let Some(cloud) = app.connect.services.cloud.as_ref() {
        ui.label(theme::text(t("Already in Connect"), Font::Headline));
        for entry in &cloud.connections {
            ui.label(entry.label());
        }
    }
    ui.label(theme::text(t("Connections on this computer"), Font::Headline));
    let limit = service::connection_limit(app);
    for entry in service::configuration::choices(app) {
        let id = entry.id();
        let mut selected = app.connect.services.selected.contains(&id);
        let count = cloud_ids.union(&app.connect.services.selected).count();
        let in_cloud = cloud_ids.contains(&id);
        let label = if in_cloud {
            tf("Use this computer’s details for %@", &[&entry.label()])
        } else {
            entry.label()
        };
        if ui.add_enabled(selected || in_cloud || count < limit, egui::Checkbox::new(&mut selected, label)).changed() {
            if selected { app.connect.services.selected.insert(id); } else { app.connect.services.selected.remove(&id); }
        }
    }
    ui.checkbox(&mut app.connect.services.preferences, t("Also synchronize language and appearance"));
    let needs_code = app.connect.services.vault.is_none() && !service::account_sync(app);
    if needs_code {
        if app.connect.services.identity.is_none() {
            if ui.button(t("Create recovery code")).clicked() {
                service::prepare(app);
            }
        } else {
            recovery(app, ui);
            ui.checkbox(&mut app.connect.services.confirm_recovery_saved, t("I saved my recovery code somewhere safe"));
        }
    }
    ui.horizontal_wrapped(|ui| {
        if ui.add_enabled(!needs_code || app.connect.services.confirm_recovery_saved, egui::Button::new(t("Merge and enable Connect"))).clicked() {
            service::confirm_review(app);
        }
        if ui.button(t("Keep using this computer only")).clicked() {
            let state = &mut app.connect.services;
            state.reviewing = false;
            state.review_ready = false;
            state.selected.clear();
            state.preferences = false;
            state.recovery_input.clear();
        }
    });
}

fn shared_data(app: &mut App, ui: &mut Ui, busy: bool) {
    widgets::section_heading(ui, &t("Data in Connect"), None);
    let cloud = app.connect.services.cloud.clone();
    group(ui, |ui| {
        let Some(cloud) = &cloud else {
            ui.label(t("Loading shared data…"));
            return;
        };
        let ids: std::collections::BTreeSet<_> = cloud.connections.iter().map(|entry| entry.id()).collect();
        if ids.is_empty() && cloud.preferences.is_none() {
            ui.label(t("No shared data in Connect."));
        }
        for entry in &cloud.connections {
            let id = entry.id();
            shared_data_row(app, ui, busy, &id, &entry.label());
        }
        if let Some(preferences) = &cloud.preferences {
            let language = preferences.language.map_or_else(|| t("System"), |language| language.native_name().into());
            let appearance = t(match preferences.appearance {
                theme::Appearance::System => "System",
                theme::Appearance::Light => "Light",
                theme::Appearance::Dark => "Dark",
            });
            shared_data_row(app, ui, busy, service::configuration::PREFERENCES_ID, &tf("Language: %@ · Appearance: %@", &[&language, &appearance]));
        }
        let local: Vec<_> = service::configuration::choices(app).into_iter().filter(|entry| !ids.contains(&entry.id())).collect();
        if !local.is_empty() {
            ui.add_space(8.0);
            ui.label(theme::text(t("Only on this computer"), Font::Headline));
        }
        let limit = service::connection_limit(app);
        for entry in local {
            let id = entry.id();
            let related = cloud.connections.iter().find(|saved| entry.same_audiobookshelf_login(saved));
            ui.horizontal_wrapped(|ui| {
                ui.label(entry.label());
                if app.connect.services.selected.contains(&id) {
                    ui.label(t("Waiting to synchronize"));
                } else if let Some(related) = related {
                    if app.connect.services.replace_target.as_deref() == Some(&id) {
                        ui.label(t("Replace the shared Audiobookshelf connection with the account verified on this computer?"));
                        if ui.add_enabled(!busy, egui::Button::new(t("Confirm replacement"))).clicked() {
                            service::replace_shared_connection(app, related.id(), id.clone());
                        }
                        if ui.button(t("Cancel")).clicked() {
                            app.connect.services.replace_target = None;
                        }
                    } else if ui.add_enabled(!busy, egui::Button::new(t("Replace on Connect"))).clicked() {
                        app.connect.services.replace_target = Some(id.clone());
                    }
                } else if ui.add_enabled(!busy && ids.union(&app.connect.services.selected).count() < limit, egui::Button::new(t("Add to Connect"))).clicked() {
                    app.connect.services.selected.insert(id);
                    service::save(app);
                    service::synchronize_now(app);
                }
            });
        }
        if !app.connect.services.backups.is_empty() {
            ui.add(egui::Label::new(theme::secondary(ui, t("Removing a connection leaves existing backups intact. Delete all Connect cloud data to remove those backups too."), Font::Footnote)).wrap());
        }
    });
}

fn shared_data_row(app: &mut App, ui: &mut Ui, busy: bool, id: &str, label: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.label(label);
        let state = &mut app.connect.services;
        if state.identity.as_ref().is_some_and(|i| i.removed.contains(id) || i.replacement.as_ref().is_some_and(|(previous, _)| previous == id)) {
            ui.label(t("Waiting to synchronize"));
        } else if state.remove_target.as_deref() == Some(id) {
            ui.label(t("Local copies and downloads are kept."));
            if ui.add_enabled(!busy, egui::Button::new(t("Confirm removal"))).clicked() {
                if id == service::configuration::PREFERENCES_ID {
                    service::remove_shared_preferences(app);
                } else {
                    service::remove_shared_connection(app, id.into());
                }
            }
            if ui.button(t("Cancel")).clicked() {
                app.connect.services.remove_target = None;
            }
        } else if ui.add_enabled(!busy, egui::Button::new(t("Remove from Connect"))).clicked() {
            state.remove_target = Some(id.into());
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
