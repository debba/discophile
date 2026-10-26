use crate::{
    app::App,
    features::connect::services as connect,
    player,
    theme::{self, Font, Palette, icons},
    ui::{player_bar::controls, widgets},
};
use discophile_core::l10n::tf;
use egui::{Align, Layout, Rect, Ui, UiBuilder, pos2, vec2};

pub fn status(ui: &mut Ui, caption: &str) {
    let response = ui.add(egui::Label::new(theme::text(caption, Font::Footnote).color(Palette::of_ui(ui).accent)).truncate());
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_role(egui::Role::Status);
        node.set_live(egui::accesskit::Live::Polite);
        node.set_live_atomic();
    });
    response.on_hover_text(caption);
}

pub fn show(app: &mut App, ui: &mut Ui) -> bool {
    let Some(remote) = connect::remote_playback(app) else { return false };
    let caption = connect::transfer_status(app).unwrap_or_else(|| {
        tf(
            if !remote.online {
                "%@ is unavailable. Choose another playback device."
            } else if remote.state.playing {
                "Playing on %@"
            } else {
                "Paused on %@"
            },
            &[&remote.name],
        )
    });
    status(ui, &caption);
    let rect = ui.available_rect_before_wrap();
    let compact = rect.width() < 720.0;
    let left_width = (rect.width() * 0.28).clamp(100.0, 300.0);
    let right_width = if compact { 44.0 } else { 160.0 };
    let left = Rect::from_min_max(rect.min, pos2(rect.left() + left_width, rect.bottom()));
    let right = Rect::from_min_max(pos2(rect.right() - right_width, rect.top()), rect.max);
    let center = Rect::from_min_max(pos2(left.right() + 12.0, rect.top()), pos2(right.left() - 12.0, rect.bottom()));
    let mut text = ui.new_child(UiBuilder::new().max_rect(left).layout(Layout::top_down(Align::Min)));
    text.add_space(12.0);
    text.add(egui::Label::new(theme::text(&remote.state.title, Font::Headline)).truncate())
        .on_hover_text(&remote.state.title);
    text.add(egui::Label::new(theme::secondary(&text, &remote.name, Font::Footnote)).truncate())
        .on_hover_text(&remote.name);
    let mut transport = ui.new_child(UiBuilder::new().max_rect(center).layout(Layout::top_down(Align::Center)));
    transport.spacing_mut().item_spacing.y = 0.0;
    transport.horizontal(|ui| {
        ui.add_space(((center.width() - 148.0) / 2.0).max(0.0));
        ui.spacing_mut().item_spacing.x = 8.0;
        if remote_button(ui, icons::SKIP_BACK, &tf("Previous on %@", &[&remote.name]), remote.online).clicked() {
            player::previous(app);
        }
        let label = tf(if remote.state.playing { "Pause on %@" } else { "Play on %@" }, &[&remote.name]);
        let play = ui.add_enabled_ui(remote.online, |ui| controls::play_button(ui, app, 34.0)).inner;
        play.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, remote.online && ui.is_enabled(), &label));
        widgets::focus_ring(ui, &play, 22);
        if play.clicked() {
            player::toggle(app);
        }
        if remote_button(ui, icons::SKIP_FORWARD, &tf("Next on %@", &[&remote.name]), remote.online).clicked() {
            player::next(app);
        }
    });
    transport.add_space(2.0);
    transport.horizontal(|ui| {
        let duration = remote.state.duration.filter(|d| *d > 0.0);
        ui.label(theme::secondary(ui, discophile_core::l10n::time(remote.state.position), Font::Caption));
        let width = (center.width() - 88.0).max(60.0);
        let output = ui
            .add_enabled_ui(remote.online && duration.is_some(), |ui| {
                controls::thin_slider(
                    ui,
                    remote.state.position,
                    0.0..=duration.unwrap_or(1.0),
                    width,
                    false,
                    5.0,
                    &tf("Playback position on %@", &[&remote.name]),
                )
            })
            .inner;
        if let Some(seconds) = output.changed {
            player::seek(app, seconds);
        }
        if let Some(duration) = duration {
            ui.label(theme::secondary(ui, discophile_core::l10n::time(duration), Font::Caption));
        }
    });
    let mut trailing = ui.new_child(UiBuilder::new().max_rect(right).layout(Layout::right_to_left(Align::Center)));
    super::services::player_devices(app, &mut trailing);
    if !compact {
        let output = trailing
            .add_enabled_ui(remote.online, |ui| {
                controls::thin_slider(
                    ui,
                    remote.state.volume as f64,
                    0.0..=1.0,
                    96.0,
                    true,
                    0.05,
                    &tf("Volume on %@", &[&remote.name]),
                )
            })
            .inner;
        if let Some(volume) = output.changed {
            player::set_volume(app, volume as f32);
        }
    }
    if remote.online && remote.state.playing {
        ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
    }
    true
}

fn remote_button(ui: &mut Ui, icon: &str, label: &str, enabled: bool) -> egui::Response {
    let response = ui.add_enabled(enabled, egui::Button::new(theme::icon(icon, 20.0)).frame(false).min_size(vec2(44.0, 44.0)));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled && ui.is_enabled(), label));
    widgets::focus_ring(ui, &response, 8);
    response.on_hover_text(label)
}

#[cfg(test)]
mod tests;
