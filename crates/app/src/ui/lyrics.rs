//! Lyrics: the side panel, the inline view in Now Playing, and Settings > Lyrics.
//! Large text with the current verse highlighted and followed; a timed line seeks when clicked. Scrolling
//! by hand stops following until "Follow lyrics" is chosen.

use discophile_core::l10n::t;
use discophile_core::lyrics::SongLyrics;
use discophile_core::lyrics_repository::LyricsSource;
use egui::{Align, CornerRadius, FontId, Margin, RichText, Sense, Stroke, Ui, vec2};

use crate::app::App;
use crate::features::lyrics::{self, LyricsStatus};
use crate::nav::SidePanel;
use crate::player;
use crate::theme::{self, Font, Palette, icons};
use crate::ui::player_bar::controls;
use crate::ui::widgets;

pub fn panel(app: &mut App, ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.label(theme::text(t("Lyrics"), Font::Title3));
        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
            if widgets::icon_button(ui, icons::X, &t("Close"), 16.0).clicked() {
                app.nav.side_panel = SidePanel::None;
            }
        });
    });
    if let Some(song) = lyrics::current_song(app) {
        ui.add(egui::Label::new(theme::text(&song.title, Font::Headline)).truncate());
        ui.add(
            egui::Label::new(theme::secondary(ui, song.artist.clone().unwrap_or_else(|| t("Unknown artist")), Font::Subheadline))
                .truncate(),
        );
    }
    ui.add_space(4.0);
    view(app, ui, 22.0);
}

/// The lyrics of the current song, filling the available space. `size` is the line font size.
pub fn view(app: &mut App, ui: &mut Ui, size: f32) {
    lyrics::ensure(app);
    if lyrics::current_song(app).is_none() {
        widgets::empty_state(ui, icons::QUOTES, &t("No lyrics available"), "");
        return;
    }
    match app.lyrics.status.clone() {
        None | Some(LyricsStatus::Loading) => {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.spinner();
                ui.label(theme::secondary(ui, t("Loading lyrics…"), Font::Callout));
            });
        }
        Some(LyricsStatus::Loaded(Some(result))) if !result.lyrics.is_empty() => {
            if result.source == LyricsSource::Lrclib
                && let Some(attribution) = result.source.attribution()
            {
                ui.hyperlink_to(theme::accent(ui, attribution, Font::Caption), "https://lrclib.net");
            }
            lines(app, ui, &result.lyrics, size);
        }
        Some(status) => {
            let failed = status == LyricsStatus::Failed;
            let message = if failed {
                t("Check your connection and try again.")
            } else if app.settings.online_lyrics {
                t("No lyrics were found for this track.")
            } else {
                t("Your server has no lyrics for this track. Enable online lyrics in Settings to search LRCLIB.")
            };
            let title = t(if failed { "Unable to load lyrics" } else { "No lyrics available" });
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.label(theme::icon(icons::QUOTES, 40.0).color(Palette::of_ui(ui).accent));
                ui.add_space(6.0);
                ui.label(theme::text(title, Font::Title3));
                ui.add(egui::Label::new(theme::secondary(ui, message, Font::Subheadline)).wrap());
                ui.add_space(10.0);
                if widgets::action_button(ui, t("Try again"), true).clicked() {
                    lyrics::retry(app);
                }
            });
        }
    }
}

#[derive(Clone, Default)]
struct Follow {
    key: Option<String>,
    paused: bool,
    last_active: Option<usize>,
}

