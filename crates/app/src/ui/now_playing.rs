//! Now Playing, inside the content area. It follows the iOS player: large artwork over a background tinted
//! by it, title and links, the timeline, transport, an inline volume slider and the round tool buttons.
//! Wide windows show lyrics or the queue beside the player; narrow ones show them in place of the artwork.

use discophile_core::Song;
use discophile_core::l10n::{self, t};
use egui::{Align, CornerRadius, Id, Layout, Rect, Sense, Ui, UiBuilder, pos2, vec2};

use crate::app::App;
use crate::nav::Route;
use crate::player::{self, Mode};
use crate::theme::{self, Font, Palette, icons};
use crate::ui::player_bar::{actions, colors, controls};
use crate::ui::widgets;

/// What shows beside (or instead of) the artwork.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Inline {
    None,
    Lyrics,
    Queue,
    /// About the current song (or what was last chosen), see `features::info`.
    Info,
}

fn inline_id() -> Id {
    Id::unique("now-playing-inline")
}

/// Shows lyrics or the queue inside Now Playing.
pub fn show_inline(app: &mut App, inline: Inline) {
    app.ctx().data_mut(|d| d.insert_temp(inline_id(), inline));
}

fn current_inline(app: &App, wide: bool) -> Inline {
    let stored = app.ctx().data_mut(|d| d.get_temp::<Inline>(inline_id()));
    let inline = stored.unwrap_or(if wide { Inline::Lyrics } else { Inline::None });
    if inline == Inline::Lyrics && crate::features::lyrics::current_song(app).is_none() {
        // Radio, podcasts and audiobooks have no lyrics; keep the choice for the next song.
        return if wide && app.player.mode == Mode::Music { Inline::Queue } else { Inline::None };
    }
    if inline == Inline::Queue && app.player.mode != Mode::Music {
        return Inline::None;
    }
    if inline == Inline::Info && app.info.view.is_none() {
        return Inline::None;
    }
    inline
}

fn toggle_inline(app: &mut App, wide: bool, inline: Inline) {
    let next = if current_inline(app, wide) == inline { Inline::None } else { inline };
    show_inline(app, next);
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    if escape
        && ctx.memory(|memory| memory.top_modal_layer().is_none())
        && !ctx.egui_wants_keyboard_input()
        && !egui::Popup::is_any_open(&ctx)
        && app.effects.details.is_none()
        && !app.effects.browser_open
    {
        app.nav.now_playing = false;
        return;
    }
    let rect = ui.max_rect();
    let song = app.player.current();
    let corners = match &song {
        Some(song) if app.player.mode == Mode::Music => colors::colors(app, song),
        Some(song) => colors::colors(app, &Song { cover_art: None, ..song.clone() }),
        None => colors::colors(app, &Song::new("audiobook", "")),
    };
    colors::paint_background(ui, rect, corners);

    let top = Rect::from_min_size(rect.min + vec2(16.0, 10.0), vec2(rect.width() - 32.0, 40.0));
    let mut bar = ui.new_child(UiBuilder::new().max_rect(top).layout(Layout::left_to_right(Align::Center)));
    if widgets::icon_button(&mut bar, icons::CARET_DOWN, &t("Close player"), 22.0).clicked() {
        app.nav.now_playing = false;
        return;
    }
    if let Some(label) = crate::ui::queue::context_label(app) {
        bar.add_space(6.0);
        bar.add(egui::Label::new(theme::accent(&bar, label, Font::Callout)).truncate());
    }

    let content = Rect::from_min_max(pos2(rect.left() + 24.0, top.bottom() + 6.0), pos2(rect.right() - 24.0, rect.bottom() - 12.0));
    if !controls::has_content(app) {
        let mut child = ui.new_child(UiBuilder::new().max_rect(content));
        widgets::empty_state(
            &mut child,
            icons::MUSIC_NOTES,
            &t("Now playing"),
            &t("Choose something from your library to start listening."),
        );
        return;
    }
    let wide = content.width() >= 940.0;
    let inline = current_inline(app, wide);
    if wide && inline != Inline::None {
        let left_width = (content.width() * 0.44).clamp(380.0, 540.0);
        let left = Rect::from_min_size(content.min, vec2(left_width, content.height()));
        let right = Rect::from_min_max(pos2(left.right() + 32.0, content.top()), content.max);
        let mut column = ui.new_child(UiBuilder::new().max_rect(left).layout(Layout::top_down(Align::Center)));
        player_column(app, &mut column, None, wide);
        let mut side = ui.new_child(UiBuilder::new().max_rect(right).layout(Layout::top_down(Align::Min)));
        inline_panel(app, &mut side, inline, wide);
    } else {
        let width = content.width().min(560.0);
        let column_rect = Rect::from_center_size(pos2(content.center().x, content.center().y), vec2(width, content.height()));
        let mut column = ui.new_child(UiBuilder::new().max_rect(column_rect).layout(Layout::top_down(Align::Center)));
        player_column(app, &mut column, (inline != Inline::None).then_some(inline), wide);
    }
}

