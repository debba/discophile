//! Settings: a list of pages on the left and the selected page on the right, inside the content area.
//! Feature pages (sound, lyrics, offline listening, rediscover, assistant) are drawn by their modules.

mod libraries;

use discophile_core::StreamingQuality;
use discophile_core::l10n::{self, Language, t};
use egui::{Align, Color32, CornerRadius, Layout, Rect, Sense, Ui, UiBuilder, pos2, vec2};

use crate::app::App;
use crate::nav::{Route, SettingsPage};
use crate::theme::{self, Appearance, Font, Palette, icons};
use crate::ui::accounts::{Look, labeled_button};
use crate::ui::widgets;

const DEVELOPER_URL: &str = "https://github.com/debba";
const SPONSORS_URL: &str = "https://github.com/sponsors/debba";
/// Activations of Version in About that unlock Advanced settings, as on iOS.
const VERSION_TAPS: u32 = 6;

#[derive(Clone, Default)]
struct AboutState {
    version_taps: u32,
    /// After Version is activated once, a labeled Advanced settings button offers the same destination
    /// to keyboard and screen reader users (the iOS accessibility action).
    offer_advanced: bool,
}

fn about_id() -> egui::Id {
    egui::Id::unique("settings-about")
}

fn pages(app: &App) -> Vec<(SettingsPage, &'static str, &'static str)> {
    let mut pages = vec![
        (SettingsPage::General, icons::GEAR_SIX, "General"),
        (SettingsPage::Accounts, icons::USERS, "Libraries and accounts"),
        (SettingsPage::Playback, icons::PLAY_CIRCLE, "Playback"),
        (SettingsPage::Sound, icons::SLIDERS_HORIZONTAL, "Equalizer and sound"),
        (SettingsPage::Lyrics, icons::TEXT_AA, "Lyrics"),
        (SettingsPage::Offline, icons::ARROW_CIRCLE_DOWN, "Offline listening"),
        (SettingsPage::Rediscover, icons::CLOCK_COUNTER_CLOCKWISE, "Rediscover your music"),
        (SettingsPage::Assistant, icons::SPARKLE, "Music assistant"),
    ];
    if app.settings.advanced_unlocked {
        pages.push((SettingsPage::Advanced, icons::WRENCH, "Advanced settings"));
    }
    pages.push((SettingsPage::About, icons::INFO, "About"));
    pages
}

pub fn show(app: &mut App, ui: &mut Ui, page: crate::nav::SettingsPage) {
    let page = if page == SettingsPage::Advanced && !app.settings.advanced_unlocked { SettingsPage::About } else { page };
    let palette = Palette::of_ui(ui);
    let rect = ui.available_rect_before_wrap();
    let list_width = if rect.width() < 760.0 { 190.0 } else { 236.0 };
    let list_rect = Rect::from_min_max(rect.min + vec2(widgets::PAGE_MARGIN - 10.0, 12.0), pos2(rect.min.x + list_width, rect.max.y - 8.0));
    let body_rect = Rect::from_min_max(pos2(rect.min.x + list_width + 8.0, rect.min.y), rect.max);

    let mut list = ui.new_child(UiBuilder::new().id_salt("settings-pages").max_rect(list_rect).layout(Layout::top_down(Align::Min)));
    page_list(app, &mut list, page);
    ui.painter().vline(list_rect.right() + 8.0, (rect.top() + 12.0)..=(rect.bottom() - 12.0), egui::Stroke::new(1.0, palette.separator));

    let mut body = ui.new_child(UiBuilder::new().id_salt("settings-body").max_rect(body_rect).layout(Layout::top_down(Align::Min)));
    widgets::page(&mut body, ("settings", page), |ui| {
        ui.set_max_width(ui.available_width().min(780.0));
        let title = pages(app).into_iter().find(|(p, _, _)| *p == page).map(|(_, _, title)| t(title)).unwrap_or_default();
        widgets::page_title(ui, &title);
        match page {
            SettingsPage::General => general(app, ui),
            SettingsPage::Accounts => libraries::show(app, ui),
            SettingsPage::Playback => playback(app, ui),
            SettingsPage::Sound => crate::ui::sound::show(app, ui),
            SettingsPage::Lyrics => crate::ui::lyrics::settings(app, ui),
            SettingsPage::Offline => crate::features::downloads::settings_ui(app, ui),
            SettingsPage::Rediscover => crate::ui::statistics::rediscover_settings(app, ui),
            SettingsPage::Assistant => crate::ui::assistant::settings(app, ui),
            SettingsPage::Advanced => advanced(app, ui),
            SettingsPage::About => about(app, ui),
        }
    });
    ui.advance_cursor_after_rect(rect);
}

