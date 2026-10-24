//! Listen Now: discovery mixes, the saved queue, listening statistics, the curator and
//! "Choose a record for tonight". Wide windows lay the cards out in two columns.

use discophile_core::album_listening::{self, AlbumSuggestion, MINUTE_OPTIONS};
use discophile_core::listening_history::DiscoveryMix;
use discophile_core::{Song, l10n};
use egui::{Align, CornerRadius, Layout, Response, Sense, Ui, vec2};

use crate::app::App;
use crate::features::{discovery, downloads, library};
use crate::nav::Route;
use crate::player;
use crate::theme::{self, Font, Palette, icons};
use crate::ui::widgets::{self, TrackRowOptions};

/// Card grids switch to two columns from this content width.
const TWO_COLUMNS: f32 = 720.0;
const CARD_HEIGHT: f32 = 84.0;

enum Card {
    Mix(DiscoveryMix, Vec<Song>),
    Statistics(f64),
}

pub fn show(app: &mut App, ui: &mut Ui) {
    widgets::page(ui, "listen-now", |ui| {
        widgets::page_title(ui, &l10n::t("Listen Now"));
        if !app.session.has_library() {
            widgets::empty_state(
                ui,
                icons::MUSIC_NOTES,
                &l10n::t("Listen Now"),
                &l10n::t("Choose something from your library to start listening."),
            );
            return;
        }
        resume_section(app, ui);
        albums_section(app, ui);
        recently_added(app, ui);
        discovery_section(app, ui);
        crate::ui::curator::listen_now_section(app, ui);
    });
}

fn resume_section(app: &mut App, ui: &mut Ui) {
    if (player::has_pending_resume(app) || (app.player.mode == player::Mode::Music && !app.player.is_playing()))
        && let Some(song) = app.player.queue.current().cloned()
        && !song.is_podcast()
        && (!app.settings.downloaded_only || downloads::is_downloaded(app, &song))
    {
        ui.add_space(8.0);
        resume_card(app, ui, &song, app.player.position());
    }
}

fn discovery_section(app: &mut App, ui: &mut Ui) {
    widgets::section_heading(ui, &l10n::t("Make room for a good listen"), Some(&l10n::t("Something familiar. Something unexpected.")));
    let mut cards = Vec::new();
    for (mix, songs) in discovery::mixes(app) {
        if !songs.is_empty() {
            cards.push(Card::Mix(mix, songs));
        }
    }
    if discovery::has_statistics(app) {
        cards.push(Card::Statistics(discovery::week_summary(app).seconds));
    }
    card_grid(ui, &cards, |ui, card| match card {
        Card::Mix(mix, songs) => mix_card(app, ui, *mix, songs),
        Card::Statistics(seconds) => statistics_card(app, ui, *seconds),
    });
    if app.discovery.loading {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(theme::secondary(ui, l10n::t("Finding something for you…"), Font::Subheadline));
        });
    }
    if let Some(error) = app.discovery.error.clone()
        && widgets::error_banner(ui, &error, true)
    {
        discovery::refresh(app, true);
    }
}

/// Lays out cards in one column, or two on wide windows.
fn card_grid<T>(ui: &mut Ui, items: &[T], mut card: impl FnMut(&mut Ui, &T)) {
    let gap = 14.0;
    let available = ui.available_width();
    let columns = if available >= TWO_COLUMNS { 2 } else { 1 };
    let width = ((available - gap * (columns as f32 - 1.0)) / columns as f32).floor();
    for row in items.chunks(columns) {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for item in row {
                ui.allocate_ui_with_layout(vec2(width, CARD_HEIGHT), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(width);
                    card(ui, item);
                });
            }
        });
        ui.add_space(gap - ui.spacing().item_spacing.y + 4.0);
    }
}

/// A card that reacts to clicks anywhere; controls added inside stay clickable on top of it.
pub fn action_card(ui: &mut Ui, height: f32, label: &str, add: impl FnOnce(&mut Ui)) -> Response {
    let palette = Palette::of_ui(ui);
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    let fill = if response.hovered() { palette.surface_raised } else { palette.surface };
    ui.painter().rect_filled(rect, CornerRadius::same(widgets::CARD_RADIUS), fill);
    let mut child =
        ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(18.0, 10.0))).layout(Layout::left_to_right(Align::Center)));
    add(&mut child);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn card_texts(ui: &mut Ui, caption: Option<&str>, title: &str, subtitle: &str) {
    ui.with_layout(Layout::top_down(Align::Min), |ui| {
        ui.spacing_mut().item_spacing.y = 3.0;
        ui.add_space(if caption.is_some() { 6.0 } else { 14.0 });
        if let Some(caption) = caption {
            ui.label(theme::secondary(ui, caption, Font::Caption));
        }
        ui.add(egui::Label::new(theme::text(title, Font::Title3)).truncate().selectable(false));
        ui.add(egui::Label::new(theme::secondary(ui, subtitle, Font::Subheadline)).truncate().selectable(false));
    });
}

