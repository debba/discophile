//! The persistent player bar along the bottom of the window: what plays (opens Now Playing), favorite,
//! transport above the timeline, and lyrics, queue, volume and More on the right.

#[path = "player/actions.rs"]
pub mod actions;
#[path = "player/colors.rs"]
pub mod colors;
#[path = "player/controls.rs"]
pub mod controls;

use discophile_core::l10n::t;
use egui::{Align, Layout, Rect, Sense, Ui, UiBuilder, pos2, vec2};

use crate::app::App;
use crate::nav::SidePanel;
use crate::player::{self, Mode};
use crate::theme::{self, Font, Palette, icons};
use crate::ui::widgets;

pub fn show(app: &mut App, ui: &mut Ui) {
    let rect = ui.max_rect();
    let compact = rect.width() < 820.0;
    let side = (rect.width() * 0.28).clamp(170.0, 340.0);
    let right_width = if compact { 150.0 } else { side };
    let left = Rect::from_min_max(rect.min, pos2(rect.left() + side, rect.bottom()));
    let right = Rect::from_min_max(pos2(rect.right() - right_width, rect.top()), rect.max);
    let center = Rect::from_min_max(pos2(left.right() + 16.0, rect.top()), pos2(right.left() - 16.0, rect.bottom()));

    let mut child = ui.new_child(UiBuilder::new().max_rect(left).layout(Layout::left_to_right(Align::Center)));
    now_playing_item(app, &mut child);
    let mut child = ui.new_child(UiBuilder::new().max_rect(center).layout(Layout::top_down(Align::Center)));
    transport(app, &mut child);
    let mut child = ui.new_child(UiBuilder::new().max_rect(right).layout(Layout::right_to_left(Align::Center)));
    trailing(app, &mut child, compact);

    let ctx = ui.ctx().clone();
    actions::custom_timer_window(app, &ctx);
}

fn now_playing_item(app: &mut App, ui: &mut Ui) {
    let palette = Palette::of_ui(ui);
    let song = app.player.current();
    let has_item = controls::has_content(app);
    let art = match (&song, app.player.mode) {
        (Some(song), Mode::Music) => widgets::artwork(ui, app, &song.id, song.cover_art.as_deref(), 56.0, 8.0),
        (Some(song), _) => widgets::artwork_with(ui, &song.id, None, 56.0, 8.0),
        (None, Mode::Audiobook) if crate::features::audiobooks::cover(app).is_some() => {
            let texture = crate::features::audiobooks::cover(app);
            widgets::artwork_with(ui, "audiobook", texture, 56.0, 8.0)
        }
        (None, _) => {
            let (rect, response) = ui.allocate_exact_size(vec2(56.0, 56.0), Sense::click());
            ui.painter().rect_filled(rect, egui::CornerRadius::same(8), palette.surface_raised);
            let glyph = if app.player.mode == Mode::Audiobook { icons::BOOK_OPEN } else { icons::MUSIC_NOTE };
            crate::ui::widgets::paint_icon(ui.painter(), rect.center(), glyph, egui::FontId::proportional(24.0), palette.secondary);
            response
        }
    };
    let show_favorite = app.session.client.is_some() && app.player.mode == Mode::Music && song.as_ref().is_some_and(|s| !s.is_podcast());
    let text_width = (ui.available_width() - if show_favorite { 44.0 } else { 4.0 }).max(40.0);
    // The artist opens its page; the rest of the item opens Now Playing.
    let artist_route = song
        .as_ref()
        .filter(|s| app.player.mode == Mode::Music && !s.is_podcast() && !app.player.status.buffering)
        .and_then(|s| crate::features::library::artist_route(app, s.artist_id.as_deref(), s.artist.as_deref()));
    let mut open_artist = None;
    let text = ui
        .allocate_ui_with_layout(vec2(text_width, 56.0), Layout::top_down(Align::Min), |ui| {
            ui.add_space(9.0);
            match actions::now_playing_text(app) {
                Some((title, subtitle)) => {
                    let title = ui.add(egui::Label::new(theme::text(title, Font::Headline)).truncate().sense(Sense::click()));
                    let subtitle = if app.player.status.buffering { t("Buffering…") } else { subtitle };
                    let sub = ui.add(egui::Label::new(theme::secondary(ui, subtitle, Font::Footnote)).truncate().sense(Sense::click()));
                    match artist_route.clone() {
                        Some(route) => {
                            let label = t("Show artist");
                            sub.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Link, true, &label));
                            if sub.on_hover_text(&label).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                open_artist = Some(route);
                            }
                            title
                        }
                        None => title.union(sub),
                    }
                }
                None => ui.add(egui::Label::new(theme::secondary(ui, t("Now playing"), Font::Callout)).selectable(false)),
            }
        })
        .inner;
    if let Some(route) = open_artist {
        app.nav.push(route);
    }
    let open = art.union(text);
    let label = t("Open Now Playing");
    open.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, has_item, &label));
    if has_item && open.on_hover_text(&label).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        app.nav.now_playing = true;
    }
    if show_favorite && let Some(song) = song {
        let starred = actions::is_favorite(app, &song);
        let label = t(if starred { "Remove from favorites" } else { "Add to favorites" });
        let response = if starred {
            let (rect, response) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::click());
            if response.hovered() {
                ui.painter().circle_filled(rect.center(), 16.0, palette.hover);
            }
            crate::ui::widgets::paint_icon(ui.painter(), rect.center(), icons::HEART, egui::FontId::new(18.0, theme::icon_fill()), palette.accent);
            response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, true, &label));
            response.on_hover_text(&label)
        } else {
            widgets::icon_button(ui, icons::HEART, &label, 18.0)
        };
        if response.clicked() {
            actions::toggle_favorite(app, &song);
        }
    }
}