fn page_list(app: &mut App, ui: &mut Ui, current: SettingsPage) {
    let palette = Palette::of_ui(ui);
    ui.label(theme::text(t("Settings"), Font::Title2));
    ui.add_space(10.0);
    egui::ScrollArea::vertical().id_salt("settings-page-list").auto_shrink([false, false]).show(ui, |ui| {
        for (page, icon, label) in pages(app) {
            let label = t(label);
            let selected = page == current;
            let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::click());
            if selected {
                ui.painter().rect_filled(
                    rect,
                    CornerRadius::same(9),
                    palette.accent.gamma_multiply(if palette.dark { 0.18 } else { 0.14 }),
                );
            } else if response.hovered() || response.has_focus() {
                ui.painter().rect_filled(rect, CornerRadius::same(9), palette.hover);
            }
            let color = if selected { palette.accent } else { palette.text };
            crate::ui::widgets::paint_icon(ui.painter(), rect.left_center() + vec2(16.0, 0.0), icon, egui::FontId::new(17.0, theme::icon_fill()), color);
            let font = if selected { Font::Headline.id() } else { Font::Body.id() };
            let galley = ui.painter().layout(label.clone(), font, color, rect.width() - 40.0);
            ui.painter().galley(pos2(rect.left() + 34.0, rect.center().y - galley.size().y / 2.0), galley, color);
            response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Tab, true, selected, &label));
            if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && !selected {
                app.nav.push(Route::Settings(page));
            }
        }
    });
}

// MARK: - Shared rows

/// A card of settings rows, like an iOS grouped section.
pub(crate) fn group<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    widgets::card(ui, add).inner
}

/// Secondary explanatory text under a group, like an iOS section footer.
pub(crate) fn footer(ui: &mut Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(theme::secondary(ui, text, Font::Footnote));
    ui.add_space(4.0);
}

/// A labeled switch with an optional description. Returns true when the value changed.
pub(crate) fn toggle_row(ui: &mut Ui, value: &mut bool, label: &str, detail: Option<&str>) -> bool {
    let palette = Palette::of_ui(ui);
    let enabled = ui.is_enabled();
    let inner = ui.horizontal(|ui| {
        let switch_width = 44.0;
        let text_width = (ui.available_width() - switch_width - 16.0).max(80.0);
        ui.vertical(|ui| {
            ui.set_width(text_width);
            ui.label(theme::text(label, Font::Body));
            if let Some(detail) = detail {
                ui.label(theme::secondary(ui, detail, Font::Footnote));
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let (rect, response) = ui.allocate_exact_size(vec2(switch_width, 26.0), Sense::click());
            let progress = if *value { 1.0 } else { 0.0 };
            let track = if *value { palette.action } else { palette.secondary.gamma_multiply(0.35) };
            let track = if enabled { track } else { track.gamma_multiply(0.5) };
            ui.painter().rect_filled(rect, CornerRadius::same(13), track);
            let knob_x = egui::lerp((rect.left() + 13.0)..=(rect.right() - 13.0), progress);
            ui.painter().circle_filled(pos2(knob_x, rect.center().y), 10.5, Color32::WHITE);
            if response.has_focus() {
                ui.painter().rect_stroke(
                    rect.expand(2.0),
                    CornerRadius::same(15),
                    egui::Stroke::new(2.0, palette.accent),
                    egui::StrokeKind::Outside,
                );
            }
            response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Switch, enabled, *value, label));
            response
        })
        .inner
    });
    let response = inner.inner;
    if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        *value = !*value;
        return true;
    }
    false
}

