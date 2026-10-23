//! Personal radio: an endless queue from the library, related to a starting song, that refills near
//! its end, avoids repeats and explains each choice ("Why this song?").

use std::collections::{HashMap, HashSet};

use discophile_core::client::SearchLimits;
use discophile_core::listening_history::{same_artist, unique};
use discophile_core::personal_radio::{
    self as radio, ARTIST_SEARCH_SIZE, DEFAULT_LIMIT, PersonalRadioMode, RANDOM_SAMPLE_SIZE, RadioReason, RadioRecommendation,
};
use discophile_core::queue::RepeatMode;
use discophile_core::{Song, l10n};
use egui::{Align, CornerRadius, Layout, Margin, Sense, Stroke, Ui, vec2};

use crate::app::App;
use crate::features::downloads;
use crate::player::{self, Mode, PlayContext};
use crate::tasks::update;
use crate::theme::{self, Font, Palette, icons};
use crate::ui::widgets;

#[derive(Default)]
pub struct RadioState {
    pub seed: Option<Song>,
    pub mode: PersonalRadioMode,
    pub busy: bool,
    pub error: Option<String>,
    recommendations: HashMap<String, RadioRecommendation>,
    /// Invalidates requests that belong to a radio that was stopped or restarted.
    revision: u64,
    advance_when_ready: bool,
    /// No further candidates: stop refilling until the radio restarts.
    exhausted: bool,
    downloaded_only: bool,
    /// The seed shown in the Personal radio window, and the style selected there.
    window: Option<Song>,
    window_mode: PersonalRadioMode,
}

/// Opens the Personal radio window for a track: choose Stay with this style or Let me explore, then start.
pub fn start_personal_radio(app: &mut App, seed: Song) {
    if seed.is_podcast() {
        return;
    }
    let on_air = app.discovery.radio.seed.as_ref().is_some_and(|s| s.stream_identifier() == seed.stream_identifier());
    app.discovery.radio.window_mode = if on_air { app.discovery.radio.mode } else { PersonalRadioMode::Familiar };
    if !on_air {
        app.discovery.radio.error = None;
    }
    app.discovery.radio.window = Some(seed);
}

/// Opens the window for the radio that is playing, if any.
pub fn open_radio_window(app: &mut App) {
    if let Some(seed) = app.discovery.radio.seed.clone() {
        start_personal_radio(app, seed);
    }
}

pub fn is_radio_active(app: &App) -> bool {
    app.discovery.radio.seed.is_some()
}

/// Ends the radio; the queue keeps playing without refills.
pub fn stop_personal_radio(app: &mut App) {
    stop(app);
    if matches!(app.player.context, Some(PlayContext::PersonalRadio { .. })) {
        app.player.context = None;
    }
}

/// The reasons a radio song was chosen, for chips in the queue or now playing.
pub fn radio_reasons(app: &App, song: &Song) -> Option<Vec<RadioReason>> {
    app.discovery.radio.seed.as_ref()?;
    app.discovery.radio.recommendations.get(song.stream_identifier()).map(|r| r.reasons.clone())
}

/// "Why this song?": the explanation for a song the radio chose, while the radio is active.
pub fn why_this_song(app: &App, song: &Song) -> Option<String> {
    let reasons = radio_reasons(app, song)?;
    Some(reasons.iter().map(|r| r.description()).collect::<Vec<_>>().join(" "))
}

fn stop(app: &mut App) {
    let radio = &mut app.discovery.radio;
    radio.revision += 1;
    radio.seed = None;
    radio.busy = false;
    radio.recommendations.clear();
    radio.advance_when_ready = false;
    radio.exhausted = false;
    radio.error = None;
}

/// The library changed: the radio and its window close.
pub(super) fn reset(app: &mut App) {
    stop(app);
    app.discovery.radio.window = None;
}

fn begin(app: &mut App, seed: Song, mode: PersonalRadioMode) {
    stop(app);
    if seed.is_podcast() {
        return;
    }
    let radio = &mut app.discovery.radio;
    radio.seed = Some(seed);
    radio.mode = mode;
    radio.downloaded_only = app.settings.downloaded_only;
    refill(app, true);
}

pub(super) fn on_music_tick(app: &mut App) {
    if app.discovery.radio.seed.is_none() || app.discovery.radio.busy {
        return;
    }
    // Starting other content or switching Downloaded only ends radio.
    if !matches!(app.player.context, Some(PlayContext::PersonalRadio { .. })) {
        stop(app);
        return;
    }
    if app.discovery.radio.downloaded_only != app.settings.downloaded_only {
        stop_personal_radio(app);
        return;
    }
    if !app.discovery.radio.exhausted && radio::needs_refill(&app.player.queue) {
        refill(app, false);
    }
}

