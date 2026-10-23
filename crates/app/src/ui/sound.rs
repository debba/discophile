//! Equalizer and sound: the 10-band equalizer or an AutoEq headphone correction with its response curve,
//! ReplayGain, transitions and the volume limit.

use discophile_core::audio_effects::{AudioEffectsSettings, EqualizerMode, ReplayGainMode, log_frequencies};
use discophile_core::l10n::{self, t};
use egui::{Align, Align2, Color32, CornerRadius, FontId, Layout, Rect, Sense, Shape, Stroke, Ui, pos2, vec2};

use crate::app::App;
use crate::features::effects;
use crate::theme::{self, Font, Palette};
use crate::ui::player_bar::controls::{self, decibels, frequency};
use crate::ui::widgets;

/// Equalizer and sound page body (also embedded in Settings > Sound).
pub fn show(app: &mut App, ui: &mut Ui) {
    equalizer(app, ui);
    loudness(app, ui);
    transitions(app, ui);
    volume_limit(app, ui);
    ui.add_space(10.0);
    ui.label(theme::secondary(
        ui,
        t("Effects apply to everything Discophile plays on this computer. Audiobooks use only the volume limit."),
        Font::Footnote,
    ));
}

fn footer(ui: &mut Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(theme::secondary(ui, text, Font::Footnote));
}

/// Applies a slider change: live while dragging, saved when released.
fn commit(app: &mut App, response: &egui::Response) {
    if response.dragged() { effects::preview(app) } else { effects::changed(app) }
}

fn equalizer(app: &mut App, ui: &mut Ui) {
    widgets::section_heading(ui, &t("Equalizer"), None);
    widgets::card(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            for (mode, label) in
                [(EqualizerMode::Off, "Off"), (EqualizerMode::Manual, "Manual"), (EqualizerMode::AutoEq, "Headphones (AutoEq)")]
            {
                if widgets::chip(ui, app.effects.settings.equalizer == mode, &t(label)).clicked() && app.effects.settings.equalizer != mode
                {
                    app.effects.settings.equalizer = mode;
                    effects::changed(app);
                }
            }
        });
        match app.effects.settings.equalizer {
            EqualizerMode::Manual => manual(app, ui),
            EqualizerMode::AutoEq => auto_eq(app, ui),
            EqualizerMode::Off => {}
        }
        if let Some(active) = app.effects.settings.active_equalizer() {
            ui.add_space(12.0);
            ui.label(theme::text(t("Equalizer response"), Font::Headline));
            ui.label(theme::secondary(ui, format!("{}: {}", t("Preamp"), decibels(active.preamp)), Font::Footnote));
            response_curve(ui, &app.effects.settings);
        }
        if let Some(error) = app.effects.error.clone().filter(|_| !app.effects.browser_open) {
            ui.add_space(8.0);
            ui.label(theme::text(error, Font::Callout).color(Palette::of_ui(ui).danger));
        }
    });
    footer(
        ui,
        &t(
            "AutoEq corrects the frequency response of a headphone model towards a neutral target. Choosing headphones downloads the AutoEq list and the selected profile from GitHub. No listening data or library information is sent.",
        ),
    );
}

fn manual(app: &mut App, ui: &mut Ui) {
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        let current = effects::current_preset(app).map(t).unwrap_or_else(|| t("Custom"));
        ui.label(theme::text(t("Presets"), Font::Body));
        egui::ComboBox::from_id_salt("eq-presets").selected_text(current).show_ui(ui, |ui| {
            for (name, bands) in effects::PRESETS {
                let selected = effects::current_preset(app) == Some(name);
                if ui.selectable_label(selected, t(name)).clicked() {
                    effects::apply_preset(app, bands);
                }
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let any = app.effects.settings.bands.iter().any(|b| *b != 0.0);
            if ui.add_enabled(any, egui::Button::new(t("Reset bands"))).clicked() {
                effects::apply_preset(app, [0.0; 10]);
            }
        });
    });
    ui.add_space(8.0);
    let count = AudioEffectsSettings::BAND_FREQUENCIES.len();
    let width = ui.available_width();
    let column = (width / count as f32).floor();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 200.0), Sense::hover());
    for (index, hertz) in AudioEffectsSettings::BAND_FREQUENCIES.into_iter().enumerate() {
        let cell = Rect::from_min_size(pos2(rect.left() + column * index as f32, rect.top()), vec2(column, rect.height()));
        let gain = app.effects.settings.bands.get(index).copied().unwrap_or(0.0);
        if let Some((value, response)) = band_slider(ui, cell, index, hertz, gain) {
            app.effects.settings.set_band(index, value);
            commit(app, &response);
        }
    }
}