/// One option of a single-choice list with an explanation and a check mark when selected.
pub(crate) fn choice_row(ui: &mut Ui, selected: bool, label: &str, detail: Option<&str>) -> bool {
    let palette = Palette::of_ui(ui);
    let id = ui.next_auto_id();
    let hovered = ui.ctx().read_response(id).is_some_and(|r| r.hovered() || r.has_focus());
    let inner = egui::Frame::new()
        .fill(if hovered { palette.hover } else { Color32::TRANSPARENT })
        .corner_radius(CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_width((ui.available_width() - 30.0).max(60.0));
                    ui.label(theme::text(label, Font::Body).color(if selected { palette.accent } else { palette.text }));
                    if let Some(detail) = detail {
                        ui.label(theme::secondary(ui, detail, Font::Footnote));
                    }
                });
                if selected {
                    ui.label(theme::icon_filled(icons::CHECK_CIRCLE, 19.0).color(palette.accent));
                }
            });
        });
    let response = ui.interact(inner.response.rect, id, Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, true, selected, label));
    response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// A row of chips for a short single choice (language, appearance). Returns the chosen index.
fn segmented(ui: &mut Ui, options: &[String], selected: usize) -> Option<usize> {
    let mut chosen = None;
    ui.horizontal_wrapped(|ui| {
        for (index, option) in options.iter().enumerate() {
            let response = widgets::chip(ui, index == selected, option);
            response.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, true, index == selected, option));
            if response.clicked() && index != selected {
                chosen = Some(index);
            }
        }
    });
    chosen
}

/// A row with trailing controls on the right and wrapping text filling the remaining width.
pub(crate) fn trailing_row<R>(ui: &mut Ui, trailing: impl FnOnce(&mut Ui) -> R, text: impl FnOnce(&mut Ui)) -> R {
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        let result = trailing(ui);
        ui.add_space(12.0);
        ui.with_layout(Layout::top_down(Align::Min), |ui| {
            ui.set_width(ui.available_width());
            text(ui);
        });
        result
    })
    .inner
}

/// A label and a value on one line.
pub(crate) fn value_row(ui: &mut Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(theme::text(label, Font::Body));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add(egui::Label::new(theme::secondary(ui, value, Font::Body)).truncate());
        });
    });
}

fn link_button(ui: &mut Ui, icon: &str, label: &str, url: &str, hint: Option<&str>) {
    let response = labeled_button(ui, icon, label, Look::Plain);
    let response = response.on_hover_text(hint.map(str::to_string).unwrap_or_else(|| url.to_string()));
    if response.clicked() {
        ui.ctx().open_url(egui::OpenUrl::new_tab(url));
    }
}

// MARK: - General

