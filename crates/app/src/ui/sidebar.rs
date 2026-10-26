//! The desktop sidebar (the iOS tab bar's sections, plus the library sections) and the content toolbar.

use discophile_core::l10n::{t, tf};
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

pub fn collapsed(app: &App) -> bool {
    app.settings.sidebar_collapsed || app.ctx().content_rect().width() < 900.0
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let collapsed = collapsed(app);
    // The scroll area below narrows its content; collapsed icons use the sidebar's own centre line, like the
    // logo and the bottom controls.
    let center_x = ui.max_rect().center().x;

    let icon = crate::artwork::app_icon(ui.ctx(), 96);
    let logo = egui::Image::new(&icon).fit_to_exact_size(vec2(32.0, 32.0)).corner_radius(CornerRadius::same(9));
    if collapsed {
        ui.vertical_centered(|ui| ui.add(logo));
    } else {
        ui.horizontal(|ui| {
            ui.add(logo);
            ui.label(theme::text("Discophile", Font::Title3));
        });
    }
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

    egui::ScrollArea::vertical().auto_shrink([false, false]).max_height((ui.available_height() - if collapsed { 200.0 } else { 160.0 }).max(40.0)).show(ui, |ui| {
        for entry in &main {
            nav_item(app, ui, entry, &selected, collapsed, center_x);
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
                nav_item(app, ui, entry, &selected, collapsed, center_x);
            }
        }
    });

    // Collapsed, every control sits on the same centre line as the nav icons.
    ui.with_layout(Layout::bottom_up(if collapsed { Align::Center } else { Align::Min }), |ui| {
        if collapsed {
            if widgets::icon_button(ui, icons::GITHUB_LOGO, &t("Discophile on GitHub"), 18.0).clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab(crate::platform::GITHUB));
            }
        } else {
            let github = ui.add(egui::Button::new(theme::secondary(ui, format!("{}  GitHub  {}", icons::GITHUB_LOGO, icons::ARROW_SQUARE_OUT), Font::Callout)).frame(false)).on_hover_text(t("Discophile on GitHub"));
            github.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, t("Discophile on GitHub")));
            if github.clicked() { ui.ctx().open_url(egui::OpenUrl::new_tab(crate::platform::GITHUB)); }
        }
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);
        if collapsed {
            if ui.ctx().content_rect().width() >= 900.0 && widgets::icon_button(ui, icons::SIDEBAR_SIMPLE, &t("Show sidebar"), 16.0).clicked() {
                app.settings.sidebar_collapsed = false;
                app.settings.save();
            }
            if widgets::icon_button(ui, icons::GEAR_SIX, &t("Settings"), 17.0).clicked() {
                app.nav.push(Route::Settings(SettingsPage::General));
            }
        } else {
            ui.horizontal(|ui| {
                if widgets::icon_button(ui, icons::CARET_LEFT, &t("Hide sidebar"), 16.0).clicked() {
                    app.settings.sidebar_collapsed = true;
                    app.settings.save();
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(egui::Button::new(theme::secondary(ui, format!("{}  {}", icons::GEAR_SIX, t("Settings")), Font::Callout)).frame(false)).clicked() {
                        app.nav.push(Route::Settings(SettingsPage::General));
                    }
                });
            });
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

