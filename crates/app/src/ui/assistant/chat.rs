//! The chat view shared by the side panel and the full page: a header (chat history, provider and model,
//! options), the messages with inline song results and collection previews, and the fixed composer.

use discophile_core::assistant::turn::MAX_REQUEST_CHARACTERS;
use discophile_core::assistant::{
    AssistantEditOperation, AssistantEditStatus, AssistantMessage, AssistantPrompts, AssistantProvider, AssistantRole,
};
use discophile_core::l10n::{self, t};
use discophile_core::{Song, assistant::AssistantChatStore};
use egui::{Align, Color32, CornerRadius, Frame, Key, Layout, Margin, Modifiers, Sense, Stroke, Ui, UiBuilder, vec2};

use crate::app::App;
use crate::features::assistant as feature;
use crate::nav::{Route, SettingsPage, SidePanel};
use crate::platform;
use crate::player::{self, Mode};
use crate::theme::{self, Font, Palette, icons};
use crate::ui::widgets;

/// The widest reading column on the full page, as on iOS.
const MAX_WIDTH: f32 = 760.0;

pub fn show(app: &mut App, ui: &mut Ui, page: bool) {
    let salt = if page { "page" } else { "panel" };
    egui::Panel::top(("assistant-header", salt))
        .frame(Frame::new().inner_margin(Margin { bottom: 6, ..Default::default() }))
        .show_separator_line(false)
        .show(ui, |ui| header(app, ui, page));
    egui::Panel::bottom(("assistant-composer", salt))
        .resizable(false)
        .frame(Frame::new().inner_margin(Margin { top: 6, ..Default::default() }))
        .show_separator_line(false)
        .show(ui, |ui| centered(ui, page, |ui| composer(app, ui, page)));
    messages(app, ui, page);
}

/// Limits the reading width on the full page.
fn centered<R>(ui: &mut Ui, page: bool, add: impl FnOnce(&mut Ui) -> R) -> R {
    if !page || ui.available_width() <= MAX_WIDTH {
        return add(ui);
    }
    let margin = (ui.available_width() - MAX_WIDTH) / 2.0;
    ui.horizontal_top(|ui| {
        ui.add_space(margin);
        ui.vertical(|ui| {
            ui.set_width(MAX_WIDTH);
            add(ui)
        })
        .inner
    })
    .inner
}

// MARK: Header

fn header(app: &mut App, ui: &mut Ui, page: bool) {
    let palette = Palette::of_ui(ui);
    ui.horizontal(|ui| {
        let unread = feature::unread_count(app);
        let history = widgets::icon_button(ui, icons::LIST, &t("Chat history"), 19.0);
        widgets::count_badge(ui, history.rect, unread);
        if history.clicked() {
            app.assistant.ui.history = true;
        }
        provider_menu(app, ui);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if !page && widgets::icon_button(ui, icons::X, &t("Close"), 16.0).clicked() {
                app.nav.side_panel = SidePanel::None;
            }
            options_menu(app, ui, page);
            if widgets::icon_button_tinted(ui, icons::NOTE_PENCIL, &t("New chat"), 18.0, Some(palette.accent)).clicked() {
                feature::new_chat(app);
            }
        });
    });
}