pub(super) fn on_queue_finished(app: &mut App) {
    if app.discovery.radio.seed.is_none() || !matches!(app.player.context, Some(PlayContext::PersonalRadio { .. })) {
        return;
    }
    if app.discovery.radio.exhausted {
        stop_personal_radio(app);
        return;
    }
    app.discovery.radio.advance_when_ready = true;
    if !app.discovery.radio.busy {
        refill(app, false);
    }
}

fn refill(app: &mut App, starting: bool) {
    let Some(seed) = app.discovery.radio.seed.clone() else { return };
    if app.discovery.radio.busy {
        return;
    }
    let downloaded_only = app.settings.downloaded_only;
    if downloaded_only && !downloads::is_downloaded(app, &seed) {
        fail(app, starting);
        return;
    }
    let radio = &mut app.discovery.radio;
    radio.busy = true;
    radio.error = None;
    let revision = radio.revision;
    let mode = radio.mode;
    let generation = app.session.generation;
    let pool = super::pool(app);
    let client = if downloaded_only { None } else { app.client() };
    let current_before = app.player.queue.current().map(|s| s.stream_identifier().to_string());
    app.tasks.spawn(async move {
        let mut similar: Vec<Song> = Vec::new();
        let mut candidates = pool;
        if let Some(client) = client {
            let artist = seed.artist.clone().filter(|a| !a.trim().is_empty());
            let found = match &artist {
                Some(artist) => client
                    .search(artist, SearchLimits { song_count: ARTIST_SEARCH_SIZE, album_count: 0, artist_count: 10, ..Default::default() })
                    .await
                    .ok(),
                None => None,
            };
            let artist_id = seed.artist_id.clone().or_else(|| {
                let name = artist.as_deref()?;
                found.as_ref()?.artist.as_ref()?.iter().find(|a| same_artist(&a.name, name)).map(|a| a.id.clone())
            });
            if let Some(id) = artist_id {
                similar = client.similar_songs(&id, discophile_core::client::DEFAULT_SIMILAR_SONGS).await.unwrap_or_default();
            }
            candidates.extend(found.and_then(|r| r.song).unwrap_or_default());
            candidates.extend(client.random_songs(RANDOM_SAMPLE_SIZE).await.unwrap_or_default());
        }
        update(move |app: &mut App| {
            if app.discovery.radio.revision != revision || app.session.generation != generation {
                return;
            }
            app.discovery.radio.busy = false;
            if downloaded_only != app.settings.downloaded_only {
                stop(app);
                return;
            }
            // Something else started while the radio was being prepared.
            let current_now = app.player.queue.current().map(|s| s.stream_identifier().to_string());
            if starting && current_now != current_before && current_now.as_deref() != Some(seed.stream_identifier()) {
                stop(app);
                return;
            }
            let excluded = radio::excluded_ids(&app.player.queue, starting);
            let similar_ids: HashSet<String> = similar.iter().map(|s| s.stream_identifier().to_string()).collect();
            let all: Vec<Song> = similar.into_iter().chain(candidates).collect();
            let selected = radio::recommend(
                &seed,
                &all,
                &similar_ids,
                &app.discovery.archive.history,
                mode,
                &excluded,
                DEFAULT_LIMIT,
                discophile_core::now_millis(),
            );
            if selected.is_empty() {
                fail(app, starting);
                return;
            }
            let songs: Vec<Song> = selected.iter().map(|r| r.song.clone()).collect();
            for item in selected {
                app.discovery.radio.recommendations.insert(item.id().to_string(), item);
            }
            if starting {
                start_playback(app, seed, mode, songs);
            } else {
                append(app, songs);
                if std::mem::take(&mut app.discovery.radio.advance_when_ready) {
                    player::next(app);
                }
            }
        })
    });
}

fn fail(app: &mut App, starting: bool) {
    let radio = &mut app.discovery.radio;
    radio.busy = false;
    radio.error = Some(l10n::t("No more radio tracks are available. Try Explore, refresh your library, or turn off Downloaded only."));
    radio.advance_when_ready = false;
    radio.exhausted = true;
    if starting {
        radio.seed = None;
        radio.recommendations.clear();
    }
}

