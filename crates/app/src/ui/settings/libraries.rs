//! Settings > Libraries and accounts: music servers (switch, verify, alias, password, remove), then the
//! Audiobookshelf and internet radio sections owned by those features.

use std::collections::HashMap;

use discophile_core::l10n::t;
use discophile_core::{MusicProvider, connection_alias};
use egui::{Align, Color32, CornerRadius, Layout, Ui, vec2};

use crate::app::App;
use crate::session;
use crate::theme::{self, Font, Palette, icons};
use crate::ui::accounts::{Look, labeled, labeled_button};
use crate::ui::widgets;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Editing {
    Alias,
    Password,
}

#[derive(Clone, Default)]
struct State {
    /// The account whose alias or password editor is open.
    editing: Option<(String, Editing)>,
    alias: String,
    password: String,
    show_password: bool,
    saving: bool,
    error: Option<String>,
    /// Verify connection results: account id to (success, message).
    verified: HashMap<String, (bool, String)>,
    verifying: Option<String>,
    /// Account awaiting removal confirmation.
    removing: Option<String>,
}

fn state_id() -> egui::Id {
    egui::Id::unique("settings-libraries")
}

fn load(ctx: &egui::Context) -> State {
    ctx.data(|d| d.get_temp(state_id())).unwrap_or_default()
}

fn store(ctx: &egui::Context, state: State) {
    ctx.data_mut(|d| d.insert_temp(state_id(), state));
}

enum Action {
    Switch(String),
    Verify(String),
    SaveAlias(String, String),
    SavePassword(String, String),
    Remove(String),
    AddAccount,
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let mut state = load(&ctx);
    let mut actions = Vec::new();

    widgets::section_heading(ui, &t("Music libraries"), None);
    let accounts: Vec<_> = app.session.accounts.clone();
    let active = app.session.account_key();
    if accounts.is_empty() {
        ui.label(theme::secondary(ui, t("No music library is connected yet."), Font::Callout));
        ui.add_space(6.0);
    }
    for account in &accounts {
        let id = account.id();
        account_card(ui, &mut state, &mut actions, account, id == active, app.session.busy.is_some());
        ui.add_space(8.0);
    }
    if labeled_button(ui, icons::PLUS_CIRCLE, &t("Add account"), Look::Filled).clicked() {
        actions.push(Action::AddAccount);
    }
    super::footer(
        ui,
        &t(
            "Switch libraries here or from the account menu in the toolbar. Each server keeps its own credentials and downloads. Switching stops current playback.",
        ),
    );

    if let Some(id) = state.removing.clone() {
        let name = accounts.iter().find(|a| a.id() == id).map(|a| a.display_name()).unwrap_or_default();
        match remove_confirmation(&ctx, &name) {
            Some(true) => {
                state.removing = None;
                actions.push(Action::Remove(id));
            }
            Some(false) => state.removing = None,
            None => {}
        }
    }

    // Save before acting: session callbacks may run synchronously and update this state themselves.
    store(&ctx, state);
    for action in actions {
        perform(app, &ctx, action);
    }

    ui.add_space(12.0);
    crate::ui::audiobooks::account_settings(app, ui);
    ui.add_space(12.0);
    crate::ui::radio::account_settings(app, ui);
}

