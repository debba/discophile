//! Audiobooks: the library page, a book's page with its embedded player, the Audiobookshelf
//! connection form and settings sections.

mod player;

use discophile_core::audiobook_reader::{AudiobookReaderPreferences, AudiobookTimeline};
use discophile_core::audiobookshelf::Audiobook;
use discophile_core::l10n::{count, t};
use discophile_core::offline_audiobook::OfflineAudiobook;
use egui::{Align, Color32, CornerRadius, Layout, Rect, Response, Sense, Ui, vec2};

use crate::app::App;
use crate::features::audiobooks::{self, Dialog, account, covers, downloads, library};
use crate::nav::{Route, SettingsPage};
use crate::theme::{self, Font, Palette, icons};
use crate::ui::widgets;

const TILE_WIDTH: f32 = 168.0;

// MARK: Shared pieces

/// A book cover with the iOS placeholder (gradient and a closed book) while it loads.
pub fn cover(ui: &mut Ui, app: &mut App, book: &str, size: f32, radius: f32) -> Response {
    let texture = covers::texture(app, book);
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    paint_cover(ui, rect, book, texture.as_ref(), radius);
    response
}

fn paint_cover(ui: &Ui, rect: Rect, seed: &str, texture: Option<&egui::TextureHandle>, radius: f32) {
    if texture.is_some() {
        widgets::paint_artwork(ui, rect, seed, texture, radius);
        return;
    }
    let corner = CornerRadius::same(radius.round().clamp(0.0, 255.0) as u8);
    let [top, bottom] = theme::ARTWORK_PALETTES[theme::artwork_seed(seed)];
    widgets::paint_gradient(ui, rect, theme::hex(top), theme::hex(bottom), corner);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        icons::BOOK,
        egui::FontId::proportional(rect.width() * 0.34),
        Color32::from_white_alpha(217),
    );
}

/// Server descriptions are HTML; show them as plain text without loading anything remote.
fn plain_description(html: &str) -> String {
    let html = html.replace("<br>", "\n").replace("<br/>", "\n").replace("<br />", "\n").replace("</p>", "\n\n");
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => text.push(c),
            _ => {}
        }
    }
    let text = text
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let mut out = lines.join("\n");
    while out.contains("\n\n\n") {
        out = out.replace("\n\n\n", "\n\n");
    }
    out.trim().to_string()
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1000 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    value /= 1000.0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if value >= 100.0 { format!("{value:.0} {}", UNITS[unit]) } else { format!("{value:.1} {}", UNITS[unit]) }
}

/// A thin progress bar along the bottom of a cover.
fn paint_progress(ui: &Ui, rect: Rect, fraction: f64) {
    if fraction <= 0.0 {
        return;
    }
    let palette = Palette::of_ui(ui);
    let bar = Rect::from_min_size(rect.left_bottom() + vec2(8.0, -10.0), vec2(rect.width() - 16.0, 4.0));
    ui.painter().rect_filled(bar, CornerRadius::same(2), Color32::from_black_alpha(140));
    let filled = Rect::from_min_size(bar.min, vec2(bar.width() * fraction.clamp(0.0, 1.0) as f32, bar.height()));
    ui.painter().rect_filled(filled, CornerRadius::same(2), palette.accent);
}

fn small_badge(ui: &Ui, rect: Rect, icon: &str) {
    let center = rect.right_bottom() + vec2(-18.0, -18.0);
    ui.painter().circle_filled(center, 13.0, Color32::from_black_alpha(170));
    ui.painter().text(center, egui::Align2::CENTER_CENTER, icon, egui::FontId::new(15.0, theme::icon_fill()), Color32::WHITE);
}