fn nav_item(app: &mut App, ui: &mut Ui, entry: &Entry, selected: &Route, collapsed: bool, center_x: f32) {
    let palette = Palette::of_ui(ui);
    let is_selected = &entry.route == selected && !app.nav.now_playing;
    let height = 38.0;
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    // Collapsed, the highlight is a square on the centre line instead of the full (narrowed) row.
    let highlight = if collapsed { egui::Rect::from_center_size(egui::pos2(center_x, rect.center().y), vec2(height + 2.0, height)) } else { rect };
    if is_selected {
        ui.painter().rect_filled(highlight, CornerRadius::same(9), palette.accent.gamma_multiply(if palette.dark { 0.18 } else { 0.14 }));
    } else if response.hovered() {
        ui.painter().rect_filled(highlight, CornerRadius::same(9), palette.hover);
    }
    let color = if is_selected { palette.accent } else { palette.secondary };
    let icon_x = if collapsed { center_x } else { rect.left() + 18.0 };
    widgets::paint_icon(ui.painter(), egui::pos2(icon_x, rect.center().y), entry.icon, egui::FontId::proportional(18.0), color);
    if !collapsed {
        let font = if is_selected { Font::Headline.id() } else { Font::Body.id() };
        ui.painter().text(egui::pos2(rect.left() + 38.0, rect.center().y), egui::Align2::LEFT_CENTER, &entry.label, font, color);
    }
    widgets::focus_ring(ui, &response.clone().with_new_rect(highlight), 9);
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

const TOOLBAR_HEIGHT: f32 = 52.0;

/// Back/forward on the left; profile, libraries, assistant and updates on the right.
pub fn toolbar(app: &mut App, ui: &mut Ui) {
    // A fixed-height bar with its controls centred, so the space above and below them is equal.
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), TOOLBAR_HEIGHT), Sense::hover());
    let separator = ui.visuals().widgets.noninteractive.bg_stroke;
    ui.painter().hline(rect.x_range(), rect.bottom() - separator.width / 2.0, separator);
    let ui = &mut ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(Layout::left_to_right(Align::Center)));
    // Explicit gaps only, so both ends line up with the page content margins.
    ui.spacing_mut().item_spacing.x = 0.0;
    // The chevron, not its hover circle, sits on the content edge.
    ui.add_space(widgets::PAGE_MARGIN - 12.0);
    ui.add_enabled_ui(app.nav.can_go_back(), |ui| {
        if widgets::icon_button(ui, icons::CARET_LEFT, &t("Back"), 17.0).clicked() {
            app.nav.back();
        }
    });
    ui.add_space(widgets::space::XS);
    ui.add_enabled_ui(app.nav.can_go_forward(), |ui| {
        if widgets::icon_button(ui, icons::CARET_RIGHT, &t("Forward"), 17.0).clicked() {
            app.nav.forward();
        }
    });
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.add_space(widgets::PAGE_MARGIN);
        #[cfg(not(target_arch = "wasm32"))]
        {
            crate::ui::connect::toolbar(app, ui);
            ui.add_space(widgets::space::S);
        }
        library_menu(app, ui);
        ui.add_space(widgets::space::S);
        if app.session.has_library() {
            let unread = assistant::unread_count(app);
            let response = widgets::icon_button_tinted(ui, icons::SPARKLE, &t("Music assistant"), 19.0, Some(Palette::of_ui(ui).accent));
            widgets::count_badge(ui, response.rect, unread);
            if response.clicked() {
                app.nav.toggle_panel(SidePanel::Assistant);
            }
        }
        ui.add_space(widgets::space::S);
        #[cfg(not(target_arch = "wasm32"))]
        crate::ui::software_update::toolbar(app, ui);
    });
}

