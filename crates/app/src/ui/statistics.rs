//! Listening statistics (period totals, a minutes chart and rankings) and Settings > Rediscover your music.

use chrono::{Datelike, NaiveDate, TimeZone, Weekday};
use discophile_core::l10n::{self, Language};
use discophile_core::listening_statistics::{Bucket, ListeningPeriod, ListeningSummary, Rank};
use egui::{Align, Align2, Color32, CornerRadius, FontId, Layout, Rect, Sense, Stroke, Ui, pos2, vec2};

use crate::app::App;
use crate::features::{discovery, downloads};
use crate::nav::Route;
use crate::player;
use crate::theme::{self, Font, Palette, icons};
use crate::ui::widgets;

/// Rankings sit side by side from this content width.
const TWO_COLUMNS: f32 = 820.0;

pub fn show(app: &mut App, ui: &mut Ui) {
    widgets::page(ui, "statistics", |ui| {
        widgets::page_title(ui, &l10n::t("Listening statistics"));
        let period = app.discovery.ui.period;
        ui.horizontal_wrapped(|ui| {
            ui.label(theme::secondary(ui, l10n::t("Period"), Font::Subheadline));
            for option in ListeningPeriod::ALL {
                if widgets::chip(ui, option == period, &option.title()).clicked() {
                    discovery::set_period(app, option);
                }
            }
        });
        ui.add_space(12.0);
        let summary = discovery::summary(app, period);
        if summary.is_empty() {
            ui.label(theme::secondary(ui, l10n::t("Nothing to count yet. Play a song and your statistics will appear here."), Font::Body));
        } else {
            tiles(ui, &summary);
            if summary.timeline.len() > 1 {
                ui.add_space(14.0);
                timeline_chart(ui, &summary);
            }
            ui.add_space(8.0);
            rankings(app, ui, &summary);
        }
        ui.add_space(16.0);
        ui.label(theme::secondary(
            ui,
            l10n::t(
                "Statistics stay on this device, separately for each library. Daily detail starts with listening after this update. All time also includes earlier totals from your listening history.",
            ),
            Font::Footnote,
        ));
    });
}

fn tiles(ui: &mut Ui, summary: &ListeningSummary) {
    let mut items = vec![
        (l10n::t("Time listened"), l10n::short_duration(summary.seconds)),
        (l10n::t("Plays"), summary.plays.to_string()),
        (l10n::t("Songs"), summary.song_count.to_string()),
        (l10n::t("Artists"), summary.artist_count.to_string()),
    ];
    if summary.period != ListeningPeriod::All {
        items.push((l10n::t("Active days"), summary.active_days.to_string()));
    }
    if summary.streak > 1 {
        items.push((l10n::t("Current streak"), l10n::count("%lld days in a row", summary.streak as i64)));
    }
    // Fill the row when the totals fit on one line instead of leaving a gap at the end.
    let gap = 20.0;
    let min_tile = ((ui.available_width() + gap) / items.len() as f32 - gap).max(150.0);
    widgets::tile_grid(ui, &items, min_tile.floor(), |ui, (title, value), _| {
        let response = widgets::card(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.label(theme::text(value, Font::Title2));
            ui.label(theme::secondary(ui, title, Font::Subheadline));
        })
        .response;
        let label = format!("{title}: {value}");
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &label));
    });
}

// MARK: Chart

fn italian() -> bool {
    l10n::language() == Language::Italian
}

fn month_name(month: u32, short: bool) -> &'static str {
    const EN: [&str; 12] =
        ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    const EN_SHORT: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    const IT: [&str; 12] =
        ["gennaio", "febbraio", "marzo", "aprile", "maggio", "giugno", "luglio", "agosto", "settembre", "ottobre", "novembre", "dicembre"];
    const IT_SHORT: [&str; 12] = ["gen", "feb", "mar", "apr", "mag", "giu", "lug", "ago", "set", "ott", "nov", "dic"];
    let index = (month.clamp(1, 12) - 1) as usize;
    match (italian(), short) {
        (false, false) => EN[index],
        (false, true) => EN_SHORT[index],
        (true, false) => IT[index],
        (true, true) => IT_SHORT[index],
    }
}

