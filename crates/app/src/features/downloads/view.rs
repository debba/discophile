//! Download controls: the track button (iOS `DownloadButton`), context menu entries,
//! Settings > Offline listening and the "Remove all music downloads" confirmation.

use discophile_core::Song;
use discophile_core::l10n::t;
use egui::{Color32, Sense, Stroke, Ui, Vec2, pos2, vec2};

use super::DownloadState;
use crate::app::App;
use crate::theme::{self, Font, Palette, icons};
use crate::ui::widgets;

// MARK: Track button

/// The accessible label for a state, as on iOS ("Download: Title").
fn label(state: DownloadState) -> String {
    t(match state {
        DownloadState::NotDownloaded => "Download",
        DownloadState::Downloading(_) => "Cancel download",
        DownloadState::Downloaded { .. } => "Downloaded",
        DownloadState::Failed => "Retry download",
    })
}

pub fn download_button(ui: &mut Ui, app: &mut App, song: &Song) {
    if crate::platform::IS_WEB {
        return;
    }
    let state = super::download_state(app, song);
    let side = 32.0;
    let sense = if matches!(state, DownloadState::Downloaded { .. }) { Sense::hover() } else { Sense::click() };
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(side), sense);
    let palette = Palette::of_ui(ui);
    let accent = palette.accent;
    let painter = ui.painter_at(rect.expand(2.0));
    let center = rect.center();
    if response.hovered() && sense.senses_click() {
        painter.circle_filled(center, side / 2.0, palette.hover);
    }
    let radius = 10.0;
    match state {
        DownloadState::NotDownloaded => {
            let stroke = Stroke::new(1.6, accent);
            painter.circle_stroke(center, radius, stroke);
            painter.line_segment([center + vec2(0.0, -5.0), center + vec2(0.0, 4.5)], stroke);
            painter.line(vec![center + vec2(-4.0, 0.5), center + vec2(0.0, 4.5), center + vec2(4.0, 0.5)], stroke);
        }
        DownloadState::Downloading(fraction) => {
            painter.circle_stroke(center, radius, Stroke::new(2.0, accent.gamma_multiply(0.25)));
            if fraction > 0.0 {
                let steps = ((fraction * 48.0).ceil() as usize).max(2);
                let points: Vec<egui::Pos2> = (0..=steps)
                    .map(|i| {
                        let angle = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * fraction * i as f32 / steps as f32;
                        center + radius * vec2(angle.cos(), angle.sin())
                    })
                    .collect();
                painter.line(points, Stroke::new(2.0, accent));
            }
            painter.rect_filled(egui::Rect::from_center_size(center, Vec2::splat(6.5)), 1.5, accent);
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
        }
        DownloadState::Downloaded { .. } => {
            painter.circle_filled(center, radius + 0.5, accent);
            let check = vec![center + vec2(-4.5, 0.2), center + vec2(-1.3, 3.4), center + vec2(4.8, -3.2)];
            painter.line(check, Stroke::new(2.0, palette.background));
        }
        DownloadState::Failed => {
            let stroke = Stroke::new(1.6, accent);
            painter.circle_stroke(center, radius, stroke);
            painter.text(center, egui::Align2::CENTER_CENTER, icons::ARROW_CLOCKWISE, egui::FontId::proportional(12.0), accent);
        }
    }
    let mut text = format!("{}: {}", label(state), song.title);
    if let DownloadState::Downloading(fraction) = state {
        text = format!("{text}, {}", super::format::percent(fraction));
    }
    let enabled = sense.senses_click();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, &text));
    let response = response.on_hover_text(&text);
    let response = if enabled { response.on_hover_cursor(egui::CursorIcon::PointingHand) } else { response };
    if response.clicked() {
        match state {
            DownloadState::Downloading(_) => super::remove(app, song),
            DownloadState::NotDownloaded | DownloadState::Failed => super::download(app, [song]),
            DownloadState::Downloaded { .. } => {}
        }
    }
    response.context_menu(|ui| song_menu(ui, app, song));
}

