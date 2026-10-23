//! The audiobook player embedded in a book's page: chapter navigation, chapter or book timeline,
//! skips, speed, sleep timer, bookmarks, exact-time seeking and per-book repeat.

use discophile_core::audiobook_reader::{AudiobookBookmark, AudiobookReaderPreferences, AudiobookTimeline, TimelineScope};
use discophile_core::l10n::{count, t, tf};
use egui::{Align, CornerRadius, Layout, Sense, Ui, vec2};

use crate::app::App;
use crate::features::audiobooks::playback;
use crate::nav::{Route, SettingsPage};
use crate::player::{self, SleepTimer};
use crate::theme::{self, Font, Palette, icons};
use crate::ui::widgets;

const SLEEP_MINUTES: [u32; 6] = [5, 10, 15, 30, 45, 60];

enum Action {
    Seek(f64),
    Skip(bool),
    Toggle,
    Rate(f64),
    Sleep(Option<u32>),
    SleepEndOfChapter,
    Bookmark,
    Repeat(bool),
    Finish,
}

pub fn section(app: &mut App, ui: &mut Ui) {
    let Some(pb) = app.audiobooks.playback.as_ref() else { return };
    let timeline = pb.timeline.clone();
    let position = pb.position;
    let duration = pb.duration();
    let offline = pb.offline;
    let repeating = pb.repeating;
    let account = pb.account.clone();
    let book = pb.book.id.clone();
    let preferences = app.audiobooks.reader.preferences.clone();
    let rate = app.player.rate as f64;
    let playing = app.player.status.playing;
    let buffering = app.player.status.buffering;
    let chapter = timeline.chapter(position).cloned();
    let palette = Palette::of_ui(ui);
    let mut actions = Vec::new();

    widgets::card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(theme::text(t("Now Playing"), Font::Headline).color(palette.accent));
            if offline {
                ui.label(theme::icon_filled(icons::ARROW_CIRCLE_DOWN, 15.0).color(palette.secondary))
                    .on_hover_text(t("Playing from downloads"));
                ui.label(theme::secondary(ui, t("Playing from downloads"), Font::Footnote));
            }
            if buffering {
                ui.spinner();
            }
        });
        ui.add_space(4.0);
        chapter_navigation(ui, &timeline, position, &mut actions);
        ui.add_space(6.0);
        progress(app, ui, &timeline, position, duration, &preferences, rate, &mut actions);
        ui.add_space(8.0);
        transport(ui, playing, &preferences, &mut actions);
        ui.add_space(8.0);
        tools(app, ui, chapter.is_some(), rate, repeating, &mut actions);
        sleep_status(app, ui);
        if let Some(saved) = app.audiobooks.player_ui.bookmark_saved {
            if ui.input(|i| i.time) - saved < 2.0 {
                ui.vertical_centered(|ui| ui.label(theme::text(t("Bookmark saved"), Font::Footnote).color(palette.positive)));
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
            } else {
                app.audiobooks.player_ui.bookmark_saved = None;
            }
        }
        if app.audiobooks.player_ui.show_bookmarks {
            ui.separator();
            bookmarks(app, ui, &account, &book, &mut actions);
        }
        for note in [app.audiobooks.reader.error.clone(), app.audiobooks.sync.error.clone()].into_iter().flatten() {
            ui.horizontal_wrapped(|ui| {
                ui.label(theme::icon(icons::INFO, 15.0).color(palette.secondary));
                ui.label(theme::secondary(ui, note, Font::Footnote));
            });
        }
    });

    for action in actions {
        match action {
            Action::Seek(target) => playback::seek(app, target),
            Action::Skip(forward) => playback::skip(app, forward),
            Action::Toggle => playback::toggle(app),
            Action::Rate(rate) => playback::set_rate(app, rate),
            Action::Sleep(minutes) => player::set_sleep_minutes(app, minutes),
            Action::SleepEndOfChapter => playback::sleep_at_end_of_chapter(app),
            Action::Bookmark => {
                let title = AudiobookBookmark::suggested_title(
                    chapter.as_ref(),
                    &app.audiobooks.playback.as_ref().map(|p| p.book.title().to_string()).unwrap_or_default(),
                );
                if app.audiobooks.reader.add(&account, &book, position, &title) {
                    app.audiobooks.player_ui.bookmark_saved = Some(ui.input(|i| i.time));
                }
            }
            Action::Repeat(enabled) => playback::set_repeating(app, enabled),
            Action::Finish => playback::mark_finished(app),
        }
    }
}