fn weekday_name(day: Weekday) -> &'static str {
    const EN: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    const IT: [&str; 7] = ["lun", "mar", "mer", "gio", "ven", "sab", "dom"];
    let index = day.num_days_from_monday() as usize;
    if italian() { IT[index] } else { EN[index] }
}

/// "Mon 12 Oct" or "lun 12 ott".
fn day_label(date: NaiveDate) -> String {
    format!("{} {} {}", weekday_name(date.weekday()), date.day(), month_name(date.month(), true))
}

/// "12 October 2026".
fn date_label(date: NaiveDate) -> String {
    format!("{} {} {}", date.day(), month_name(date.month(), false), date.year())
}

fn bucket_label(bucket: &Bucket, monthly: bool) -> String {
    if monthly { format!("{} {}", month_name(bucket.start.month(), false), bucket.start.year()) } else { day_label(bucket.start) }
}

/// A rounded step for the minutes axis: about three grid lines.
fn axis_step(max_minutes: f64) -> f64 {
    let raw = (max_minutes / 3.0).max(1.0);
    let magnitude = 10f64.powf(raw.log10().floor());
    let normalized = raw / magnitude;
    let nice = if normalized <= 1.0 {
        1.0
    } else if normalized <= 2.0 {
        2.0
    } else if normalized <= 5.0 {
        5.0
    } else {
        10.0
    };
    nice * magnitude
}

/// Minutes per day or month, drawn with the painter. Every bar has hover text and an accessible label.
fn timeline_chart(ui: &mut Ui, summary: &ListeningSummary) {
    let monthly = summary.period.is_monthly();
    let palette = Palette::of_ui(ui);
    widgets::card(ui, |ui| {
        ui.label(theme::text(l10n::t("Minutes listened"), Font::Headline));
        ui.add_space(6.0);
        let height = 190.0;
        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
        let axis_width = 40.0;
        let label_height = 18.0;
        let plot = Rect::from_min_max(pos2(rect.left() + axis_width, rect.top() + 6.0), pos2(rect.right(), rect.bottom() - label_height));
        let max_minutes = summary.timeline.iter().map(|b| b.seconds / 60.0).fold(0.0, f64::max);
        let step = axis_step(max_minutes);
        let top = (max_minutes / step).ceil().max(1.0) * step;
        let painter = ui.painter_at(rect);
        let small = FontId::proportional(11.0);
        let mut value = 0.0;
        while value <= top + 0.001 {
            let y = plot.bottom() - (value / top) as f32 * plot.height();
            painter.line_segment([pos2(plot.left(), y), pos2(plot.right(), y)], Stroke::new(1.0, palette.secondary.gamma_multiply(0.2)));
            painter.text(pos2(plot.left() - 8.0, y), Align2::RIGHT_CENTER, format!("{value:.0}"), small.clone(), palette.secondary);
            value += step;
        }
        let count = summary.timeline.len();
        let slot = plot.width() / count as f32;
        let bar_width = (slot * 0.7).clamp(2.0, 30.0);
        let label_every = match (monthly, count) {
            (false, n) if n <= 7 => 1,
            (false, _) => 5,
            (true, n) => n.div_ceil(12).max(1),
        };
        let hover = response.hover_pos();
        let mut hovered: Option<&Bucket> = None;
        for (index, bucket) in summary.timeline.iter().enumerate() {
            let center = plot.left() + slot * (index as f32 + 0.5);
            let column = Rect::from_min_max(pos2(center - slot / 2.0, plot.top()), pos2(center + slot / 2.0, plot.bottom()));
            let is_hovered = hover.is_some_and(|p| column.x_range().contains(p.x));
            if is_hovered {
                hovered = Some(bucket);
                painter.rect_filled(column, CornerRadius::same(4), palette.hover);
            }
            let minutes = bucket.seconds / 60.0;
            let bar_height = ((minutes / top) as f32 * plot.height()).max(if minutes > 0.0 { 2.0 } else { 0.0 });
            if bar_height > 0.0 {
                let bar = Rect::from_min_max(
                    pos2(center - bar_width / 2.0, plot.bottom() - bar_height),
                    pos2(center + bar_width / 2.0, plot.bottom()),
                );
                let color = if is_hovered { palette.accent } else { palette.accent.gamma_multiply(0.85) };
                painter.rect_filled(bar, CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 }, color);
            }
            let from_end = count - 1 - index;
            if from_end.is_multiple_of(label_every) {
                let text = if monthly {
                    month_name(bucket.start.month(), true).to_string()
                } else if count <= 7 {
                    weekday_name(bucket.start.weekday()).to_string()
                } else {
                    format!("{} {}", bucket.start.day(), month_name(bucket.start.month(), true))
                };
                painter.text(pos2(center, rect.bottom() - 2.0), Align2::CENTER_BOTTOM, text, small.clone(), palette.secondary);
            }
            // An accessible element per bar, with its value.
            let label = format!("{}: {}", bucket_label(bucket, monthly), l10n::short_duration(bucket.seconds));
            let bar_response = ui.interact(column, ui.scope_id().with(("timeline-bar", index)), Sense::hover());
            bar_response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &label));
            bar_response.on_hover_text(label);
        }
        let caption =
            hovered.map(|b| format!("{} · {}", bucket_label(b, monthly), l10n::short_duration(b.seconds))).unwrap_or_else(|| " ".into());
        ui.label(theme::secondary(ui, caption, Font::Subheadline).monospace());
    });
}