/// A grid tile: cover, title, author and a status line. Clicking opens the book.
fn book_tile(ui: &mut Ui, app: &mut App, book: &Audiobook, width: f32, record: Option<&OfflineAudiobook>) {
    let account = app.audiobooks.account.as_ref().map(|a| a.id()).unwrap_or_default();
    let saved = app.audiobooks.sync.saved(&account, &book.id);
    let current = audiobooks::is_current(app, &book.id);
    let art = cover(ui, app, &book.id, width, 12.0);
    if let Some(progress) = saved.filter(|p| !p.is_finished) {
        paint_progress(ui, art.rect, progress.progress());
    }
    if record.is_some_and(OfflineAudiobook::ready) {
        small_badge(ui, art.rect, icons::ARROW_CIRCLE_DOWN);
    } else if current {
        small_badge(ui, art.rect, icons::SPEAKER_HIGH);
    }
    let palette = Palette::of_ui(ui);
    let title_color = if current { palette.accent } else { palette.text };
    let title = ui.add(egui::Label::new(theme::text(book.title(), Font::Headline).color(title_color)).truncate().sense(Sense::click()));
    let author = book.author();
    let mut response = art.union(title);
    if !author.is_empty() {
        response =
            response.union(ui.add(egui::Label::new(theme::secondary(ui, author, Font::Subheadline)).truncate().sense(Sense::click())));
    }
    let status = match record {
        Some(r) if r.ready() => Some(t("Downloaded")),
        Some(r) if r.queued() => Some(t("Downloading…")),
        Some(_) => Some(t("Download incomplete")),
        None => match saved {
            Some(p) if p.is_finished => Some(t("Finished")),
            _ => book.media.duration.map(AudiobookTimeline::clock),
        },
    };
    if let Some(status) = status {
        ui.label(theme::secondary(ui, status, Font::Footnote));
    }
    if let Some(record) = record.filter(|r| r.queued()) {
        let fraction = downloads_fraction(app, record);
        ui.add(egui::ProgressBar::new(fraction as f32).desired_width(width).desired_height(4.0)).on_hover_text(t("Download progress"));
    }
    let label = format!("{}, {}", book.title(), book.author());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
    if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        app.nav.push(Route::Audiobook(book.id.clone()));
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn downloads_fraction(app: &App, record: &OfflineAudiobook) -> f64 {
    downloads::fraction(app, record)
}

#[cfg(target_arch = "wasm32")]
fn downloads_fraction(_app: &App, _record: &OfflineAudiobook) -> f64 {
    0.0
}

fn not_connected(app: &mut App, ui: &mut Ui) {
    widgets::page(ui, "audiobooks-welcome", |ui| {
        widgets::page_title(ui, &t("Audiobooks"));
        widgets::empty_state(
            ui,
            icons::BOOKS,
            &t("Your books, at your own pace"),
            &t("Connect Audiobookshelf to browse your audiobooks and continue where you left off."),
        );
        ui.vertical_centered(|ui| {
            if widgets::action_button(ui, t("Connect Audiobookshelf"), false).clicked() {
                open_connect(app);
            }
            if let Some(error) = &app.audiobooks.error {
                ui.add_space(8.0);
                ui.label(theme::secondary(ui, error, Font::Footnote));
            }
        });
    });
}

fn open_connect(app: &mut App) {
    app.audiobooks.form.reset();
    app.audiobooks.dialog = Some(Dialog::Connect);
}

// MARK: Library page

pub fn show(app: &mut App, ui: &mut Ui) {
    if app.audiobooks.account.is_none() {
        not_connected(app, ui);
        return;
    }
    library::ensure_loaded(app);
    library::tick(app);
    let account = app.audiobooks.account.clone().expect("connected");
    widgets::page(ui, "audiobooks", |ui| {
        widgets::page_header(ui, &t("Audiobooks"), |ui| {
            if widgets::icon_button(ui, icons::GEAR, &t("Settings"), 18.0).clicked() {
                app.nav.push(Route::Settings(SettingsPage::Accounts));
            }
            let downloaded_only = app.audiobooks.browse.downloaded_only;
            if !downloaded_only
                && app.audiobooks.client.is_some()
                && widgets::icon_button(ui, icons::ARROWS_CLOCKWISE, &t("Refresh"), 18.0).clicked()
            {
                library::refresh(app);
            }
        });
        ui.label(theme::secondary(ui, account.display_name(), Font::Subheadline));
        ui.add_space(8.0);
        toolbar(app, ui);
        ui.add_space(6.0);
        if let Some(error) = app.audiobooks.browse.error.clone() {
            if widgets::error_banner(ui, &error, true) {
                app.audiobooks.browse.error = None;
                library::refresh(app);
            }
            ui.add_space(6.0);
        } else if let Some(error) = app.audiobooks.error.clone() {
            widgets::error_banner(ui, &error, false);
            if ui.button(t("Connect Audiobookshelf")).clicked() {
                open_connect(app);
            }
        }
        for note in [app.audiobooks.sync.error.clone(), app.audiobooks.downloads.error.clone()].into_iter().flatten() {
            ui.horizontal_wrapped(|ui| {
                ui.label(theme::icon(icons::INFO, 15.0).color(Palette::of_ui(ui).secondary));
                ui.label(theme::secondary(ui, note, Font::Footnote));
            });
        }
        let browse = &app.audiobooks.browse;
        let query = browse.search_text().to_lowercase();
        let downloaded_only = browse.downloaded_only;
        let account_id = account.id();
        let mut saved: Vec<OfflineAudiobook> = app
            .audiobooks
            .downloads
            .records
            .iter()
            .filter(|r| r.account == account_id && (!downloaded_only || r.ready()))
            .filter(|r| query.is_empty() || format!("{} {}", r.book.title(), r.book.author()).to_lowercase().contains(&query))
            .cloned()
            .collect();
        saved.sort_by_key(|r| r.book.title().to_lowercase());
        if !saved.is_empty() {
            widgets::section_heading(ui, &t("On this computer"), None);
            widgets::tile_grid(ui, &saved, TILE_WIDTH, |ui, record, width| book_tile(ui, app, &record.book, width, Some(record)));
        }
        let browse = &app.audiobooks.browse;
        let loading = browse.loading;
        let books: Vec<Audiobook> = if downloaded_only {
            Vec::new()
        } else {
            browse.books.iter().filter(|b| !saved.iter().any(|r| r.book.id == b.id)).cloned().collect()
        };
        let empty = browse.books.is_empty() && saved.is_empty() && browse.error.is_none();
        let many_matches = !browse.search_text().is_empty() && browse.books.len() >= discophile_core::audiobookshelf::SEARCH_LIMIT;
        if !books.is_empty() {
            if !saved.is_empty() {
                widgets::section_heading(ui, &t("Audiobook library"), None);
            } else {
                ui.add_space(8.0);
            }
            widgets::tile_grid(ui, &books, TILE_WIDTH, |ui, book, width| book_tile(ui, app, book, width, None));
        }
        if loading {
            ui.vertical_centered(|ui| {
                ui.add_space(12.0);
                ui.spinner();
                ui.label(theme::secondary(ui, t("Loading audiobooks…"), Font::Footnote));
            });
        } else if library::can_load_more(app) {
            ui.vertical_centered(|ui| {
                if widgets::action_button(ui, t("Load more audiobooks"), true).clicked() {
                    library::load_more(app);
                }
            });
        }
        if !loading && empty {
            if downloaded_only {
                widgets::empty_state(
                    ui,
                    icons::ARROW_CIRCLE_DOWN,
                    &t("No audiobooks found"),
                    &t("Turn off Downloaded only to browse your server and save something for later."),
                );
            } else {
                widgets::empty_state(
                    ui,
                    icons::BOOKS,
                    &t("No audiobooks found"),
                    &t("Choose another library or search for a different title or author."),
                );
            }
        }
        if many_matches {
            ui.label(theme::secondary(ui, t("Showing the first 100 matches. Refine your search to find more books."), Font::Footnote));
        }
    });
}

fn toolbar(app: &mut App, ui: &mut Ui) {
    ui.horizontal_wrapped(|ui| {
        let before = app.audiobooks.browse.query.clone();
        widgets::search_field(ui, &mut app.audiobooks.browse.query, &t("Titles and authors"), 300.0);
        if app.audiobooks.browse.query != before {
            library::query_changed(app);
        }
        let browse = &app.audiobooks.browse;
        if !browse.downloaded_only && browse.libraries.len() > 1 {
            let mut selected = browse.selected.clone();
            let name = browse.libraries.iter().find(|l| l.id == selected).map(|l| l.name.clone()).unwrap_or_default();
            let libraries = browse.libraries.clone();
            egui::ComboBox::from_id_salt("audiobook-library")
                .selected_text(name)
                .width(200.0)
                .show_ui(ui, |ui| {
                    for library in &libraries {
                        ui.selectable_value(&mut selected, library.id.clone(), &library.name);
                    }
                })
                .response
                .on_hover_text(t("Audiobook library"));
            if selected != app.audiobooks.browse.selected {
                library::select_library(app, selected);
            }
        }
        if !crate::platform::IS_WEB {
            let downloaded_only = app.audiobooks.browse.downloaded_only;
            if widgets::chip(ui, downloaded_only, &t("Downloaded only")).clicked() {
                library::set_downloaded_only(app, !downloaded_only);
            }
        }
    });
}

// MARK: Book page

pub fn book(app: &mut App, ui: &mut Ui, id: &str) {
    if app.audiobooks.account.is_none() {
        not_connected(app, ui);
        return;
    }
    library::ensure_detail(app, id);
    let book = library::known_book(app, id);
    let current = audiobooks::is_current(app, id);
    widgets::page(ui, ("audiobook", id), |ui| {
        let Some(book) = book else {
            match app.audiobooks.detail.as_ref().and_then(|d| d.error.clone()) {
                Some(error) => {
                    if widgets::error_banner(ui, &error, true) {
                        library::load_detail(app, id);
                    }
                }
                None => widgets::loading(ui),
            }
            return;
        };
        header(app, ui, &book, current);
        if let Some(error) = app.audiobooks.detail.as_ref().filter(|d| d.id == id).and_then(|d| d.error.clone()) {
            ui.add_space(8.0);
            if widgets::error_banner(ui, &error, true) {
                library::load_detail(app, id);
            }
        }
        if current {
            ui.add_space(14.0);
            player::section(app, ui);
        }
        if let Some(description) = book.media.metadata.description.as_deref().map(plain_description).filter(|d| !d.is_empty()) {
            widgets::section_heading(ui, &t("Description"), None);
            ui.add(egui::Label::new(theme::secondary(ui, description, Font::Body)).wrap());
        }
        chapters(app, ui, &book, current);
        ui.add_space(18.0);
        let note = if crate::platform::IS_WEB {
            t(
                "Downloads are available in the desktop app. In the browser, books stream from your server and your position syncs while you listen.",
            )
        } else {
            t(
                "Downloaded books play without a connection. Your position is saved on this computer and syncs when you reconnect. Music and audiobook download filters are separate.",
            )
        };
        ui.label(theme::secondary(ui, note, Font::Footnote));
    });
}

fn header(app: &mut App, ui: &mut Ui, book: &Audiobook, current: bool) {
    let wide = ui.available_width() > 620.0;
    let size = if wide { 230.0 } else { (ui.available_width()).min(260.0) };
    let layout = if wide { Layout::left_to_right(Align::Min) } else { Layout::top_down(Align::Min) };
    ui.with_layout(layout, |ui| {
        let art = cover(ui, app, &book.id, size, 14.0);
        if current && app.audiobooks.playback.as_ref().is_some_and(|p| p.offline) {
            small_badge(ui, art.rect, icons::ARROW_CIRCLE_DOWN);
        }
        ui.add_space(if wide { 22.0 } else { 12.0 });
        ui.with_layout(Layout::top_down(Align::Min), |ui| {
            ui.label(theme::secondary(ui, t("Audiobook").to_uppercase(), Font::Caption));
            ui.add(egui::Label::new(theme::text(book.title(), Font::Title)).wrap());
            if let Some(subtitle) = book.media.metadata.subtitle.as_deref().filter(|s| !s.is_empty()) {
                ui.label(theme::secondary(ui, subtitle, Font::Callout));
            }
            let author = book.author();
            if !author.is_empty() {
                ui.label(theme::text(author, Font::Title3).color(Palette::of_ui(ui).accent));
            }
            let narrator = book.narrator();
            let mut facts = Vec::new();
            if !narrator.is_empty() {
                facts.push(format!("{}: {narrator}", t("Narrator")));
            }
            if let Some(duration) = book.media.duration.filter(|d| *d > 0.0) {
                facts.push(format!("{}: {}", t("Duration"), AudiobookTimeline::clock(duration)));
            }
            if !facts.is_empty() {
                ui.label(theme::secondary(ui, facts.join("  ·  "), Font::Subheadline));
            }
            let progress = app.audiobooks.detail.as_ref().filter(|d| d.id == book.id).and_then(|d| d.progress);
            if let Some(progress) = progress.filter(|p| p.current_time > 0.0) {
                let label = if progress.is_finished { t("Finished") } else { t("Saved position") };
                ui.label(theme::secondary(
                    ui,
                    format!(
                        "{label}: {} / {}",
                        AudiobookTimeline::clock(progress.current_time),
                        AudiobookTimeline::clock(progress.duration)
                    ),
                    Font::Subheadline,
                ));
            }
            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                primary_action(app, ui, book, current, progress.is_some_and(|p| p.current_time > 0.0 && !p.is_finished));
                #[cfg(not(target_arch = "wasm32"))]
                download_controls(app, ui, &book.id);
            });
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(error) = app.audiobooks.downloads.error.clone() {
                ui.label(theme::secondary(ui, error, Font::Footnote));
            }
        });
    });
}