fn library_menu(app: &mut App, ui: &mut Ui) -> egui::Response {
    let palette = Palette::of_ui(ui);
    let label = app.session.active_account().filter(|_| ui.available_width() > 360.0).map(|account| {
        let name = account.display_name();
        let short: String = name.chars().take(28).collect();
        format!("{}{}", short, if name.chars().count() > 28 { "…" } else { "" })
    });
    let symbol = theme::icon(icons::BOOKS, 17.0).color(palette.secondary);
    let button = match label {
        Some(label) => egui::Button::new((symbol, theme::text(label, Font::Callout).color(palette.text))),
        None => egui::Button::new(symbol),
    };
    let button = button
        .right_text(theme::icon(icons::CARET_DOWN, 11.0).color(palette.secondary))
        .gap(widgets::space::XS).fill(palette.surface).stroke(egui::Stroke::new(1.0, palette.separator))
        .corner_radius(CornerRadius::same(10)).min_size(vec2(0.0, widgets::CONTROL));
    let response = ui.scope(|ui| {
        ui.spacing_mut().button_padding.x = widgets::space::S;
        ui.add(button)
    }).inner;
    if response.clicked() {
        response.request_focus();
    }
    let popup_id = egui::Popup::default_response_id(&response);
    let was_open = egui::Popup::is_id_open(ui.ctx(), popup_id);
    egui::Popup::menu(&response).accessible_name(t("Switch library"))
        .align(egui::emath::RectAlign::BOTTOM_END).gap(8.0).width(widgets::ACCOUNT_MENU_WIDTH)
        .frame(widgets::account_menu_frame(ui)).show(|ui| {
        widgets::account_menu_layout(ui);
        egui::Frame::new().inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
            ui.label(theme::text(t("Switch library"), Font::Headline).font(egui::FontId::new(15.0, theme::semibold())));
        });
        if !app.session.accounts.is_empty() {
            let active = app.session.account_key();
            let accounts: Vec<(String, String, String)> =
                app.session.accounts.iter().map(|a| (a.id(), a.display_name(), a.credentials.username.clone())).collect();
            let max_height = (ui.ctx().content_rect().height() - 200.0).clamp(widgets::CONTROL_LARGE, 320.0);
            egui::ScrollArea::vertical().max_height(max_height).auto_shrink([false, true]).show(ui, |ui| {
                for (id, name, user) in accounts {
                    let current = id == active;
                    let response = widgets::account_menu_choice(ui, icons::BOOKS, &name, current)
                        .on_hover_text(format!("{name} · {user}"));
                    if response.has_focus() { response.scroll_to_me(Some(Align::Center)); }
                    if response.clicked() {
                        session::switch_account(app, &id);
                        ui.close();
                    }
                }
            });
            ui.add_space(widgets::space::S);
            ui.separator();
            ui.add_space(widgets::space::S);
        }
        if widgets::account_menu_action(ui, icons::PLUS_CIRCLE, &t("Add account"), None, None).clicked() {
            app.sheets.add_account = true;
            ui.close();
        }
        if widgets::account_menu_action(ui, icons::GEAR_SIX, &t("Settings"), None, None).clicked() {
            app.nav.push(Route::Settings(SettingsPage::General));
            ui.close();
        }
    });
    let accessible_name = app.session.active_account().map(|account| tf("Switch library: %@", &[&account.display_name()])).unwrap_or_else(|| t("Switch library"));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), &accessible_name));
    let open = egui::Popup::is_id_open(ui.ctx(), popup_id);
    if was_open && !open && ui.input(|i| i.key_pressed(egui::Key::Escape)) { response.request_focus(); }
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_has_popup(egui::accesskit::HasPopup::Menu);
        node.set_expanded(open);
    });
    widgets::focus_ring(ui, &response, 10);
    response.on_hover_text(t("Switch library"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use discophile_core::{Credentials, MusicProvider, ServerAccount};

    fn fixture() -> App {
        let mut app = App::connect_test_app();
        let credentials = Credentials::simple("https://music.example.invalid", "fixture", "fixture", false, MusicProvider::Subsonic).unwrap();
        let account = ServerAccount::new(credentials, "Una libreria con un nome molto lungo, accenti è e caratteri 日本語");
        app.session.active = Some(account.id());
        app.session.accounts.push(account);
        app
    }

    fn key(key: egui::Key) -> egui::Event {
        egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }
    }

    #[test]
    fn library_menu_exposes_full_names_selection_and_popup_state_at_large_text_sizes() {
        for scale in [1.0, 1.5] {
            let mut app = fixture();
            let ctx = app.ctx().clone();
            ctx.enable_accesskit();
            theme::install_fonts(&ctx);
            theme::apply(&ctx, true);
            ctx.set_zoom_factor(scale);
            let name = app.session.accounts[0].display_name();
            let mut tree = None;
            for frame in 0..3 {
                let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(900.0, 600.0))), ..Default::default() };
                let mut output = ctx.run_ui(input, |ui| {
                    let response = library_menu(&mut app, ui);
                    if frame == 0 { egui::Popup::open_id(&ctx, egui::Popup::default_response_id(&response)); }
                });
                output.textures_delta.clear();
                tree = output.platform_output.accesskit_update;
            }
            let tree = tree.unwrap();
            let choice = tree.nodes.iter().map(|(_, node)| node).find(|node| node.role() == egui::Role::MenuItemRadio).unwrap();
            assert_eq!(choice.label(), Some(name.as_str()), "assistive technology must receive the unabridged library name");
            assert_eq!(choice.toggled(), Some(egui::accesskit::Toggled::True));
            let bounds = choice.bounds().unwrap();
            assert!(bounds.height() >= 44.0 && bounds.x0 >= 0.0 && bounds.x1 <= 900.0);
            assert!(bounds.width() <= widgets::ACCOUNT_MENU_WIDTH as f64 * scale as f64, "long labels must not widen the menu");
            assert!(tree.nodes.iter().any(|(_, node)| node.role() == egui::Role::Menu && node.label() == Some(t("Switch library").as_str())));
            assert!(tree.nodes.iter().any(|(_, node)| node.has_popup() == Some(egui::accesskit::HasPopup::Menu) && node.is_expanded() == Some(true) && node.label().is_some_and(|label| label.contains(&name))));
        }
    }

    #[test]
    fn library_menu_supports_keyboard_opening_and_escape_restores_trigger_focus() {
        let mut app = fixture();
        let ctx = app.ctx().clone();
        theme::install_fonts(&ctx);
        let mut trigger = None;
        let mut popup = None;
        for frame in 0..5 {
            let events = match frame { 1 => vec![key(egui::Key::Enter)], 2 => vec![key(egui::Key::ArrowDown)], 4 => vec![key(egui::Key::Escape)], _ => Vec::new() };
            let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                let response = library_menu(&mut app, ui);
                trigger = Some(response.id);
                popup = Some(egui::Popup::default_response_id(&response));
                if frame == 0 { response.request_focus(); }
            });
            output.textures_delta.clear();
            if frame == 3 {
                assert!(egui::Popup::is_id_open(&ctx, popup.unwrap()));
                assert_ne!(ctx.memory(|memory| memory.focused()), trigger);
                assert!(widgets::keyboard_navigation(&ctx));
            }
        }
        assert!(!egui::Popup::is_id_open(&ctx, popup.unwrap()));
        assert_eq!(ctx.memory(|memory| memory.focused()), trigger);
    }
}
