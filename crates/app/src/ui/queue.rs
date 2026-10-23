//! The queue panel: what plays now, who continues the queue, and the upcoming songs, which can be played,
//! reordered by dragging (or from the context menu) and removed.

use discophile_core::Song;
use discophile_core::l10n::{self, t};
use egui::{Align, CornerRadius, Layout, Sense, Stroke, Ui, UiBuilder, pos2, vec2};

use crate::app::App;
use crate::nav::SidePanel;
use crate::player::{self, Mode, PlayContext};
use crate::theme::{self, Font, Palette, icons};
use crate::ui::player_bar::{actions, controls};
use crate::ui::widgets;

const ROW_HEIGHT: f32 = 50.0;

pub fn panel(app: &mut App, ui: &mut Ui) {
    header(app, ui, true);
    body(app, ui);
}

/// The queue without the close button, for Now Playing.
pub fn inline(app: &mut App, ui: &mut Ui) {
    header(app, ui, false);
    body(app, ui);
}

fn header(app: &mut App, ui: &mut Ui, closable: bool) {
    let music = app.player.mode == Mode::Music;
    ui.horizontal(|ui| {
        ui.label(theme::text(t("Playing next"), Font::Title3));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if closable && widgets::icon_button(ui, icons::X, &t("Close"), 16.0).clicked() {
                app.nav.side_panel = SidePanel::None;
            }
            if music {
                let repeat = player::repeat_mode(app) != discophile_core::queue::RepeatMode::Off;
                if controls::transport_button(ui, actions::repeat_icon(app), &actions::repeat_label(app), 16.0, true, repeat).clicked() {
                    player::cycle_repeat(app);
                }
                if controls::transport_button(ui, icons::SHUFFLE, &t("Shuffle"), 16.0, true, app.player.shuffled).clicked() {
                    player::toggle_shuffle(app);
                }
            }
        });
    });
    if let Some(label) = context_label(app) {
        let palette = Palette::of_ui(ui);
        egui::Frame::new()
            .fill(palette.accent.gamma_multiply(0.12))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(egui::Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.label(theme::icon(context_icon(app), 15.0).color(palette.accent));
                    ui.label(theme::text(label, Font::Callout).color(palette.accent));
                });
                // Personal radio explains why the current song was chosen (owned by discovery).
                if matches!(app.player.context, Some(PlayContext::PersonalRadio { .. }))
                    && let Some(song) = app.player.queue.current().cloned()
                    && let Some(reason) = crate::features::discovery::why_this_song(app, &song)
                {
                    ui.label(theme::text(t("Why this song?"), Font::Footnote).color(palette.accent).strong());
                    ui.label(theme::secondary(ui, reason, Font::Footnote));
                }
            });
    }
    ui.add_space(4.0);
}

/// Who controls the continuation of the queue.
pub fn context_label(app: &App) -> Option<String> {
    match app.player.context.as_ref()? {
        PlayContext::PersonalRadio { seed, explore } => {
            let mode = t(if *explore { "Let me explore" } else { "Stay with this style" });
            Some(format!("{} · {} · {mode}", t("Personal radio"), seed.title))
        }
        PlayContext::AlbumFocus { title, .. } => Some(format!("{} · {title}", t("Listen to the whole album"))),
        PlayContext::Mix { title } => Some(title.clone()),
    }
}

fn context_icon(app: &App) -> &'static str {
    match app.player.context {
        Some(PlayContext::PersonalRadio { .. }) => icons::RADIO,
        Some(PlayContext::AlbumFocus { .. }) => icons::DISC,
        _ => icons::SPARKLE,
    }
}

fn body(app: &mut App, ui: &mut Ui) {
    match app.player.mode {
        Mode::Radio => {
            let name = app.player.radio.as_ref().map(|s| s.display_name()).unwrap_or_default();
            widgets::card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(theme::icon(icons::RADIO, 22.0).color(Palette::of_ui(ui).accent));
                    ui.vertical(|ui| {
                        ui.label(theme::text(name, Font::Headline));
                        controls::badge(ui, &t("Live"), Palette::of_ui(ui).accent);
                    });
                });
            });
            return;
        }
        Mode::Audiobook => {
            widgets::empty_state(ui, icons::BOOK_OPEN, &t("Audiobook"), &t("Chapters are in the audiobook's page."));
            return;
        }
        _ => {}
    }
    if app.player.queue.is_empty() {
        widgets::empty_state(ui, icons::QUEUE, &t("Playing next"), &t("Choose something from your library to start listening."));
        return;
    }
    let current_index = app.player.queue.index();
    if let Some(song) = app.player.queue.current().cloned() {
        ui.label(theme::secondary(ui, t("Now playing"), Font::Footnote));
        let playing = app.player.status.playing;
        let options = widgets::TrackRowOptions { show_artwork: true, is_current: true, is_playing: playing, ..Default::default() };
        let row = widgets::track_row(ui, app, &song, options, |_, _| {});
        if row.response.clicked() || row.play {
            app.nav.now_playing = true;
        }
    }
    let upcoming = app.player.queue.upcoming().len();
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(theme::text(t("Up next"), Font::Headline));
        ui.label(theme::secondary(ui, l10n::count("%lld songs", upcoming as i64), Font::Footnote));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if upcoming > 0 && ui.add(egui::Button::new(theme::accent(ui, t("Clear upcoming"), Font::Callout)).frame(false)).clicked() {
                clear_upcoming(app);
            }
        });
    });
    if upcoming == 0 {
        ui.label(theme::secondary(ui, t("Nothing is queued after this song."), Font::Callout));
        return;
    }
    let songs: Vec<Song> = app.player.queue.upcoming().to_vec();
    let mut action: Option<Action> = None;
    egui::ScrollArea::vertical().id_salt("queue-upcoming").auto_shrink([false, false]).show_rows(
        ui,
        ROW_HEIGHT,
        songs.len(),
        |ui, range| {
            for offset in range {
                let index = current_index + 1 + offset;
                if let Some(next) = row(app, ui, &songs[offset], index, offset, songs.len()) {
                    action = Some(next);
                }
            }
        },
    );
    match action {
        Some(Action::Play(index)) => player::select_queue_index(app, index),
        Some(Action::Remove(index)) => player::remove_from_queue(app, index),
        Some(Action::Move(from, to)) => player::move_in_queue(app, from, to),
        None => {}
    }
}