fn general(app: &mut App, ui: &mut Ui) {
    widgets::section_heading(ui, &t("Language"), None);
    let languages = [None, Some(Language::English), Some(Language::Italian)];
    let labels: Vec<String> = languages.iter().map(|l| l.map(|l| l.native_name().to_string()).unwrap_or_else(|| t("System"))).collect();
    let current = languages.iter().position(|l| *l == app.settings.language).unwrap_or(0);
    if let Some(index) = segmented(ui, &labels, current) {
        app.settings.language = languages[index];
        app.settings.save();
        l10n::set_language(app.settings.effective_language());
    }
    footer(ui, &t("System follows the language of this computer. Changes apply immediately."));

    widgets::section_heading(ui, &t("Appearance"), None);
    let appearances = [Appearance::System, Appearance::Light, Appearance::Dark];
    let labels = [t("System"), t("Light"), t("Dark")];
    let current = appearances.iter().position(|a| *a == app.settings.appearance).unwrap_or(0);
    if let Some(index) = segmented(ui, &labels, current) {
        app.settings.appearance = appearances[index];
        app.settings.save();
    }

    widgets::section_heading(ui, &t("Text size"), None);
    let scales = crate::settings::Settings::SCALES;
    let labels: Vec<String> = scales.iter().map(|s| format!("{}%", (s * 100.0).round() as i32)).collect();
    let current = scales.iter().position(|s| (s - app.settings.interface_scale).abs() < 0.01).unwrap_or(usize::MAX);
    if let Some(index) = segmented(ui, &labels, current) {
        app.settings.interface_scale = scales[index];
        app.settings.save();
        ui.ctx().set_zoom_factor(scales[index]);
    }
    footer(ui, &t("Scales text and controls. You can also press Ctrl + and Ctrl − (Cmd on macOS), and Ctrl 0 to reset."));

    widgets::section_heading(ui, &t("Window"), None);
    group(ui, |ui| {
        let mut collapsed = app.settings.sidebar_collapsed;
        if toggle_row(ui, &mut collapsed, &t("Show only icons in the sidebar"), Some(&t("Labels appear when you point at an icon."))) {
            app.settings.sidebar_collapsed = collapsed;
            app.settings.save();
        }
        ui.separator();
        let mut awake = app.settings.keep_awake;
        ui.add_enabled_ui(!crate::platform::IS_WEB, |ui| {
            let detail = if crate::platform::IS_WEB {
                t("Available in the desktop app.")
            } else {
                t("Prevents this computer from going to sleep while Discophile plays. Your system's power settings are unchanged.")
            };
            if toggle_row(ui, &mut awake, &t("Keep the computer awake while playing"), Some(&detail)) {
                app.settings.keep_awake = awake;
                app.settings.save();
            }
        });
    });

    #[cfg(not(target_arch = "wasm32"))]
    {
        widgets::section_heading(ui, &t("Window behavior"), None);
        group(ui, |ui| {
            if toggle_row(
                ui,
                &mut app.settings.minimize_on_close,
                &t("Keep Discophile in the system tray when closing"),
                Some(&t("Playback continues. Open the tray menu to restore the window or quit Discophile.")),
            ) {
                app.settings.save();
            }
            if !app.tray.as_ref().is_some_and(|tray| tray.available()) {
                footer(ui, &t("The system tray is unavailable. Closing will minimize the window instead."));
            } else if !app.can_hide_window {
                footer(ui, &t("This window system cannot hide the window. Closing will minimize it; the tray controls remain available."));
            }
            ui.separator();
            if labeled_button(ui, icons::POWER, &t("Quit Discophile"), Look::Plain).clicked() {
                app.request_quit();
            }
        });
    }

    widgets::section_heading(ui, &t("Introduction"), None);
    if labeled_button(ui, icons::SPARKLE, &t("Show the introduction again"), Look::Tinted).clicked() {
        app.settings.introduction_completed = false;
        app.settings.save();
    }
}

// MARK: - Playback

fn playback(app: &mut App, ui: &mut Ui) {
    widgets::section_heading(ui, &t("Streaming quality"), None);
    group(ui, |ui| {
        let options = [
            (StreamingQuality::Original, "Original", "The file as stored on your server, without conversion."),
            (StreamingQuality::High, "High quality · 320 kbps", "MP3 up to 320 kbps, converted by the server"),
            (StreamingQuality::Compact, "Data saver · 128 kbps", "MP3 up to 128 kbps, converted by the server"),
        ];
        for (quality, label, detail) in options {
            if choice_row(ui, app.settings.streaming_quality == quality, &t(label), Some(&t(detail))) {
                app.settings.streaming_quality = quality;
                app.settings.save();
            }
        }
    });
    footer(
        ui,
        &t(
            "High quality and Data saver ask your server for MP3 audio. Quality also applies to new downloads. Scrobbling sends listening history to your server when enabled.",
        ),
    );

    ui.add_space(8.0);
    group(ui, |ui| {
        let mut scrobble = app.settings.scrobble;
        if toggle_row(ui, &mut scrobble, &t("Scrobble listening history"), None) {
            app.settings.scrobble = scrobble;
            app.settings.save();
        }
    });

    crate::ui::audiobooks::playback_settings(app, ui);
}