fn primary_action(app: &mut App, ui: &mut Ui, book: &Audiobook, current: bool, has_progress: bool) {
    let starting = app.audiobooks.starting.as_ref().is_some_and(|(id, _)| *id == book.id);
    let loading = app.audiobooks.detail.as_ref().is_some_and(|d| d.id == book.id && d.loading);
    if current {
        let playing = app.player.status.playing;
        let (icon, label) = if playing { (icons::PAUSE, t("Pause")) } else { (icons::PLAY, t("Play")) };
        if widgets::pill_button(ui, icon, label, true).clicked() {
            audiobooks::toggle(app);
        }
        return;
    }
    let label = if has_progress { t("Continue listening") } else { t("Start") };
    let enabled = !starting && !loading && app.audiobooks.starting.is_none();
    if ui.add_enabled_ui(enabled, |ui| widgets::pill_button(ui, icons::PLAY, label, true)).inner.clicked() {
        audiobooks::start_book(app, &book.id, None);
    }
    if starting || loading {
        ui.spinner();
        ui.label(theme::secondary(ui, t("Loading audiobook…"), Font::Footnote));
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn download_controls(app: &mut App, ui: &mut Ui, id: &str) {
    if app.audiobooks.client.is_none() && downloads::record(app, id).is_none() {
        return;
    }
    let record = downloads::record(app, id).cloned();
    let palette = Palette::of_ui(ui);
    match record {
        Some(record) if record.ready() => {
            ui.label(theme::icon_filled(icons::ARROW_CIRCLE_DOWN, 18.0).color(palette.positive));
            ui.label(theme::text(t("Downloaded"), Font::Callout).color(palette.positive));
            ui.label(theme::secondary(ui, format_bytes(record.bytes()), Font::Footnote));
            if widgets::icon_button(ui, icons::TRASH, &t("Remove audiobook download"), 17.0).clicked() {
                app.audiobooks.dialog = Some(Dialog::RemoveDownload(id.to_string()));
            }
        }
        _ if downloads::is_preparing(app, id) => {
            ui.spinner();
            ui.label(theme::secondary(ui, t("Preparing audiobook download…"), Font::Footnote));
            if ui.button(t("Cancel download")).clicked() {
                downloads::cancel_preparation(app, id);
            }
        }
        Some(record) if record.queued() => {
            let fraction = downloads::fraction(app, &record);
            ui.add(egui::ProgressBar::new(fraction as f32).desired_width(160.0).show_percentage()).on_hover_text(t("Download progress"));
            if ui.button(t("Cancel download")).clicked() {
                downloads::remove(app, id);
            }
        }
        record => {
            let label = if record.is_none() { t("Download audiobook") } else { t("Retry audiobook download") };
            let enabled = app.audiobooks.client.is_some();
            if ui.add_enabled_ui(enabled, |ui| widgets::pill_button(ui, icons::ARROW_CIRCLE_DOWN, label, false)).inner.clicked() {
                downloads::download(app, id);
            }
            if record.is_some() && ui.button(t("Remove audiobook download")).clicked() {
                downloads::remove(app, id);
            }
        }
    }
}

fn chapters(app: &mut App, ui: &mut Ui, book: &Audiobook, current: bool) {
    let (timeline, position) = match app.audiobooks.playback.as_ref().filter(|_| current) {
        Some(pb) => (pb.timeline.clone(), Some(pb.position)),
        None => {
            let chapters = book.media.chapters.clone().unwrap_or_default();
            if chapters.is_empty() {
                return;
            }
            let duration = book.media.duration.unwrap_or_else(|| chapters.iter().map(|c| c.end).fold(0.0, f64::max));
            (AudiobookTimeline::new(duration, &chapters, book.media.tracks.as_deref().unwrap_or_default()), None)
        }
    };
    if timeline.chapters.len() < 2 && !current {
        return;
    }
    let active = position.and_then(|p| timeline.chapter(p)).map(|c| c.id);
    // Only plural counts are shown, so the key needs no plural forms.
    let subtitle = (timeline.chapters.len() > 1).then(|| count("%lld chapters", timeline.chapters.len() as i64));
    widgets::section_heading(ui, &t("Chapters"), subtitle.as_deref());
    let palette = Palette::of_ui(ui);
    let disabled = app.audiobooks.starting.is_some();
    for chapter in &timeline.chapters {
        let is_active = active == Some(chapter.id);
        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::click());
        if response.hovered() || is_active {
            ui.painter().rect_filled(
                rect,
                CornerRadius::same(8),
                if is_active { palette.accent.gamma_multiply(0.12) } else { palette.hover },
            );
        }
        let mut child =
            ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(10.0, 4.0))).layout(Layout::left_to_right(Align::Center)));
        let row = &mut child;
        let marker = if is_active {
            theme::icon(if app.player.status.playing { icons::SPEAKER_HIGH } else { icons::SPEAKER_NONE }, 15.0).color(palette.accent)
        } else {
            theme::secondary(row, (chapter.id + 1).to_string(), Font::Callout)
        };
        row.allocate_ui_with_layout(vec2(28.0, 36.0), Layout::centered_and_justified(egui::Direction::LeftToRight), |ui| ui.label(marker));
        row.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(theme::secondary(ui, AudiobookTimeline::clock(chapter.duration()), Font::Callout).monospace());
            ui.label(theme::secondary(ui, AudiobookTimeline::clock(chapter.start), Font::Footnote).monospace());
            ui.add_space(8.0);
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                let color = if is_active { palette.accent } else { palette.text };
                ui.add(egui::Label::new(theme::text(chapter.display_title(), Font::Body).color(color)).truncate().selectable(false));
            });
        });
        let label = format!("{}, {}", chapter.display_title(), AudiobookTimeline::clock(chapter.start));
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, !disabled, &label));
        if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && !disabled {
            if current {
                audiobooks::seek(app, chapter.start);
            } else {
                audiobooks::start_book(app, &book.id, Some(chapter.start));
            }
        }
    }
}