fn account_card(
    ui: &mut Ui,
    state: &mut State,
    actions: &mut Vec<Action>,
    account: &discophile_core::ServerAccount,
    active: bool,
    busy: bool,
) {
    let palette = Palette::of_ui(ui);
    let id = account.id();
    let provider = account.credentials.provider;
    widgets::card(ui, |ui| {
        ui.horizontal(|ui| {
            let mark = if active {
                theme::icon_filled(icons::CHECK_CIRCLE, 20.0).color(palette.accent)
            } else {
                theme::icon_filled(icons::MUSIC_NOTES, 20.0).color(palette.secondary)
            };
            ui.add_sized([24.0, 24.0], egui::Label::new(mark).selectable(false));
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(theme::text(account.display_name(), Font::Headline));
                    if active {
                        ui.label(theme::accent(ui, t("Active"), Font::Footnote));
                    }
                });
                ui.label(theme::secondary(ui, format!("{} · {}", provider.title(), account.subtitle()), Font::Subheadline));
            });
        });
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if !active && ui.add_enabled(!busy, labeled(ui, icons::ARROWS_CLOCKWISE, &t("Switch"), Look::Filled)).clicked() {
                actions.push(Action::Switch(id.clone()));
            }
            if ui.add_enabled(!busy, labeled(ui, icons::CHECK_CIRCLE, &t("Verify connection"), Look::Tinted)).clicked() {
                actions.push(Action::Verify(id.clone()));
            }
            if labeled_button(ui, icons::PENCIL_SIMPLE, &t("Edit alias"), Look::Tinted).clicked() {
                state.editing = Some((id.clone(), Editing::Alias));
                state.alias = account.name.clone();
                state.error = None;
            }
            let password_label = if provider == MusicProvider::Jellyfin { t("Change password") } else { t("Change password or API key") };
            if labeled_button(ui, icons::KEY, &password_label, Look::Tinted).clicked() {
                state.editing = Some((id.clone(), Editing::Password));
                state.password.clear();
                state.show_password = false;
                state.error = None;
            }
            if ui.add_enabled(!busy, labeled(ui, icons::TRASH, &t("Remove server"), Look::Danger)).clicked() {
                state.removing = Some(id.clone());
            }
        });

        if state.verifying.as_deref() == Some(id.as_str()) {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(theme::secondary(ui, t("Verifying…"), Font::Callout));
            });
        } else if let Some((ok, message)) = state.verified.get(&id) {
            let (icon, color) = if *ok { (icons::CHECK_CIRCLE, palette.positive) } else { (icons::WARNING_CIRCLE, palette.danger) };
            ui.horizontal_wrapped(|ui| {
                ui.label(theme::icon_filled(icon, 16.0).color(color));
                ui.label(theme::text(message, Font::Callout).color(color));
            });
        }

        match state.editing.clone() {
            Some((editing, Editing::Alias)) if editing == id => {
                ui.add_space(6.0);
                crate::ui::accounts::field_label(ui, &t("Alias (optional)"));
                let suggested = connection_alias::suggested(provider.alias_title(), Some(&account.credentials.server), true);
                let field = ui.add(egui::TextEdit::singleline(&mut state.alias).align(egui::Align2::LEFT_CENTER).hint_text(suggested.clone()).desired_width(f32::INFINITY));
                let hint = format!("{}. {} {suggested}", t("Alias (optional)"), t("Leave empty to use the suggested name."));
                field.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, &hint));
                let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                ui.horizontal(|ui| {
                    if widgets::action_button(ui, t("Save alias"), false).clicked() || enter {
                        actions.push(Action::SaveAlias(id.clone(), state.alias.clone()));
                        state.editing = None;
                    }
                    if widgets::action_button(ui, t("Cancel"), true).clicked() {
                        state.editing = None;
                    }
                });
                ui.label(theme::secondary(
                    ui,
                    t("Changing the alias keeps your password, downloads and current playback."),
                    Font::Footnote,
                ));
            }
            Some((editing, Editing::Password)) if editing == id => {
                ui.add_space(6.0);
                let label = if provider == MusicProvider::Jellyfin { t("New password") } else { t("New password or API key") };
                crate::ui::accounts::field_label(ui, &label);
                let (field, _) = ui
                    .add_enabled_ui(!state.saving, |ui| {
                        crate::ui::accounts::password_field(ui, &mut state.password, &mut state.show_password, &label)
                    })
                    .inner;
                let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                ui.horizontal(|ui| {
                    let ready = !state.password.is_empty() && !state.saving;
                    let label = if state.saving { t("Connecting…") } else { t("Save") };
                    let save = ui.add_enabled(
                        ready,
                        egui::Button::new(theme::text(label, Font::Headline).color(Color32::WHITE))
                            .fill(palette.action)
                            .corner_radius(CornerRadius::same(14))
                            .min_size(vec2(0.0, 40.0)),
                    );
                    if state.saving {
                        ui.spinner();
                    }
                    if (save.clicked() || (enter && ready)) && ready {
                        actions.push(Action::SavePassword(id.clone(), state.password.clone()));
                    }
                    if ui
                        .add_enabled(!state.saving, egui::Button::new(theme::accent(ui, t("Cancel"), Font::Headline)).frame(false))
                        .clicked()
                    {
                        state.editing = None;
                        state.password.clear();
                    }
                });
                if let Some(error) = &state.error {
                    widgets::error_banner(ui, error, false);
                }
                ui.label(theme::secondary(ui, t("The new password is checked with your server before it is saved."), Font::Footnote));
            }
            _ => {}
        }
    });
}