// MARK: Rankings

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Song,
    Artist,
    Album,
    Genre,
}

fn rankings(app: &mut App, ui: &mut Ui, summary: &ListeningSummary) {
    let sections = [
        (l10n::t("Top songs"), &summary.songs, Kind::Song),
        (l10n::t("Top artists"), &summary.artists, Kind::Artist),
        (l10n::t("Top albums"), &summary.albums, Kind::Album),
        (l10n::t("Top genres"), &summary.genres, Kind::Genre),
    ];
    let sections: Vec<_> = sections.into_iter().filter(|(_, ranks, _)| !ranks.is_empty()).collect();
    let available = ui.available_width();
    if available < TWO_COLUMNS {
        for (title, ranks, kind) in sections {
            rank_section(app, ui, &title, ranks, kind);
        }
        return;
    }
    let gap = 24.0;
    let width = ((available - gap) / 2.0).floor();
    for pair in sections.chunks(2) {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for (title, ranks, kind) in pair {
                ui.allocate_ui_with_layout(vec2(width, 0.0), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(width);
                    rank_section(app, ui, title, ranks, *kind);
                });
            }
        });
    }
}

fn rank_section(app: &mut App, ui: &mut Ui, title: &str, ranks: &[Rank], kind: Kind) {
    ui.add_space(12.0);
    ui.label(theme::text(title, Font::Title3));
    ui.add_space(4.0);
    for (index, rank) in ranks.iter().enumerate() {
        let target = match kind {
            Kind::Song => Some(icons::PLAY_CIRCLE),
            Kind::Artist if rank.song.artist_id.is_some() => Some(icons::CARET_RIGHT),
            Kind::Album if rank.song.album_id.is_some() => Some(icons::CARET_RIGHT),
            _ => None,
        };
        if rank_row(app, ui, index, rank, kind, target).clicked() {
            match kind {
                Kind::Song => player::play_songs(app, vec![rank.song.clone()], 0, false),
                Kind::Artist => {
                    if let Some(id) = rank.song.artist_id.clone() {
                        app.nav.push(Route::Artist(id));
                    }
                }
                Kind::Album => {
                    if let Some(id) = rank.song.album_id.clone() {
                        app.nav.push(Route::Album(id));
                    }
                }
                Kind::Genre => {}
            }
        }
    }
}