/// A vertical band slider in `cell`: gain above, frequency below. Double-click resets to 0 dB.
fn band_slider(ui: &mut Ui, cell: Rect, index: usize, hertz: f64, gain: f64) -> Option<(f64, egui::Response)> {
    let palette = Palette::of_ui(ui);
    let (low, high) = AudioEffectsSettings::BAND_RANGE;
    let track = Rect::from_min_max(pos2(cell.center().x - 14.0, cell.top() + 22.0), pos2(cell.center().x + 14.0, cell.bottom() - 22.0));
    let label = frequency(hertz);
    let response = ui.interact(track, ui.scope_id().with(("eq-band", index)), Sense::click_and_drag());
    let to_y = |g: f64| egui::remap(g as f32, low as f32..=high as f32, track.bottom() - 6.0..=track.top() + 6.0);
    let mut value = None;
    if response.double_clicked() {
        value = Some(0.0);
    } else if let Some(pointer) = response.interact_pointer_pos()
        && (response.dragged() || response.clicked() || response.drag_stopped())
    {
        let g = egui::remap_clamp(pointer.y, track.bottom() - 6.0..=track.top() + 6.0, low as f32..=high as f32) as f64;
        value = Some((g * 2.0).round() / 2.0);
    }
    if response.has_focus() {
        let (up, down) = ui.input(|i| (i.key_pressed(egui::Key::ArrowUp), i.key_pressed(egui::Key::ArrowDown)));
        if up || down {
            value = Some((gain + if up { 0.5 } else { -0.5 }).clamp(low, high));
        }
    }
    let shown = value.unwrap_or(gain);
    let rail = Rect::from_center_size(track.center(), vec2(4.0, track.height() - 12.0));
    let rail_color = if palette.dark { Color32::from_white_alpha(40) } else { Color32::from_black_alpha(30) };
    ui.painter().rect_filled(rail, CornerRadius::same(2), rail_color);
    let zero = to_y(0.0);
    let thumb_y = to_y(shown);
    let filled = Rect::from_min_max(pos2(rail.left(), zero.min(thumb_y)), pos2(rail.right(), zero.max(thumb_y)));
    ui.painter().rect_filled(filled, CornerRadius::same(2), palette.accent);
    ui.painter().line_segment(
        [pos2(rail.left() - 5.0, zero), pos2(rail.right() + 5.0, zero)],
        Stroke::new(1.0, palette.secondary.gamma_multiply(0.5)),
    );
    let active = response.hovered() || response.dragged() || response.has_focus();
    ui.painter().circle_filled(pos2(rail.center().x, thumb_y), if active { 8.0 } else { 7.0 }, palette.surface);
    ui.painter().circle_stroke(pos2(rail.center().x, thumb_y), if active { 8.0 } else { 7.0 }, Stroke::new(2.0, palette.accent));
    let small = FontId::new(10.5, egui::FontFamily::Proportional);
    let value_text = if shown.abs() < 0.05 { "0".to_string() } else { controls::decimal(shown, 1) };
    ui.painter().text(
        pos2(cell.center().x, cell.top() + 8.0),
        Align2::CENTER_CENTER,
        value_text,
        small.clone(),
        if shown.abs() < 0.05 { palette.secondary } else { palette.accent },
    );
    ui.painter().text(pos2(cell.center().x, cell.bottom() - 8.0), Align2::CENTER_CENTER, &label, small, palette.secondary);
    let spoken = decibels(shown);
    response.widget_info(|| egui::WidgetInfo::slider(true, shown, format!("{label}, {spoken}")));
    let response = response.on_hover_text(format!("{label}: {spoken}")).on_hover_cursor(egui::CursorIcon::ResizeVertical);
    value.filter(|v| (*v - gain).abs() > 1e-9 || response.drag_stopped()).map(|v| (v, response))
}