/// Returns `Some(true)` to remove, `Some(false)` to cancel, `None` while open.
fn remove_confirmation(ctx: &egui::Context, name: &str) -> Option<bool> {
    let modal = crate::ui::widgets::modal(ctx, egui::Id::unique("remove-server")).show(ctx, |ui| {
        ui.set_width(400.0);
        let palette = Palette::of_ui(ui);
        ui.label(theme::text(t("Remove this server?"), Font::Title3));
        ui.label(theme::secondary(ui, name, Font::Subheadline));
        ui.add_space(6.0);
        let message = if crate::platform::IS_WEB {
            "This removes only this server's saved connection from this browser. Other libraries are kept. Music on your server is not deleted."
        } else {
            "This removes only this server's saved connection and downloads from this computer. Other libraries are kept. Music on your server is not deleted."
        };
        ui.label(theme::text(t(message), Font::Body));
        ui.add_space(12.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let remove = egui::Button::new(theme::text(t("Remove server"), Font::Headline).color(Color32::WHITE))
                .fill(palette.danger_fill)
                .corner_radius(CornerRadius::same(14))
                .min_size(vec2(0.0, 40.0));
            if ui.add(remove).clicked() {
                return Some(true);
            }
            if widgets::action_button(ui, t("Cancel"), true).clicked() {
                return Some(false);
            }
            None
        })
        .inner
    });
    if modal.inner.is_some() {
        return modal.inner;
    }
    modal.should_close().then_some(false)
}

fn perform(app: &mut App, ctx: &egui::Context, action: Action) {
    match action {
        Action::AddAccount => {
            app.sheets.add_account_provider = None;
            app.sheets.add_account = true;
        }
        Action::Switch(id) => session::switch_account(app, &id),
        Action::Remove(id) => {
            let mut state = load(ctx);
            state.verified.remove(&id);
            if state.editing.as_ref().is_some_and(|(editing, _)| *editing == id) {
                state.editing = None;
            }
            store(ctx, state);
            session::remove_account(app, &id);
        }
        Action::SaveAlias(id, alias) => {
            session::rename_account(app, &id, &alias);
        }
        Action::Verify(id) => {
            let mut state = load(ctx);
            state.verifying = Some(id.clone());
            state.verified.remove(&id);
            store(ctx, state);
            let key = id.clone();
            session::verify_account(app, &id, move |app, error| {
                let ctx = app.ctx().clone();
                let mut state = load(&ctx);
                state.verifying = None;
                let result = match error {
                    None => (true, t("Connection verified.")),
                    Some(message) => (false, message),
                };
                state.verified.insert(key, result);
                store(&ctx, state);
            });
        }
        Action::SavePassword(id, password) => {
            let mut state = load(ctx);
            state.saving = true;
            state.error = None;
            store(ctx, state);
            let key = id.clone();
            session::update_password(app, &id, &password, move |app, error| {
                let ctx = app.ctx().clone();
                let mut state = load(&ctx);
                state.saving = false;
                match error {
                    None => {
                        if state.editing.as_ref().is_some_and(|(editing, _)| *editing == key) {
                            state.editing = None;
                        }
                        state.password.clear();
                        state.verified.insert(key, (true, t("Connection verified.")));
                        app.toast(t("Password saved."));
                    }
                    Some(message) => state.error = Some(message),
                }
                store(&ctx, state);
            });
        }
    }
}
