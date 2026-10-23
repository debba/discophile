//! The desktop sidebar (the iOS tab bar's sections, plus the library sections) and the content toolbar.

use discophile_core::l10n::t;
use egui::{Align, CornerRadius, Layout, Sense, Ui, vec2};

use crate::app::App;
use crate::features::{assistant, audiobooks};
use crate::nav::{Route, SettingsPage, SidePanel};
use crate::session;
use crate::theme::{self, Font, Palette, icons};
use crate::ui::widgets;

struct Entry {
    route: Route,
    icon: &'static str,
    label: String,
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let collapsed = app.settings.sidebar_collapsed;

    ui.horizontal(|ui| {
        let icon = crate::artwork::app_icon(ui.ctx(), 96);
        ui.add(egui::Image::new(&icon).fit_to_exact_size(vec2(32.0, 32.0)).corner_radius(CornerRadius::same(9)));
        if !collapsed {
            ui.label(theme::text("Discophile", Font::Title3));
        }
    });
    ui.add_space(22.0);

    if app.session.has_library() && !collapsed {
        let mut text = app.nav.search_text.clone();
        let response = search_box(ui, &mut text);
        if response.changed() || (response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
            app.nav.search_text = text;
            if app.nav.current != Route::Search {
                app.nav.select(Route::Search);
            }
        }
        ui.add_space(8.0);
    }

    let selected = app.nav.section();
    let mut main = Vec::new();
    if app.session.has_library() {
        main.push(Entry { route: Route::ListenNow, icon: icons::PLAY_CIRCLE, label: t("Listen Now") });
        if collapsed {
            main.push(Entry { route: Route::Search, icon: icons::MAGNIFYING_GLASS, label: t("Search") });
        }
    }
    if audiobooks::has_account(app) {
        main.push(Entry { route: Route::Audiobooks, icon: icons::BOOKS, label: t("Audiobooks") });
    }
    main.push(Entry { route: Route::Radio, icon: icons::RADIO, label: t("Radio") });

    egui::ScrollArea::vertical().auto_shrink([false, false]).max_height(ui.available_height() - 96.0).show(ui, |ui| {
        for entry in &main {
            nav_item(app, ui, entry, &selected, collapsed);
        }
        if app.session.has_library() {
            ui.add_space(22.0);
            if !collapsed {
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    ui.label(theme::secondary(ui, t("Library"), Font::Footnote).strong());
                });
                ui.add_space(6.0);
            }
            let mut library = vec![
                Entry { route: Route::Albums, icon: icons::SQUARES_FOUR, label: t("Albums") },
                Entry { route: Route::Artists, icon: icons::MICROPHONE_STAGE, label: t("Artists") },
                Entry { route: Route::Songs, icon: icons::MUSIC_NOTES, label: t("Songs") },
                Entry { route: Route::Playlists, icon: icons::PLAYLIST, label: t("Playlists") },
                Entry { route: Route::Favorites, icon: icons::HEART, label: t("Favorites") },
            ];
            if app.session.provider().is_some_and(|p| p.supports_podcasts()) {
                library.push(Entry { route: Route::Podcasts, icon: icons::MICROPHONE, label: t("Podcasts") });
            }
            for entry in &library {
                nav_item(app, ui, entry, &selected, collapsed);
            }
        }
    });

    ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
        ui.horizontal(|ui| {
            let collapse_icon = if collapsed { icons::SIDEBAR_SIMPLE } else { icons::CARET_LEFT };
            let label = if collapsed { t("Show sidebar") } else { t("Hide sidebar") };
            if widgets::icon_button(ui, collapse_icon, &label, 16.0).clicked() {
                app.settings.sidebar_collapsed = !collapsed;
                app.settings.save();
            }
            if !collapsed {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(egui::Button::new(theme::secondary(ui, format!("{}  {}", icons::GEAR_SIX, t("Settings")), Font::Callout)).frame(false)).clicked() {
                        app.nav.push(Route::Settings(SettingsPage::General));
                    }
                });
            }
        });
        if collapsed
            && widgets::icon_button(ui, icons::GEAR_SIX, &t("Settings"), 17.0).clicked() {
                app.nav.push(Route::Settings(SettingsPage::General));
            }
        if app.session.has_library() {
            downloaded_only_toggle(app, ui, collapsed);
        }
        ui.add_space(10.0);
        ui.separator();
        ui.add_space(10.0);
    });
}

fn search_box(ui: &mut Ui, text: &mut String) -> egui::Response {
    let width = ui.available_width();
    widgets::search_field_with_id(ui, Some(egui::Id::unique("search-field")), text, &t("Artists, albums, songs"), width)
}