fn auto_eq(app: &mut App, ui: &mut Ui) {
    ui.add_space(10.0);
    if let Some(profile) = app.effects.settings.auto_eq.clone() {
        controls::value_row(ui, &t("Profile"), &profile.name);
        ui.label(theme::secondary(ui, &profile.source, Font::Footnote));
        ui.add_space(6.0);
    }
    ui.horizontal_wrapped(|ui| {
        if widgets::action_button(ui, t("Choose headphones"), false).clicked() {
            app.effects.error = None;
            app.effects.browser_open = true;
        }
        let importing = app.effects.importing;
        if ui.add_enabled_ui(!importing, |ui| widgets::action_button(ui, t("Import ParametricEQ file"), true)).inner.clicked() {
            app.effects.error = None;
            effects::import_profile(app);
        }
        if importing {
            ui.spinner();
        }
    });
}

/// The active equalizer's response from 20 Hz to 20 kHz on a logarithmic axis.
fn response_curve(ui: &mut Ui, settings: &AudioEffectsSettings) {
    let palette = Palette::of_ui(ui);
    let frequencies = log_frequencies(160, 20.0, 20_000.0);
    let gains = settings.response_curve(48_000.0, &frequencies);
    let extent = gains.iter().fold(6.0f64, |m, g| m.max(g.abs())).ceil();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 170.0), Sense::hover());
    let plot = Rect::from_min_max(pos2(rect.left() + 44.0, rect.top() + 6.0), pos2(rect.right() - 6.0, rect.bottom() - 20.0));
    let x = |f: f64| plot.left() + plot.width() * ((f / 20.0).log10() / 3.0) as f32;
    let y = |g: f64| egui::remap(g as f32, -extent as f32..=extent as f32, plot.bottom()..=plot.top());
    let grid = Stroke::new(1.0, palette.secondary.gamma_multiply(0.2));
    let small = FontId::new(10.5, egui::FontFamily::Proportional);
    for hertz in [20.0, 100.0, 1_000.0, 10_000.0] {
        ui.painter().line_segment([pos2(x(hertz), plot.top()), pos2(x(hertz), plot.bottom())], grid);
        ui.painter().text(pos2(x(hertz), plot.bottom() + 4.0), Align2::CENTER_TOP, frequency(hertz), small.clone(), palette.secondary);
    }
    for gain in [-extent, 0.0, extent] {
        let stroke = if gain == 0.0 { Stroke::new(1.0, palette.secondary.gamma_multiply(0.45)) } else { grid };
        ui.painter().line_segment([pos2(plot.left(), y(gain)), pos2(plot.right(), y(gain))], stroke);
        let label = if gain == 0.0 { "0 dB".to_string() } else { decibels(gain) };
        ui.painter().text(pos2(plot.left() - 6.0, y(gain)), Align2::RIGHT_CENTER, label, small.clone(), palette.secondary);
    }
    let points: Vec<egui::Pos2> = frequencies.iter().zip(&gains).map(|(f, g)| pos2(x(*f), y(*g))).collect();
    ui.painter().add(Shape::line(points, Stroke::new(2.0, palette.accent)));
}

