//! Player controls shared by the player bar, Now Playing, the queue and the sound settings:
//! the play button with its buffering ring, compact sliders, round tool buttons and switches.

use std::f32::consts::TAU;

use discophile_core::l10n::{self, t};
use egui::{Align2, Color32, CornerRadius, FontId, Rect, Response, Sense, Shape, Stroke, Ui, Vec2, pos2, vec2};

use crate::app::App;
use crate::theme::{self, Font, Palette, icons};

/// Whether the transport has something to control (playing, paused or a restored queue).
pub fn has_content(app: &App) -> bool {
    app.player.mode != crate::player::Mode::Idle || crate::player::has_pending_resume(app)
}

/// The play/pause button: a filled circle with a thin spinning ring while buffering. The ring is drawn in
/// space reserved around the circle, so buffering never moves the layout.
pub fn play_button(ui: &mut Ui, app: &App, diameter: f32) -> Response {
    let status = &app.player.status;
    let active = status.playing || status.buffering;
    let buffering = status.buffering;
    let enabled = has_content(app) && ui.is_enabled();
    let side = diameter + 10.0;
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(side), if enabled { Sense::click() } else { Sense::hover() });
    let palette = Palette::of_ui(ui);
    let center = rect.center();
    let fill = if !enabled {
        palette.secondary.gamma_multiply(0.35)
    } else if response.hovered() {
        palette.action.gamma_multiply(1.15)
    } else {
        palette.action
    };
    let scale = if response.is_pointer_button_down_on() { 0.95 } else { 1.0 };
    ui.painter().circle_filled(center, diameter / 2.0 * scale, fill);
    let glyph = if active { icons::PAUSE } else { icons::PLAY };
    let offset = if active { 0.0 } else { diameter * 0.03 };
    crate::ui::widgets::paint_icon(ui.painter(), center + vec2(offset, 0.0), glyph, FontId::new(diameter * 0.44 * scale, theme::icon_fill()), Color32::WHITE);
    if buffering {
        let time = ui.input(|i| i.time) as f32;
        let start = (time / 0.9).fract() * TAU;
        arc(ui, center, diameter / 2.0 + 3.5, start, start + 0.28 * TAU, Stroke::new(2.5, palette.accent));
        ui.ctx().request_repaint();
    }
    let label = t(if active { "Pause" } else { "Play" });
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, &label));
    let response =
        if buffering { response.on_hover_text(format!("{label} · {}", t("Buffering…"))) } else { response.on_hover_text(&label) };
    if enabled { response.on_hover_cursor(egui::CursorIcon::PointingHand) } else { response }
}

/// A circular arc from `start` to `end` radians.
pub fn arc(ui: &Ui, center: egui::Pos2, radius: f32, start: f32, end: f32, stroke: Stroke) {
    let steps = 24;
    let points: Vec<egui::Pos2> = (0..=steps)
        .map(|i| {
            let angle = start + (end - start) * i as f32 / steps as f32;
            center + vec2(angle.cos(), angle.sin()) * radius
        })
        .collect();
    ui.painter().add(Shape::line(points, stroke));
}

/// A borderless transport icon (previous, next, shuffle, repeat). `active` tints it and adds a dot below.
pub fn transport_button(ui: &mut Ui, glyph: &str, label: &str, size: f32, enabled: bool, active: bool) -> Response {
    let palette = Palette::of_ui(ui);
    let response =
        ui.add_enabled_ui(enabled, |ui| crate::ui::widgets::icon_button_tinted(ui, glyph, label, size, active.then_some(palette.accent)));
    let response = response.inner;
    if active && enabled {
        let dot = pos2(response.rect.center().x, response.rect.bottom() - 2.0);
        ui.painter().circle_filled(dot, 2.0, palette.accent);
    }
    response
}

/// The round tool button under the iOS player's controls: a surface circle with an outline, tinted and
/// badged with a check mark when active.
pub fn tool_button(ui: &mut Ui, glyph: &str, label: &str, active: bool) -> Response {
    tool_button_with(ui, glyph, label, active, None)
}