enum Action {
    Play(usize),
    Remove(usize),
    Move(usize, usize),
}

/// Payload while dragging a queue row: its absolute queue index.
struct Dragged(usize);

fn row(app: &mut App, ui: &mut Ui, song: &Song, index: usize, offset: usize, count: usize) -> Option<Action> {
    let palette = Palette::of_ui(ui);
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click_and_drag());
    let mut action = None;
    let dragging_this = response.dragged();
    if response.hovered() || dragging_this {
        ui.painter().rect_filled(rect, CornerRadius::same(8), palette.hover);
    }
    let mut child = ui.new_child(UiBuilder::new().max_rect(rect.shrink2(vec2(6.0, 4.0))).layout(Layout::left_to_right(Align::Center)));
    let ui_row = &mut child;
    ui_row.label(theme::icon(icons::DOTS_SIX_VERTICAL, 15.0).color(palette.secondary.gamma_multiply(if response.hovered() {
        1.0
    } else {
        0.5
    })));
    widgets::artwork(ui_row, app, &song.id, song.cover_art.as_deref(), 38.0, 6.0);
    ui_row.with_layout(Layout::right_to_left(Align::Center), |ui| {
        if response.hovered() || ui.rect_contains_pointer(rect) {
            if widgets::icon_button(ui, icons::X, &t("Remove"), 14.0).clicked() {
                action = Some(Action::Remove(index));
            }
        } else {
            ui.add_space(32.0);
        }
        let duration = song.duration.map(l10n::time).unwrap_or_default();
        ui.label(theme::secondary(ui, duration, Font::Footnote));
        ui.with_layout(Layout::top_down(Align::Min), |ui| {
            ui.add_space(6.0);
            ui.add(egui::Label::new(theme::text(&song.title, Font::Body)).truncate().selectable(false));
            ui.add(
                egui::Label::new(theme::secondary(ui, song.artist.clone().unwrap_or_default(), Font::Footnote))
                    .truncate()
                    .selectable(false),
            );
        });
    });
    let label = format!("{}, {}", song.title, song.artist.clone().unwrap_or_default());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
    if response.drag_started() {
        response.dnd_set_drag_payload(Dragged(index));
    }
    if let Some(dragged) = response.dnd_hover_payload::<Dragged>()
        && let Some(pointer) = ui.ctx().pointer_hover_pos()
    {
        let below = pointer.y > rect.center().y;
        let y = if below { rect.bottom() } else { rect.top() };
        if dragged.0 != index {
            ui.painter().line_segment([pos2(rect.left() + 4.0, y), pos2(rect.right() - 4.0, y)], Stroke::new(2.0, palette.accent));
        }
    }
    if let Some(dragged) = response.dnd_release_payload::<Dragged>()
        && let Some(pointer) = ui.ctx().pointer_interact_pos()
    {
        let from = dragged.0;
        let below = pointer.y > rect.center().y;
        let to = match (from < index, below) {
            (true, true) => index,
            (true, false) => index - 1,
            (false, true) => index + 1,
            (false, false) => index,
        };
        if from != to {
            action = Some(Action::Move(from, to));
        }
    }
    if response.clicked() && action.is_none() {
        action = Some(Action::Play(index));
    }
    response.context_menu(|ui| {
        if ui.button(format!("{}  {}", icons::PLAY, t("Play"))).clicked() {
            action = Some(Action::Play(index));
        }
        let first = app.player.queue.index() + 1;
        if ui.add_enabled(offset > 0, egui::Button::new(format!("{}  {}", icons::ARROW_LINE_UP, t("Play next")))).clicked() {
            action = Some(Action::Move(index, first));
        }
        if ui.add_enabled(offset > 0, egui::Button::new(format!("{}  {}", icons::ARROW_UP, t("Move up")))).clicked() {
            action = Some(Action::Move(index, index - 1));
        }
        if ui.add_enabled(offset + 1 < count, egui::Button::new(format!("{}  {}", icons::ARROW_DOWN, t("Move down")))).clicked() {
            action = Some(Action::Move(index, index + 1));
        }
        ui.separator();
        if ui.button(format!("{}  {}", icons::TRASH, t("Remove"))).clicked() {
            action = Some(Action::Remove(index));
        }
    });
    action
}

/// Removes every song after the current one.
pub fn clear_upcoming(app: &mut App) {
    let current = app.player.queue.index();
    for index in (current + 1..app.player.queue.songs().len()).rev() {
        app.player.queue.remove(index);
    }
    player::refresh_next(app);
}