fn rank_row(app: &mut App, ui: &mut Ui, index: usize, rank: &Rank, kind: Kind, target: Option<&str>) -> egui::Response {
    let palette = Palette::of_ui(ui);
    let width = ui.available_width();
    let sense = if target.is_some() { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(vec2(width, 58.0), sense);
    if response.hovered() && target.is_some() {
        ui.painter().rect_filled(rect, CornerRadius::same(8), palette.hover);
    }
    let subtitle = rank
        .subtitle
        .iter()
        .cloned()
        .chain([l10n::short_duration(rank.seconds), l10n::count("%lld plays", rank.plays)])
        .collect::<Vec<_>>()
        .join(" · ");
    let mut child =
        ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(8.0, 4.0))).layout(Layout::left_to_right(Align::Center)));
    let ui = &mut child;
    ui.allocate_ui_with_layout(vec2(24.0, 40.0), Layout::centered_and_justified(egui::Direction::LeftToRight), |ui| {
        ui.label(theme::secondary(ui, (index + 1).to_string(), Font::Headline));
    });
    if kind != Kind::Genre {
        widgets::artwork(ui, app, rank.song.album_id.as_deref().unwrap_or(&rank.song.id), rank.song.cover_art.as_deref(), 44.0, 8.0);
    }
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        if let Some(icon) = target {
            ui.label(theme::icon(icon, 18.0).color(palette.accent));
        }
        ui.with_layout(Layout::top_down(Align::Min), |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.add_space(8.0);
            ui.add(egui::Label::new(theme::text(&rank.title, Font::Headline)).truncate().selectable(false));
            ui.add(egui::Label::new(theme::secondary(ui, &subtitle, Font::Caption)).truncate().selectable(false));
        });
    });
    let label = format!("{}. {}, {subtitle}", index + 1, rank.title);
    let role = if target.is_some() { egui::Role::Button } else { egui::Role::Label };
    response.widget_info(|| egui::WidgetInfo::labeled(role, true, &label));
    if kind == Kind::Song { response.on_hover_text(l10n::t("Play")) } else { response }
}

// MARK: Settings > Rediscover your music

/// Settings > Rediscover your music: statistics entry, history review and clearing.
pub fn rediscover_settings(app: &mut App, ui: &mut Ui) {
    let palette = Palette::of_ui(ui);
    widgets::card(ui, |ui| {
        if settings_row(ui, icons::CHART_BAR, &l10n::t("Listening statistics"), icons::CARET_RIGHT).clicked() {
            app.nav.push(Route::Statistics);
        }
        ui.separator();
        let expanded = app.discovery.ui.history_expanded;
        let caret = if expanded { icons::CARET_DOWN } else { icons::CARET_RIGHT };
        if settings_row(ui, icons::CLOCK_COUNTER_CLOCKWISE, &l10n::t("Listening history"), caret).clicked() {
            app.discovery.ui.history_expanded = !expanded;
        }
        ui.separator();
        let enabled = app.session.has_library();
        let clear = ui
            .add_enabled(
                enabled,
                egui::Button::new(
                    theme::text(format!("{}  {}", icons::TRASH, l10n::t("Clear listening history")), Font::Body).color(palette.danger),
                )
                .frame(false)
                .min_size(vec2(0.0, 36.0)),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if clear.clicked() {
            app.discovery.ui.confirm_clear = true;
        }
    });
    ui.add_space(6.0);
    crate::ui::settings::footer(
        ui,
        &l10n::t(
            "Listening history stays on this device, separately for each library. Suggestions use a sample of your collection and do not include listening in other apps.",
        ),
    );
    if app.discovery.ui.history_expanded {
        ui.add_space(12.0);
        history_list(app, ui);
    }
}

fn settings_row(ui: &mut Ui, icon: &str, title: &str, trailing: &str) -> egui::Response {
    let palette = Palette::of_ui(ui);
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect.expand2(vec2(6.0, 0.0)), CornerRadius::same(8), palette.hover);
    }
    let painter = ui.painter();
    painter.text(pos2(rect.left() + 10.0, rect.center().y), Align2::CENTER_CENTER, icon, FontId::proportional(18.0), palette.accent);
    painter.text(pos2(rect.left() + 30.0, rect.center().y), Align2::LEFT_CENTER, title, Font::Body.id(), palette.text);
    painter.text(pos2(rect.right() - 6.0, rect.center().y), Align2::RIGHT_CENTER, trailing, FontId::proportional(16.0), palette.secondary);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, title));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The 100 most recent songs in this library's history; clicking one plays it.