fn chapter_navigation(ui: &mut Ui, timeline: &AudiobookTimeline, position: f64, actions: &mut Vec<Action>) {
    let chapter = timeline.chapter(position);
    ui.horizontal(|ui| {
        let previous = timeline.previous(position);
        let can_previous = chapter.is_some_and(|c| c.id > 0 || position > 0.0);
        let pressed = ui
            .add_enabled_ui(can_previous, |ui| widgets::icon_button(ui, icons::CARET_DOUBLE_LEFT, &t("Previous chapter"), 20.0))
            .inner
            .clicked();
        if pressed && let Some(target) = previous {
            actions.push(Action::Seek(target));
        }
        let next = timeline.next(position);
        let width = (ui.available_width() - 46.0).max(120.0);
        let title = chapter.map(|c| c.display_title()).unwrap_or_else(|| t("Audiobook"));
        let mut selected = chapter.map(|c| c.id);
        egui::ComboBox::from_id_salt("abs-chapter")
            .selected_text(theme::text(title, Font::Headline))
            .width(width)
            .height(420.0)
            .show_ui(ui, |ui| {
                for c in &timeline.chapters {
                    let label = format!("{}  ·  {}", c.display_title(), AudiobookTimeline::clock(c.start));
                    if ui.selectable_value(&mut selected, Some(c.id), label).clicked() {
                        actions.push(Action::Seek(c.start));
                    }
                }
            })
            .response
            .on_hover_text(t("Choose a chapter"));
        let pressed =
            ui.add_enabled_ui(next.is_some(), |ui| widgets::icon_button(ui, icons::CARET_RIGHT, &t("Next chapter"), 20.0)).inner.clicked();
        if pressed && let Some(target) = next {
            actions.push(Action::Seek(target));
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn progress(
    app: &mut App,
    ui: &mut Ui,
    timeline: &AudiobookTimeline,
    position: f64,
    duration: f64,
    preferences: &AudiobookReaderPreferences,
    rate: f64,
    actions: &mut Vec<Action>,
) {
    let scrub = app.audiobooks.player_ui.scrub;
    let window = scrub.map(|(w, _)| w).unwrap_or_else(|| timeline.window(position, preferences.scope));
    let mut value = scrub.map(|(_, v)| v).unwrap_or_else(|| window.elapsed(position));
    let shown = if scrub.is_some() { window.position(value) } else { position };
    ui.scope(|ui| {
        ui.spacing_mut().slider_width = ui.available_width();
        let label = if preferences.scope == TimelineScope::Chapter { t("Chapter position") } else { t("Book position") };
        let response = ui
            .add(egui::Slider::new(&mut value, 0.0..=window.duration().max(0.01)).show_value(false).trailing_fill(true))
            .on_hover_text(&label);
        if response.drag_stopped() {
            app.audiobooks.player_ui.scrub = None;
            actions.push(Action::Seek(window.position(value)));
        } else if response.dragged() {
            app.audiobooks.player_ui.scrub = Some((window, value));
        } else if response.changed() {
            actions.push(Action::Seek(window.position(value)));
        }
    });
    let display = |seconds: f64| AudiobookTimeline::clock(preferences.displayed_time(seconds, rate));
    let elapsed = display(window.elapsed(shown));
    let trailing = if preferences.show_remaining { format!("−{}", display(window.remaining(shown))) } else { display(window.duration()) };
    let scope_label = match (preferences.scope, timeline.chapter(shown)) {
        (TimelineScope::Chapter, Some(chapter)) => tf("Chapter %1$lld of %2$lld", &[&(chapter.id + 1), &timeline.chapters.len()]),
        _ => format!("{:.0}%", if duration > 0.0 { (shown / duration).clamp(0.0, 1.0) * 100.0 } else { 0.0 }),
    };
    let mut toggle_scope = false;
    let mut toggle_remaining = false;
    ui.columns(3, |columns| {
        columns[0].label(theme::secondary(&columns[0], elapsed, Font::Subheadline).monospace()).on_hover_text(t("Elapsed time"));
        columns[1].vertical_centered(|ui| {
            if ui
                .add(egui::Label::new(theme::secondary(ui, scope_label, Font::Subheadline)).sense(Sense::click()))
                .on_hover_text(t("Switch between chapter and book time"))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                toggle_scope = true;
            }
        });
        columns[2].with_layout(Layout::right_to_left(Align::Min), |ui| {
            let hint = if preferences.show_remaining { t("Time remaining") } else { t("Duration") };
            if ui
                .add(egui::Label::new(theme::secondary(ui, trailing, Font::Subheadline).monospace()).sense(Sense::click()))
                .on_hover_text(format!("{hint} · {}", t("Switch between remaining time and duration")))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                toggle_remaining = true;
            }
        });
    });
    if toggle_scope || toggle_remaining {
        let preferences = &mut app.audiobooks.reader.preferences;
        if toggle_scope {
            preferences.scope = preferences.scope.toggled();
        }
        if toggle_remaining {
            preferences.show_remaining = !preferences.show_remaining;
        }
        app.audiobooks.reader.save_preferences();
        app.audiobooks.player_ui.scrub = None;
    }
}

/// Centers a row of fixed-width controls.
fn centered_row(ui: &mut Ui, width: f32, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
        add(ui);
    });
}