fn provider_menu(app: &mut App, ui: &mut Ui) {
    let Some((provider, model)) = app.assistant.store.selected().map(|c| (c.provider, c.model_id.clone())) else { return };
    let name = feature::provider_name(app, provider, &model);
    let palette = Palette::of_ui(ui);
    let label = theme::text(format!("{name}  {}", icons::CARET_DOWN), Font::Headline).color(palette.text);
    let button = egui::Button::new(label).frame(false);
    let (response, _) = egui::containers::menu::MenuButton::from_button(button).ui(ui, |ui| {
        ui.set_min_width(240.0);
        let check = |selected: bool| if selected { icons::CHECK } else { " " };
        let local = provider == AssistantProvider::Local;
        if ui.button(format!("{}  {}", check(local), t("Local model"))).clicked() {
            feature::set_provider(app, AssistantProvider::Local);
            ui.close();
        }
        if !platform::IS_WEB && ui.button(format!("{}  ChatGPT", check(!local))).clicked() {
            feature::set_provider(app, AssistantProvider::ChatGpt);
            ui.close();
        }
        if !local && !platform::IS_WEB {
            ui.separator();
            ui.label(theme::secondary(ui, t("Model"), Font::Footnote));
            let chosen = if model.is_empty() { app.assistant.config.chatgpt_model.clone() } else { model.clone() };
            let models = app.assistant.chatgpt.models.clone();
            if models.is_empty() && app.assistant.chatgpt.models_loading {
                ui.spinner();
            }
            for item in models {
                if ui.button(format!("{}  {}", check(item.slug == chosen), item.display_name)).clicked() {
                    feature::set_model(app, &item.slug);
                    ui.close();
                }
            }
        }
        ui.separator();
        if ui.button(format!("{}  {}", icons::GEAR_SIX, t("Music assistant"))).clicked() {
            app.nav.push(Route::Settings(SettingsPage::Assistant));
            ui.close();
        }
    });
    response.on_hover_text(t("AI provider and model"));
}

fn options_menu(app: &mut App, ui: &mut Ui, page: bool) {
    let palette = Palette::of_ui(ui);
    let button = egui::Button::new(theme::icon(icons::DOTS_THREE, 19.0).color(palette.text)).frame(false);
    let (response, _) = egui::containers::menu::MenuButton::from_button(button).ui(ui, |ui| {
        ui.set_min_width(250.0);
        let item = |ui: &mut Ui, icon: &str, label: String| {
            let clicked = ui.button(format!("{icon}  {label}")).clicked();
            if clicked {
                ui.close();
            }
            clicked
        };
        if item(ui, icons::NOTE_PENCIL, t("New chat")) {
            feature::new_chat(app);
        }
        if item(ui, icons::LIST, t("Chat history")) {
            app.assistant.ui.history = true;
        }
        if page {
            if item(ui, icons::SIDEBAR_SIMPLE, t("Show in side panel")) {
                app.nav.back();
                app.nav.side_panel = SidePanel::Assistant;
            }
        } else if item(ui, icons::ARROWS_OUT_SIMPLE, t("Open in main window")) {
            app.nav.side_panel = SidePanel::None;
            app.nav.push(Route::Assistant);
        }
        ui.separator();
        if !platform::IS_WEB && item(ui, icons::GEAR_SIX, t("ChatGPT connection")) {
            app.nav.push(Route::Settings(SettingsPage::Assistant));
        }
        if item(ui, icons::INFO, t("About this assistant")) {
            app.assistant.ui.privacy = true;
        }
        let draft = app.assistant.store.selected().map(|c| c.draft.trim().to_string()).unwrap_or_default();
        let enabled = !draft.is_empty();
        let copy =
            ui.add_enabled(enabled, egui::Button::new(format!("{}  {}", icons::ARROW_SQUARE_OUT, t("Copy request and open ChatGPT"))));
        if copy.clicked() {
            ui.close();
            copy_and_open(app, ui, &draft);
        }
    });
    response.on_hover_text(t("Chat options"));
}

/// Copies the request with the included track metadata, for pasting into ChatGPT. That conversation
/// cannot operate Discophile.
fn copy_and_open(app: &App, ui: &Ui, draft: &str) {
    let include = app.assistant.store.selected().is_some_and(|c| c.include_track);
    let song = if include { feature::context_song(app) } else { None };
    ui.ctx().copy_text(format!("{draft}\n{}", AssistantPrompts::context(song.as_ref())));
    ui.ctx().open_url(egui::OpenUrl::new_tab("https://chatgpt.com/"));
}

// MARK: Messages

fn messages(app: &mut App, ui: &mut Ui, page: bool) {
    // The chats are drawn from a moved-out store so rows can use the app (artwork, player).
    let store = std::mem::take(&mut app.assistant.store);
    let mut draft: Option<String> = None;
    let scroll = std::mem::take(&mut app.assistant.ui.scroll_to_bottom);
    egui::ScrollArea::vertical().id_salt(("assistant-messages", page)).auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
        centered(ui, page, |ui| {
            ui.add_space(4.0);
            message_list(app, ui, &store, &mut draft);
            ui.add_space(8.0);
            if scroll {
                ui.scroll_to_cursor(Some(Align::BOTTOM));
            }
        });
    });
    app.assistant.store = store;
    if let Some(text) = draft {
        feature::set_draft(app, text);
        app.assistant.ui.focus_composer = true;
    }
}

