//! The Connect identity, kept separate from the music library selector.

use discophile_core::l10n::{t, tf};
use egui::{Align2, Frame, Margin, Rect, Response, Sense, Stroke, Ui, vec2};

use crate::app::App;
use crate::features::connect as feature;
use crate::nav::{Route, SettingsPage};
use crate::theme::{self, Font, Palette, icons};
use crate::ui::widgets;

pub fn toolbar(app: &mut App, ui: &mut Ui) {
    let palette = Palette::of_ui(ui);
    if !feature::is_signed_in(app) {
        let pending = app.connect.sign_in.is_some();
        let label = t(if pending { "Signing in…" } else { "Sign in" });
        let symbol = theme::icon(
            if pending {
                icons::CIRCLE_NOTCH
            } else {
                icons::USER_CIRCLE
            },
            17.0,
        )
        .color(palette.secondary);
        let button = egui::Button::new((symbol, theme::text(label, Font::Callout)))
            .gap(widgets::space::S)
            .fill(palette.surface)
            .stroke(Stroke::new(1.0, palette.separator))
            .corner_radius(10)
            .min_size(vec2(0.0, widgets::CONTROL));
        let ready = !app.connect.checking_instance && (!app.connect.busy || pending);
        let response = ui.add_enabled(ready, button);
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::Role::Button,
                ready,
                t("Sign in to Discophile Connect"),
            )
        });
        if response
            .on_hover_text(t("Sign in to Discophile Connect"))
            .clicked()
        {
            open_settings(app);
            if !pending {
                feature::sign_in(app);
            }
        }
        return;
    }

    let account = app.connect.account.clone();
    let name = account
        .as_ref()
        .map(|a| a.account.name.trim())
        .filter(|name| !name.is_empty());
    let email = account.as_ref().and_then(|a| a.account.email.as_deref());
    let display_name = name
        .or(email)
        .map(str::to_string)
        .unwrap_or_else(|| t("Your account"));
    let initials = name.or(email).map(initials).unwrap_or_default();
    let texture = app.connect.profile.texture.clone();
    let label = tf("Profile: %@", &[&display_name]);
    let response = profile_button(ui, texture.as_ref(), &initials, &label);
    egui::Popup::menu(&response)
        .align(egui::emath::RectAlign::BOTTOM_END)
        .gap(8.0)
        .width(312.0)
        .frame(widgets::account_menu_frame(ui))
        .show(|ui| {
            ui.set_width(296.0);
            ui.spacing_mut().item_spacing = vec2(8.0, 2.0);
            ui.spacing_mut().button_padding = vec2(12.0, 8.0);
            Frame::new().inner_margin(Margin::same(12)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = widgets::space::M;
                    let (rect, _) = ui.allocate_exact_size(vec2(44.0, 44.0), Sense::hover());
                    paint_picture(ui, rect, texture.as_ref(), &initials);
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width());
                        ui.spacing_mut().item_spacing.y = 3.0;
                        ui.add(
                            egui::Label::new(theme::text(&display_name, Font::Headline)).truncate(),
                        )
                        .on_hover_text(&display_name);
                        if let Some(email) = email.filter(|email| *email != display_name) {
                            ui.add(
                                egui::Label::new(theme::secondary(ui, email, Font::Footnote))
                                    .truncate(),
                            )
                            .on_hover_text(email);
                        }
                        if let Some(account) = &account {
                            let plan =
                                super::desktop::tier_label(&account.plan.tier, &account.plan.name);
                            plan_badge(ui, &plan);
                        } else {
                            ui.add(
                                egui::Label::new(theme::secondary(
                                    ui,
                                    t(if app.connect.busy {
                                        "Loading your account…"
                                    } else {
                                        "Account details unavailable"
                                    }),
                                    Font::Footnote,
                                ))
                                .truncate(),
                            );
                        }
                    });
                });
            });
            divider(ui);
            if widgets::account_menu_action(
                ui,
                icons::USER_CIRCLE,
                &t("Manage profile"),
                Some(icons::ARROW_UP_RIGHT),
                None,
            )
            .clicked()
            {
                let dashboard = app
                    .connect
                    .server_info
                    .as_ref()
                    .and_then(|info| info.dashboard.clone())
                    .unwrap_or_else(|| app.connect.origin.url("/account"));
                ui.ctx()
                    .open_url(egui::OpenUrl::new_tab(dashboard.as_str()));
                ui.close();
            }
            if widgets::account_menu_action(ui, icons::GEAR_SIX, &t("Connect settings"), None, None)
                .clicked()
            {
                open_settings(app);
                ui.close();
            }
            let refresh = ui
                .add_enabled_ui(!app.connect.busy, |ui| {
                    widgets::account_menu_action(
                        ui,
                        icons::ARROWS_CLOCKWISE,
                        &t(if app.connect.busy {
                            "Refreshing account…"
                        } else {
                            "Refresh account"
                        }),
                        None,
                        None,
                    )
                })
                .inner;
            if refresh.clicked() {
                feature::refresh_account(app);
                ui.close();
            }
            if let Some(error) = &app.connect.error {
                Frame::new()
                    .inner_margin(Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(
                                theme::text(
                                    format!("{}  {error}", icons::WARNING_CIRCLE),
                                    Font::Footnote,
                                )
                                .color(palette.danger),
                            )
                            .wrap(),
                        );
                    });
            }
            divider(ui);
            if widgets::account_menu_action(
                ui,
                icons::SIGN_OUT,
                &t("Sign out"),
                None,
                Some(palette.danger),
            )
            .clicked()
            {
                feature::sign_out(app);
                ui.close();
            }
            ui.add_space(widgets::space::S);
            Frame::new()
                .inner_margin(Margin::symmetric(12, 4))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = widgets::space::S;
                        ui.label(theme::icon(icons::GLOBE, 13.0).color(palette.secondary));
                        let origin = app.connect.origin.as_str();
                        let domain = origin
                            .trim_start_matches("https://")
                            .trim_start_matches("http://");
                        ui.add(
                            egui::Label::new(theme::secondary(ui, domain, Font::Caption))
                                .truncate(),
                        )
                        .on_hover_text(origin);
                    });
                });
        });
}