fn skip_button(ui: &mut Ui, forward: bool, seconds: u32) -> egui::Response {
    let side = 60.0;
    let (rect, response) = ui.allocate_exact_size(vec2(side, side), Sense::click());
    let palette = Palette::of_ui(ui);
    if response.hovered() {
        ui.painter().circle_filled(rect.center(), side / 2.0, palette.hover);
    }
    let glyph = if forward { icons::ARROW_CLOCKWISE } else { icons::ARROW_COUNTER_CLOCKWISE };
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, glyph, egui::FontId::proportional(46.0), palette.accent);
    ui.painter().text(
        rect.center() + vec2(0.0, 1.0),
        egui::Align2::CENTER_CENTER,
        seconds.to_string(),
        egui::FontId::new(if seconds > 99 { 10.0 } else { 12.0 }, theme::bold()),
        palette.accent,
    );
    let label = count(if forward { "Forward %lld seconds" } else { "Back %lld seconds" }, seconds as i64);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
    response.on_hover_text(label).on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn transport(ui: &mut Ui, playing: bool, preferences: &AudiobookReaderPreferences, actions: &mut Vec<Action>) {
    let gap = 36.0;
    centered_row(ui, 60.0 * 2.0 + 64.0 + gap * 2.0, |ui| {
        ui.spacing_mut().item_spacing.x = gap;
        if skip_button(ui, false, preferences.backward_seconds).clicked() {
            actions.push(Action::Skip(false));
        }
        ui.vertical(|ui| {
            ui.add_space(-2.0);
            let label = if playing { t("Pause") } else { t("Play") };
            if widgets::round_play_button(ui, playing, &label, 64.0).clicked() {
                actions.push(Action::Toggle);
            }
        });
        if skip_button(ui, true, preferences.forward_seconds).clicked() {
            actions.push(Action::Skip(true));
        }
    });
}

