use crate::{
    app::App,
    features::connect::services as connect,
    player,
    theme::{self, Font, Palette, icons},
    ui::{player_bar::controls, widgets},
};
use discophile_core::Song;
use discophile_core::l10n::{t, tf};
use egui::{Align, CornerRadius, Layout, Rect, Sense, Ui, UiBuilder, pos2, vec2};

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
    // Artwork, title and artist of the remote item; the whole item opens the remote Now Playing.
    let mut item = ui.new_child(UiBuilder::new().max_rect(left).layout(Layout::left_to_right(Align::Center)));
    let texture = artwork_texture(app, remote.state.media.as_ref());
    let art = widgets::artwork_with(&mut item, &seed(&remote), texture, 56.0, 8.0);
    item.add_space(10.0);
    let (title, subtitle) = titles(&remote);
    let text = item
        .allocate_ui_with_layout(vec2(item.available_width().max(40.0), 56.0), Layout::top_down(Align::Min), |ui| {
            ui.add_space(9.0);
            let title = ui.add(egui::Label::new(theme::text(&title, Font::Headline)).truncate().sense(Sense::click()));
            let sub = ui.add(egui::Label::new(theme::secondary(ui, &subtitle, Font::Footnote)).truncate().sense(Sense::click()));
            title.union(sub)
        })
        .inner;
    let open = art.union(text);
    let label = t("Open Now Playing");
    open.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
    if open.on_hover_text(&label).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        app.nav.now_playing = true;
    }
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

/// The remote item's title and its artist (or the device name when the sender shares no details).
fn titles(remote: &connect::RemotePlayback) -> (String, String) {
    match &remote.state.media {
        Some(media) if !media.title.is_empty() => {
            let subtitle = match media.kind.as_str() {
                "radio" => t("Live radio"),
                _ => media.artist.clone().unwrap_or_else(|| remote.name.clone()),
            };
            (media.title.clone(), subtitle)
        }
        _ => (remote.state.title.clone(), remote.name.clone()),
    }
}

fn seed(remote: &connect::RemotePlayback) -> String {
    remote.state.media.as_ref().and_then(|m| m.item_id.clone()).unwrap_or_else(|| remote.state.title.clone())
}

/// Artwork for what another device plays, loaded with this computer's own credentials when it is signed in to the
/// same music library or Audiobookshelf account. Nothing is fetched from the other device.
pub fn artwork_texture(app: &mut App, media: Option<&connect::PresenceMedia>) -> Option<egui::TextureHandle> {
    let media = media?;
    match media.kind.as_str() {
        "music" | "podcast" => {
            if media.account_key.as_deref() != Some(app.session.account_key().as_str()) {
                return None;
            }
            app.artwork_texture(media.cover_art.as_deref()?)
        }
        "audiobook" => {
            let account = app.audiobooks.account.as_ref()?.id();
            if media.account_key.as_deref() != Some(account.as_str()) {
                return None;
            }
            crate::features::audiobooks::covers::texture(app, media.item_id.as_deref()?)
        }
        _ => None,
    }
}

