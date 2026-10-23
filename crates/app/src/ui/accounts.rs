//! The Add account sheet: choose a provider, then sign in. Music servers use the form here;
//! Audiobookshelf and internet radio embed the forms owned by their features.

use discophile_core::connection_alias;
use discophile_core::l10n::t;
use discophile_core::{MusicProvider, validate_server};
use egui::{Align, CornerRadius, Layout, Sense, Ui, vec2};

use crate::app::App;
use crate::session;
use crate::theme::{self, Font, Palette, icons};
use crate::ui::widgets;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Choice {
    Music(MusicProvider),
    Audiobookshelf,
    Radio,
}

impl Choice {
    fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "subsonic" | "opensubsonic" => Some(Self::Music(MusicProvider::Subsonic)),
            "navidrome" => Some(Self::Music(MusicProvider::Navidrome)),
            "jellyfin" => Some(Self::Music(MusicProvider::Jellyfin)),
            "audiobookshelf" => Some(Self::Audiobookshelf),
            "radio" => Some(Self::Radio),
            _ => None,
        }
    }
}

#[derive(Clone, Default)]
struct Form {
    /// The provider was chosen in this session of the sheet (`None` shows the provider list).
    choice: Option<Choice>,
    /// The sheet opened directly on a provider, so Back closes it instead of showing the list.
    preselected: bool,
    alias: String,
    server: String,
    username: String,
    password: String,
    show_password: bool,
    allow_http: bool,
    busy: bool,
    error: Option<String>,
    /// Bumped whenever the sheet closes, so a late connection result does not touch a new form.
    attempt: u64,
}

fn form_id() -> egui::Id {
    egui::Id::unique("add-account-form")
}

enum Action {
    None,
    Close,
    Connect,
}

/// The Add account sheet (provider choice then form), shown while `app.sheets.add_account` is set.
pub fn add_account_window(app: &mut App, ctx: &egui::Context) {
    if !app.sheets.add_account {
        return;
    }
    let mut form: Form = ctx.data(|d| d.get_temp(form_id())).unwrap_or_default();
    if let Some(provider) = app.sheets.add_account_provider.take() {
        form = Form { attempt: form.attempt, ..Default::default() };
        form.choice = Choice::parse(&provider);
        form.preselected = form.choice.is_some();
    }

    let mut action = Action::None;
    let max_height = ctx.content_rect().height() - 80.0;
    let modal = egui::Modal::new(egui::Id::unique("add-account")).show(ctx, |ui| {
        ui.set_width(480.0f32.min(ctx.content_rect().width() - 48.0));
        egui::ScrollArea::vertical()
            .id_salt("add-account-scroll")
            .max_height(max_height)
            .min_scrolled_height(max_height)
            .auto_shrink([false, true])
            .show(ui, |ui| match form.choice {
                None => {
                    if provider_list(ui, &mut form) {
                        action = Action::Close;
                    }
                }
                Some(Choice::Music(provider)) => {
                    action = music_form(ui, &mut form, provider);
                }
                Some(choice) => {
                    if header(ui, &mut form, &title(choice)) {
                        action = Action::Close;
                    }
                    let done = if choice == Choice::Audiobookshelf {
                        crate::ui::audiobooks::connect_form(app, ui)
                    } else {
                        crate::ui::radio::station_form(app, ui)
                    };
                    if done {
                        action = Action::Close;
                    }
                }
            });
    });
    let escape = modal.is_top_modal && ctx.input(|i| i.key_pressed(egui::Key::Escape));
    if escape && !form.busy {
        action = Action::Close;
    }

    match action {
        Action::Close => {
            app.sheets.add_account = false;
            form = Form { attempt: form.attempt + 1, ..Default::default() };
            ctx.data_mut(|d| d.insert_temp(form_id(), form));
        }
        Action::Connect => {
            let Some(Choice::Music(provider)) = form.choice else { return };
            form.busy = true;
            form.error = None;
            let attempt = form.attempt;
            let (server, username, password, allow_http, alias) = (
                form.server.trim().to_string(),
                form.username.trim().to_string(),
                form.password.clone(),
                form.allow_http,
                form.alias.clone(),
            );
            ctx.data_mut(|d| d.insert_temp(form_id(), form));
            session::add_account(app, provider, &server, &username, &password, allow_http, &alias, move |app, error| {
                let ctx = app.ctx().clone();
                let mut form: Form = ctx.data(|d| d.get_temp(form_id())).unwrap_or_default();
                if form.attempt != attempt {
                    return;
                }
                form.busy = false;
                match error {
                    None => {
                        app.sheets.add_account = false;
                        form = Form { attempt: form.attempt + 1, ..Default::default() };
                    }
                    Some(message) => form.error = Some(message),
                }
                ctx.data_mut(|d| d.insert_temp(form_id(), form));
            });
        }
        Action::None => {
            ctx.data_mut(|d| d.insert_temp(form_id(), form));
        }
    }
}