fn resume_card(app: &mut App, ui: &mut Ui, song: &Song, position: f64) {
    let caption = l10n::t("Continue listening");
    let title = match app.player.context.as_ref() {
        Some(player::PlayContext::AlbumFocus { title, .. } | player::PlayContext::Mix { title }) => title.clone(),
        _ => song.album.clone().unwrap_or_else(|| song.title.clone()),
    };
    let subtitle = format!("{} · {} · {}", song.title, song.artist.clone().unwrap_or_else(|| l10n::t("Unknown artist")), l10n::time(position));
    let next = app.player.queue.songs().get(app.player.queue.index() + 1).map(|s| format!("{}: {}", l10n::t("Next track"), s.title));
    let label = format!("{caption}, {}, {subtitle}", song.title);
    let mut play = false;
    let response = action_card(ui, CARD_HEIGHT, &label, |ui| {
        widgets::artwork(ui, app, song.album_id.as_deref().unwrap_or(&song.id), song.cover_art.as_deref(), 60.0, 10.0);
        ui.add_space(4.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            play = widgets::icon_button_tinted(ui, icons::PLAY, &l10n::t("Play"), 20.0, Some(Palette::of_ui(ui).accent)).clicked();
            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                ui.add(egui::Label::new(theme::text(format!("{caption} · {title}"), Font::Headline)).truncate().selectable(false));
                ui.add(egui::Label::new(theme::secondary(ui, &subtitle, Font::Subheadline)).truncate().selectable(false));
            });
        });
    });
    if let Some(next) = next {
        response.clone().on_hover_text(next);
    }
    if play || response.clicked() {
        player::play(app);
    }
}

fn mix_icon(mix: DiscoveryMix) -> &'static str {
    match mix {
        DiscoveryMix::Rediscover => icons::CLOCK_COUNTER_CLOCKWISE,
        DiscoveryMix::Unheard => icons::SPARKLE,
        DiscoveryMix::Short => icons::TIMER,
    }
}

fn mix_card(app: &mut App, ui: &mut Ui, mix: DiscoveryMix, songs: &[Song]) {
    let title = mix.title();
    let total: f64 = songs.iter().filter_map(|s| s.duration).sum();
    let subtitle = format!("{} · {}", l10n::count("%lld songs", songs.len() as i64), l10n::time(total));
    let (mut play, mut list) = (false, false);
    let response = action_card(ui, CARD_HEIGHT, &format!("{title}, {subtitle}, {}", l10n::t("Play selection")), |ui| {
        let accent = Palette::of_ui(ui).accent;
        ui.allocate_ui_with_layout(vec2(40.0, 40.0), Layout::centered_and_justified(egui::Direction::LeftToRight), |ui| {
            ui.label(theme::icon(mix_icon(mix), 28.0).color(accent));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            list = widgets::icon_button(ui, icons::LIST_BULLETS, &format!("{title}, {}", l10n::t("View selection")), 22.0).clicked();
            play = widgets::icon_button_tinted(ui, icons::PLAY, &l10n::t("Play selection"), 20.0, Some(accent)).clicked();
            card_texts(ui, None, &title, &subtitle);
        });
    });
    response.clone().on_hover_text(mix.explanation());
    if list {
        app.discovery.ui.selection =
            Some(discovery::SelectionWindow { title: title.clone(), explanation: mix.explanation(), songs: songs.to_vec() });
    } else if play || response.clicked() {
        discovery::play_selection(app, &title, songs.to_vec(), 0, false);
    }
}

fn statistics_card(app: &mut App, ui: &mut Ui, seconds: f64) {
    let title = l10n::t("Listening statistics");
    let subtitle = l10n::tf("%@ in the last 7 days", &[&l10n::short_duration(seconds)]);
    let response = action_card(ui, CARD_HEIGHT, &format!("{title}, {subtitle}"), |ui| {
        let palette = Palette::of_ui(ui);
        ui.allocate_ui_with_layout(vec2(40.0, 40.0), Layout::centered_and_justified(egui::Direction::LeftToRight), |ui| {
            ui.label(theme::icon_filled(icons::CHART_BAR, 28.0).color(palette.accent));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(theme::icon(icons::CARET_RIGHT, 18.0).color(palette.secondary));
            card_texts(ui, None, &title, &subtitle);
        });
    });
    if response.clicked() {
        app.nav.push(Route::Statistics);
    }
}

// MARK: Choose a record for tonight