fn message_list(app: &mut App, ui: &mut Ui, store: &AssistantChatStore, draft: &mut Option<String>) {
    let palette = Palette::of_ui(ui);
    if let Some(error) = &app.assistant.error {
        widgets::error_banner(ui, error, false);
    }
    let Some(conversation) = store.selected() else {
        if !app.session.has_library() {
            widgets::empty_state(ui, icons::SPARKLE, &t("Music assistant"), &t("Choose something from your library to start listening."));
        }
        return;
    };
    if conversation.messages.is_empty() && !app.assistant.ui.focusing {
        welcome(app, ui, draft);
    }
    let blocked = feature::is_busy(app);
    for message in &conversation.messages {
        ui.add_space(10.0);
        if message.role == AssistantRole::User {
            user_message(ui, message, draft);
        } else {
            assistant_message(app, ui, conversation.id, message, blocked);
        }
    }
    if feature::answering(app) == Some(conversation.id) || app.assistant.ui.focusing {
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(theme::text(t("Working…"), Font::Callout).color(palette.secondary));
        });
    }
}

fn welcome(app: &App, ui: &mut Ui, draft: &mut Option<String>) {
    let palette = Palette::of_ui(ui);
    ui.add_space(20.0);
    ui.label(theme::icon(icons::WAVEFORM, 34.0).color(palette.accent));
    ui.add_space(6.0);
    ui.add(egui::Label::new(theme::text(t("What shall we listen to?"), Font::Title)).wrap());
    ui.label(theme::secondary(ui, t("Find a song, make a mix, or talk about music."), Font::Body));
    ui.add_space(10.0);
    let mut suggestions =
        vec![(icons::PLAYLIST, "Create a playlist for a relaxed evening."), (icons::MAGNIFYING_GLASS, "Find songs by my favorite artist.")];
    if feature::context_song(app).is_some() {
        suggestions.push((icons::SPARKLE, "Tell me about this track and suggest similar music."));
    }
    for (icon, key) in suggestions {
        let text = t(key);
        let (response, ()) = widgets::clickable_card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(theme::icon(icon, 18.0).color(palette.secondary));
                ui.add(egui::Label::new(theme::text(&text, Font::Callout).color(palette.secondary)).wrap().selectable(false));
            });
        });
        if response.clicked() {
            *draft = Some(text);
        }
        ui.add_space(4.0);
    }
}

fn bubble_frame(ui: &Ui) -> Frame {
    let palette = Palette::of_ui(ui);
    Frame::new().fill(palette.surface).corner_radius(CornerRadius::same(18)).inner_margin(Margin::symmetric(14, 10))
}

fn user_message(ui: &mut Ui, message: &AssistantMessage, draft: &mut Option<String>) {
    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
        let width = (ui.available_width() * 0.85).max(120.0);
        let response = bubble_frame(ui)
            .show(ui, |ui| {
                ui.set_max_width(width);
                ui.add(egui::Label::new(theme::text(&message.text, Font::Body)).wrap())
            })
            .inner;
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, format!("{}: {}", t("You"), message.text)));
        response.context_menu(|ui| {
            if ui.button(t("Use this message")).clicked() {
                *draft = Some(message.text.clone());
                ui.close();
            }
            if ui.button(t("Copy")).clicked() {
                ui.ctx().copy_text(message.text.clone());
                ui.close();
            }
        });
    });
}