// MARK: - Advanced

fn advanced(app: &mut App, ui: &mut Ui) {
    widgets::section_heading(ui, &t("Diagnostics"), None);
    group(ui, |ui| {
        value_row(ui, &t("Version"), env!("CARGO_PKG_VERSION"));
        let platform =
            if crate::platform::IS_WEB { t("Web browser") } else { format!("{} · {}", std::env::consts::OS, std::env::consts::ARCH) };
        value_row(ui, &t("Platform"), &platform);
        let library = app
            .session
            .active_account()
            .map(|a| format!("{} · {}", a.credentials.provider.title(), a.display_name()))
            .unwrap_or_else(|| t("Not connected"));
        value_row(ui, &t("Active library"), &library);
        value_row(ui, &t("Music libraries"), &app.session.accounts.len().to_string());
    });

    widgets::section_heading(ui, &t("Storage"), None);
    group(ui, |ui| {
        #[cfg(not(target_arch = "wasm32"))]
        {
            folder_row(app, ui, &t("Data folder"), crate::platform::storage::data_dir());
            ui.separator();
            folder_row(app, ui, &t("Cache folder"), crate::platform::storage::cache_dir());
            ui.separator();
        }
        #[cfg(target_arch = "wasm32")]
        {
            ui.label(theme::secondary(ui, t("Settings and saved data are kept in this browser's local storage."), Font::Callout));
            ui.separator();
        }
        let clear = trailing_row(
            ui,
            |ui| widgets::action_button(ui, t("Clear artwork cache"), true).clicked(),
            |ui| {
                ui.label(theme::text(t("Artwork cache"), Font::Body));
                ui.label(theme::secondary(
                    ui,
                    t("Artwork is downloaded again when needed. Downloaded music keeps its own artwork."),
                    Font::Footnote,
                ));
            },
        );
        if clear {
            clear_artwork_cache(app);
        }
    });

    ui.add_space(16.0);
    if ui.add(egui::Button::new(theme::accent(ui, t("Hide advanced settings"), Font::Headline)).frame(false)).clicked() {
        app.settings.advanced_unlocked = false;
        app.settings.save();
        app.nav.push(Route::Settings(SettingsPage::About));
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn folder_row(app: &mut App, ui: &mut Ui, label: &str, path: std::path::PathBuf) {
    let open = trailing_row(
        ui,
        |ui| labeled_button(ui, icons::FOLDER_OPEN, &t("Open folder"), Look::Tinted).clicked(),
        |ui| {
            ui.label(theme::text(label, Font::Body));
            ui.add(egui::Label::new(theme::secondary(ui, path.display().to_string(), Font::Footnote).monospace()).wrap());
        },
    );
    if open
        && let Err(error) = open_folder(&path) {
            app.show_error(format!("{}: {error}", t("Unable to open the folder")));
        }
}

/// Opens a folder in the system file manager.
#[cfg(not(target_arch = "wasm32"))]
fn open_folder(path: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)?;
    let program = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "explorer"
    } else {
        "xdg-open"
    };
    let mut child = std::process::Command::new(program).arg(path).spawn()?;
    // Reap the launcher so it does not linger as a zombie process.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

fn clear_artwork_cache(app: &mut App) {
    app.artwork.clear();
    #[cfg(not(target_arch = "wasm32"))]
    {
        let dir = crate::platform::storage::cache_dir().join("artwork");
        app.tasks.spawn(async move {
            let _ = tokio::fs::remove_dir_all(&dir).await;
            crate::tasks::update(|app: &mut App| app.toast(t("Artwork cache cleared.")))
        });
    }
    #[cfg(target_arch = "wasm32")]
    app.toast(t("Artwork cache cleared."));
}

// MARK: - About

fn about(app: &mut App, ui: &mut Ui) {
    let palette = Palette::of_ui(ui);
    let mut state: AboutState = ui.ctx().data(|d| d.get_temp(about_id())).unwrap_or_default();

    group(ui, |ui| {
        ui.horizontal(|ui| {
            let icon = crate::artwork::app_icon(ui.ctx(), 192);
            ui.add(egui::Image::new(&icon).fit_to_exact_size(vec2(72.0, 72.0)).corner_radius(CornerRadius::same(20)));
            ui.vertical(|ui| {
                ui.add_space(6.0);
                ui.label(theme::text("Discophile", Font::Title2));
                ui.label(theme::secondary(ui, t("Music, audiobooks and internet radio from your own servers."), Font::Subheadline));
            });
        });
        ui.add_space(8.0);
        ui.separator();
        let version = env!("CARGO_PKG_VERSION");
        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::click());
        if response.hovered() || response.has_focus() {
            ui.painter().rect_filled(rect, CornerRadius::same(8), palette.hover);
        }
        ui.painter().text(rect.left_center() + vec2(6.0, 0.0), egui::Align2::LEFT_CENTER, t("Version"), Font::Body.id(), palette.text);
        ui.painter().text(rect.right_center() - vec2(6.0, 0.0), egui::Align2::RIGHT_CENTER, version, Font::Body.id(), palette.secondary);
        let accessible = format!("{} {version}", t("Version"));
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &accessible));
        if response.clicked() {
            state.version_taps += 1;
            state.offer_advanced = true;
            if state.version_taps >= VERSION_TAPS {
                state.version_taps = 0;
                open_advanced(app);
            }
        }
        if state.offer_advanced && !app.settings.advanced_unlocked {
            ui.add_space(2.0);
            if ui.add(egui::Button::new(theme::accent(ui, t("Advanced settings"), Font::Callout)).frame(false)).clicked() {
                open_advanced(app);
            }
        }
    });
    ui.add_space(8.0);
    if labeled_button(ui, icons::SPARKLE, &t("Welcome to Discophile"), Look::Tinted).clicked() {
        app.settings.introduction_completed = false;
        app.settings.save();
    }

    widgets::section_heading(ui, &t("Meet the developer"), None);
    group(ui, |ui| {
        ui.label(theme::text("Andrea Debernardi", Font::Headline));
        ui.label(theme::secondary(ui, "@debba", Font::Subheadline));
        ui.add_space(6.0);
        ui.label(theme::text(t("Hi, I'm Andrea, the developer of Discophile."), Font::Callout).strong());
        ui.label(theme::text(
            t("If Discophile is useful to you, even a small contribution through GitHub Sponsors helps me spend time on fixes, accessibility, and new features."),
            Font::Callout,
        ));
        ui.add_space(4.0);
        link_button(ui, icons::GITHUB_LOGO, &t("Developer on GitHub"), DEVELOPER_URL, None);
        link_button(ui, icons::HEART, &t("Support on GitHub Sponsors"), SPONSORS_URL, Some(&t("Opens GitHub Sponsors in your browser")));
    });
    footer(ui, &t("Thank you for supporting independent development."));

    widgets::section_heading(ui, &t("Privacy"), None);
    group(ui, |ui| {
        ui.horizontal_top(|ui| {
            ui.label(theme::icon_filled(icons::SHIELD_CHECK, 22.0).color(palette.accent));
            ui.vertical(|ui| {
                ui.label(theme::text(
                    t("Discophile connects directly to your servers. There is no Discophile backend or account, and no advertising, tracking or analytics."),
                    Font::Callout,
                ));
                let storage = if crate::platform::IS_WEB {
                    "In the web player, credentials are kept in this browser's storage. Use it only on a device you trust."
                } else {
                    "Passwords and access tokens are stored in your system's keychain."
                };
                ui.label(theme::secondary(ui, t(storage), Font::Callout));
            });
        });
    });

    ui.ctx().data_mut(|d| d.insert_temp(about_id(), state));
}

fn open_advanced(app: &mut App) {
    app.settings.advanced_unlocked = true;
    app.settings.save();
    app.nav.push(Route::Settings(SettingsPage::Advanced));
}