// MARK: Settings

/// The Audiobookshelf section of Settings > Libraries and accounts.
pub fn account_settings(app: &mut App, ui: &mut Ui) {
    widgets::section_heading(ui, &t("Audiobooks"), None);
    widgets::card(ui, |ui| {
        let Some(account) = app.audiobooks.account.clone() else {
            ui.horizontal(|ui| {
                ui.label(theme::icon(icons::BOOKS, 22.0).color(Palette::of_ui(ui).accent));
                ui.vertical(|ui| {
                    ui.label(theme::text("Audiobookshelf", Font::Headline));
                    ui.label(theme::secondary(ui, t("Not connected"), Font::Subheadline));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if widgets::action_button(ui, t("Connect Audiobookshelf"), false).clicked() {
                        open_connect(app);
                    }
                });
            });
            return;
        };
        ui.horizontal(|ui| {
            ui.label(theme::icon(icons::BOOKS, 22.0).color(Palette::of_ui(ui).accent));
            ui.vertical(|ui| {
                ui.label(theme::text(account.display_name(), Font::Headline));
                ui.label(theme::secondary(ui, account.subtitle(), Font::Subheadline));
            });
        });
        if let Some(error) = app.audiobooks.error.clone() {
            ui.label(theme::text(error, Font::Footnote).color(Palette::of_ui(ui).danger));
        }
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if ui.button(t("Edit connection")).clicked() {
                open_connect(app);
            }
            let danger = Palette::of_ui(ui).danger;
            if ui.button(theme::text(t("Disconnect Audiobookshelf"), Font::Body).color(danger)).clicked() {
                app.audiobooks.dialog = Some(Dialog::Disconnect);
            }
        });
        #[cfg(not(target_arch = "wasm32"))]
        {
            ui.separator();
            ui.label(theme::text(t("Offline audiobooks"), Font::Headline));
            let (bytes, ready) = downloads::usage(app);
            egui::Grid::new("abs-offline").num_columns(2).spacing(vec2(24.0, 6.0)).show(ui, |ui| {
                ui.label(t("Downloaded audiobooks"));
                ui.label(theme::secondary(ui, ready.to_string(), Font::Body));
                ui.end_row();
                ui.label(t("Storage used"));
                ui.label(theme::secondary(ui, format_bytes(bytes), Font::Body));
                ui.end_row();
            });
            let has_records = !app.audiobooks.downloads.records.is_empty();
            let danger = Palette::of_ui(ui).danger;
            if ui
                .add_enabled(has_records, egui::Button::new(theme::text(t("Remove audiobook downloads"), Font::Body).color(danger)))
                .clicked()
            {
                app.audiobooks.dialog = Some(Dialog::RemoveDownloads);
            }
        }
        #[cfg(target_arch = "wasm32")]
        ui.label(theme::secondary(ui, t("Downloads are available in the desktop app."), Font::Footnote));
    });
}