/// Download entries for a track's context menu.
pub fn song_menu(ui: &mut Ui, app: &mut App, song: &Song) {
    if crate::platform::IS_WEB {
        return;
    }
    let state = super::download_state(app, song);
    match state {
        DownloadState::NotDownloaded => {
            if ui.button(format!("{}  {}", icons::ARROW_CIRCLE_DOWN, t("Download"))).clicked() {
                super::download(app, [song]);
                ui.close();
            }
        }
        DownloadState::Failed => {
            if ui.button(format!("{}  {}", icons::ARROW_CLOCKWISE, t("Retry download"))).clicked() {
                super::download(app, [song]);
                ui.close();
            }
            if ui.button(format!("{}  {}", icons::TRASH, t("Remove download"))).clicked() {
                super::remove(app, song);
                ui.close();
            }
        }
        DownloadState::Downloading(_) | DownloadState::Downloaded { .. } => {
            let automatic = matches!(state, DownloadState::Downloaded { automatic: true }) || is_automatic(app, song);
            if automatic && ui.button(format!("{}  {}", icons::PUSH_PIN, t("Keep download"))).clicked() {
                super::keep(app, song);
                ui.close();
            }
            let title = if matches!(state, DownloadState::Downloading(_)) { "Cancel download" } else { "Remove download" };
            if ui.button(format!("{}  {}", icons::TRASH, t(title))).clicked() {
                super::remove(app, song);
                ui.close();
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn is_automatic(app: &App, song: &Song) -> bool {
    app.downloads.record(&app.downloads.account, &song.id).is_some_and(|r| r.is_automatic())
}

#[cfg(target_arch = "wasm32")]
fn is_automatic(_app: &App, _song: &Song) -> bool {
    false
}

// MARK: Settings

/// An iOS-style switch row. Returns true when toggled.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn switch_row(ui: &mut Ui, title: &str, value: bool, enabled: bool) -> bool {
    let mut toggled = false;
    ui.horizontal(|ui| {
        ui.add_enabled(enabled, egui::Label::new(theme::text(title, Font::Body)).selectable(false));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let size = vec2(40.0, 24.0);
            let (rect, response) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
            let palette = Palette::of_ui(ui);
            let how_on = ui.ctx().animate_bool(response.id, value);
            let off = if palette.dark { Color32::from_white_alpha(40) } else { Color32::from_black_alpha(36) };
            let mut fill = off.lerp_to_gamma(palette.action, how_on);
            if !enabled {
                fill = fill.gamma_multiply(0.5);
            }
            ui.painter().rect_filled(rect, 12.0, fill);
            let x = egui::lerp((rect.left() + 12.0)..=(rect.right() - 12.0), how_on);
            ui.painter().circle_filled(pos2(x, rect.center().y), 10.0, Color32::WHITE);
            response.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, enabled, value, title));
            if response.clicked() {
                toggled = true;
            }
            if enabled {
                response.on_hover_cursor(egui::CursorIcon::PointingHand);
            }
        });
    });
    toggled
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn value_row(ui: &mut Ui, title: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(theme::text(title, Font::Body));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(theme::secondary(ui, value, Font::Body));
        });
    });
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn footer(ui: &mut Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(theme::secondary(ui, text, Font::Footnote));
}

pub fn settings_ui(app: &mut App, ui: &mut Ui) {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = app;
        widgets::section_heading(ui, &t("Offline listening"), None);
        widgets::card(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(theme::icon(icons::DOWNLOAD_SIMPLE, 20.0).color(Palette::of_ui(ui).accent));
                ui.label(theme::text(
                    t("Offline listening requires the Discophile desktop app. The web player streams music from your server."),
                    Font::Body,
                ));
            });
        });
    }
    #[cfg(not(target_arch = "wasm32"))]
    native::settings_ui(app, ui);
}