fn transport(app: &mut App, ui: &mut Ui) {
    let live = app.player.is_live();
    let audiobook = app.player.mode == Mode::Audiobook;
    let music = app.player.mode == Mode::Music;
    let enabled = controls::has_content(app);
    let width = ui.available_width();
    ui.spacing_mut().item_spacing = vec2(8.0, 0.0);
    let row = Rect::from_min_size(ui.max_rect().min, vec2(width, 44.0));
    let mut buttons = ui.new_child(UiBuilder::new().max_rect(row).layout(Layout::left_to_right(Align::Center)));
    let total = 4.0 * 32.0 + 46.0 + 4.0 * 8.0;
    buttons.add_space(((width - total) / 2.0).max(0.0));
    let ui_b = &mut buttons;
    if audiobook {
        ui_b.add_space(40.0);
        if controls::transport_button(ui_b, icons::ARROW_COUNTER_CLOCKWISE, &t("Skip back"), 19.0, true, false).clicked() {
            player::previous(app);
        }
    } else {
        let shuffled = app.player.shuffled;
        if controls::transport_button(ui_b, icons::SHUFFLE, &t("Shuffle"), 17.0, music, shuffled && music).clicked() {
            player::toggle_shuffle(app);
        }
        if controls::transport_button(ui_b, icons::SKIP_BACK, &t("Previous track"), 19.0, music, false).clicked() {
            player::previous(app);
        }
    }
    if controls::play_button(ui_b, app, 36.0).clicked() {
        actions::toggle(app);
    }
    if audiobook {
        if controls::transport_button(ui_b, icons::ARROW_CLOCKWISE, &t("Skip forward"), 19.0, true, false).clicked() {
            player::next(app);
        }
    } else {
        if controls::transport_button(ui_b, icons::SKIP_FORWARD, &t("Next track"), 19.0, music, false).clicked() {
            player::next(app);
        }
        let repeat = player::repeat_mode(app) != discophile_core::queue::RepeatMode::Off;
        if controls::transport_button(ui_b, actions::repeat_icon(app), &actions::repeat_label(app), 17.0, music, repeat && music).clicked()
        {
            player::cycle_repeat(app);
        }
    }

    let timeline = Rect::from_min_max(pos2(row.left(), row.bottom() + 2.0), pos2(row.right(), ui.max_rect().bottom()));
    let mut line = ui.new_child(UiBuilder::new().max_rect(timeline).layout(Layout::left_to_right(Align::Center)));
    if live {
        line.add_space((width / 2.0 - 24.0).max(0.0));
        controls::badge(&mut line, &t("Live"), Palette::of_ui(ui).accent);
        return;
    }
    timeline_row(app, &mut line, enabled);
}