fn start_playback(app: &mut App, seed: Song, mode: PersonalRadioMode, songs: Vec<Song>) {
    let context = PlayContext::PersonalRadio { seed: seed.clone(), explore: mode == PersonalRadioMode::Explore };
    let queue: Vec<Song> = std::iter::once(seed.clone()).chain(songs).collect();
    let seed_playing =
        app.player.mode == Mode::Music && app.player.current().is_some_and(|s| s.stream_identifier() == seed.stream_identifier());
    if seed_playing {
        // Keep the starting track playing and replace what follows it.
        app.player.queue.replace(queue, 0, false);
        app.player.queue.repeat_mode = RepeatMode::Off;
        app.player.shuffled = false;
        app.player.context = Some(context);
        player::refresh_next(app);
        if !app.player.is_playing() {
            player::play(app);
        }
    } else {
        player::play_songs_with_context(app, queue, 0, false, Some(context));
        app.player.queue.repeat_mode = RepeatMode::Off;
    }
}

fn append(app: &mut App, songs: Vec<Song>) {
    let known: HashSet<String> = app.player.queue.songs().iter().map(|s| s.stream_identifier().to_string()).collect();
    for song in unique(&songs) {
        if !known.contains(song.stream_identifier()) {
            app.player.queue.append(song);
        }
    }
    player::refresh_next(app);
}

// MARK: Window

fn mode_icon(mode: PersonalRadioMode) -> &'static str {
    match mode {
        PersonalRadioMode::Familiar => icons::CROSSHAIR,
        PersonalRadioMode::Explore => icons::COMPASS,
    }
}

fn reason_icon(reason: RadioReason) -> &'static str {
    match reason {
        RadioReason::Server => icons::HARD_DRIVES,
        RadioReason::Artist => icons::USER,
        RadioReason::Genre => icons::GUITAR,
        RadioReason::Era => icons::CALENDAR,
        RadioReason::Unheard => icons::SPARKLE,
        RadioReason::Rediscover => icons::CLOCK_COUNTER_CLOCKWISE,
        RadioReason::Collection => icons::MUSIC_NOTE,
    }
}

/// Reason chips, wrapping onto as many lines as needed.
pub fn reason_chips(ui: &mut Ui, reasons: &[RadioReason]) {
    let palette = Palette::of_ui(ui);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        for reason in reasons {
            egui::Frame::new()
                .fill(palette.accent.gamma_multiply(0.12))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(Margin::symmetric(8, 3))
                .show(ui, |ui| {
                    ui.label(theme::text(format!("{}  {}", reason_icon(*reason), reason.label()), Font::Caption).color(palette.accent))
                        .on_hover_text(reason.description());
                });
        }
    });
}

pub(super) fn window(app: &mut App, ctx: &egui::Context) {
    let Some(seed) = app.discovery.radio.window.clone() else { return };
    let mut open = true;
    egui::Window::new(l10n::t("Personal radio"))
        .id(egui::Id::unique("personal-radio"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(460.0)
        .default_height(560.0)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| contents(app, ui, &seed));
        });
    if !open {
        app.discovery.radio.window = None;
    }
}