/// Space the controls under the artwork need.
const CONTROLS_HEIGHT: f32 = 420.0;

fn player_column(app: &mut App, ui: &mut Ui, replace: Option<Inline>, wide: bool) {
    let width = ui.available_width();
    let height = ui.available_height();
    let media = (height - CONTROLS_HEIGHT).max(140.0);
    egui::ScrollArea::vertical().id_salt("now-playing-column").auto_shrink([false, false]).show(ui, |ui| {
        ui.set_width(width);
        ui.vertical_centered(|ui| {
            match replace {
                Some(inline) => {
                    let rect = ui.allocate_exact_size(vec2(width, media), Sense::hover()).0;
                    let mut child = ui.new_child(UiBuilder::new().max_rect(rect).layout(Layout::top_down(Align::Min)));
                    inline_panel(app, &mut child, inline, wide);
                }
                None => {
                    let size = media.min(width).clamp(140.0, 560.0);
                    artwork(app, ui, size);
                }
            }
            ui.add_space(14.0);
            titles(app, ui);
            ui.add_space(10.0);
            timeline(app, ui, width, wide);
            ui.add_space(6.0);
            transport(app, ui, width);
            ui.add_space(8.0);
            volume(app, ui, width);
            ui.add_space(12.0);
            tools(app, ui, width, wide);
        });
    });
}

fn artwork(app: &mut App, ui: &mut Ui, size: f32) {
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    let radius = (size * 0.035).clamp(8.0, 16.0);
    let corner = CornerRadius::same(radius as u8);
    let shadow = egui::Shadow { offset: [0, 10], blur: 34, spread: 0, color: egui::Color32::from_black_alpha(90) };
    ui.painter().add(shadow.as_shape(rect, corner));
    let song = app.player.current();
    let texture = match (&song, app.player.mode) {
        (Some(song), Mode::Music) => song.cover_art.as_deref().and_then(|cover| app.artwork_texture(cover)),
        (_, Mode::Audiobook) => crate::features::audiobooks::cover(app),
        _ => None,
    };
    let seed = song.as_ref().map(|s| s.id.clone()).unwrap_or_else(|| "audiobook".into());
    widgets::paint_artwork(ui, rect, &seed, texture.as_ref(), radius);
    if app.player.mode == Mode::Radio {
        let palette = Palette::of_ui(ui);
        let badge = Rect::from_min_size(rect.left_top() + vec2(12.0, 12.0), vec2(0.0, 0.0));
        let galley = ui.painter().layout_no_wrap(t("Live").to_uppercase(), egui::FontId::new(12.0, theme::bold()), egui::Color32::WHITE);
        let pill = Rect::from_min_size(badge.min, galley.size() + vec2(14.0, 8.0));
        ui.painter().rect_filled(pill, CornerRadius::same(8), palette.action);
        ui.painter().galley(pill.center() - galley.size() / 2.0, galley, egui::Color32::WHITE);
    }
    if let Some(song) = song.filter(|_| app.player.mode == Mode::Music)
        && crate::features::downloads::is_downloaded(app, &song)
    {
        let center = rect.right_bottom() - vec2(22.0, 22.0);
        ui.painter().circle_filled(center, 15.0, egui::Color32::from_black_alpha(165));
        crate::ui::widgets::paint_icon(ui.painter(), center, icons::ARROW_CIRCLE_DOWN, egui::FontId::new(20.0, theme::icon_fill()), egui::Color32::WHITE);
        ui.interact(Rect::from_center_size(center, vec2(30.0, 30.0)), Id::unique("np-downloaded"), Sense::hover())
            .on_hover_text(t("Playing from downloads"));
    }
    if response.double_clicked() {
        actions::toggle(app);
    }
}

