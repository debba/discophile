//! Settings > Discophile Connect: the instance (the official one or a custom address) and the optional
//! account with its plan. The account never restricts local playback.

use discophile_core::l10n::t;
use egui::{Ui, vec2};

use crate::app::App;
use crate::features::connect as feature;
use crate::theme::{self, Font, Palette, icons};
use crate::ui::accounts::{Look, labeled};
use crate::ui::settings::{footer, group};
use crate::ui::widgets;

#[cfg(not(target_arch = "wasm32"))]
mod services;
#[cfg(not(target_arch = "wasm32"))]
mod profile;
#[cfg(not(target_arch = "wasm32"))]
pub use profile::toolbar;
#[cfg(not(target_arch = "wasm32"))]
pub use services::player_devices;

pub fn settings(app: &mut App, ui: &mut Ui) {
    ui.add(egui::Label::new(theme::text(
        t("An optional account for Discophile services on a Connect instance. Discophile works fully without it."),
        Font::Callout,
    )).wrap());
    instance_section(app, ui);
    account_section(app, ui);
    #[cfg(not(target_arch = "wasm32"))]
    services::settings(app, ui);
    footer(
        ui,
        &t("Standalone use and direct connections to your own servers never need a Connect account, and your plan never limits local playback."),
    );
    footer(ui, &t("Connect synchronizes only the connections and shared preferences you select. Your audio files stay on your servers and computers."));
}

pub fn onboarding(app: &mut App, ui: &mut Ui) {
    ui.label(theme::secondary(ui, t("Connect server"), Font::Footnote));
    ui.label(theme::text(app.connect.origin.to_string(), Font::Callout));
    egui::CollapsingHeader::new(t("Use another Connect server"))
        .id_salt("onboarding-connect-instance")
        .default_open(!app.connect.origin.is_official())
        .show(ui, |ui| instance_section(app, ui));
    ui.add_space(20.0);
    #[cfg(not(target_arch = "wasm32"))]
    sign_in_actions(app, ui);
    if let Some(error) = app.connect.error.clone() {
        ui.add_space(8.0);
        error_text(ui, &error);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn sign_in_actions(app: &mut App, ui: &mut Ui) {
    if let Some(url) = app.connect.sign_in.as_ref().map(|s| s.url.clone()) {
        ui.horizontal_wrapped(|ui| {
            ui.spinner();
            let text = if url.is_some() {
                t("Finish signing in in your browser. Discophile receives the answer on this computer.")
            } else {
                t("Contacting the instance…")
            };
            ui.label(theme::secondary(ui, text, Font::Callout));
        });
        ui.horizontal_wrapped(|ui| {
            if let Some(url) = &url
                && ui.link(t("Open the sign-in page again")).clicked()
            {
                ui.ctx().open_url(egui::OpenUrl::new_tab(url));
            }
            if ui.button(t("Cancel")).clicked() {
                feature::cancel_sign_in(app);
            }
        });
    } else {
        let ready = !app.connect.busy && !app.connect.checking_instance;
        if ui.add_enabled_ui(ready, |ui| {
            crate::ui::welcome::wide_action(ui, Some(icons::USER_CIRCLE), &t("Sign in to Discophile Connect"), None, false)
        }).inner.clicked() {
            feature::sign_in(app);
        }
    }
}

fn error_text(ui: &mut Ui, text: &str) {
    let palette = Palette::of_ui(ui);
    ui.add(egui::Label::new(theme::text(format!("{}  {text}", icons::WARNING_CIRCLE), Font::Callout).color(palette.danger)).wrap());
}

// MARK: Instance

fn instance_section(app: &mut App, ui: &mut Ui) {
    widgets::section_heading(ui, &t("Instance"), None);
    let palette = Palette::of_ui(ui);
    let state = &app.connect;
    let origin = state.origin.clone();
    let name = state.server_info.as_ref().and_then(|info| info.name.clone());
    let checking = state.checking_instance;
    let error = state.instance_error.clone();
    let signed_in = feature::is_signed_in(app);
    let mut apply = false;
    let mut official = false;
    group(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(theme::icon_filled(icons::GLOBE, 22.0).color(palette.accent));
            ui.vertical(|ui| {
                let fallback = if origin.is_official() { t("Official instance") } else { t("Custom instance") };
                ui.add(egui::Label::new(theme::text(name.clone().unwrap_or(fallback), Font::Headline)).wrap());
                let detail = if origin.is_official() && name.is_some() {
                    format!("{} · {}", origin, t("Official instance"))
                } else {
                    origin.to_string()
                };
                ui.add(egui::Label::new(theme::secondary(ui, detail, Font::Callout)).wrap());
            });
        });
        if origin.is_insecure() {
            ui.add_space(6.0);
            ui.add(
                egui::Label::new(
                    theme::text(
                        format!(
                            "{}  {}",
                            icons::LOCK_SIMPLE_OPEN,
                            t("This address uses HTTP, so the connection is not encrypted. Use it only to develop on this computer.")
                        ),
                        Font::Callout,
                    )
                    .color(palette.danger),
                )
                .wrap(),
            );
        }
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);
        let label = ui.label(theme::text(t("Custom address"), Font::Headline));
        ui.horizontal_wrapped(|ui| {
            let response = ui
                .add_enabled(
                    !checking,
                    egui::TextEdit::singleline(&mut app.connect.address)
                        .align(egui::Align2::LEFT_CENTER)
                        .hint_text("https://connect.example.org")
                        .desired_width(300.0)
                        .margin(vec2(10.0, 8.0)),
                )
                .labelled_by(label.id);
            if response.changed() {
                app.connect.instance_error = None;
            }
            let typed = !app.connect.address.trim().is_empty();
            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && typed {
                apply = true;
            }
            if ui.add_enabled(typed && !checking, labeled(ui, icons::CHECK_CIRCLE, &t("Use this address"), Look::Tinted)).clicked() {
                apply = true;
            }
            if checking {
                ui.spinner();
                ui.label(theme::secondary(ui, t("Checking the instance…"), Font::Callout));
            }
        });
        if let Some(error) = &error {
            ui.add_space(4.0);
            error_text(ui, error);
        }
        if !origin.is_official() {
            ui.add_space(6.0);
            if ui.add_enabled(!checking, labeled(ui, icons::ARROW_COUNTER_CLOCKWISE, &t("Use the official instance"), Look::Plain)).clicked() {
                official = true;
            }
        }
    });
    footer(
        ui,
        &t(if signed_in {
            "Changing the instance signs you out of this one and requires a new sign-in. Nothing is moved to the new instance."
        } else {
            "Changing the instance requires a new sign-in. Nothing is moved to the new instance."
        }),
    );
    if apply {
        feature::apply_instance(app);
    }
    if official {
        feature::use_official_instance(app);
    }
}