/// Audiobook playback preferences (skips, speed, adjusted times) for Settings > Playback.
pub fn playback_settings(app: &mut App, ui: &mut Ui) {
    widgets::section_heading(ui, &t("Audiobooks"), None);
    widgets::card(ui, |ui| {
        let mut preferences = app.audiobooks.reader.preferences.clone();
        egui::Grid::new("abs-playback-settings").num_columns(2).spacing(vec2(24.0, 10.0)).show(ui, |ui| {
            for (label, value, salt) in [
                (t("Skip backward"), &mut preferences.backward_seconds, "abs-skip-back"),
                (t("Skip forward"), &mut preferences.forward_seconds, "abs-skip-forward"),
            ] {
                ui.label(label);
                egui::ComboBox::from_id_salt(salt).selected_text(count("%lld seconds", *value as i64)).show_ui(ui, |ui| {
                    for interval in AudiobookReaderPreferences::SKIP_INTERVALS {
                        ui.selectable_value(value, interval, count("%lld seconds", interval as i64));
                    }
                });
                ui.end_row();
            }
            ui.label(t("Playback speed"));
            ui.add(
                egui::Slider::new(&mut preferences.rate, AudiobookReaderPreferences::MIN_RATE..=AudiobookReaderPreferences::MAX_RATE)
                    .step_by(AudiobookReaderPreferences::RATE_STEP)
                    .custom_formatter(|v, _| format!("{v:.2}×")),
            );
            ui.end_row();
        });
        ui.checkbox(&mut preferences.adjust_time_for_speed, t("Adjust times to playback speed"));
        ui.label(theme::secondary(
            ui,
            t("Show listening time at the selected speed. Bookmarks and chapter positions always use the original book timeline."),
            Font::Footnote,
        ));
        if preferences != app.audiobooks.reader.preferences {
            let rate_changed = preferences.rate != app.audiobooks.reader.preferences.rate;
            app.audiobooks.reader.preferences = preferences;
            app.audiobooks.reader.save_preferences();
            if rate_changed {
                let rate = app.audiobooks.reader.preferences.rate;
                crate::features::audiobooks::playback::set_rate(app, rate);
            }
        }
    });
}