fn titles(app: &mut App, ui: &mut Ui) {
    let palette = Palette::of_ui(ui);
    let Some((title, subtitle)) = actions::now_playing_text(app) else { return };
    ui.add(egui::Label::new(theme::text(title, Font::Title)).wrap().halign(Align::Center));
    let song = app.player.current().filter(|_| app.player.mode == Mode::Music);
    let Some(song) = song else {
        ui.label(theme::secondary(ui, subtitle, Font::Title3));
        if let Some(id) = crate::features::audiobooks::playing_book(app) {
            let link = ui.add(egui::Label::new(theme::accent(ui, t("Open book"), Font::Subheadline)).sense(Sense::click()));
            if link.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                app.nav.push(Route::Audiobook(id));
            }
        }
        return;
    };
    let artist = song.artist.clone().unwrap_or_else(|| t("Unknown artist"));
    let artist_text = theme::text(artist, Font::Title3).color(palette.accent);
    let route = if song.is_podcast() { None } else { crate::features::library::artist_route(app, song.artist_id.as_deref(), song.artist.as_deref()) };
    match route {
        Some(route) => {
            if ui
                .add(egui::Label::new(artist_text).sense(Sense::click()).truncate())
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                app.nav.push(route);
            }
        }
        None => {
            ui.add(egui::Label::new(artist_text).truncate());
        }
    }
    if let Some(album) = song.album.clone().filter(|a| !a.is_empty()) {
        let year = song.year.filter(|y| *y > 0).map(|y| format!(" · {y}")).unwrap_or_default();
        let text = theme::secondary(ui, format!("{album}{year}"), Font::Subheadline);
        match song.album_id.clone() {
            Some(id) => {
                let response =
                    ui.add(egui::Label::new(text).sense(Sense::click()).truncate()).on_hover_cursor(egui::CursorIcon::PointingHand);
                if response.clicked() {
                    app.nav.push(Route::Album(id));
                }
            }
            None => {
                ui.add(egui::Label::new(text).truncate());
            }
        }
    }
}

fn timeline(app: &mut App, ui: &mut Ui, width: f32, wide: bool) {
    let palette = Palette::of_ui(ui);
    if app.player.is_live() {
        ui.add_space(6.0);
        controls::badge(ui, &t("Live"), palette.accent);
        ui.add_space(6.0);
        return;
    }
    let (position, duration) = actions::timeline(app);
    let output = ui
        .add_enabled_ui(duration.is_some(), |ui| {
            controls::thin_slider(ui, position, 0.0..=duration.unwrap_or(1.0), width, false, 10.0, &t("Playback position"))
        })
        .inner;
    if let Some(seconds) = output.changed {
        actions::seek(app, seconds);
    }
    let show_remaining = crate::ui::player_bar::remaining_preference(ui);
    let (elapsed, trailing) = controls::times(output.shown, duration, show_remaining);
    let row = ui.allocate_exact_size(vec2(width, 22.0), Sense::hover()).0;
    let font = egui::FontId::new(12.5, theme::semibold());
    ui.painter().text(row.left_center(), egui::Align2::LEFT_CENTER, elapsed, font.clone(), palette.secondary);
    let trailing_rect = Rect::from_min_max(pos2(row.right() - 70.0, row.top()), row.max);
    ui.painter().text(row.right_center(), egui::Align2::RIGHT_CENTER, trailing, font.clone(), palette.secondary);
    let label = t(if show_remaining { "Time remaining" } else { "Duration" });
    let toggle = ui.interact(trailing_rect, Id::unique("np-remaining"), Sense::click()).on_hover_text(label);
    if toggle.clicked() {
        crate::ui::player_bar::set_remaining_preference(ui, !show_remaining);
    }
    if app.player.mode == Mode::Music && app.player.queue.songs().len() > 1 {
        let text = l10n::tf("Track %1$lld of %2$lld", &[&(app.player.queue.index() + 1), &app.player.queue.songs().len()]);
        let center = Rect::from_center_size(row.center(), vec2(160.0, row.height()));
        ui.painter().text(center.center(), egui::Align2::CENTER_CENTER, text, font, palette.secondary);
        if ui
            .interact(center, Id::unique("np-track-of"), Sense::click())
            .on_hover_text(t("Playing next"))
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            toggle_inline(app, wide, Inline::Queue);
        }
    }
}