// MARK: Account

fn account_section(app: &mut App, ui: &mut Ui) {
    widgets::section_heading(ui, &t("Account"), None);
    #[cfg(target_arch = "wasm32")]
    {
        let _ = app;
        group(ui, |ui| {
            ui.add(
                egui::Label::new(theme::text(
                    t("Signing in to Discophile Connect needs Discophile for computers: the web player cannot receive the sign-in redirect."),
                    Font::Body,
                ))
                .wrap(),
            );
        });
    }
    #[cfg(not(target_arch = "wasm32"))]
    desktop::account_controls(app, ui);
}

/// The account controls of Discophile for computers, the only build that can sign in.
#[cfg(not(target_arch = "wasm32"))]
mod desktop {
    use discophile_core::connect::{AccountInfo, EntitlementKind, EntitlementStatus, PlanSource, ServerInfo, Tier};
    use discophile_core::l10n::{count, t};
    use egui::Ui;

    use super::error_text;
    use crate::app::App;
    use crate::features::connect as feature;
    use crate::theme::{self, Font, Palette, icons};
    use crate::ui::accounts::{Look, labeled};
    use crate::ui::settings::{footer, group, value_row};

    pub(super) fn account_controls(app: &mut App, ui: &mut Ui) {
        let state = &app.connect;
        let signed_in = state.credential.is_some();
        let account = state.account.clone();
        let info = state.server_info.clone();
        let busy = state.busy;
        let pending = state.sign_in.is_some();
        let error = state.error.clone();
        let notice = state.notice.clone();
        group(ui, |ui| {
            if pending {
                super::sign_in_actions(app, ui);
            } else if signed_in {
                match &account {
                    Some(account) => account_details(ui, account, info.as_ref()),
                    None if busy => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(theme::secondary(ui, t("Loading your account…"), Font::Callout));
                        });
                    }
                    None => {
                        ui.label(theme::text(t("Signed in. Your account details are not available right now."), Font::Body));
                    }
                }
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    if let Some(dashboard) = info.as_ref().and_then(|i| i.dashboard.clone()) {
                        let manage = ui.add(labeled(ui, icons::ARROW_SQUARE_OUT, &t("Manage account"), Look::Tinted));
                        if manage.on_hover_text(dashboard.as_str()).clicked() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(dashboard.as_str()));
                        }
                    }
                    if ui.add_enabled(!busy, labeled(ui, icons::ARROWS_CLOCKWISE, &t("Refresh"), Look::Plain)).clicked() {
                        feature::refresh_account(app);
                    }
                    if busy && account.is_some() {
                        ui.spinner();
                    }
                    if ui.add(labeled(ui, icons::SIGN_OUT, &t("Sign out"), Look::Danger)).clicked() {
                        feature::sign_out(app);
                    }
                });
            } else {
                ui.add(
                    egui::Label::new(theme::text(
                        t("Sign in to see your plan on this instance. You sign in on the instance's website in your browser."),
                        Font::Body,
                    ))
                    .wrap(),
                );
                ui.add_space(8.0);
                super::sign_in_actions(app, ui);
            }
            if let Some(error) = &error {
                ui.add_space(6.0);
                error_text(ui, error);
            }
            if let Some(notice) = &notice {
                ui.add_space(6.0);
                ui.add(egui::Label::new(theme::secondary(ui, notice, Font::Callout)).wrap());
            }
        });
        footer(
            ui,
            &t("Access tokens use this computer's secrets store. Signing out removes them and revokes Discophile's access on the instance."),
        );
    }

    fn account_details(ui: &mut Ui, account: &AccountInfo, info: Option<&ServerInfo>) {
        let palette = Palette::of_ui(ui);
        let details = &account.account;
        let name = if details.name.trim().is_empty() { t("Not set") } else { details.name.clone() };
        value_row(ui, &t("Name"), &name);
        let email = match &details.email {
            Some(email) if details.email_verified => email.clone(),
            Some(email) => format!("{email} · {}", t("not verified")),
            None => t("Not set"),
        };
        value_row(ui, &t("Email"), &email);
        let plan = &account.plan;
        value_row(ui, &t("Plan"), &tier_label(&plan.tier, &plan.name));
        value_row(ui, &t("Assigned"), &source_label(&plan.source));
        let expires = plan.expires_at.map(|date| crate::ui::curator::date_label(date.timestamp_millis())).unwrap_or_else(|| t("Never"));
        value_row(ui, &t("Expires"), &expires);
        let entitlements = plan.known_entitlements();
        ui.add_space(10.0);
        ui.label(theme::text(t("Included in your plan"), Font::Headline));
        if entitlements.is_empty() {
            ui.label(theme::secondary(ui, t("No Connect services are included in this plan."), Font::Callout));
            return;
        }
        for (kind, entitlement) in entitlements {
            let status = info.map(|info| entitlement.status(info)).unwrap_or(EntitlementStatus::Planned);
            let (marker, color) = match status {
                EntitlementStatus::Planned => (t("Planned · not available yet"), palette.secondary),
                EntitlementStatus::NotInThisVersion => (t("Not supported by this version of Discophile yet"), palette.secondary),
                EntitlementStatus::Included => (t("Included"), palette.positive),
            };
            ui.add_space(4.0);
            ui.add(egui::Label::new(theme::text(entitlement_label(kind, entitlement.limit), Font::Body)).wrap());
            ui.add(egui::Label::new(theme::text(marker, Font::Footnote).color(color)).wrap());
        }
    }

    /// Plan names are product names, identical in every language.
    pub(super) fn tier_label(tier: &Tier, name: &str) -> String {
        match tier {
            Tier::Free => "Free".into(),
            Tier::Connect => "Connect".into(),
            Tier::Supporter => "Supporter".into(),
            Tier::Other(id) if name.trim().is_empty() => id.clone(),
            Tier::Other(_) => name.trim().to_string(),
        }
    }

    fn source_label(source: &PlanSource) -> String {
        t(match source {
            PlanSource::Default => "Default plan",
            PlanSource::Manual => "Assigned by the instance's administrators",
            PlanSource::Other(_) => "Assigned by the instance",
        })
    }

    pub(super) fn entitlement_label(kind: EntitlementKind, limit: Option<u64>) -> String {
        match kind {
            EntitlementKind::DevicesPaired => match limit {
                Some(limit) => count("Up to %lld paired playback devices", limit.min(i64::MAX as u64) as i64),
                None => t("Paired playback devices"),
            },
            EntitlementKind::SyncConnections => match limit {
                Some(limit) => discophile_core::l10n::tf("Encrypted synchronization of up to %lld server connections", &[&limit]),
                None => t("Encrypted synchronization of library connections"),
            },
            EntitlementKind::SyncPreferences => t("Encrypted synchronization of chosen preferences"),
            EntitlementKind::BackupConfiguration => t("Encrypted configuration backups"),
            EntitlementKind::PlaybackRemote => t("Remote control"),
            EntitlementKind::PlaybackHandoff => t("Playback handoff between devices"),
            EntitlementKind::SupporterRecognition => t("Optional public recognition"),
            EntitlementKind::Previews => t("Access to previews when they exist"),
        }
    }
}