fn title(choice: Choice) -> String {
    match choice {
        Choice::Music(provider) => provider.title().to_string(),
        Choice::Audiobookshelf => "Audiobookshelf".to_string(),
        Choice::Radio => t("Internet Radio"),
    }
}

/// The sheet's title row. Returns true when the sheet should close.
fn header(ui: &mut Ui, form: &mut Form, title: &str) -> bool {
    let mut close = false;
    ui.horizontal(|ui| {
        if form.choice.is_some() && !form.preselected {
            ui.add_enabled_ui(!form.busy, |ui| {
                if glyph_button(ui, icons::CARET_LEFT, &t("Back"), 17.0).clicked() {
                    form.choice = None;
                    form.error = None;
                }
            });
        }
        ui.label(theme::text(title, Font::Title2));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let label = if form.choice.is_none() { t("Done") } else { t("Cancel") };
            if ui.add(egui::Button::new(theme::accent(ui, label, Font::Headline)).frame(false)).clicked() {
                close = true;
            }
        });
    });
    ui.add_space(8.0);
    close
}

/// Step one: the provider list. Returns true when the sheet should close.
fn provider_list(ui: &mut Ui, form: &mut Form) -> bool {
    let close = header(ui, form, &t("Add account"));
    group_label(ui, &t("Music libraries"));
    for provider in MusicProvider::ALL {
        if provider_row(ui, icons::MUSIC_NOTES, provider.title()).clicked() {
            form.choice = Some(Choice::Music(provider));
            form.error = None;
        }
    }
    group_label(ui, &t("Audiobooks"));
    if provider_row(ui, icons::BOOKS, "Audiobookshelf").clicked() {
        form.choice = Some(Choice::Audiobookshelf);
    }
    group_label(ui, &t("Internet Radio"));
    if provider_row(ui, icons::RADIO, &t("Internet Radio")).clicked() {
        form.choice = Some(Choice::Radio);
    }
    ui.label(theme::secondary(ui, t("Add a station using its direct audio stream URL. No account is needed."), Font::Footnote));
    close
}

fn group_label(ui: &mut Ui, text: &str) {
    ui.add_space(8.0);
    ui.label(theme::secondary(ui, text, Font::Footnote).strong());
}