fn nav_item(app: &mut App, ui: &mut Ui, entry: &Entry, selected: &Route, collapsed: bool) {
    let palette = Palette::of_ui(ui);
    let is_selected = &entry.route == selected && !app.nav.now_playing;
    let height = 38.0;
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    if is_selected {
        ui.painter().rect_filled(rect, CornerRadius::same(9), palette.accent.gamma_multiply(if palette.dark { 0.18 } else { 0.14 }));
        if !collapsed {
            let indicator = egui::Rect::from_center_size(egui::pos2(rect.left() + 2.0, rect.center().y), vec2(3.0, 16.0));
            ui.painter().rect_filled(indicator, CornerRadius::same(2), palette.accent);
        }
    } else if response.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(9), palette.hover);
    }
    let color = if is_selected { palette.accent } else { palette.secondary };
    let icon_x = if collapsed { rect.center().x } else { rect.left() + 18.0 };
    widgets::paint_icon(ui.painter(), egui::pos2(icon_x, rect.center().y), entry.icon, egui::FontId::proportional(18.0), color);
    if !collapsed {
        let font = if is_selected { Font::Headline.id() } else { Font::Body.id() };
        ui.painter().text(egui::pos2(rect.left() + 38.0, rect.center().y), egui::Align2::LEFT_CENTER, &entry.label, font, color);
    }
    widgets::focus_ring(ui, &response, 9);
    let response = if collapsed { response.on_hover_text(&entry.label) } else { response };
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Tab, true, is_selected, &entry.label));
    if response.clicked() {
        app.nav.select(entry.route.clone());
    }
}

/// The "Downloaded only" filter, available on desktop where downloads exist.
fn downloaded_only_toggle(app: &mut App, ui: &mut Ui, collapsed: bool) {
    if crate::platform::IS_WEB {
        return;
    }
    let palette = Palette::of_ui(ui);
    let on = app.settings.downloaded_only;
    let icon = if on { icons::ARROW_CIRCLE_DOWN } else { icons::FUNNEL_SIMPLE };
    let label = if on { t("Downloaded only") } else { t("All content") };
    let response = if collapsed {
        widgets::icon_button_tinted(ui, icon, &label, 17.0, Some(palette.accent))
    } else {
        let text = theme::text(format!("{icon}  {label}"), Font::Callout).color(palette.accent);
        ui.add(
            egui::Button::new(text)
                .fill(palette.surface)
                .stroke(egui::Stroke::new(1.0, if on { palette.accent } else { palette.secondary.gamma_multiply(0.3) }))
                .corner_radius(CornerRadius::same(16))
                .min_size(vec2(ui.available_width(), 32.0)),
        )
        .on_hover_text(t("Filter this library to content saved on this computer"))
    };
    if response.clicked() {
        app.settings.downloaded_only = !on;
        app.settings.save();
        crate::features::library::on_filter_changed(app);
    }
}

/// Back/forward on the left; assistant and the library menu on the right.
pub fn toolbar(app: &mut App, ui: &mut Ui) {
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.add_space(widgets::PAGE_MARGIN - 8.0);
        ui.add_enabled_ui(app.nav.can_go_back(), |ui| {
            if widgets::icon_button(ui, icons::CARET_LEFT, &t("Back"), 17.0).clicked() {
                app.nav.back();
            }
        });
        ui.add_enabled_ui(app.nav.can_go_forward(), |ui| {
            if widgets::icon_button(ui, icons::CARET_RIGHT, &t("Forward"), 17.0).clicked() {
                app.nav.forward();
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(widgets::PAGE_MARGIN - 8.0);
            account_menu(app, ui);
            if app.session.has_library() {
                let unread = assistant::unread_count(app);
                let response = widgets::icon_button_tinted(ui, icons::SPARKLE, &t("Music assistant"), 19.0, Some(Palette::of_ui(ui).accent));
                widgets::count_badge(ui, response.rect, unread);
                if response.clicked() {
                    app.nav.toggle_panel(SidePanel::Assistant);
                }
            }
        });
    });
    ui.add_space(8.0);
    ui.separator();
}

fn account_menu(app: &mut App, ui: &mut Ui) {
    let palette = Palette::of_ui(ui);
    let label = app.session.active_account().filter(|_| ui.available_width() > 360.0).map(|account| {
        let name = account.display_name();
        let short: String = name.chars().take(28).collect();
        format!("{}  {}{}  {}", icons::USER_CIRCLE, short, if name.chars().count() > 28 { "…" } else { "" }, icons::CARET_DOWN)
    });
    let text = match label {
        Some(label) => theme::text(label, Font::Callout).color(palette.text),
        None => theme::icon(icons::USER_CIRCLE, 22.0).color(palette.accent),
    };
    let button = egui::Button::new(text).fill(palette.surface_raised).corner_radius(CornerRadius::same(8));
    egui::containers::menu::MenuButton::from_button(button).ui(ui, |ui| {
        ui.set_min_width(240.0);
        if !app.session.accounts.is_empty() {
            ui.label(theme::secondary(ui, t("Switch library"), Font::Footnote));
            let active = app.session.account_key();
            let accounts: Vec<(String, String, String)> =
                app.session.accounts.iter().map(|a| (a.id(), a.display_name(), a.credentials.username.clone())).collect();
            for (id, name, user) in accounts {
                let label = if id == active { format!("{}  {name}", icons::CHECK) } else { format!("      {name} · {user}") };
                if ui.button(label).clicked() {
                    session::switch_account(app, &id);
                    ui.close();
                }
            }
            ui.separator();
        }
        if ui.button(format!("{}  {}", icons::PLUS_CIRCLE, t("Add account"))).clicked() {
            app.sheets.add_account = true;
            ui.close();
        }
        if ui.button(format!("{}  {}", icons::GEAR_SIX, t("Settings"))).clicked() {
            app.nav.push(Route::Settings(SettingsPage::General));
            ui.close();
        }
    });
}