pub fn windows(app: &mut App, ctx: &egui::Context) {
    if !app.downloads.confirm_remove_all {
        return;
    }
    let mut confirmed = false;
    let mut cancelled = false;
    let modal = crate::ui::widgets::modal(ctx, egui::Id::unique("remove-all-music-downloads")).show(ctx, |ui| {
        ui.set_width(400.0);
        ui.label(theme::text(t("Remove all music downloads?"), Font::Title3));
        ui.add_space(6.0);
        ui.label(theme::text(t("Your server's library is unchanged. You can download this content again later."), Font::Body));
        ui.label(theme::secondary(ui, t("Automatic downloads are turned off."), Font::Footnote));
        ui.add_space(12.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let palette = Palette::of_ui(ui);
            let remove = egui::Button::new(theme::text(t("Remove all music downloads"), Font::Headline).color(Color32::WHITE))
                .fill(palette.danger)
                .corner_radius(14)
                .min_size(vec2(0.0, 40.0));
            if ui.add(remove).clicked() {
                confirmed = true;
            }
            if widgets::action_button(ui, t("Cancel"), true).clicked() {
                cancelled = true;
            }
        });
    });
    if confirmed {
        super::remove_all(app);
    }
    if confirmed || cancelled || modal.should_close() {
        app.downloads.confirm_remove_all = false;
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use discophile_core::l10n::{count, t, tf};
    use discophile_core::offline_catalog::OfflineState;
    use egui::{Ui, vec2};

    use super::super::{automatic, format};
    use super::{footer, switch_row, value_row};
    use crate::app::App;
    use crate::theme::{self, Font, Palette, icons};
    use crate::ui::widgets;

    pub fn settings_ui(app: &mut App, ui: &mut Ui) {
        let has_library = app.session.has_library();
        offline_section(app, ui, has_library);
        automatic_section(app, ui, has_library);
        playlists_section(app, ui, has_library);
        manage_section(app, ui);
    }

    fn offline_section(app: &mut App, ui: &mut Ui, has_library: bool) {
        widgets::section_heading(ui, &t("Offline listening"), None);
        let (ready, pending, failed) = counts(app, None);
        let bytes = super::super::total_bytes(app);
        widgets::card(ui, |ui| {
            if let Some(error) = app.downloads.error.clone() {
                ui.label(theme::text(error, Font::Callout).color(Palette::of_ui(ui).accent));
            }
            value_row(ui, &t("Storage used"), &format::bytes(bytes));
            value_row(ui, &t("Downloaded tracks"), &ready.to_string());
            if pending > 0 {
                value_row(ui, &t("Pending downloads"), &pending.to_string());
            }
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                if failed > 0 && ui.add_enabled(has_library, egui::Button::new(t("Retry failed downloads"))).clicked() {
                    super::super::retry_failed(app);
                }
                let palette = Palette::of_ui(ui);
                let any = ready + pending + failed > 0;
                let remove = egui::Button::new(theme::text(t("Remove all music downloads"), Font::Body).color(if any {
                    palette.danger
                } else {
                    palette.secondary
                }));
                if ui.add_enabled(any, remove).clicked() {
                    app.downloads.confirm_remove_all = true;
                }
            });
        });
        footer(
            ui,
            &t(
                "Use the download button beside a track or Download all in an album, playlist, or podcast. Turn on Downloaded only in your library to filter saved content. Downloads stay on this computer.",
            ),
        );
        let folder = super::super::store::root();
        ui.label(theme::secondary(ui, tf("Folder: %@", &[&folder.display()]), Font::Caption));
    }

    /// (ready, pending, failed) records of the active library; `automatic` filters by kind.
    fn counts(app: &App, automatic: Option<bool>) -> (usize, usize, usize) {
        let mut result = (0, 0, 0);
        for record in app.downloads.account_records().filter(|r| automatic.is_none_or(|a| r.is_automatic() == a)) {
            match record.state {
                OfflineState::Ready => result.0 += 1,
                OfflineState::Queued => result.1 += 1,
                OfflineState::Failed => result.2 += 1,
            }
        }
        result
    }

    fn automatic_section(app: &mut App, ui: &mut Ui, has_library: bool) {
        widgets::section_heading(ui, &t("Automatic downloads"), None);
        let settings = app.downloads.automatic.settings.clone();
        widgets::card(ui, |ui| {
            if switch_row(ui, &t("Automatic downloads"), settings.enabled, has_library) {
                automatic::update_settings(app, |s| s.enabled = !s.enabled);
            }
            if switch_row(ui, &t("Keep my discovery mixes available"), settings.include_mixes, has_library) {
                automatic::update_settings(app, |s| s.include_mixes = !s.include_mixes);
            }
            ui.horizontal(|ui| {
                ui.label(theme::text(t("Music to keep available"), Font::Body));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut minutes = settings.minutes;
                    ui.add_enabled_ui(has_library, |ui| {
                        egui::ComboBox::from_id_salt("automatic-minutes").selected_text(count("%lld minutes", minutes)).show_ui(ui, |ui| {
                            for option in format::MINUTE_OPTIONS {
                                ui.selectable_value(&mut minutes, option, count("%lld minutes", option));
                            }
                        });
                    });
                    if minutes != settings.minutes {
                        automatic::update_settings(app, |s| s.minutes = minutes);
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.label(theme::text(t("Automatic downloads limit"), Font::Body));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut limit = settings.limit_mb;
                    ui.add_enabled_ui(has_library, |ui| {
                        egui::ComboBox::from_id_salt("automatic-limit").selected_text(format::limit_label(limit)).show_ui(ui, |ui| {
                            for option in format::LIMIT_OPTIONS {
                                ui.selectable_value(&mut limit, option, format::limit_label(option));
                            }
                        });
                    });
                    if limit != settings.limit_mb {
                        automatic::update_settings(app, |s| s.limit_mb = limit);
                    }
                });
            });
            ui.separator();
            let automatic_bytes: i64 =
                app.downloads.account_records().filter(|r| r.is_automatic() && r.state == OfflineState::Ready).map(|r| r.bytes).sum();
            let (ready, pending, _) = counts(app, Some(true));
            value_row(ui, &t("Storage used"), &format::bytes(automatic_bytes));
            value_row(ui, &t("Downloaded tracks"), &ready.to_string());
            value_row(ui, &t("Pending downloads"), &pending.to_string());
            ui.add_space(4.0);
            let state = &app.downloads.automatic;
            let (busy, enabled) = (state.busy, state.settings.enabled);
            let error = state.error.clone().or_else(|| state.transfer_error.clone());
            ui.horizontal(|ui| {
                if ui.add_enabled(enabled && !busy && has_library, egui::Button::new(t("Update automatic downloads"))).clicked() {
                    automatic::refresh(app, true);
                }
                if busy {
                    ui.spinner();
                    ui.label(theme::secondary(ui, t("Working…"), Font::Callout));
                }
            });
            if let Some(error) = error {
                ui.label(theme::text(error, Font::Callout).color(Palette::of_ui(ui).accent));
            }
        });
        footer(
            ui,
            &t(
                "Mixes and selected playlists refresh when you open Discophile, when your library refreshes and every hour while Discophile is running. Transfers continue while Discophile is open.",
            ),
        );
        footer(ui, &t("The limit applies to automatic audio files. Manual downloads and artwork are separate."));
    }

    fn playlists_section(app: &mut App, ui: &mut Ui, has_library: bool) {
        widgets::section_heading(ui, &t("Keep these playlists downloaded"), None);
        if has_library {
            automatic::load_playlists(app, false);
        }
        let playlists = app.downloads.automatic.playlists.clone().unwrap_or_default();
        let selected = app.downloads.automatic.settings.playlist_ids.clone();
        let loading = app.downloads.automatic.playlists_loading;
        let error = app.downloads.automatic.playlists_error.clone();
        widgets::card(ui, |ui| {
            if let Some(error) = error
                && widgets::error_banner(ui, &error, true) {
                    automatic::load_playlists(app, true);
                }
            if loading && playlists.is_empty() {
                ui.spinner();
            } else if playlists.is_empty() {
                ui.label(theme::secondary(ui, t("Your playlists will appear here."), Font::Body));
            }
            for playlist in &playlists {
                let on = selected.contains(&playlist.id);
                if switch_row(ui, &playlist.name, on, has_library) {
                    let id = playlist.id.clone();
                    automatic::update_settings(app, |s| {
                        s.playlist_ids.retain(|p| *p != id);
                        if !on {
                            s.playlist_ids.push(id);
                        }
                    });
                }
            }
        });
        footer(
            ui,
            &t(
                "Playlists take priority within the space limit. Automatic files may be replaced as your selections change. Manual downloads are kept. Choose Keep download on an automatic track to keep it permanently.",
            ),
        );
    }

    fn manage_section(app: &mut App, ui: &mut Ui) {
        let total = app.downloads.account_records().count();
        if total == 0 {
            return;
        }
        widgets::section_heading(ui, &t("Manage downloads"), None);
        widgets::card(ui, |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut app.downloads.list_filter).align(egui::Align2::LEFT_CENTER).hint_text(t("Filter downloads")).desired_width(f32::INFINITY),
            );
            let filter = discophile_core::listening_history::fold(app.downloads.list_filter.trim());
            let mut rows: Vec<(uuid::Uuid, discophile_core::Song, OfflineState, bool, i64)> = app
                .downloads
                .account_records()
                .filter(|r| {
                    filter.is_empty()
                        || discophile_core::listening_history::fold(&format!("{} {}", r.song.title, r.song.subtitle())).contains(&filter)
                })
                .map(|r| (r.id, r.song.clone(), r.state, r.is_automatic(), r.bytes))
                .collect();
            let order = |state: OfflineState| match state {
                OfflineState::Queued => 0,
                OfflineState::Failed => 1,
                OfflineState::Ready => 2,
            };
            rows.sort_by_key(|row| order(row.2));
            let palette = Palette::of_ui(ui);
            let row_height = 44.0;
            egui::ScrollArea::vertical().id_salt("manage-downloads").max_height(380.0).auto_shrink([false, true]).show_rows(
                ui,
                row_height,
                rows.len(),
                |ui, range| {
                    for (id, song, state, is_automatic, bytes) in &rows[range] {
                        ui.allocate_ui(vec2(ui.available_width(), row_height), |ui| {
                            ui.horizontal(|ui| {
                                ui.set_min_height(row_height - 4.0);
                                let status = match state {
                                    OfflineState::Queued => format::percent(app.downloads.progress(*id)),
                                    OfflineState::Failed => t("Download failed"),
                                    OfflineState::Ready if *is_automatic => format!("{} · {}", format::bytes(*bytes), t("Automatic")),
                                    OfflineState::Ready => format::bytes(*bytes),
                                };
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let trash_label =
                                        if *state == OfflineState::Queued { t("Cancel download") } else { t("Remove download") };
                                    let glyph = if *state == OfflineState::Queued { icons::X_CIRCLE } else { icons::TRASH };
                                    if widgets::icon_button(ui, glyph, &trash_label, 16.0).clicked() {
                                        super::super::manager::remove_ids(app, &[*id]);
                                    }
                                    if *state == OfflineState::Failed
                                        && widgets::icon_button(ui, icons::ARROW_CLOCKWISE, &t("Retry download"), 16.0).clicked()
                                    {
                                        super::super::download(app, [song]);
                                    }
                                    if *is_automatic
                                        && *state != OfflineState::Failed
                                        && widgets::icon_button(ui, icons::PUSH_PIN, &t("Keep download"), 16.0).clicked()
                                    {
                                        super::super::manager::keep_record(app, *id);
                                    }
                                    let color = if *state == OfflineState::Failed { palette.danger } else { palette.secondary };
                                    ui.label(theme::text(status, Font::Footnote).color(color));
                                    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                                        ui.add(egui::Label::new(theme::text(&song.title, Font::Body)).truncate().selectable(false));
                                        let subtitle = song.subtitle();
                                        if !subtitle.is_empty() {
                                            ui.add(
                                                egui::Label::new(theme::secondary(ui, subtitle, Font::Footnote))
                                                    .truncate()
                                                    .selectable(false),
                                            );
                                        }
                                    });
                                });
                            });
                        });
                    }
                },
            );
        });
    }
}