fn loudness(app: &mut App, ui: &mut Ui) {
    widgets::section_heading(ui, &t("Loudness"), None);
    widgets::card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(theme::text(t("ReplayGain"), Font::Body));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                for mode in ReplayGainMode::ALL.into_iter().rev() {
                    if widgets::chip(ui, app.effects.settings.replay_gain == mode, &t(mode.title())).clicked()
                        && app.effects.settings.replay_gain != mode
                    {
                        app.effects.settings.replay_gain = mode;
                        effects::changed(app);
                    }
                }
            });
        });
        if app.effects.settings.replay_gain != ReplayGainMode::Off {
            ui.add_space(8.0);
            let (low, high) = AudioEffectsSettings::PREAMP_RANGE;
            let preamp = app.effects.settings.replay_gain_preamp;
            controls::value_row(ui, &t("Preamp"), &decibels(preamp));
            let output = controls::thin_slider(ui, preamp, low..=high, ui.available_width(), true, 0.5, &t("Preamp"));
            if let Some(value) = output.changed {
                app.effects.settings.replay_gain_preamp = (value * 2.0).round() / 2.0;
                commit(app, &output.response);
            } else if output.response.drag_stopped() {
                effects::changed(app);
            }
            ui.add_space(6.0);
            let mut prevent = app.effects.settings.prevent_clipping;
            if controls::switch_row(ui, &mut prevent, &t("Prevent clipping")).changed() {
                app.effects.settings.prevent_clipping = prevent;
                effects::changed(app);
            }
        }
    });
    footer(
        ui,
        &t(
            "ReplayGain evens out loudness using tags your server reports, such as Navidrome with OpenSubsonic. Automatic uses album gain while an album plays in order and track gain otherwise. Tracks without tags play unchanged.",
        ),
    );
}

fn transitions(app: &mut App, ui: &mut Ui) {
    widgets::section_heading(ui, &t("Transitions"), None);
    widgets::card(ui, |ui| {
        let mut gapless = app.settings.gapless;
        if controls::switch_row(ui, &mut gapless, &t("Gapless playback")).changed() {
            app.settings.gapless = gapless;
            effects::transitions_changed(app);
        }
        ui.add_space(6.0);
        let mut crossfade = app.settings.crossfade;
        if controls::switch_row(ui, &mut crossfade, &t("Crossfade")).changed() {
            app.settings.crossfade = crossfade;
            effects::transitions_changed(app);
        }
        if app.settings.crossfade {
            let seconds = app.settings.crossfade_seconds.clamp(1.0, 12.0) as f64;
            let output = controls::thin_slider(ui, seconds, 1.0..=12.0, ui.available_width(), false, 1.0, &t("Crossfade"));
            let shown = output.shown.round();
            ui.label(theme::secondary(ui, l10n::count("%lld seconds", shown as i64), Font::Footnote));
            if let Some(value) = output.changed {
                app.settings.crossfade_seconds = value.round().clamp(1.0, 12.0) as f32;
                effects::transitions_changed(app);
            }
            ui.add_space(6.0);
            let mut keep = app.settings.keep_album_gapless;
            if controls::switch_row(ui, &mut keep, &t("Keep album tracks gapless")).changed() {
                app.settings.keep_album_gapless = keep;
                effects::transitions_changed(app);
            }
        }
    });
    footer(
        ui,
        &t(
            "Gapless playback prepares the next song 30 seconds before the end so it starts without a pause. Crossfade overlaps songs that advance automatically; skipping, Repeat One, podcasts and very short songs do not crossfade. Songs converted to MP3 by the server can keep a short silence at their edges.",
        ),
    );
}

fn volume_limit(app: &mut App, ui: &mut Ui) {
    widgets::section_heading(ui, &t("Volume limit"), None);
    widgets::card(ui, |ui| {
        let limit = app.effects.settings.volume_limit;
        let (low, high) = AudioEffectsSettings::VOLUME_LIMIT_RANGE;
        let output = controls::thin_slider(ui, limit, low..=high, ui.available_width(), true, 1.0, &t("Maximum level"));
        let shown = output.shown.round();
        let value = if shown == 0.0 { t("No limit") } else { decibels(shown) };
        controls::value_row(ui, &t("Maximum level"), &value);
        if let Some(value) = output.changed {
            app.effects.settings.volume_limit = value.round().clamp(low, high);
            commit(app, &output.response);
        } else if output.response.drag_stopped() {
            effects::changed(app);
        }
    });
    footer(
        ui,
        &t(
            "Lowers Discophile's output at every volume step, including audiobooks, for sensitive headphones. A limiter also keeps equalizer and ReplayGain boosts from clipping.",
        ),
    );
}