fn rate_label(rate: f64) -> String {
    let text = format!("{rate:.2}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    format!("{text}×")
}

fn tools(app: &mut App, ui: &mut Ui, has_chapter: bool, rate: f64, repeating: bool, actions: &mut Vec<Action>) {
    let palette = Palette::of_ui(ui);
    let sleep = app.player.sleep;
    centered_row(ui, 6.0 * 44.0 + 5.0 * 14.0, |ui| {
        ui.spacing_mut().item_spacing.x = 14.0;
        // Speed
        ui.menu_button(theme::text(rate_label(rate), Font::Headline).color(palette.accent), |ui| {
            ui.set_min_width(280.0);
            ui.label(theme::text(t("Set playback speed"), Font::Headline));
            let mut value = rate;
            ui.horizontal(|ui| {
                ui.label(theme::secondary(ui, "0.5", Font::Footnote));
                ui.spacing_mut().slider_width = 180.0;
                if ui
                    .add(
                        egui::Slider::new(&mut value, AudiobookReaderPreferences::MIN_RATE..=AudiobookReaderPreferences::MAX_RATE)
                            .step_by(0.05)
                            .show_value(false),
                    )
                    .on_hover_text(t("Playback speed"))
                    .changed()
                {
                    actions.push(Action::Rate(value));
                }
                ui.label(theme::secondary(ui, "4.0", Font::Footnote));
            });
            ui.horizontal(|ui| {
                if ui
                    .add_enabled_ui(rate > AudiobookReaderPreferences::MIN_RATE, |ui| {
                        widgets::icon_button(ui, icons::MINUS_CIRCLE, &t("Slower"), 18.0)
                    })
                    .inner
                    .clicked()
                {
                    actions.push(Action::Rate(rate - 0.05));
                }
                for preset in [1.0, 1.25, 1.5, 2.0, 3.0] {
                    if widgets::chip(ui, (rate - preset).abs() < 0.001, &rate_label(preset)).clicked() {
                        actions.push(Action::Rate(preset));
                    }
                }
                if ui
                    .add_enabled_ui(rate < AudiobookReaderPreferences::MAX_RATE, |ui| {
                        widgets::icon_button(ui, icons::PLUS_CIRCLE, &t("Faster"), 18.0)
                    })
                    .inner
                    .clicked()
                {
                    actions.push(Action::Rate(rate + 0.05));
                }
            });
        })
        .response
        .on_hover_text(t("Playback speed"));
        // Sleep timer
        let active = sleep.is_some();
        let moon = if active { theme::icon_filled(icons::MOON, 20.0).color(palette.accent) } else { theme::icon(icons::MOON, 20.0) };
        ui.menu_button(moon, |ui| {
            ui.label(theme::secondary(ui, t("Pause playback"), Font::Footnote));
            if ui.selectable_label(sleep.is_none(), t("Off")).clicked() {
                actions.push(Action::Sleep(None));
                ui.close();
            }
            for minutes in SLEEP_MINUTES {
                if ui.button(count("%lld minutes", minutes as i64)).clicked() {
                    actions.push(Action::Sleep(Some(minutes)));
                    ui.close();
                }
            }
            if ui
                .add_enabled(has_chapter, egui::Button::selectable(sleep == Some(SleepTimer::EndOfChapter), t("End of current chapter")))
                .clicked()
            {
                actions.push(Action::SleepEndOfChapter);
                ui.close();
            }
            if ui.button(format!("{}…", t("Custom timer"))).clicked() {
                app.audiobooks.player_ui.custom_timer = Some(20);
                ui.close();
            }
        })
        .response
        .on_hover_text(t("Sleep timer"));
        // Bookmarks
        if widgets::icon_button(ui, icons::BOOKMARK_SIMPLE, &t("Create bookmark"), 20.0).clicked() {
            actions.push(Action::Bookmark);
        }
        let showing = app.audiobooks.player_ui.show_bookmarks;
        let tint = showing.then_some(palette.accent);
        if widgets::icon_button_tinted(ui, icons::BOOKMARKS_SIMPLE, &t("Bookmarks"), 20.0, tint).clicked() {
            app.audiobooks.player_ui.show_bookmarks = !showing;
        }
        // Go to time
        if widgets::icon_button(ui, icons::CLOCK, &t("Go to time"), 20.0).clicked() {
            let position = app.audiobooks.playback.as_ref().map(|p| p.position.max(0.0) as u64).unwrap_or(0);
            app.audiobooks.player_ui.jump = Some(((position / 3600) as u32, (position / 60 % 60) as u32, (position % 60) as u32));
        }
        // More
        ui.menu_button(theme::icon(icons::DOTS_THREE, 20.0), |ui| {
            if ui.button(t("Go to beginning")).clicked() {
                actions.push(Action::Seek(0.0));
                ui.close();
            }
            if ui.button(t("Mark as finished")).clicked() {
                actions.push(Action::Finish);
                ui.close();
            }
            let mut value = repeating;
            if ui.checkbox(&mut value, t("Repeat this book")).changed() {
                actions.push(Action::Repeat(value));
            }
            ui.separator();
            if ui.button(t("Playback settings")).clicked() {
                app.nav.push(Route::Settings(SettingsPage::Playback));
                ui.close();
            }
        })
        .response
        .on_hover_text(t("More playback actions"));
        if repeating {
            ui.label(theme::icon(icons::REPEAT, 16.0).color(palette.accent)).on_hover_text(t("Repeat this book"));
        }
    });
}

fn sleep_status(app: &App, ui: &mut Ui) {
    let text = match app.player.sleep {
        Some(SleepTimer::EndOfChapter) => Some(t("End of current chapter")),
        Some(SleepTimer::At(_)) => player::sleep_remaining(app).map(AudiobookTimeline::clock),
        _ => None,
    };
    if let Some(text) = text {
        let palette = Palette::of_ui(ui);
        ui.vertical_centered(|ui| {
            ui.horizontal(|ui| {
                ui.label(theme::icon_filled(icons::MOON, 14.0).color(palette.accent));
                ui.label(theme::secondary(ui, format!("{}: {text}", t("Sleep timer")), Font::Footnote));
            });
        });
    }
}

fn bookmarks(app: &mut App, ui: &mut Ui, account: &str, book: &str, actions: &mut Vec<Action>) {
    let palette = Palette::of_ui(ui);
    ui.label(theme::text(t("Bookmarks"), Font::Headline));
    let values = app.audiobooks.reader.for_book(account, book);
    if values.is_empty() {
        ui.label(theme::secondary(ui, t("No bookmarks yet"), Font::Callout));
        ui.label(theme::secondary(ui, t("Save a moment with the bookmark button in the player."), Font::Footnote));
    }
    let mut rename: Option<(uuid::Uuid, String)> = None;
    let mut remove = None;
    let mut cancel = false;
    for bookmark in &values {
        let editing = app.audiobooks.player_ui.renaming.as_ref().filter(|(id, _)| *id == bookmark.id).map(|(_, title)| title.clone());
        if let Some(mut title) = editing {
            ui.horizontal(|ui| {
                let response = ui.add(
                    egui::TextEdit::singleline(&mut title).hint_text(t("Name")).desired_width(ui.available_width() - 160.0).char_limit(200),
                );
                let enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui.button(t("Save")).clicked() || enter {
                    rename = Some((bookmark.id, title.clone()));
                }
                if ui.button(t("Cancel")).clicked() {
                    cancel = true;
                }
            });
            if rename.is_none() && !cancel {
                app.audiobooks.player_ui.renaming = Some((bookmark.id, title));
            }
            continue;
        }
        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 42.0), Sense::click());
        if response.hovered() {
            ui.painter().rect_filled(rect, CornerRadius::same(8), palette.hover);
        }
        let mut child =
            ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(10.0, 4.0))).layout(Layout::left_to_right(Align::Center)));
        let row = &mut child;
        row.label(theme::icon(icons::BOOKMARK_SIMPLE, 15.0).color(palette.accent));
        row.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if widgets::icon_button(ui, icons::TRASH, &t("Delete bookmark"), 15.0).clicked() {
                remove = Some(bookmark.id);
            }
            if widgets::icon_button(ui, icons::PENCIL_SIMPLE, &t("Rename bookmark"), 15.0).clicked() {
                app.audiobooks.player_ui.renaming = Some((bookmark.id, bookmark.title.clone()));
            }
            ui.label(theme::secondary(ui, AudiobookTimeline::clock(bookmark.position), Font::Footnote).monospace());
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.add(egui::Label::new(theme::text(bookmark.display_title(), Font::Body)).truncate().selectable(false));
            });
        });
        let label = format!("{}, {}", bookmark.display_title(), AudiobookTimeline::clock(bookmark.position));
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
        response.context_menu(|ui| {
            if ui.button(t("Rename bookmark")).clicked() {
                app.audiobooks.player_ui.renaming = Some((bookmark.id, bookmark.title.clone()));
                ui.close();
            }
            if ui.button(t("Delete bookmark")).clicked() {
                remove = Some(bookmark.id);
                ui.close();
            }
        });
        if response.clicked() {
            actions.push(Action::Seek(bookmark.position));
        }
    }
    if let Some((id, title)) = rename {
        app.audiobooks.reader.rename(id, &title);
        app.audiobooks.player_ui.renaming = None;
    }
    if cancel {
        app.audiobooks.player_ui.renaming = None;
    }
    if let Some(id) = remove {
        app.audiobooks.reader.remove(id);
    }
    ui.label(theme::secondary(ui, t("Bookmarks are saved on this computer and are available offline."), Font::Footnote));
}

