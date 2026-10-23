//! Internet Radio: the stations page, the station form and the stations section of Settings.

use discophile_core::InternetRadioStation;
use discophile_core::l10n::t;
use egui::{Align, Color32, CornerRadius, Layout, Sense, Ui, vec2};

use crate::app::App;
use crate::features::radio::{self, StationForm};
use crate::theme::{self, Font, Palette, icons};
use crate::ui::widgets;

fn live_badge(ui: &mut Ui) {
    let palette = Palette::of_ui(ui);
    egui::Frame::new().fill(palette.action).corner_radius(CornerRadius::same(6)).inner_margin(egui::Margin::symmetric(6, 2)).show(
        ui,
        |ui| {
            ui.label(theme::text(t("Live").to_uppercase(), Font::Caption).color(Color32::WHITE));
        },
    );
}

fn station_art(ui: &mut Ui, station: &InternetRadioStation, size: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    let seed = station.id.to_string();
    let [top, bottom] = theme::ARTWORK_PALETTES[theme::artwork_seed(&seed)];
    widgets::paint_gradient(ui, rect, theme::hex(top), theme::hex(bottom), CornerRadius::same(12));
    crate::ui::widgets::paint_icon(ui.painter(), rect.center(), icons::RADIO, egui::FontId::proportional(size * 0.4), Color32::from_white_alpha(225));
    response
}

fn station_card(app: &mut App, ui: &mut Ui, station: &InternetRadioStation) {
    let palette = Palette::of_ui(ui);
    let current = radio::is_current(app, station.id);
    let playing = current && app.player.status.playing;
    let fill = if current { palette.surface_raised } else { palette.surface };
    let mut play = false;
    let mut target: Option<egui::Response> = None;
    widgets::card_frame(ui).fill(fill).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let art = station_art(ui, station, 64.0);
            ui.add_space(4.0);
            let text = ui
                .vertical(|ui| {
                    ui.set_width((ui.available_width() - 90.0).max(60.0));
                    let color = if current { palette.accent } else { palette.text };
                    let name = ui.add(
                        egui::Label::new(theme::text(station.display_name(), Font::Headline).color(color))
                            .truncate()
                            .selectable(false)
                            .sense(Sense::click()),
                    );
                    let host = station.stream_url.host_str().unwrap_or_default().to_string();
                    let host = ui.add(
                        egui::Label::new(theme::secondary(ui, host, Font::Subheadline)).truncate().selectable(false).sense(Sense::click()),
                    );
                    if current {
                        ui.horizontal(|ui| {
                            live_badge(ui);
                            if app.player.status.buffering {
                                ui.label(theme::secondary(ui, t("Buffering…"), Font::Footnote));
                            }
                        });
                    }
                    name.union(host)
                })
                .inner;
            target = Some(art.union(text));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.menu_button(theme::icon(icons::DOTS_THREE, 20.0), |ui| station_menu(app, ui, station)).response.on_hover_text(format!(
                    "{}: {}",
                    t("Manage station"),
                    station.display_name()
                ));
                let label = format!("{}: {}", if playing { t("Pause") } else { t("Play") }, station.display_name());
                if widgets::round_play_button(ui, playing, &label, 40.0).clicked() {
                    play = true;
                }
            });
        });
    });
    if let Some(target) = target {
        let target = target.on_hover_cursor(egui::CursorIcon::PointingHand);
        target.context_menu(|ui| station_menu(app, ui, station));
        play |= target.clicked();
    }
    if play {
        radio::play(app, station);
    }
}

fn station_menu(app: &mut App, ui: &mut Ui, station: &InternetRadioStation) {
    if ui.button(t("Edit station")).clicked() {
        app.radio.editor = Some(StationForm::editing(station));
        ui.close();
    }
    if ui.button(t("Remove station")).clicked() {
        app.radio.removing = Some(station.id);
        ui.close();
    }
}