/// A tool button with a small caption under the glyph (sleep timer countdown, speed).
pub fn tool_button_with(ui: &mut Ui, glyph: &str, label: &str, active: bool, caption: Option<&str>) -> Response {
    let diameter = 44.0;
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(diameter + 4.0), Sense::click());
    let palette = Palette::of_ui(ui);
    let enabled = ui.is_enabled();
    let center = rect.center();
    let fill = if response.hovered() && enabled { palette.surface_raised } else { palette.surface };
    ui.painter().circle_filled(center, diameter / 2.0, fill);
    let border = if active { palette.accent } else { palette.secondary.gamma_multiply(0.25) };
    ui.painter().circle_stroke(center, diameter / 2.0 - 0.5, Stroke::new(1.0, border));
    let color = if !enabled {
        palette.secondary.gamma_multiply(0.5)
    } else if active {
        palette.accent
    } else {
        palette.text
    };
    let glyph_center = if caption.is_some() { center - vec2(0.0, 5.0) } else { center };
    let font = if active { FontId::new(20.0, theme::icon_fill()) } else { FontId::proportional(20.0) };
    crate::ui::widgets::paint_icon(ui.painter(), glyph_center, glyph, font, color);
    if let Some(caption) = caption {
        ui.painter().text(center + vec2(0.0, 11.0), Align2::CENTER_CENTER, caption, FontId::new(9.5, theme::semibold()), color);
    }
    if active {
        let badge = center + vec2(diameter / 2.0 - 5.0, -diameter / 2.0 + 5.0);
        ui.painter().circle_filled(badge, 7.0, palette.surface);
        crate::ui::widgets::paint_icon(ui.painter(), badge, icons::CHECK_CIRCLE, FontId::new(13.0, theme::icon_fill()), palette.accent);
    }
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, enabled, active, label));
    let response = response.on_hover_text(label);
    if enabled { response.on_hover_cursor(egui::CursorIcon::PointingHand) } else { response }
}

/// What a slider interaction asks for.
pub struct SliderOutput {
    pub response: Response,
    /// The value to display: the dragged value while scrubbing, otherwise the current value.
    pub shown: f64,
    /// A value to apply now.
    pub changed: Option<f64>,
}

/// A thin rounded slider with a thumb on hover. With `live`, values apply while dragging (volume);
/// otherwise only when released (seeking), so audio isn't reloaded on every movement.
pub fn thin_slider(
    ui: &mut Ui,
    value: f64,
    range: std::ops::RangeInclusive<f64>,
    width: f32,
    live: bool,
    step: f64,
    label: &str,
) -> SliderOutput {
    let enabled = ui.is_enabled() && range.end() > range.start();
    let height = 18.0;
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), if enabled { Sense::click_and_drag() } else { Sense::hover() });
    let (low, high) = (*range.start(), *range.end());
    let span = (high - low).max(f64::EPSILON);
    let rail = Rect::from_center_size(rect.center(), vec2(rect.width() - 8.0, 4.0));
    let mut shown = value.clamp(low, high);
    let mut changed = None;
    if enabled {
        if let Some(pointer) = response.interact_pointer_pos()
            && (response.dragged() || response.is_pointer_button_down_on() || response.clicked() || response.drag_stopped())
        {
            let fraction = ((pointer.x - rail.left()) / rail.width()).clamp(0.0, 1.0) as f64;
            shown = low + fraction * span;
            if live || response.drag_stopped() || response.clicked() {
                changed = Some(shown);
            }
        }
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y + i.smooth_scroll_delta.x);
            if live && scroll.abs() > 0.5 {
                shown = (value + step * scroll.signum() as f64 * 0.5).clamp(low, high);
                changed = Some(shown);
            }
        }
        if response.has_focus() {
            let (left, right) = ui.input(|i| (i.key_pressed(egui::Key::ArrowLeft), i.key_pressed(egui::Key::ArrowRight)));
            if left || right {
                shown = (value + if right { step } else { -step }).clamp(low, high);
                changed = Some(shown);
            }
        }
    }
    let palette = Palette::of_ui(ui);
    let active = response.hovered() || response.dragged() || response.has_focus();
    let rail = if active { rail.expand2(vec2(0.0, 1.0)) } else { rail };
    let fraction = ((shown - low) / span).clamp(0.0, 1.0) as f32;
    let filled = Rect::from_min_max(rail.min, pos2(rail.left() + rail.width() * fraction, rail.bottom()));
    let track = if palette.dark { Color32::from_white_alpha(40) } else { Color32::from_black_alpha(30) };
    ui.painter().rect_filled(rail, CornerRadius::same(3), track);
    let fill = if !enabled {
        palette.secondary.gamma_multiply(0.4)
    } else if active {
        palette.accent
    } else {
        palette.text.gamma_multiply(0.75)
    };
    ui.painter().rect_filled(filled, CornerRadius::same(3), fill);
    if enabled && active {
        let thumb = pos2(filled.right(), rail.center().y);
        ui.painter().circle_filled(thumb, 6.5, palette.surface);
        ui.painter().circle_stroke(thumb, 6.5, Stroke::new(2.0, palette.accent));
    }
    response.widget_info(|| egui::WidgetInfo::slider(enabled, shown, label));
    SliderOutput { response, shown, changed }
}