fn lines(app: &mut App, ui: &mut Ui, lyrics: &SongLyrics, size: f32) {
    let palette = Palette::of_ui(ui);
    let synced = lyrics.is_synced();
    let id = ui.scope_id().with("lyrics-follow");
    let mut follow = ui.data(|d| d.get_temp::<Follow>(id)).unwrap_or_default();
    if follow.key != app.lyrics.key {
        follow = Follow { key: app.lyrics.key.clone(), ..Default::default() };
    }
    if !synced {
        ui.label(theme::secondary(ui, t("These lyrics aren't time-synced."), Font::Caption));
    }
    let active = lyrics.active_line(app.player.position());
    let scroll_now = synced && !follow.paused && active != follow.last_active;
    let mut seek = None;
    let output = egui::ScrollArea::vertical().id_salt(id.with("scroll")).auto_shrink([false, false]).show(ui, |ui| {
        let height = ui.clip_rect().height();
        ui.add_space(12.0);
        for (index, line) in lyrics.lines.iter().enumerate() {
            let selected = active == Some(index);
            let line_id = id.with(index);
            let hovered = synced && ui.ctx().read_response(line_id).is_some_and(|r| r.hovered());
            let text = if line.text.trim().is_empty() && synced { "•••".to_string() } else { line.text.clone() };
            // Like iOS: the current verse is bright, the rest recede; no box that could read as focus.
            let color = if selected {
                palette.accent
            } else if synced {
                palette.secondary.gamma_multiply(0.75)
            } else {
                palette.text
            };
            let fill = if hovered && !selected { palette.hover } else { egui::Color32::TRANSPARENT };
            let frame = egui::Frame::new().inner_margin(Margin::symmetric(12, 8)).corner_radius(CornerRadius::same(12)).fill(fill);
            let inner = frame.show(ui, |ui| {
                ui.set_width(ui.available_width());
                let font = FontId::new(if synced { size } else { size * 0.8 }, if synced { theme::bold() } else { theme::semibold() });
                ui.add(egui::Label::new(RichText::new(text).font(font).color(color)).wrap().selectable(!synced));
            });
            let rect = inner.response.rect;
            if let Some(start) = line.start.filter(|_| synced) {
                let response = ui.interact(rect, line_id, Sense::click());
                let label = if line.text.trim().is_empty() { t("Instrumental") } else { line.text.clone() };
                response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, &label));
                crate::ui::widgets::focus_ring(ui, &response, 12);
                if response.on_hover_text(t("Play from this line")).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    seek = Some(start);
                }
            }
            if selected && scroll_now {
                ui.scroll_to_rect(rect, Some(Align::Center));
            }
            ui.add_space(2.0);
        }
        ui.add_space(height * 0.4);
    });
    if scroll_now {
        follow.last_active = active;
    }
    let area = output.inner_rect;
    let user_scrolled = ui.rect_contains_pointer(area) && ui.input(|i| i.smooth_scroll_delta.y.abs() > 0.5 || i.is_scrolling());
    let dragging_bar = ui.rect_contains_pointer(area)
        && ui.input(|i| i.pointer.primary_down() && i.pointer.is_decidedly_dragging())
        && ui.input(|i| i.pointer.press_origin()).is_some_and(|p| p.x > area.right() - 14.0);
    if synced && (user_scrolled || dragging_bar) {
        follow.paused = true;
    }
    if let Some(start) = seek {
        player::seek(app, start);
        follow.paused = false;
        follow.last_active = None;
    }
    if synced && follow.paused {
        let size = vec2(170.0, 36.0);
        let rect = egui::Rect::from_center_size(egui::pos2(area.center().x, area.bottom() - size.y / 2.0 - 10.0), size);
        let button = egui::Button::new(
            theme::text(format!("{}  {}", icons::ARROW_LINE_DOWN, t("Follow lyrics")), Font::Headline).color(palette.accent),
        )
        .fill(palette.surface_raised)
        .stroke(Stroke::new(1.0, palette.accent.gamma_multiply(0.4)))
        .corner_radius(CornerRadius::same(18));
        if ui.put(rect, button).clicked() {
            follow.paused = false;
            follow.last_active = None;
        }
    }
    ui.data_mut(|d| d.insert_temp(id, follow));
}

/// Settings > Lyrics: online lyrics from LRCLIB (off by default) and what it sends.
pub fn settings(app: &mut App, ui: &mut Ui) {
    widgets::card(ui, |ui| {
        let mut online = app.settings.online_lyrics;
        if controls::switch_row(ui, &mut online, &t("Online lyrics (LRCLIB)")).changed() {
            lyrics::set_online(app, online);
        }
        ui.add_space(4.0);
        ui.hyperlink_to(theme::accent(ui, t("About LRCLIB"), Font::Callout), "https://lrclib.net");
    });
    ui.add_space(6.0);
    ui.label(theme::secondary(
        ui,
        t("When enabled, opening lyrics searches LRCLIB if your server has no synced text. The song title, artist, album and duration are sent to LRCLIB. Found lyrics are cached on this computer for offline use. Synced lyrics aren't available for every song."),
        Font::Footnote,
    ));
    ui.add_space(4.0);
    ui.label(theme::secondary(
        ui,
        t("Turning the option off stops LRCLIB lookups and hides its cached texts; server lyrics remain available. Try again in the lyrics view forces a fresh lookup."),
        Font::Footnote,
    ));
}