/// Lays out a horizontally centered row of known width.
fn centered_row<R>(ui: &mut Ui, width: f32, content: f32, height: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let inner = Rect::from_center_size(rect.center(), vec2(content.min(width), height));
    let mut child = ui.new_child(UiBuilder::new().max_rect(inner).layout(Layout::left_to_right(Align::Center)));
    add(&mut child)
}

fn transport(app: &mut App, ui: &mut Ui, width: f32) {
    let audiobook = app.player.mode == Mode::Audiobook;
    let music = app.player.mode == Mode::Music;
    let content = 2.0 * 52.0 + 84.0 + 2.0 * 36.0;
    centered_row(ui, width, content, 90.0, |ui| {
        ui.spacing_mut().item_spacing.x = 36.0;
        let (back, back_label) =
            if audiobook { (icons::ARROW_COUNTER_CLOCKWISE, t("Skip back")) } else { actions::back_control(app) };
        let enabled = music || audiobook;
        if big_icon(ui, back, &back_label, enabled).clicked() {
            player::previous(app);
        }
        if controls::play_button(ui, app, 74.0).clicked() {
            actions::toggle(app);
        }
        let (forward, forward_label) =
            if audiobook { (icons::ARROW_CLOCKWISE, t("Skip forward")) } else { actions::forward_control(app) };
        if big_icon(ui, forward, &forward_label, enabled).clicked() {
            player::next(app);
        }
    });
}

fn big_icon(ui: &mut Ui, glyph: &str, label: &str, enabled: bool) -> egui::Response {
    let palette = Palette::of_ui(ui);
    let (rect, response) = ui.allocate_exact_size(vec2(52.0, 52.0), if enabled { Sense::click() } else { Sense::hover() });
    if response.hovered() && enabled {
        ui.painter().circle_filled(rect.center(), 26.0, palette.hover);
    }
    let color = if enabled { palette.text } else { palette.secondary.gamma_multiply(0.5) };
    let scale = if response.is_pointer_button_down_on() { 0.92 } else { 1.0 };
    crate::ui::widgets::paint_icon(ui.painter(), rect.center(), glyph, egui::FontId::new(32.0 * scale, theme::icon_fill()), color);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    response.on_hover_text(label)
}

fn volume(app: &mut App, ui: &mut Ui, width: f32) {
    let palette = Palette::of_ui(ui);
    centered_row(ui, width, width, 32.0, |ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        let muted = actions::is_muted(app);
        let label = t(if muted { "Unmute" } else { "Mute" });
        let glyph = if muted { icons::SPEAKER_SLASH } else { icons::SPEAKER_NONE };
        if widgets::icon_button_tinted(ui, glyph, &label, 15.0, Some(palette.secondary)).clicked() {
            actions::toggle_mute(app);
        }
        let slider_width = (ui.available_width() - 42.0).max(60.0);
        let output = controls::thin_slider(ui, app.player.volume as f64, 0.0..=1.0, slider_width, true, 0.05, &t("Volume"));
        if let Some(volume) = output.changed {
            player::set_volume(app, volume as f32);
        }
        ui.label(theme::icon(icons::SPEAKER_HIGH, 15.0).color(palette.secondary));
    });
}