fn albums_section(app: &mut App, ui: &mut Ui) {
    widgets::section_heading(ui, &l10n::t("Tonight’s album"), None);
    ui.horizontal_wrapped(|ui| {
        let current = app.discovery.ui.minutes;
        if widgets::chip(ui, current.is_none(), &l10n::t("Any duration")).clicked() {
            discovery::set_album_minutes(app, None);
        }
        for minutes in MINUTE_OPTIONS {
            if widgets::chip(ui, current == Some(minutes), &l10n::tf("Up to %@ min", &[&minutes.to_string()])).clicked() {
                discovery::set_album_minutes(app, Some(minutes));
            }
        }
    });
    ui.add_space(18.0);
    let suggestions = discovery::album_suggestions(app);
    let Some(featured) = suggestions.first() else {
        if app.discovery.loading {
            widgets::loading(ui);
        } else {
            let message = if app.settings.downloaded_only {
                "Download every track of an album to use this mode offline."
            } else {
                "No album fits this duration. Choose a longer listening time."
            };
            ui.label(theme::secondary(ui, l10n::t(message), Font::Subheadline));
        }
        return;
    };
    let available = ui.available_width();
    if available >= 680.0 {
        let side = (available * 0.28).clamp(180.0, 270.0);
        let main = available - side - 28.0;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 28.0;
            ui.allocate_ui_with_layout(vec2(main, 0.0), Layout::top_down(Align::Min), |ui| {
                ui.set_width(main);
                featured_album(app, ui, featured);
            });
            ui.allocate_ui_with_layout(vec2(side, 0.0), Layout::top_down(Align::Min), |ui| {
                ui.set_width(side);
                ui.label(theme::secondary(ui, l10n::t("Or try"), Font::Subheadline));
                ui.add_space(10.0);
                for suggestion in suggestions.iter().skip(1) {
                    alternative_album(app, ui, suggestion);
                    ui.add_space(14.0);
                }
            });
        });
    } else {
        featured_album(app, ui, featured);
        for suggestion in suggestions.iter().skip(1) {
            ui.add_space(12.0);
            alternative_album(app, ui, suggestion);
        }
    }
}

fn featured_album(app: &mut App, ui: &mut Ui, suggestion: &AlbumSuggestion) {
    let available = ui.available_width();
    let size = (available * 0.42).clamp(180.0, 260.0).min(available);
    if available >= 520.0 {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 24.0;
            featured_artwork(app, ui, suggestion, size);
            ui.allocate_ui_with_layout(vec2(available - size - 24.0, size), Layout::top_down(Align::Min), |ui| {
                ui.set_width(available - size - 24.0);
                featured_album_details(app, ui, suggestion);
            });
        });
    } else {
        featured_artwork(app, ui, suggestion, size);
        featured_album_details(app, ui, suggestion);
    }
}

fn featured_artwork(app: &mut App, ui: &mut Ui, suggestion: &AlbumSuggestion, size: f32) {
    let album = &suggestion.album;
    let art = widgets::artwork(ui, app, &album.id, album.cover_art.as_deref(), size, 16.0);
    let label = format!("{}: {}", l10n::t("Show album"), album.name);
    if widgets::tile_target(ui, art.rect, ("featured-album", &album.id), &label).clicked() {
        app.nav.push(Route::Album(album.id.clone()));
    }
}

fn featured_album_details(app: &mut App, ui: &mut Ui, suggestion: &AlbumSuggestion) {
    let album = &suggestion.album;
    ui.add_space(12.0);
    ui.label(theme::text(&album.name, Font::Title2));
    ui.label(theme::secondary(ui, album_metadata(album), Font::Subheadline));
    ui.add_space(8.0);
    ui.label(theme::secondary(ui, suggestion.reason.description(), Font::Callout));
    ui.add_space(12.0);
    let loading = app.discovery.ui.album_loading.is_some();
    ui.horizontal_wrapped(|ui| {
        ui.add_enabled_ui(!loading, |ui| {
            if widgets::pill_button(ui, icons::PLAY, l10n::t("Listen to the whole album"), true).clicked() {
                discovery::play_album(app, album.clone());
            }
        });
        if widgets::pill_button(ui, icons::ARROWS_CLOCKWISE, l10n::t("Another recommendation"), false).clicked() {
            discovery::more_album_suggestions(app);
        }
        if loading {
            ui.spinner();
        }
    });
}

fn album_metadata(album: &discophile_core::Album) -> String {
    let artist = album.artist.clone().unwrap_or_else(|| l10n::t("Unknown artist"));
    match album_listening::duration(album) {
        Some(duration) => format!("{artist} · {}", l10n::time(duration)),
        None => artist,
    }
}