/// A labeled on/off switch filling the row: the label on the left and the switch on the right.
pub fn switch_row(ui: &mut Ui, on: &mut bool, label: &str) -> Response {
    let mut response = ui
        .horizontal(|ui| {
            ui.label(theme::text(label, Font::Body));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| switch(ui, on, label)).inner
        })
        .inner;
    if response.clicked() {
        response.mark_changed();
    }
    response
}

/// An iOS-style switch.
pub fn switch(ui: &mut Ui, on: &mut bool, label: &str) -> Response {
    let size = vec2(40.0, 24.0);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let palette = Palette::of_ui(ui);
    let progress = ui.ctx().animate_bool_responsive(response.id, *on);
    let off = if palette.dark { Color32::from_white_alpha(45) } else { Color32::from_black_alpha(35) };
    let fill = off.lerp_to_gamma(palette.action, progress);
    ui.painter().rect_filled(rect, CornerRadius::same(12), fill);
    let x = egui::lerp((rect.left() + 12.0)..=(rect.right() - 12.0), progress);
    ui.painter().circle_filled(pos2(x, rect.center().y), 9.5, Color32::WHITE);
    let value = *on;
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, ui.is_enabled(), value, label));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A label with its value at the trailing edge, like SwiftUI's `LabeledContent`.
pub fn value_row(ui: &mut Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(theme::text(label, Font::Body));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add(egui::Label::new(theme::secondary(ui, value, Font::Body)).truncate());
        });
    });
}

/// `+1.5 dB` with the interface language's decimal separator.
pub fn decibels(value: f64) -> String {
    let sign = if value > 0.05 { "+" } else { "" };
    format!("{sign}{} dB", decimal(value, 1))
}

/// `31 Hz`, `1 kHz`, `2.5 kHz`.
pub fn frequency(hertz: f64) -> String {
    if hertz >= 1_000.0 {
        let khz = hertz / 1_000.0;
        let digits = if (khz - khz.round()).abs() < 0.05 { 0 } else { 1 };
        format!("{} kHz", decimal(khz, digits))
    } else {
        format!("{} Hz", hertz.round() as i64)
    }
}

/// `44.1 kHz` from hertz.
pub fn sample_rate(hertz: f64) -> String {
    let khz = hertz / 1_000.0;
    let digits = if (khz - khz.round()).abs() < 0.05 { 0 } else { 1 };
    format!("{} kHz", decimal(khz, digits))
}

pub fn decimal(value: f64, digits: usize) -> String {
    let text = format!("{value:.digits$}");
    let text = if text == "-0" || text == "-0.0" { text.trim_start_matches('-').to_string() } else { text };
    if l10n::language() == l10n::Language::Italian { text.replace('.', ",") } else { text }
}

/// A small rounded label, such as "Live".
pub fn badge(ui: &mut Ui, text: &str, color: Color32) -> Response {
    let galley = ui.painter().layout_no_wrap(text.to_uppercase(), FontId::new(10.5, theme::bold()), color);
    let size = galley.size() + vec2(12.0, 6.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(6), color.gamma_multiply(0.16));
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, color);
    response
}

/// The playback time pair under a timeline: elapsed and remaining (or total) time.
pub fn times(position: f64, duration: Option<f64>, show_remaining: bool) -> (String, String) {
    let elapsed = l10n::time(position);
    let trailing = match duration {
        Some(duration) if show_remaining => format!("−{}", l10n::time((duration - position).max(0.0))),
        Some(duration) => l10n::time(duration),
        None => "--:--".into(),
    };
    (elapsed, trailing)
}