fn tools(app: &mut App, ui: &mut Ui, width: f32, wide: bool) {
    let music = app.player.mode == Mode::Music;
    let song = app.player.current();
    let lyrics_available = crate::features::lyrics::current_song(app).is_some();
    let speed = actions::speed_applies(app);
    let count = 2 + usize::from(music) * 4 + usize::from(speed) - usize::from(music && !lyrics_available);
    let content = count as f32 * 48.0 + (count.saturating_sub(1)) as f32 * 14.0;
    let inline = current_inline(app, wide);
    centered_row(ui, width, content, 56.0, |ui| {
        ui.spacing_mut().item_spacing.x = 14.0;
        if music && controls::tool_button(ui, icons::SHUFFLE, &t("Shuffle"), app.player.shuffled).clicked() {
            player::toggle_shuffle(app);
        }
        let caption = actions::sleep_caption(app);
        let sleep = controls::tool_button_with(ui, icons::MOON, &t("Sleep timer"), caption.is_some(), caption.as_deref());
        egui::Popup::menu(&sleep).show(|ui| actions::sleep_menu(ui, app));
        if music {
            let repeat = player::repeat_mode(app) != discophile_core::queue::RepeatMode::Off;
            if controls::tool_button(ui, actions::repeat_icon(app), &actions::repeat_label(app), repeat).clicked() {
                player::cycle_repeat(app);
            }
            if lyrics_available && controls::tool_button(ui, icons::QUOTES, &t("Lyrics"), inline == Inline::Lyrics).clicked() {
                toggle_inline(app, wide, Inline::Lyrics);
            }
            if controls::tool_button(ui, icons::QUEUE, &t("Playing next"), inline == Inline::Queue).clicked() {
                toggle_inline(app, wide, Inline::Queue);
            }
        }
        if speed {
            let label = actions::speed_label(app.player.rate);
            let active = (app.player.rate - 1.0).abs() > 0.01;
            let response = controls::tool_button_with(ui, icons::GAUGE, &t("Playback speed"), active, Some(&label));
            egui::Popup::menu(&response).show(|ui| actions::speed_menu(ui, app));
        }
        let more = controls::tool_button(ui, icons::DOTS_THREE, &t("More playback actions"), false);
        egui::Popup::menu(&more).show(|ui| actions::more_menu(ui, app, song.as_ref()));
    });
}

fn inline_panel(app: &mut App, ui: &mut Ui, inline: Inline, wide: bool) {
    let palette = Palette::of_ui(ui);
    ui.horizontal(|ui| {
        if crate::features::lyrics::current_song(app).is_some() && widgets::chip(ui, inline == Inline::Lyrics, &t("Lyrics")).clicked() {
            show_inline(app, Inline::Lyrics);
        }
        if app.player.mode == Mode::Music && widgets::chip(ui, inline == Inline::Queue, &t("Playing next")).clicked() {
            show_inline(app, Inline::Queue);
        }
        let current = app.player.current().filter(|_| app.player.mode == Mode::Music);
        if (current.is_some() || inline == Inline::Info) && widgets::chip(ui, inline == Inline::Info, &t("About")).clicked() {
            match current {
                Some(song) => crate::features::info::about_song(app, &song),
                None => show_inline(app, Inline::Info),
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if widgets::icon_button(ui, icons::X, &t("Close"), 15.0).clicked() {
                show_inline(app, Inline::None);
            }
        });
    });
    ui.add_space(6.0);
    let fill = if palette.dark { egui::Color32::from_black_alpha(60) } else { egui::Color32::from_white_alpha(120) };
    egui::Frame::new().fill(fill).corner_radius(CornerRadius::same(16)).inner_margin(egui::Margin::same(14)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.set_height(ui.available_height());
        match inline {
            Inline::Lyrics => crate::ui::lyrics::view(app, ui, if wide { 28.0 } else { 24.0 }),
            Inline::Queue => crate::ui::queue::inline(app, ui),
            Inline::Info => crate::ui::info::inline(app, ui),
            Inline::None => {}
        }
    });
}