/// Elapsed time, the seek slider and the remaining time.
fn timeline_row(app: &mut App, ui: &mut Ui, enabled: bool) {
    let (position, duration) = actions::timeline(app);
    let label_width = 48.0;
    ui.spacing_mut().item_spacing.x = 8.0;
    let slider_width = (ui.available_width() - 2.0 * (label_width + 8.0)).max(60.0);
    let elapsed_rect = ui.allocate_exact_size(vec2(label_width, 18.0), Sense::hover()).0;
    let output = ui.add_enabled_ui(enabled && duration.is_some(), |ui| {
        controls::thin_slider(ui, position, 0.0..=duration.unwrap_or(1.0), slider_width, false, 5.0, &t("Playback position"))
    });
    let output = output.inner;
    let remaining_rect = ui.allocate_exact_size(vec2(label_width, 18.0), Sense::click());
    let show_remaining = remaining_preference(ui);
    let (elapsed, trailing) = controls::times(output.shown, duration, show_remaining);
    let palette = Palette::of_ui(ui);
    let font = egui::FontId::new(11.5, egui::FontFamily::Proportional);
    ui.painter().text(elapsed_rect.right_center(), egui::Align2::RIGHT_CENTER, elapsed, font.clone(), palette.secondary);
    ui.painter().text(remaining_rect.0.left_center(), egui::Align2::LEFT_CENTER, trailing, font, palette.secondary);
    let label = t(if show_remaining { "Time remaining" } else { "Duration" });
    if remaining_rect.1.on_hover_text(label).clicked() {
        set_remaining_preference(ui, !show_remaining);
    }
    if let Some(seconds) = output.changed {
        actions::seek(app, seconds);
    }
}

/// Whether timelines show remaining time (true) or the total duration, shared by the bar and Now Playing.
pub fn remaining_preference(ui: &Ui) -> bool {
    ui.ctx().data_mut(|d| *d.get_persisted_mut_or_insert_with(egui::Id::unique("player-shows-remaining"), || true))
}

pub fn set_remaining_preference(ui: &Ui, value: bool) {
    ui.ctx().data_mut(|d| d.insert_persisted(egui::Id::unique("player-shows-remaining"), value));
}

fn trailing(app: &mut App, ui: &mut Ui, compact: bool) {
    let palette = Palette::of_ui(ui);
    ui.spacing_mut().item_spacing.x = 4.0;
    let more = widgets::icon_button(ui, icons::DOTS_THREE, &t("More playback actions"), 20.0);
    egui::Popup::menu(&more).show(|ui| {
        let song = app.player.current();
        actions::more_menu(ui, app, song.as_ref());
    });
    if !compact {
        let output = controls::thin_slider(ui, app.player.volume as f64, 0.0..=1.0, 96.0, true, 0.05, &t("Volume"));
        if let Some(volume) = output.changed {
            player::set_volume(app, volume as f32);
        }
    }
    let muted = actions::is_muted(app);
    let label = t(if muted { "Unmute" } else { "Mute" });
    if widgets::icon_button(ui, actions::volume_icon(app.player.volume), &label, 19.0).clicked() {
        actions::toggle_mute(app);
    }
    let queue_open = app.nav.side_panel == SidePanel::Queue && !app.nav.now_playing;
    let tint = |on: bool| on.then_some(palette.accent);
    if widgets::icon_button_tinted(ui, icons::QUEUE, &t("Playing next"), 19.0, tint(queue_open)).clicked() {
        if app.nav.now_playing {
            app.nav.now_playing = false;
            app.nav.side_panel = SidePanel::Queue;
        } else {
            app.nav.toggle_panel(SidePanel::Queue);
        }
    }
    let lyrics_open = app.nav.side_panel == SidePanel::Lyrics && !app.nav.now_playing;
    let has_lyrics = crate::features::lyrics::current_song(app).is_some() || app.player.mode == Mode::Idle;
    let lyrics =
        ui.add_enabled_ui(has_lyrics, |ui| widgets::icon_button_tinted(ui, icons::QUOTES, &t("Lyrics"), 19.0, tint(lyrics_open))).inner;
    if lyrics.clicked() {
        if app.nav.now_playing {
            app.nav.now_playing = false;
            app.nav.side_panel = SidePanel::Lyrics;
        } else {
            app.nav.toggle_panel(SidePanel::Lyrics);
        }
    }
    if let Some(caption) = actions::sleep_caption(app) {
        ui.add_space(4.0);
        ui.label(theme::accent(ui, format!("{} {caption}", icons::MOON), Font::Caption)).on_hover_text(t("Sleep timer"));
    }
}