pub fn show(app: &mut App, ui: &mut Ui) {
    widgets::page(ui, "radio", |ui| {
        widgets::page_header(ui, &t("Internet Radio"), |ui| {
            if widgets::pill_button(ui, icons::PLUS, t("Add station"), false).clicked() {
                app.radio.editor = Some(StationForm::default());
            }
        });
        if let Some(error) = app.radio.error.clone() {
            widgets::error_banner(ui, &error, false);
            ui.add_space(8.0);
        }
        let stations = app.radio.stations.clone();
        if stations.is_empty() {
            widgets::empty_state(
                ui,
                icons::RADIO,
                &t("Your radio stations"),
                &t("Add a station using its direct audio stream URL. No account is needed."),
            );
            ui.vertical_centered(|ui| {
                if widgets::action_button(ui, t("Add station"), false).clicked() {
                    app.radio.editor = Some(StationForm::default());
                }
                ui.add_space(widgets::space::L);
                ui.set_max_width(520.0);
                ui.add(egui::Label::new(theme::secondary(ui, radio::format_note(), Font::Footnote)).wrap().halign(egui::Align::Center));
            });
        } else {
            ui.add_space(8.0);
            widgets::tile_grid(ui, &stations, 320.0, |ui, station, _| station_card(app, ui, station));
            ui.add_space(widgets::space::M);
            ui.label(theme::secondary(ui, radio::format_note(), Font::Footnote));
        }
    });
}

/// The station fields. Returns true when Save was pressed with valid input.
fn form_fields(ui: &mut Ui, form: &mut StationForm) -> bool {
    let width = ui.available_width().min(460.0);
    let suggested = discophile_core::connection_alias::suggested("Radio", url::Url::parse(form.stream.trim()).ok().as_ref(), false);
    ui.label(theme::secondary(ui, t("Alias (optional)"), Font::Subheadline));
    ui.add(egui::TextEdit::singleline(&mut form.name).align(egui::Align2::LEFT_CENTER).hint_text(suggested).desired_width(width))
        .on_hover_text(t("Leave empty to use the suggested name."));
    ui.label(theme::secondary(ui, t("Stream URL"), Font::Subheadline));
    let response =
        ui.add(egui::TextEdit::singleline(&mut form.stream).align(egui::Align2::LEFT_CENTER).hint_text("https://stream.example.com/live.mp3").desired_width(width));
    let mut submit = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    if form.is_http() {
        ui.checkbox(&mut form.allow_http, t("Allow HTTP for this station"));
        ui.label(theme::secondary(ui, t("HTTP does not encrypt your connection. Only use it on a trusted local network."), Font::Footnote));
    }
    if form.looks_like_playlist() {
        let palette = Palette::of_ui(ui);
        ui.horizontal_wrapped(|ui| {
            ui.label(theme::icon(icons::WARNING, 15.0).color(palette.danger));
            let message = if crate::platform::IS_WEB {
                t("This looks like a playlist or HLS address. Your browser may not play it; use the direct stream URL listed inside the playlist instead.")
            } else {
                t("This looks like an HLS address, which is not supported on this computer. Use the station's direct MP3, AAC or Ogg stream instead.")
            };
            ui.add(egui::Label::new(theme::text(message, Font::Footnote).color(palette.danger)).wrap());
        });
    }
    ui.add(egui::Label::new(theme::secondary(ui, radio::format_note(), Font::Footnote)).wrap());
    if let Some(error) = &form.error {
        ui.add(egui::Label::new(theme::text(error, Font::Callout).color(Palette::of_ui(ui).accent)).wrap());
    }
    ui.add_space(6.0);
    let ready = !form.stream.trim().is_empty();
    if ui.add_enabled_ui(ready, |ui| widgets::action_button(ui, t("Save"), false)).inner.clicked() {
        submit = true;
    }
    submit && ready
}