/// Audiobookshelf sign-in form body, embedded in the Add account sheet. Returns true once connected.
pub fn connect_form(app: &mut App, ui: &mut Ui) -> bool {
    let account = app.audiobooks.account.clone();
    let form = &mut app.audiobooks.form;
    form.prefill(account.as_ref());
    let palette = Palette::of_ui(ui);
    let mut submit = false;
    let suggested = discophile_core::connection_alias::suggested("Audiobookshelf", url::Url::parse(form.server.trim()).ok().as_ref(), true);
    let width = ui.available_width().min(460.0);
    ui.add_enabled_ui(!form.busy, |ui| {
        ui.label(theme::secondary(ui, t("Alias (optional)"), Font::Subheadline));
        ui.add(egui::TextEdit::singleline(&mut form.name).hint_text(suggested).desired_width(width))
            .on_hover_text(t("Leave empty to use the suggested name."));
        ui.label(theme::secondary(ui, t("Server URL"), Font::Subheadline));
        ui.add(egui::TextEdit::singleline(&mut form.server).hint_text("https://audiobooks.example.com").desired_width(width));
        ui.label(theme::secondary(ui, t("Username"), Font::Subheadline));
        ui.add(egui::TextEdit::singleline(&mut form.username).desired_width(width));
        ui.label(theme::secondary(ui, t("Password"), Font::Subheadline));
        ui.horizontal(|ui| {
            let response = ui.add(egui::TextEdit::singleline(&mut form.password).password(!form.show_password).desired_width(width - 46.0));
            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                submit = true;
            }
            let (icon, label) = if form.show_password { (icons::EYE_SLASH, t("Hide password")) } else { (icons::EYE, t("Show password")) };
            if widgets::icon_button(ui, icon, &label, 18.0).clicked() {
                form.show_password = !form.show_password;
            }
        });
        ui.checkbox(&mut form.allow_http, t("Allow HTTP on a trusted local network"));
        if form.allow_http {
            ui.label(theme::secondary(
                ui,
                t("HTTP does not encrypt your connection. Only use it on a trusted local network."),
                Font::Footnote,
            ));
        }
    });
    if account.is_some() && form.password.is_empty() {
        ui.label(theme::secondary(ui, t("Leave the password empty to change only the alias."), Font::Footnote));
    }
    if let Some(error) = &form.error {
        ui.add(egui::Label::new(theme::text(error, Font::Callout).color(palette.accent)).wrap());
    }
    ui.add_space(6.0);
    let alias_only = form.editing_alias_only(account.as_ref());
    let ready =
        !form.busy && !form.server.trim().is_empty() && !form.username.trim().is_empty() && (alias_only || !form.password.is_empty());
    ui.horizontal(|ui| {
        let label = if alias_only { t("Save alias") } else { t("Connect Audiobookshelf") };
        if ui.add_enabled_ui(ready, |ui| widgets::action_button(ui, label, false)).inner.clicked() {
            submit = true;
        }
        if form.busy {
            ui.spinner();
            ui.label(theme::secondary(ui, t("Connecting…"), Font::Footnote));
        }
    });
    ui.add_space(6.0);
    ui.add(egui::Label::new(theme::secondary(ui, account::storage_note(), Font::Footnote)).wrap());
    if submit && ready {
        account::connect(app);
    }
    let form = &mut app.audiobooks.form;
    if form.done {
        form.reset();
        return true;
    }
    false
}