fn contents(app: &mut App, ui: &mut Ui, seed: &Song) {
    let palette = Palette::of_ui(ui);
    let on_air = app.discovery.radio.seed.as_ref().is_some_and(|s| s.stream_identifier() == seed.stream_identifier());
    widgets::card(ui, |ui| {
        ui.horizontal(|ui| {
            widgets::artwork(ui, app, seed.album_id.as_deref().unwrap_or(&seed.id), seed.cover_art.as_deref(), 64.0, 10.0);
            ui.vertical(|ui| {
                ui.label(theme::secondary(ui, l10n::t(if on_air { "On air" } else { "Starting from" }), Font::Caption));
                ui.add(egui::Label::new(theme::text(&seed.title, Font::Headline)).truncate());
                let artist = seed.artist.clone().unwrap_or_else(|| l10n::t("Unknown artist"));
                ui.add(egui::Label::new(theme::secondary(ui, artist, Font::Subheadline)).truncate());
            });
            if on_air {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(theme::icon(icons::BROADCAST, 24.0).color(palette.accent));
                });
            }
        });
    });

    ui.add_space(10.0);
    ui.label(theme::text(l10n::t("Radio style"), Font::Title3));
    ui.add_space(4.0);
    for option in PersonalRadioMode::ALL {
        let selected = app.discovery.radio.window_mode == option;
        if mode_option(ui, option, selected).clicked() {
            app.discovery.radio.window_mode = option;
        }
        ui.add_space(6.0);
    }

    ui.add_space(6.0);
    let mode = app.discovery.radio.window_mode;
    let busy = app.discovery.radio.busy;
    ui.horizontal(|ui| {
        let label = l10n::t(if on_air { "Restart radio" } else { "Start personal radio" });
        let enabled = !busy && !(on_air && mode == app.discovery.radio.mode);
        if ui.add_enabled_ui(enabled, |ui| widgets::action_button(ui, format!("{}  {label}", icons::BROADCAST), false)).inner.clicked() {
            begin(app, seed.clone(), mode);
        }
        if on_air && widgets::action_button(ui, format!("{}  {}", icons::STOP, l10n::t("Stop radio")), true).clicked() {
            stop_personal_radio(app);
        }
        if busy {
            ui.spinner();
        }
    });
    if let Some(error) = app.discovery.radio.error.clone()
        && widgets::error_banner(ui, &error, true)
    {
        begin(app, seed.clone(), mode);
    }

    if on_air {
        let current = app.player.current().map(|s| s.stream_identifier().to_string());
        let upcoming: Vec<(Song, Vec<RadioReason>)> = unique(app.player.queue.songs().get(app.player.queue.index()..).unwrap_or_default())
            .into_iter()
            .filter_map(|song| app.discovery.radio.recommendations.get(song.stream_identifier()).map(|r| (song, r.reasons.clone())))
            .collect();
        if !upcoming.is_empty() {
            ui.add_space(10.0);
            ui.label(theme::text(l10n::t("Why this song?"), Font::Title3));
            ui.add_space(4.0);
            for (song, reasons) in upcoming {
                let playing = current.as_deref() == Some(song.stream_identifier());
                reason_row(app, ui, &song, &reasons, playing);
                ui.add_space(6.0);
            }
        }
    }
    ui.add_space(8.0);
    ui.label(theme::secondary(
        ui,
        l10n::t("Radio uses artist, genre, years and server suggestions. Explore adds more discoveries. It does not analyze audio."),
        Font::Footnote,
    ));
}

fn mode_option(ui: &mut Ui, option: PersonalRadioMode, selected: bool) -> egui::Response {
    let palette = Palette::of_ui(ui);
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, 68.0), Sense::click());
    let fill = if response.hovered() { palette.surface_raised } else { palette.surface };
    let stroke = if selected { Stroke::new(1.5, palette.accent.gamma_multiply(0.6)) } else { Stroke::NONE };
    ui.painter().rect(rect, CornerRadius::same(widgets::CARD_RADIUS), fill, stroke, egui::StrokeKind::Inside);
    let mut child =
        ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(14.0, 8.0))).layout(Layout::left_to_right(Align::Center)));
    child.label(theme::icon(mode_icon(option), 24.0).color(palette.accent));
    child.add_space(6.0);
    child.with_layout(Layout::right_to_left(Align::Center), |ui| {
        let check = if selected {
            theme::icon_filled(icons::CHECK_CIRCLE, 20.0).color(palette.accent)
        } else {
            theme::icon(icons::CIRCLE, 20.0).color(palette.secondary)
        };
        ui.label(check);
        ui.with_layout(Layout::top_down(Align::Min), |ui| {
            ui.add_space(4.0);
            ui.label(theme::text(option.title(), Font::Headline));
            ui.add(egui::Label::new(theme::secondary(ui, option.explanation(), Font::Footnote)).wrap());
        });
    });
    let label = format!("{}, {}", option.title(), option.explanation());
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, true, selected, &label));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn reason_row(app: &mut App, ui: &mut Ui, song: &Song, reasons: &[RadioReason], playing: bool) {
    let palette = Palette::of_ui(ui);
    let response = widgets::card_frame(ui)
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                widgets::artwork(ui, app, song.album_id.as_deref().unwrap_or(&song.id), song.cover_art.as_deref(), 44.0, 8.0);
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        if playing {
                            ui.label(theme::icon(icons::WAVEFORM, 15.0).color(palette.accent)).on_hover_text(l10n::t("Now playing"));
                        }
                        ui.add(egui::Label::new(theme::text(&song.title, Font::Headline)).truncate());
                    });
                    let artist = song.artist.clone().unwrap_or_else(|| l10n::t("Unknown artist"));
                    ui.label(theme::secondary(ui, artist, Font::Caption));
                    reason_chips(ui, reasons);
                });
            });
        })
        .response;
    let description = reasons.iter().map(|r| r.description()).collect::<Vec<_>>().join(" ");
    response.on_hover_text(description);
}