fn assistant_message(app: &mut App, ui: &mut Ui, conversation: uuid::Uuid, message: &AssistantMessage, blocked: bool) {
    let palette = Palette::of_ui(ui);
    let source = message.source.clone().unwrap_or_else(|| t("Music assistant"));
    ui.label(theme::text(format!("{}  {source}", icons::SPARKLE), Font::Caption).color(palette.secondary));
    if !message.text.is_empty() {
        let color = if message.is_error { palette.accent } else { palette.text };
        ui.add(egui::Label::new(theme::text(&message.text, Font::Body).color(color)).wrap());
    }
    let deleted = message.collection_deleted == Some(true);
    if let Some(edit) = &message.edit {
        ui.add_space(4.0);
        widgets::card(ui, |ui| {
            ui.label(theme::text(format!("{}  {}", icons::PLAYLIST, operation_title(edit.operation)), Font::Headline));
            ui.label(theme::text(&edit.target.title, Font::Callout));
            ui.add_space(4.0);
            let label = t(if edit.status == AssistantEditStatus::Pending { "Review changes" } else { "View changes" });
            if ui.add_enabled(!blocked || edit.status != AssistantEditStatus::Pending, egui::Button::new(label)).clicked() {
                app.assistant.ui.review = Some(message.id);
            }
        });
    } else if let Some(title) = message.playlist_title.as_deref().filter(|_| !deleted) {
        ui.add_space(4.0);
        widgets::card(ui, |ui| {
            ui.label(theme::text(format!("{}  {title}", icons::PLAYLIST), Font::Headline));
            ui.add_space(4.0);
            play_button(app, ui, &message.songs, blocked, title);
            for index in 0..message.songs.len().min(4) {
                song_row(app, ui, &message.songs, index, !blocked);
            }
            if message.songs.len() > 4 {
                ui.label(theme::secondary(ui, l10n::count("%lld songs", message.songs.len() as i64), Font::Caption));
            }
            ui.add_space(4.0);
            let label = t(if message.saved { "Playlist saved" } else { "Review playlist" });
            let enabled = !message.saved && !blocked && !app.settings.downloaded_only;
            if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                open_editor(app, conversation, message);
            }
        });
    } else if !message.songs.is_empty() && !deleted {
        ui.add_space(4.0);
        widgets::card(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                let title = message.collection_title().map(str::to_string).unwrap_or_else(|| t("Library selection"));
                if message.songs.len() > 1 {
                    play_button(app, ui, &message.songs, blocked, &title);
                }
                let enabled = !blocked && !app.settings.downloaded_only && app.session.has_library();
                let create = ui.add_enabled(
                    enabled,
                    egui::Button::new(theme::text(format!("{}  {}", icons::LIST_PLUS, t("Create playlist")), Font::Callout)),
                );
                if create.clicked() {
                    open_editor(app, conversation, message);
                }
                let add = ui.add_enabled(
                    enabled,
                    egui::Button::new(theme::text(format!("{}  {}", icons::QUEUE, t("Add to playlist")), Font::Callout)),
                );
                if add.clicked() {
                    crate::features::library::open_playlist_picker(app, message.songs.clone());
                }
            });
            ui.add_space(6.0);
            for index in 0..message.songs.len() {
                song_row(app, ui, &message.songs, index, !blocked);
            }
        });
    }
}

pub fn operation_title(operation: AssistantEditOperation) -> String {
    use AssistantEditOperation::*;
    t(match operation {
        Add => "Add songs",
        Remove => "Remove songs",
        Replace => "Replace playlist contents",
        Rename => "Rename collection",
        Delete => "Delete collection",
        Favorite => "Add to favorites",
        Unfavorite => "Remove from favorites",
    })
}

fn open_editor(app: &mut App, conversation: uuid::Uuid, message: &AssistantMessage) {
    let title = message.playlist_title.clone().or_else(|| message.selection_title.clone()).unwrap_or_else(|| t("Library selection"));
    app.assistant.ui.editor =
        Some(feature::PlaylistEditor { conversation, message: message.id, title, songs: message.songs.clone(), error: None });
}

/// "Play": queues every song of a selection.
fn play_button(app: &mut App, ui: &mut Ui, songs: &[Song], blocked: bool, title: &str) {
    let palette = Palette::of_ui(ui);
    let text = theme::text(format!("{}  {}", icons::PLAY, t("Play")), Font::Callout).color(Color32::WHITE);
    let button = egui::Button::new(text).fill(palette.action).corner_radius(CornerRadius::same(14));
    let response = ui.add_enabled(!songs.is_empty() && !blocked, button).on_hover_text(t("Play selection"));
    if response.clicked() {
        crate::features::discovery::play_selection(app, title, songs.to_vec(), 0, false);
    }
}