// MARK: Windows

pub fn windows(app: &mut App, ctx: &egui::Context) {
    player::windows(app, ctx);
    let Some(dialog) = app.audiobooks.dialog.clone() else { return };
    if dialog == Dialog::Connect {
        let modal = egui::Modal::new(egui::Id::unique("abs-connect")).show(ctx, |ui| {
            ui.set_width(460.0);
            let close = ui
                .horizontal(|ui| {
                    ui.label(theme::text(t("Audiobookshelf connection"), Font::Title3));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        widgets::icon_button(ui, icons::X, &t("Done"), 16.0).clicked()
                    })
                    .inner
                })
                .inner;
            ui.add_space(8.0);
            let connected = connect_form(app, ui);
            close || connected
        });
        if modal.inner || modal.should_close() {
            app.audiobooks.form.reset();
            app.audiobooks.dialog = None;
        }
        return;
    }
    let (title, message, action) = match &dialog {
        Dialog::Disconnect => (
            t("Disconnect Audiobookshelf?"),
            t(
                "This removes the audiobook connection and its downloaded books from this computer and stops audiobook playback. Your server and music connections are kept.",
            ),
            t("Disconnect Audiobookshelf"),
        ),
        Dialog::RemoveDownloads => (
            t("Remove audiobook downloads?"),
            t("This removes audiobook files from this computer. Your saved positions and server library are kept."),
            t("Remove audiobook downloads"),
        ),
        Dialog::RemoveDownload(_) => (
            t("Remove audiobook download?"),
            t("This removes audiobook files from this computer. Your saved positions and server library are kept."),
            t("Remove audiobook download"),
        ),
        Dialog::Connect => unreachable!(),
    };
    let mut confirmed = false;
    let mut cancelled = false;
    let modal = egui::Modal::new(egui::Id::unique("abs-confirm")).show(ctx, |ui| {
        ui.set_width(400.0);
        ui.label(theme::text(title, Font::Title3));
        ui.add_space(6.0);
        ui.add(egui::Label::new(theme::text(message, Font::Body)).wrap());
        ui.add_space(12.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let danger = Palette::of_ui(ui).danger;
            if ui.add(egui::Button::new(theme::text(action, Font::Headline).color(Color32::WHITE)).fill(danger)).clicked() {
                confirmed = true;
            }
            if ui.button(t("Cancel")).clicked() {
                cancelled = true;
            }
        });
    });
    if confirmed {
        app.audiobooks.dialog = None;
        match dialog {
            Dialog::Disconnect => account::disconnect(app),
            Dialog::RemoveDownloads => {
                if let Some(account) = app.audiobooks.account.as_ref().map(|a| a.id()) {
                    downloads::remove_account(app, &account);
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            Dialog::RemoveDownload(book) => downloads::remove(app, &book),
            _ => {}
        }
    } else if cancelled || modal.should_close() {
        app.audiobooks.dialog = None;
    }
}