fn alternative_album(app: &mut App, ui: &mut Ui, suggestion: &AlbumSuggestion) {
    let album = &suggestion.album;
    let metadata = album_metadata(album);
    let response = action_card(ui, 100.0, &format!("{}, {metadata}", album.name), |ui| {
        widgets::artwork(ui, app, &album.id, album.cover_art.as_deref(), 64.0, 10.0);
        ui.add_space(8.0);
        ui.vertical(|ui| {
            ui.add(egui::Label::new(theme::text(&album.name, Font::Headline)).wrap().selectable(false));
            ui.add(egui::Label::new(theme::secondary(ui, &metadata, Font::Caption)).wrap().selectable(false));
        });
    });
    if response.on_hover_text(l10n::t("Listen to the whole album")).clicked() {
        discovery::play_album(app, album.clone());
    }
}

fn recently_added(app: &mut App, ui: &mut Ui) {
    library::ensure(app, library::keys::NEWEST);
    let albums = if app.settings.downloaded_only {
        let downloaded = library::downloaded_albums(app);
        app.library.snapshot.newest.iter().filter(|a| downloaded.iter().any(|d| d.id == a.id)).cloned().collect::<Vec<_>>()
    } else {
        app.library.snapshot.newest.clone()
    };
    if albums.is_empty() {
        return;
    }
    widgets::section_heading(ui, &l10n::t("Recently added"), None);
    if ui.link(l10n::t("Show all")).clicked() {
        library::set_album_list(app, library::AlbumListType::Newest);
        app.nav.push(Route::Albums);
    }
    ui.add_space(8.0);
    let width = ((ui.available_width() - 4.0 * 16.0) / 5.0).clamp(120.0, 160.0);
    egui::ScrollArea::horizontal().id_salt("home-recent-albums").show(ui, |ui| {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 16.0;
            for album in albums.iter().take(12) {
                ui.allocate_ui_with_layout(vec2(width, 0.0), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(width);
                    if widgets::album_tile(ui, app, album, width).clicked() {
                        app.nav.push(Route::Album(album.id.clone()));
                    }
                });
            }
        });
    });
}

// MARK: Selection window

/// The track list of a discovery mix (Play, Shuffle, double-click a row to start there).
pub fn windows(app: &mut App, ctx: &egui::Context) {
    let Some(selection) = app.discovery.ui.selection.clone() else { return };
    let songs: Vec<Song> = if app.settings.downloaded_only {
        selection.songs.iter().filter(|s| downloads::is_downloaded(app, s)).cloned().collect()
    } else {
        selection.songs.clone()
    };
    let mut open = true;
    let mut done = false;
    egui::Window::new(&selection.title)
        .id(egui::Id::unique("discovery-selection"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_size(vec2(560.0, 620.0))
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| {
            ui.label(theme::secondary(ui, &selection.explanation, Font::Subheadline));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add_enabled_ui(!songs.is_empty(), |ui| {
                    if widgets::pill_button(ui, icons::PLAY, l10n::t("Play"), true).clicked() {
                        discovery::play_selection(app, &selection.title, songs.clone(), 0, false);
                    }
                    if widgets::pill_button(ui, icons::SHUFFLE, l10n::t("Shuffle"), false).clicked() {
                        discovery::play_selection(app, &selection.title, songs.clone(), 0, true);
                    }
                });
                ui.label(theme::secondary(ui, widgets::songs_summary(&songs), Font::Footnote));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    done = ui.button(l10n::t("Done")).clicked();
                });
            });
            ui.add_space(6.0);
            if songs.is_empty() {
                ui.label(theme::secondary(ui, l10n::t("No tracks available"), Font::Subheadline));
                return;
            }
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                let current = app.player.current().map(|s| s.id);
                for (index, song) in songs.iter().enumerate() {
                    let is_current = current.as_deref() == Some(song.id.as_str());
                    let options = TrackRowOptions {
                        show_artwork: true,
                        is_current,
                        is_playing: is_current && app.player.is_playing(),
                        ..Default::default()
                    };
                    let row = widgets::track_row(ui, app, song, options, |ui, app| downloads::download_button(ui, app, song));
                    if row.play {
                        discovery::play_selection(app, &selection.title, songs.clone(), index, false);
                    }
                    row.response.context_menu(|ui| {
                        if ui.button(l10n::t("Play")).clicked() {
                            discovery::play_selection(app, &selection.title, songs.clone(), index, false);
                        }
                        if ui.button(l10n::t("Play next")).clicked() {
                            player::play_next(app, vec![song.clone()]);
                        }
                        if ui.button(l10n::t("Add to queue")).clicked() {
                            player::add_to_queue(app, vec![song.clone()]);
                        }
                        if ui.button(l10n::t("Personal radio")).clicked() {
                            discovery::start_personal_radio(app, song.clone());
                        }
                    });
                }
            });
        });
    if !open || done {
        app.discovery.ui.selection = None;
    }
}