/// A result row. The current song is highlighted; clicking it toggles playback without rebuilding the
/// queue, while another row plays the whole result from that song.
pub fn song_row(app: &mut App, ui: &mut Ui, songs: &[Song], index: usize, enabled: bool) {
    let Some(song) = songs.get(index) else { return };
    let palette = Palette::of_ui(ui);
    let current = app.player.mode == Mode::Music && app.player.current().is_some_and(|c| c.stream_identifier() == song.stream_identifier());
    let active = current && app.player.is_playing();
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 50.0), Sense::click());
    if current {
        ui.painter().rect_filled(rect, CornerRadius::same(8), palette.accent.gamma_multiply(0.12));
    } else if response.hovered() && enabled {
        ui.painter().rect_filled(rect, CornerRadius::same(8), palette.hover);
    }
    let inner = rect.shrink2(vec2(6.0, 5.0));
    let art = egui::Rect::from_min_size(inner.left_top(), vec2(40.0, 40.0));
    let texture = song.cover_art.as_deref().and_then(|cover| app.artwork_texture(cover));
    widgets::paint_artwork(ui, art, &song.id, texture.as_ref(), 6.0);
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(egui::Rect::from_min_max(egui::pos2(art.right() + 10.0, inner.top()), inner.right_bottom()))
            .layout(Layout::right_to_left(Align::Center)),
    );
    let glyph = if active { icons::PAUSE_CIRCLE } else { icons::PLAY_CIRCLE };
    child.label(theme::icon_filled(glyph, 24.0).color(if enabled { palette.accent } else { palette.secondary }));
    child.with_layout(Layout::top_down(Align::Min), |ui| {
        ui.add_space(2.0);
        let color = if current { palette.accent } else { palette.text };
        ui.add(egui::Label::new(theme::text(&song.title, Font::Body).color(color)).truncate().selectable(false));
        let artist = song.artist.clone().unwrap_or_else(|| t("Unknown artist"));
        let subtitle =
            [artist, song.album.clone().unwrap_or_default()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
        ui.add(egui::Label::new(theme::secondary(ui, subtitle, Font::Footnote)).truncate().selectable(false));
    });
    let label = format!("{}, {}, {}", t(if active { "Pause" } else { "Play" }), song.title, song.artist.clone().unwrap_or_default());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, &label));
    let response = if enabled { response.on_hover_cursor(egui::CursorIcon::PointingHand) } else { response };
    let keyboard = response.has_focus() && ui.input(|i| i.key_pressed(Key::Enter));
    if enabled && (response.clicked() || keyboard) {
        if current {
            player::toggle(app);
        } else {
            player::play_songs(app, songs.to_vec(), index, false);
        }
    }
}

// MARK: Composer