fn provider_row(ui: &mut Ui, icon: &str, label: &str) -> egui::Response {
    let palette = Palette::of_ui(ui);
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::click());
    let fill = if response.hovered() || response.has_focus() { palette.surface_raised } else { palette.background };
    ui.painter().rect_filled(rect, CornerRadius::same(12), fill);
    ui.painter().text(
        rect.left_center() + vec2(22.0, 0.0),
        egui::Align2::CENTER_CENTER,
        icon,
        egui::FontId::new(19.0, theme::icon_fill()),
        palette.accent,
    );
    ui.painter().text(rect.left_center() + vec2(44.0, 0.0), egui::Align2::LEFT_CENTER, label, Font::Body.id(), palette.text);
    ui.painter().text(
        rect.right_center() - vec2(16.0, 0.0),
        egui::Align2::CENTER_CENTER,
        icons::CARET_RIGHT,
        egui::FontId::new(14.0, theme::icon_fill()),
        palette.secondary,
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Step two for music servers.
fn music_form(ui: &mut Ui, form: &mut Form, provider: MusicProvider) -> Action {
    let palette = Palette::of_ui(ui);
    if header(ui, form, &t("Connect your server")) {
        return Action::Close;
    }
    ui.horizontal(|ui| {
        let icon = crate::artwork::app_icon(ui.ctx(), 128);
        ui.add(egui::Image::new(&icon).fit_to_exact_size(vec2(56.0, 56.0)).corner_radius(CornerRadius::same(16)));
        ui.vertical(|ui| {
            ui.label(theme::text(t("Make yourself at home."), Font::Title3));
            let note = if crate::platform::IS_WEB {
                "Connect directly to your music server. In the web player, credentials are kept in this browser's storage."
            } else {
                "Connect directly to your music server. Your credentials stay in this computer's keychain."
            };
            ui.label(theme::secondary(ui, t(note), Font::Subheadline));
        });
    });
    ui.add_space(10.0);
    ui.label(theme::text(provider.title(), Font::Headline));
    ui.add_space(2.0);

    let mut submit = false;
    ui.add_enabled_ui(!form.busy, |ui| {
        let suggested = connection_alias::suggested(provider.alias_title(), validate_server(&form.server, true).ok().as_ref(), true);
        field_label(ui, &t("Alias (optional)"));
        let alias = ui.add(egui::TextEdit::singleline(&mut form.alias).hint_text(suggested.clone()).desired_width(f32::INFINITY));
        let hint = format!("{} {suggested}", t("Leave empty to use the suggested name."));
        alias.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, format!("{}. {hint}", t("Alias (optional)"))));

        field_label(ui, &t("Server URL"));
        let server =
            ui.add(egui::TextEdit::singleline(&mut form.server).hint_text("https://music.example.com").desired_width(f32::INFINITY));
        server.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, t("Server URL")));

        field_label(ui, &t("Username"));
        let user = ui.add(egui::TextEdit::singleline(&mut form.username).desired_width(f32::INFINITY));
        user.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, t("Username")));

        let password_label = if provider == MusicProvider::Jellyfin { t("Password") } else { t("Password or API key") };
        field_label(ui, &password_label);
        let (password, _) = password_field(ui, &mut form.password, &mut form.show_password, &password_label);
        if password.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            submit = true;
        }

        ui.add_space(6.0);
        ui.checkbox(&mut form.allow_http, t("Allow HTTP for a trusted local server"));
        ui.label(theme::secondary(ui, t("HTTP does not encrypt your connection. Only use it on a trusted local network."), Font::Footnote));
    });

    if let Some(error) = &form.error {
        ui.add_space(6.0);
        widgets::error_banner(ui, error, false);
    }
    ui.add_space(10.0);
    let ready = !form.server.trim().is_empty() && !form.username.trim().is_empty() && !form.password.is_empty();
    let label = if form.busy { t("Connecting…") } else { t("Connect") };
    let button = egui::Button::new(theme::text(label, Font::Headline).color(egui::Color32::WHITE))
        .fill(palette.action)
        .corner_radius(CornerRadius::same(16))
        .min_size(vec2(ui.available_width(), 44.0));
    ui.horizontal(|ui| {
        let response = ui.add_enabled(ready && !form.busy, button);
        if form.busy {
            let spinner = egui::Rect::from_center_size(response.rect.left_center() + vec2(24.0, 0.0), vec2(18.0, 18.0));
            ui.put(spinner, egui::Spinner::new().color(egui::Color32::WHITE));
        }
        if response.clicked() {
            submit = true;
        }
    });
    ui.add_space(4.0);
    let footer = if provider == MusicProvider::Jellyfin {
        "Sign in with your Jellyfin username and password. Discophile saves an access token instead of your password. Browser sign-in is not supported."
    } else {
        "Use your Subsonic password or dedicated API key. Two-factor login and browser-only sign-in are not supported."
    };
    ui.label(theme::secondary(ui, t(footer), Font::Footnote));
    if submit && ready && !form.busy { Action::Connect } else { Action::None }
}