/// Add-station form body, embedded in the Add account sheet. Returns true when saved.
pub fn station_form(app: &mut App, ui: &mut Ui) -> bool {
    let mut form = std::mem::take(&mut app.radio.form);
    let saved = form_fields(ui, &mut form) && radio::save_form(app, &mut form);
    app.radio.form = if saved { StationForm::default() } else { form };
    saved
}

/// The stations section of Settings > Libraries and accounts.
pub fn account_settings(app: &mut App, ui: &mut Ui) {
    widgets::section_heading(ui, &t("Internet Radio"), None);
    widgets::card(ui, |ui| {
        let palette = Palette::of_ui(ui);
        let stations = app.radio.stations.clone();
        ui.horizontal(|ui| {
            ui.label(theme::text(t("Saved stations"), Font::Headline));
            ui.label(theme::secondary(ui, stations.len().to_string(), Font::Body));
        });
        if let Some(error) = app.radio.error.clone() {
            ui.label(theme::text(error, Font::Footnote).color(palette.danger));
        }
        for station in &stations {
            ui.horizontal(|ui| {
                ui.label(theme::icon(icons::RADIO, 18.0).color(palette.accent));
                ui.vertical(|ui| {
                    ui.label(theme::text(station.display_name(), Font::Body));
                    ui.label(theme::secondary(ui, station.stream_url.host_str().unwrap_or_default(), Font::Footnote));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if widgets::icon_button(ui, icons::TRASH, &format!("{}: {}", t("Remove station"), station.display_name()), 16.0)
                        .clicked()
                    {
                        app.radio.removing = Some(station.id);
                    }
                    if widgets::icon_button(ui, icons::PENCIL_SIMPLE, &format!("{}: {}", t("Edit station"), station.display_name()), 16.0)
                        .clicked()
                    {
                        app.radio.editor = Some(StationForm::editing(station));
                    }
                });
            });
        }
        ui.add_space(4.0);
        if widgets::pill_button(ui, icons::PLUS, t("Add station"), false).clicked() {
            app.radio.editor = Some(StationForm::default());
        }
    });
}

/// The station editor and the removal confirmation.
pub fn windows(app: &mut App, ctx: &egui::Context) {
    if let Some(mut form) = app.radio.editor.take() {
        let title = if form.id.is_some() { t("Edit station") } else { t("Add station") };
        let mut saved = false;
        let mut close = false;
        let modal = crate::ui::widgets::modal(ctx, egui::Id::unique("radio-editor")).show(ctx, |ui| {
            ui.set_width(440.0);
            ui.horizontal(|ui| {
                ui.label(theme::text(title, Font::Title3));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if widgets::icon_button(ui, icons::X, &t("Cancel"), 16.0).clicked() {
                        close = true;
                    }
                });
            });
            ui.add_space(8.0);
            if form_fields(ui, &mut form) {
                saved = radio::save_form(app, &mut form);
            }
        });
        if !(saved || close || modal.should_close()) {
            app.radio.editor = Some(form);
        }
    }
    if let Some(id) = app.radio.removing {
        let name = app.radio.stations.iter().find(|s| s.id == id).map(InternetRadioStation::display_name);
        let Some(name) = name else {
            app.radio.removing = None;
            return;
        };
        let mut confirmed = false;
        let mut cancelled = false;
        let modal = crate::ui::widgets::modal(ctx, egui::Id::unique("radio-remove")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.label(theme::text(t("Remove station?"), Font::Title3));
            ui.add_space(4.0);
            ui.label(theme::secondary(ui, name, Font::Body));
            ui.add_space(12.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let danger = Palette::of_ui(ui).danger;
                if ui.add(egui::Button::new(theme::text(t("Remove station"), Font::Headline).color(Color32::WHITE)).fill(danger)).clicked()
                {
                    confirmed = true;
                }
                if ui.button(t("Cancel")).clicked() {
                    cancelled = true;
                }
            });
        });
        if confirmed {
            app.radio.removing = None;
            radio::remove(app, id);
        } else if cancelled || modal.should_close() {
            app.radio.removing = None;
        }
    }
}