fn history_list(app: &mut App, ui: &mut Ui) {
    ui.label(theme::text(l10n::t("Listening history"), Font::Title3));
    ui.add_space(4.0);
    let entries: Vec<_> = discovery::history(app)
        .entries()
        .iter()
        .filter(|e| !app.settings.downloaded_only || downloads::is_downloaded(app, &e.song))
        .take(100)
        .map(|e| (e.song.clone(), e.last_played, e.play_count))
        .collect();
    if entries.is_empty() {
        ui.label(theme::secondary(ui, l10n::t("Your listening story starts with a song."), Font::Subheadline));
        return;
    }
    let palette = Palette::of_ui(ui);
    for (song, last_played, plays) in entries {
        let date = chrono::Local.timestamp_millis_opt(last_played).single().map(|d| date_label(d.date_naive())).unwrap_or_default();
        let title = format!("{} · {}", song.title, song.artist.clone().unwrap_or_else(|| l10n::t("Unknown artist")));
        let detail = format!("{date} · {}", l10n::count("%lld plays", plays));
        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 46.0), Sense::click());
        if response.hovered() {
            ui.painter().rect_filled(rect, CornerRadius::same(8), palette.hover);
        }
        let painter = ui.painter_at(rect);
        painter.text(
            pos2(rect.left() + 16.0, rect.center().y),
            Align2::CENTER_CENTER,
            icons::PLAY_CIRCLE,
            FontId::proportional(20.0),
            palette.accent,
        );
        painter.text(pos2(rect.left() + 36.0, rect.top() + 14.0), Align2::LEFT_CENTER, &title, Font::Body.id(), palette.text);
        painter.text(pos2(rect.left() + 36.0, rect.bottom() - 12.0), Align2::LEFT_CENTER, &detail, Font::Caption.id(), palette.secondary);
        let label = format!("{title}, {detail}");
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
        if response.on_hover_text(l10n::t("Play")).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            player::play_songs(app, vec![song], 0, false);
        }
    }
}

/// The Clear listening history confirmation.
pub fn windows(app: &mut App, ctx: &egui::Context) {
    if !app.discovery.ui.confirm_clear {
        return;
    }
    let palette = Palette::of(ctx);
    let mut clear = false;
    let mut cancel = false;
    let modal = crate::ui::widgets::modal(ctx, egui::Id::unique("clear-listening-history")).show(ctx, |ui| {
        ui.set_width(400.0);
        ui.label(theme::text(l10n::t("Clear listening history?"), Font::Title3));
        ui.add_space(6.0);
        ui.label(theme::text(
            l10n::t("This clears this library's local listening history and resume position. Your music, playlists and server history are kept."),
            Font::Body,
        ));
        ui.add_space(12.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let button = egui::Button::new(theme::text(l10n::t("Clear listening history"), Font::Headline).color(Color32::WHITE))
                .fill(palette.danger_fill)
                .corner_radius(CornerRadius::same(12))
                .min_size(vec2(0.0, 36.0));
            clear = ui.add(button).clicked();
            cancel = ui.button(l10n::t("Cancel")).clicked();
        });
    });
    if clear {
        discovery::clear_history(app);
        app.toast(l10n::t("Listening history cleared"));
    }
    if clear || cancel || modal.should_close() {
        app.discovery.ui.confirm_clear = false;
    }
}