pub(crate) fn field_label(ui: &mut Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(theme::secondary(ui, text, Font::Subheadline));
}

/// A password field with a show/hide toggle. Returns the text field and toggle responses.
pub(crate) fn password_field(ui: &mut Ui, text: &mut String, visible: &mut bool, label: &str) -> (egui::Response, egui::Response) {
    ui.horizontal(|ui| {
        let width = ui.available_width() - 40.0;
        let field = ui.add(egui::TextEdit::singleline(text).password(!*visible).desired_width(width));
        field.widget_info(|| egui::WidgetInfo::labeled(egui::Role::PasswordInput, true, label));
        let (icon, toggle_label) = if *visible { (icons::EYE_SLASH, t("Hide password")) } else { (icons::EYE, t("Show password")) };
        let toggle = glyph_button(ui, icon, &toggle_label, 17.0);
        if toggle.clicked() {
            let had_focus = field.has_focus();
            *visible = !*visible;
            if had_focus {
                field.request_focus();
            }
        }
        (field, toggle)
    })
    .inner
}

/// Button looks used by the account and settings screens.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Look {
    /// The filled pink call to action.
    Filled,
    /// A raised surface with accent text.
    Tinted,
    /// Accent text without a background.
    Plain,
    /// Destructive text without a background.
    Danger,
}

/// A rounded button with a symbol and a label. Symbols use the filled Phosphor face, which egui's
/// default emoji font cannot shadow (it covers part of the private-use area used by outlined glyphs).
pub(crate) fn labeled_button(ui: &mut Ui, icon: &str, label: &str, look: Look) -> egui::Response {
    let button = labeled(ui, icon, label, look);
    ui.add(button)
}

/// The widget behind `labeled_button`, for `ui.add_enabled`.
pub(crate) fn labeled(ui: &Ui, icon: &str, label: &str, look: Look) -> egui::Button<'static> {
    let palette = Palette::of_ui(ui);
    let (color, fill) = match look {
        Look::Filled => (egui::Color32::WHITE, Some(palette.action)),
        Look::Tinted => (palette.accent, Some(palette.surface_raised)),
        Look::Plain => (palette.accent, None),
        Look::Danger => (palette.danger, None),
    };
    let symbol = egui::RichText::new(icon).font(egui::FontId::new(15.0, theme::icon_fill())).color(color);
    let text = theme::text(label, Font::Headline).color(color);
    let button = egui::Button::new((symbol, text)).corner_radius(CornerRadius::same(12)).min_size(vec2(0.0, 34.0));
    match fill {
        Some(fill) => button.fill(fill),
        None => button.frame(false),
    }
}

/// A borderless round icon button (see `widgets::icon_button`) drawn with the filled Phosphor face.
pub(crate) fn glyph_button(ui: &mut Ui, icon: &str, label: &str, size: f32) -> egui::Response {
    let side = (size + 14.0).max(32.0);
    let (rect, response) = ui.allocate_exact_size(egui::Vec2::splat(side), Sense::click());
    let palette = Palette::of_ui(ui);
    if response.hovered() || response.has_focus() {
        ui.painter().circle_filled(rect.center(), side / 2.0, palette.hover);
    }
    let color = if ui.is_enabled() { palette.text } else { palette.secondary.gamma_multiply(0.5) };
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, icon, egui::FontId::new(size, theme::icon_fill()), color);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label));
    response.on_hover_text(label).on_hover_cursor(egui::CursorIcon::PointingHand)
}