/// "Go to time" and "Custom timer" windows.
pub fn windows(app: &mut App, ctx: &egui::Context) {
    if app.audiobooks.playback.is_none() {
        app.audiobooks.player_ui.jump = None;
        app.audiobooks.player_ui.custom_timer = None;
        return;
    }
    if let Some((mut hours, mut minutes, mut seconds)) = app.audiobooks.player_ui.jump {
        let duration = app.audiobooks.playback.as_ref().map(|p| p.duration().clamp(0.0, 315_360_000.0)).unwrap_or(0.0);
        let mut close = false;
        let mut jump = None;
        let modal = egui::Modal::new(egui::Id::unique("abs-jump")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.label(theme::text(t("Go to time"), Font::Title3));
            ui.label(theme::secondary(ui, t("Book position"), Font::Subheadline));
            ui.add_space(8.0);
            egui::Grid::new("abs-jump-grid").num_columns(3).spacing(vec2(18.0, 4.0)).show(ui, |ui| {
                for label in [t("Hours"), t("Minutes"), t("Seconds")] {
                    ui.label(theme::secondary(ui, label, Font::Caption));
                }
                ui.end_row();
                ui.add(egui::DragValue::new(&mut hours).range(0..=(duration / 3600.0) as u32));
                ui.add(egui::DragValue::new(&mut minutes).range(0..=59));
                ui.add(egui::DragValue::new(&mut seconds).range(0..=59));
                ui.end_row();
            });
            let target = (hours * 3600 + minutes * 60 + seconds) as f64;
            ui.add_space(6.0);
            ui.label(
                theme::secondary(
                    ui,
                    format!("{} / {}", AudiobookTimeline::clock(target), AudiobookTimeline::clock(duration)),
                    Font::Subheadline,
                )
                .monospace(),
            );
            ui.add_space(10.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add_enabled_ui(target <= duration, |ui| widgets::action_button(ui, t("Jump to position"), false)).inner.clicked() {
                    jump = Some(target);
                }
                if ui.button(t("Cancel")).clicked() {
                    close = true;
                }
            });
        });
        app.audiobooks.player_ui.jump = Some((hours, minutes, seconds));
        if let Some(target) = jump {
            app.audiobooks.player_ui.jump = None;
            playback::seek(app, target);
        } else if close || modal.should_close() {
            app.audiobooks.player_ui.jump = None;
        }
    }
    if let Some(mut minutes) = app.audiobooks.player_ui.custom_timer {
        let mut close = false;
        let mut start = false;
        let modal = egui::Modal::new(egui::Id::unique("abs-custom-timer")).show(ctx, |ui| {
            ui.set_width(320.0);
            ui.label(theme::text(t("Custom timer"), Font::Title3));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(t("Pause playback after"));
                ui.add(egui::DragValue::new(&mut minutes).range(1..=180));
                ui.label(theme::secondary(
                    ui,
                    count("%lld minutes", minutes as i64).replace(&minutes.to_string(), "").trim().to_string(),
                    Font::Body,
                ));
            });
            ui.add_space(10.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if widgets::action_button(ui, t("Start sleep timer"), false).clicked() {
                    start = true;
                }
                if ui.button(t("Cancel")).clicked() {
                    close = true;
                }
            });
        });
        app.audiobooks.player_ui.custom_timer = Some(minutes);
        if start {
            app.audiobooks.player_ui.custom_timer = None;
            player::set_sleep_minutes(app, Some(minutes));
        } else if close || modal.should_close() {
            app.audiobooks.player_ui.custom_timer = None;
        }
    }
}