/// Now Playing for another device: its artwork, details, timeline, transport and volume, with handoff.
pub fn now_playing(app: &mut App, ui: &mut Ui) -> bool {
    let Some(remote) = connect::remote_playback(app) else { return false };
    let rect = ui.max_rect();
    let media = remote.state.media.clone();
    let same_library = media.as_ref().is_some_and(|m| {
        matches!(m.kind.as_str(), "music" | "podcast") && m.account_key.as_deref() == Some(app.session.account_key().as_str())
    });
    let song = Song {
        cover_art: media.as_ref().filter(|_| same_library).and_then(|m| m.cover_art.clone()),
        ..Song::new(seed(&remote), "")
    };
    let corners = crate::ui::player_bar::colors::colors(app, &song);
    crate::ui::player_bar::colors::paint_background(ui, rect, corners);

    let top = Rect::from_min_size(rect.min + vec2(16.0, 10.0), vec2(rect.width() - 32.0, 40.0));
    let mut bar = ui.new_child(UiBuilder::new().max_rect(top).layout(Layout::left_to_right(Align::Center)));
    if widgets::icon_button(&mut bar, icons::CARET_DOWN, &t("Close player"), 22.0).clicked() {
        app.nav.now_playing = false;
        return true;
    }
    bar.add_space(6.0);
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
    bar.add(egui::Label::new(theme::accent(&bar, &caption, Font::Callout)).truncate());

    let content = Rect::from_min_max(pos2(rect.left() + 24.0, top.bottom() + 6.0), pos2(rect.right() - 24.0, rect.bottom() - 12.0));
    let width = content.width().min(560.0);
    let column = Rect::from_center_size(content.center(), vec2(width, content.height()));
    let mut ui = ui.new_child(UiBuilder::new().max_rect(column).layout(Layout::top_down(Align::Center)));
    let ui = &mut ui;
    egui::ScrollArea::vertical().id_salt("remote-now-playing").auto_shrink([false, false]).show(ui, |ui| {
        ui.set_width(width);
        ui.vertical_centered(|ui| {
            let size = (ui.available_height() - 360.0).min(width).clamp(140.0, 520.0);
            let (art, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
            let radius = (size * 0.035).clamp(8.0, 16.0);
            let shadow = egui::Shadow { offset: [0, 10], blur: 34, spread: 0, color: egui::Color32::from_black_alpha(90) };
            ui.painter().add(shadow.as_shape(art, CornerRadius::same(radius as u8)));
            let texture = artwork_texture(app, media.as_ref());
            let placeholder = match media.as_ref().map(|m| m.kind.as_str()) {
                Some("audiobook") => icons::BOOK_OPEN,
                Some("radio") => icons::RADIO,
                _ => icons::MUSIC_NOTE,
            };
            widgets::paint_artwork_or(ui, art, &seed(&remote), texture.as_ref(), radius, placeholder);
            ui.add_space(14.0);
            let (title, subtitle) = titles(&remote);
            ui.add(egui::Label::new(theme::text(title, Font::Title)).wrap().halign(Align::Center));
            ui.add(egui::Label::new(theme::text(subtitle, Font::Title3).color(Palette::of_ui(ui).accent)).truncate());
            if let Some(album) = media.as_ref().and_then(|m| m.album.clone()).filter(|a| !a.is_empty()) {
                ui.add(egui::Label::new(theme::secondary(ui, album, Font::Subheadline)).truncate());
            }
            ui.add_space(10.0);

            let duration = remote.state.duration.filter(|d| *d > 0.0);
            let output = ui
                .add_enabled_ui(remote.online && duration.is_some(), |ui| {
                    controls::thin_slider(
                        ui,
                        remote.state.position,
                        0.0..=duration.unwrap_or(1.0),
                        width,
                        false,
                        10.0,
                        &tf("Playback position on %@", &[&remote.name]),
                    )
                })
                .inner;
            if let Some(seconds) = output.changed {
                player::seek(app, seconds);
            }
            let row = ui.allocate_exact_size(vec2(width, 22.0), Sense::hover()).0;
            let font = egui::FontId::new(12.5, theme::semibold());
            let palette = Palette::of_ui(ui);
            let painter = ui.painter();
            painter.text(row.left_center(), egui::Align2::LEFT_CENTER, discophile_core::l10n::time(output.shown), font.clone(), palette.secondary);
            if let Some(duration) = duration {
                let remaining = format!("−{}", discophile_core::l10n::time((duration - output.shown).max(0.0)));
                painter.text(row.right_center(), egui::Align2::RIGHT_CENTER, remaining, font, palette.secondary);
            }
            ui.add_space(6.0);

            ui.horizontal(|ui| {
                ui.add_space(((width - 196.0) / 2.0).max(0.0));
                ui.spacing_mut().item_spacing.x = 16.0;
                if remote_button(ui, icons::SKIP_BACK, &tf("Previous on %@", &[&remote.name]), remote.online).clicked() {
                    player::previous(app);
                }
                let label = tf(if remote.state.playing { "Pause on %@" } else { "Play on %@" }, &[&remote.name]);
                let play = ui.add_enabled_ui(remote.online, |ui| controls::play_button(ui, app, 56.0)).inner;
                play.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, remote.online && ui.is_enabled(), &label));
                widgets::focus_ring(ui, &play, 33);
                if play.on_hover_text(&label).clicked() {
                    player::toggle(app);
                }
                if remote_button(ui, icons::SKIP_FORWARD, &tf("Next on %@", &[&remote.name]), remote.online).clicked() {
                    player::next(app);
                }
            });
            ui.add_space(8.0);
            let volume = ui
                .add_enabled_ui(remote.online, |ui| {
                    controls::thin_slider(ui, remote.state.volume as f64, 0.0..=1.0, width * 0.6, true, 0.05, &tf("Volume on %@", &[&remote.name]))
                })
                .inner;
            if let Some(volume) = volume.changed {
                player::set_volume(app, volume as f32);
            }
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                let buttons = 2.0 * 220.0 + 12.0;
                ui.add_space(((width - buttons) / 2.0).max(0.0));
                let bring = ui.add_enabled(
                    remote.online && remote.state.can_handoff && connect::can_transfer(app),
                    egui::Button::new(theme::text(t("Continue here"), Font::Callout)).min_size(vec2(220.0, 36.0)),
                );
                if bring.clicked() {
                    connect::request_transfer(app, remote.id);
                }
                if ui
                    .add(egui::Button::new(theme::text(t("Listen independently here"), Font::Callout)).min_size(vec2(220.0, 36.0)))
                    .clicked()
                {
                    connect::listen_independently(app);
                    app.nav.now_playing = false;
                }
            });
        });
    });
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