fn composer(app: &mut App, ui: &mut Ui, page: bool) {
    let palette = Palette::of_ui(ui);
    let Some(conversation) = app.assistant.store.selected() else { return };
    let id = conversation.id;
    let provider = conversation.provider;
    let include_track = conversation.include_track;
    let mut draft = conversation.draft.clone();
    let selection = conversation.active_selection().map(|m| m.playlist_title.clone().unwrap_or_else(|| t("Current selection")));
    let busy = feature::is_busy(app);
    let answering = feature::answering(app);
    let unavailable = feature::unavailable_reason(app, provider);
    let has_library = app.session.has_library();

    if let Some(title) = selection {
        ui.add(egui::Label::new(theme::text(format!("{}  {title}", icons::PLAYLIST), Font::Caption).color(palette.secondary)).truncate());
    }
    if let Some(reason) = &unavailable {
        ui.horizontal_wrapped(|ui| {
            ui.label(theme::text(reason, Font::Footnote).color(palette.secondary));
            if ui.link(t("Set up")).clicked() {
                app.nav.push(Route::Settings(SettingsPage::Assistant));
            }
        });
    }
    if let Some(song) = feature::context_song(app) {
        let glyph = if include_track { icons::CHECK_CIRCLE } else { icons::CIRCLE };
        let text = theme::text(format!("{glyph}  {}", song.title), Font::Caption).color(palette.secondary);
        let response = ui.add(egui::Button::new(text).frame(false).truncate());
        let state = t(if include_track { "On" } else { "Off" });
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::Role::CheckBox, true, format!("{}, {}, {state}", t("Include this track"), song.title))
        });
        if response.on_hover_text(t("Include this track")).clicked() {
            feature::toggle_include_track(app);
        }
    }

    let editor_id = egui::Id::unique(("assistant-composer-text", page));
    let focused = ui.memory(|m| m.has_focus(editor_id));
    let trimmed_len = draft.trim().chars().count();
    let can_send = !busy && trimmed_len > 0 && trimmed_len <= MAX_REQUEST_CHARACTERS && unavailable.is_none() && has_library;
    // Enter sends; Shift+Enter adds a line.
    let enter = focused && can_send && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
    let mut send = false;
    let mut stop = false;
    Frame::new()
        .fill(palette.surface)
        .corner_radius(CornerRadius::same(22))
        .stroke(Stroke::new(1.0, palette.separator))
        .inner_margin(Margin { left: 14, right: 6, top: 6, bottom: 6 })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let button_width = 40.0;
                let edit = egui::TextEdit::multiline(&mut draft)
                    .id(editor_id)
                    .hint_text(t("Message Discophile"))
                    .frame(Frame::NONE)
                    .desired_rows(1)
                    .desired_width(ui.available_width() - button_width)
                    .font(Font::Body.id());
                let response = egui::ScrollArea::vertical()
                    .id_salt(("assistant-composer-scroll", page))
                    .max_height(120.0)
                    .show(ui, |ui| ui.add(edit))
                    .inner;
                response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::MultilineTextInput, true, t("Your request")));
                if std::mem::take(&mut app.assistant.ui.focus_composer) {
                    response.request_focus();
                }
                if response.changed() {
                    feature::set_draft(app, draft.clone());
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if answering == Some(id) {
                        if widgets::icon_button_tinted(ui, icons::STOP_CIRCLE, &t("Stop response"), 24.0, Some(palette.accent)).clicked() {
                            stop = true;
                        }
                    } else {
                        ui.add_enabled_ui(can_send, |ui| {
                            let response =
                                widgets::icon_button_tinted(ui, icons::ARROW_CIRCLE_UP, &t("Send request"), 26.0, Some(palette.accent));
                            if response.on_hover_text(t("Enter sends, Shift+Enter adds a line.")).clicked() {
                                send = true;
                            }
                        });
                    }
                });
            });
        });
    if enter {
        send = true;
    }
    if stop {
        feature::cancel(app);
    }
    if send {
        let text = draft.trim().to_string();
        feature::send(app, &text);
        ui.memory_mut(|m| m.request_focus(editor_id));
    }
    if let Some(running) = answering.filter(|running| *running != id) {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                theme::text(format!("{}  {}", icons::CHAT_CIRCLE_DOTS, t("Discophile is answering in another chat.")), Font::Caption)
                    .color(palette.secondary),
            );
            if ui.link(t("Go to chat")).clicked() {
                feature::select_chat(app, running);
            }
        });
    }
    if trimmed_len > MAX_REQUEST_CHARACTERS {
        ui.label(theme::text(t("Keep your request within 1,000 characters."), Font::Caption).color(palette.accent));
    }
    let disclosure = t(if provider == AssistantProvider::Local {
        "Local model · AI can make mistakes"
    } else {
        "ChatGPT · Requests, recent chat and music metadata shared with OpenAI"
    });
    ui.vertical_centered(|ui| {
        let response =
            ui.add(egui::Label::new(theme::text(disclosure, Font::Caption).color(palette.secondary)).wrap().sense(Sense::click()));
        if response.on_hover_text(t("About this assistant")).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            app.assistant.ui.privacy = true;
        }
    });
}