fn open_settings(app: &mut App) {
    app.nav.now_playing = false;
    app.nav.push(Route::Settings(SettingsPage::Connect));
}

fn initials(name: &str) -> String {
    let mut words = name.split_whitespace();
    let first = words.next().and_then(|word| word.chars().next());
    let last = words.next_back().and_then(|word| word.chars().next());
    first
        .into_iter()
        .chain(last)
        .flat_map(char::to_uppercase)
        .take(2)
        .collect()
}

fn profile_button(
    ui: &mut Ui,
    texture: Option<&egui::TextureHandle>,
    initials: &str,
    label: &str,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(60.0, widgets::CONTROL), Sense::click());
    if response.clicked() {
        response.request_focus();
    }
    let palette = Palette::of_ui(ui);
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
    let fill = if open || response.hovered() || response.is_pointer_button_down_on() {
        palette.surface_raised
    } else {
        palette.surface
    };
    ui.painter().rect_filled(rect, 10, fill);
    ui.painter().rect_stroke(
        rect,
        10,
        Stroke::new(1.0, palette.separator),
        egui::StrokeKind::Inside,
    );
    let image = Rect::from_min_size(rect.min + vec2(4.0, 4.0), vec2(28.0, 28.0));
    paint_picture(ui, image, texture, initials);
    widgets::paint_icon(
        ui.painter(),
        rect.right_center() - vec2(13.0, 0.0),
        icons::CARET_DOWN,
        egui::FontId::proportional(11.0),
        palette.secondary,
    );
    widgets::focus_ring(ui, &response, 10);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label));
    response
        .on_hover_text(label)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn paint_picture(ui: &Ui, rect: Rect, texture: Option<&egui::TextureHandle>, initials: &str) {
    let palette = Palette::of_ui(ui);
    let radius = rect.width() / 2.0;
    ui.painter()
        .circle_filled(rect.center(), radius, palette.surface_raised);
    if let Some(texture) = texture {
        egui::Image::new(texture)
            .corner_radius(radius as u8)
            .paint_at(ui, rect);
    } else if initials.is_empty() {
        widgets::paint_icon(
            ui.painter(),
            rect.center(),
            icons::USER_CIRCLE,
            egui::FontId::proportional(rect.width() * 0.65),
            palette.accent,
        );
    } else {
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            initials,
            egui::FontId::new(rect.width() * 0.37, theme::semibold()),
            palette.accent,
        );
    }
    ui.painter()
        .circle_stroke(rect.center(), radius, Stroke::new(0.5, palette.separator));
}

fn plan_badge(ui: &mut Ui, plan: &str) {
    let palette = Palette::of_ui(ui);
    let short: String = plan.chars().take(28).collect();
    Frame::new()
        .fill(palette.accent.gamma_multiply(0.10))
        .corner_radius(5)
        .inner_margin(Margin::symmetric(7, 2))
        .show(ui, |ui| {
            ui.label(
                theme::text(short, Font::Caption)
                    .font(egui::FontId::new(11.0, theme::semibold()))
                    .color(palette.accent),
            )
            .on_hover_text(plan);
        });
}

fn divider(ui: &mut Ui) {
    ui.add_space(widgets::space::S);
    ui.separator();
    ui.add_space(widgets::space::S);
}
