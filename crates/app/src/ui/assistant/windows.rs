//! Assistant dialogs: Chat history (search, reopen, rename, delete), the review of an edit proposal with
//! its explicit Confirm changes, the playlist preview before saving a selection, and About this assistant.

use chrono::{Datelike, Local, TimeZone};
use discophile_core::Song;
use discophile_core::assistant::{AssistantEditOperation, AssistantEditStatus, AssistantProvider};
use discophile_core::l10n::{self, t};
use egui::{Align, CornerRadius, Layout, Sense, Ui, vec2};
use uuid::Uuid;

use super::chat::operation_title;
use crate::app::App;
use crate::features::assistant as feature;
use crate::platform;
use crate::theme::{self, Font, Palette, icons};
use crate::ui::widgets;

pub fn show(app: &mut App, ctx: &egui::Context) {
    history(app, ctx);
    rename(app, ctx);
    delete(app, ctx);
    review(app, ctx);
    editor(app, ctx);
    privacy(app, ctx);
}

fn title(ui: &mut Ui, text: &str) {
    ui.label(theme::text(text, Font::Title3));
    ui.add_space(6.0);
}

fn footer(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.add_space(12.0);
    ui.with_layout(Layout::right_to_left(Align::Center), add);
}

// MARK: Chat history

#[derive(Clone, Copy, PartialEq, Eq)]
enum Period {
    Today,
    Yesterday,
    Week,
    Earlier,
}

impl Period {
    const ALL: [Period; 4] = [Period::Today, Period::Yesterday, Period::Week, Period::Earlier];

    fn of(millis: i64) -> Self {
        let today = Local::now().date_naive();
        let day = Local.timestamp_millis_opt(millis).single().map(|d| d.date_naive()).unwrap_or(today);
        match (today - day).num_days() {
            i64::MIN..=0 => Period::Today,
            1 => Period::Yesterday,
            2..=7 => Period::Week,
            _ => Period::Earlier,
        }
    }

    fn title(self) -> String {
        t(match self {
            Period::Today => "Today",
            Period::Yesterday => "Yesterday",
            Period::Week => "Previous 7 days",
            Period::Earlier => "Earlier",
        })
    }
}

/// "14:05" for recent chats, "12 Oct 2026" for older ones.
fn time_label(millis: i64, period: Period) -> String {
    let Some(date) = Local.timestamp_millis_opt(millis).single() else { return String::new() };
    if matches!(period, Period::Today | Period::Yesterday) {
        return date.format("%H:%M").to_string();
    }
    const EN: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    const IT: [&str; 12] = ["gen", "feb", "mar", "apr", "mag", "giu", "lug", "ago", "set", "ott", "nov", "dic"];
    let months = if l10n::language() == l10n::Language::Italian { IT } else { EN };
    format!("{} {} {}", date.day(), months[date.month0() as usize], date.year())
}

struct Row {
    id: Uuid,
    title: String,
    preview: String,
    detail: String,
    unread: bool,
    selected: bool,
    icon: &'static str,
}

fn history(app: &mut App, ctx: &egui::Context) {
    if !app.assistant.ui.history {
        return;
    }
    let store = &app.assistant.store;
    let items = store.chat_list(&app.assistant.ui.history_search);
    let mut groups: Vec<(Period, Vec<Row>)> = Period::ALL.iter().map(|p| (*p, Vec::new())).collect();
    for item in &items {
        let Some(chat) = store.conversation(item.id) else { continue };
        let period = Period::of(item.updated);
        let collection = chat.active_selection().is_some_and(|m| m.playlist_title.is_some() || m.playlist_id.is_some());
        let icon = if item.selected {
            icons::CHECK_CIRCLE
        } else if collection {
            icons::PLAYLIST
        } else if chat.provider == AssistantProvider::ChatGpt {
            icons::CHATS
        } else {
            icons::SPARKLE
        };
        let preview = chat
            .messages
            .iter()
            .rev()
            .find(|m| !m.text.is_empty())
            .map(|m| m.text.clone())
            .unwrap_or_else(|| if chat.draft.is_empty() { t("No messages yet") } else { chat.draft.clone() });
        let provider = if chat.provider == AssistantProvider::ChatGpt { "ChatGPT".to_string() } else { t("Local model") };
        let row = Row {
            id: item.id,
            title: item.title.clone(),
            preview: preview.lines().next().unwrap_or_default().chars().take(140).collect(),
            detail: format!("{provider} · {}", time_label(item.updated, period)),
            unread: item.unread,
            selected: item.selected,
            icon,
        };
        if let Some(group) = groups.iter_mut().find(|(p, _)| *p == period) {
            group.1.push(row);
        }
    }
    let mut close = false;
    let mut open: Option<Uuid> = None;
    let mut new_chat = false;
    let modal = egui::Modal::new(egui::Id::unique("assistant-history")).show(ctx, |ui| {
        ui.set_width(480.0);
        ui.horizontal(|ui| {
            ui.label(theme::text(t("Chat history"), Font::Title3));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button(format!("{}  {}", icons::NOTE_PENCIL, t("New chat"))).clicked() {
                    new_chat = true;
                }
            });
        });
        ui.add_space(6.0);
        widgets::search_field(ui, &mut app.assistant.ui.history_search, &t("Search chats"), ui.available_width());
        ui.label(theme::secondary(ui, t("Chats stay on this computer, separately for each music library."), Font::Footnote));
        ui.add_space(6.0);
        egui::ScrollArea::vertical().max_height(440.0).auto_shrink([false, true]).show(ui, |ui| {
            for (period, rows) in &groups {
                if rows.is_empty() {
                    continue;
                }
                ui.add_space(6.0);
                ui.label(theme::secondary(ui, period.title(), Font::Headline));
                for row in rows {
                    if history_row(app, ui, row) {
                        open = Some(row.id);
                    }
                }
            }
            if items.is_empty() && !app.assistant.ui.history_search.trim().is_empty() {
                ui.label(theme::secondary(ui, t("No chats match your search."), Font::Callout));
            }
        });
        footer(ui, |ui| {
            if ui.button(t("Done")).clicked() {
                close = true;
            }
        });
    });
    if new_chat {
        feature::new_chat(app);
        close = true;
    }
    if let Some(id) = open {
        feature::select_chat(app, id);
        feature::open(app);
        close = true;
    }
    if close || modal.should_close() {
        app.assistant.ui.history = false;
    }
}

/// A chat row; returns true when it was chosen. Its menu renames or deletes.
fn history_row(app: &mut App, ui: &mut Ui, row: &Row) -> bool {
    let palette = Palette::of_ui(ui);
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 66.0), Sense::click());
    if row.selected {
        ui.painter().rect_filled(rect, CornerRadius::same(10), palette.accent.gamma_multiply(0.12));
    } else if response.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(10), palette.hover);
    }
    let mut child =
        ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(10.0, 6.0))).layout(Layout::left_to_right(Align::Center)));
    child.label(theme::icon(row.icon, 20.0).color(palette.accent));
    child.with_layout(Layout::right_to_left(Align::Center), |ui| {
        let button = egui::Button::new(theme::icon(icons::DOTS_THREE_CIRCLE, 19.0).color(palette.secondary)).frame(false);
        let (menu, _) = egui::containers::menu::MenuButton::from_button(button).ui(ui, |ui| {
            if ui.button(format!("{}  {}", icons::PENCIL_SIMPLE, t("Rename chat"))).clicked() {
                app.assistant.ui.rename = Some((row.id, row.title.clone()));
                ui.close();
            }
            if ui.button(format!("{}  {}", icons::TRASH, t("Delete chat"))).clicked() {
                app.assistant.ui.delete = Some(row.id);
                ui.close();
            }
        });
        menu.on_hover_text(format!("{}: {}", t("Manage chat"), row.title));
        ui.with_layout(Layout::top_down(Align::Min), |ui| {
            ui.horizontal(|ui| {
                if row.unread {
                    let (dot, _) = ui.allocate_exact_size(vec2(9.0, 9.0), Sense::hover());
                    ui.painter().circle_filled(dot.center(), 4.5, palette.action);
                }
                ui.add(egui::Label::new(theme::text(&row.title, Font::Headline)).truncate().selectable(false));
            });
            ui.add(egui::Label::new(theme::secondary(ui, &row.preview, Font::Footnote)).truncate().selectable(false));
            ui.add(egui::Label::new(theme::secondary(ui, &row.detail, Font::Caption)).truncate().selectable(false));
        });
    });
    let label = if row.unread { format!("{}, {}", row.title, t("Unread")) } else { row.title.clone() };
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, row.selected, &label));
    response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

fn rename(app: &mut App, ctx: &egui::Context) {
    let Some((id, mut text)) = app.assistant.ui.rename.take() else { return };
    let mut close = false;
    let mut save = false;
    let modal = egui::Modal::new(egui::Id::unique("assistant-rename")).show(ctx, |ui| {
        ui.set_width(380.0);
        title(ui, &t("Rename chat"));
        let response =
            ui.add(egui::TextEdit::singleline(&mut text).hint_text(t("Chat title")).desired_width(f32::INFINITY).margin(vec2(10.0, 8.0)));
        if !response.has_focus() && !response.lost_focus() {
            response.request_focus();
        }
        let valid = !text.trim().is_empty();
        if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && valid {
            save = true;
        }
        footer(ui, |ui| {
            if ui.add_enabled(valid, egui::Button::new(theme::text(t("Save"), Font::Headline))).clicked() {
                save = true;
            }
            if ui.button(t("Cancel")).clicked() {
                close = true;
            }
        });
    });
    if save {
        feature::rename_chat(app, id, &text);
    } else if !close && !modal.should_close() {
        app.assistant.ui.rename = Some((id, text));
    }
}

fn delete(app: &mut App, ctx: &egui::Context) {
    let Some(id) = app.assistant.ui.delete else { return };
    let mut close = false;
    let mut confirm = false;
    let modal = egui::Modal::new(egui::Id::unique("assistant-delete")).show(ctx, |ui| {
        ui.set_width(380.0);
        title(ui, &t("Delete this chat?"));
        ui.label(theme::text(t("This removes the conversation from this computer. Saved playlists are kept."), Font::Body));
        footer(ui, |ui| {
            let palette = Palette::of_ui(ui);
            let button = egui::Button::new(theme::text(t("Delete chat"), Font::Headline).color(egui::Color32::WHITE)).fill(palette.danger);
            if ui.add(button).clicked() {
                confirm = true;
            }
            if ui.button(t("Cancel")).clicked() {
                close = true;
            }
        });
    });
    if confirm {
        feature::delete_chat(app, id);
    }
    if confirm || close || modal.should_close() {
        app.assistant.ui.delete = None;
    }
}

// MARK: Review changes

fn track_list(ui: &mut Ui, songs: &[Song], salt: &str) {
    egui::ScrollArea::vertical().id_salt(salt).max_height(220.0).auto_shrink([false, true]).show(ui, |ui| {
        for song in songs {
            ui.add(egui::Label::new(theme::text(&song.title, Font::Body)).truncate());
            ui.add(egui::Label::new(theme::secondary(ui, song.artist.clone().unwrap_or_default(), Font::Footnote)).truncate());
            ui.add_space(3.0);
        }
    });
}

fn review(app: &mut App, ctx: &egui::Context) {
    let Some(message_id) = app.assistant.ui.review else { return };
    let Some(edit) = app.assistant.store.selected().and_then(|c| c.message(message_id)).and_then(|m| m.edit.clone()) else {
        app.assistant.ui.review = None;
        return;
    };
    let busy = feature::is_busy(app);
    let pending = edit.status == AssistantEditStatus::Pending;
    let server = edit.target.playlist_id.is_some() || edit.is_favorite_change();
    let blocked = busy || app.library.changing_playlist || (app.settings.downloaded_only && server);
    let mut close = false;
    let mut confirm = false;
    let modal = egui::Modal::new(egui::Id::unique("assistant-review")).show(ctx, |ui| {
        let palette = Palette::of_ui(ui);
        ui.set_width(520.0);
        title(ui, &t("Review changes"));
        ui.label(theme::text(format!("{}  {}", icons::SPARKLE, operation_title(edit.operation)), Font::Headline));
        ui.label(theme::text(&edit.target.title, Font::Body));
        if edit.operation == AssistantEditOperation::Rename {
            ui.label(theme::text(format!("→ {}", edit.title), Font::Headline));
        }
        let summary = if edit.is_favorite_change() {
            l10n::count("%lld songs", edit.tracks.len() as i64)
        } else {
            format!("{} → {}", l10n::count("%lld songs", edit.before.len() as i64), l10n::count("%lld songs", edit.after.len() as i64))
        };
        ui.label(theme::secondary(ui, summary, Font::Callout));
        let note = if server { "Changes are saved to your music server only after confirmation." } else { "Changes are saved on this computer only after confirmation." };
        ui.label(theme::secondary(ui, t(note), Font::Footnote));
        if edit.operation == AssistantEditOperation::Delete {
            ui.add_space(4.0);
            ui.label(theme::text(t("This deletes the collection, keeping the songs in your music library."), Font::Callout).color(palette.accent));
        }
        ui.add_space(6.0);
        if edit.status == AssistantEditStatus::Failed || (edit.status == AssistantEditStatus::Applying && !busy) {
            ui.label(
                theme::text(
                    t("The change could not be confirmed. Some server changes may have been saved. Refresh and ask for a new review before trying again."),
                    Font::Callout,
                )
                .color(palette.accent),
            );
        } else if edit.status == AssistantEditStatus::Applied {
            ui.label(theme::text(format!("{}  {}", icons::CHECK_CIRCLE, t("Changes applied")), Font::Callout).color(palette.positive));
        } else if edit.status == AssistantEditStatus::Cancelled {
            ui.label(theme::secondary(ui, t("Change cancelled"), Font::Callout));
        }
        if !matches!(edit.operation, AssistantEditOperation::Rename | AssistantEditOperation::Delete) {
            ui.add_space(6.0);
            ui.label(theme::text(t("Selected songs"), Font::Headline));
            track_list(ui, &edit.tracks, "assistant-review-selected");
            if edit.operation == AssistantEditOperation::Remove {
                ui.label(theme::secondary(ui, t("Every occurrence of the selected songs will be removed from this collection."), Font::Footnote));
            }
            if !edit.is_favorite_change() {
                egui::CollapsingHeader::new(t("Contents after confirmation")).id_salt("assistant-review-after").show(ui, |ui| {
                    track_list(ui, &edit.after, "assistant-review-after-list");
                });
            }
        }
        if !edit.is_favorite_change() && !edit.before.is_empty() {
            egui::CollapsingHeader::new(t("Original contents")).id_salt("assistant-review-before").show(ui, |ui| {
                track_list(ui, &edit.before, "assistant-review-before-list");
            });
        }
        if busy && edit.status == AssistantEditStatus::Applying {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(theme::secondary(ui, t("Saving…"), Font::Callout));
            });
        }
        footer(ui, |ui| {
            if pending {
                let destructive = edit.operation == AssistantEditOperation::Delete;
                let fill = if destructive { palette.danger } else { palette.action };
                let button = egui::Button::new(theme::text(t("Confirm changes"), Font::Headline).color(egui::Color32::WHITE)).fill(fill);
                if ui.add_enabled(!blocked, button).clicked() {
                    confirm = true;
                }
            }
            let label = t(if pending { "Cancel" } else { "Done" });
            if ui.add_enabled(!busy || !pending, egui::Button::new(label)).clicked() {
                close = true;
            }
        });
    });
    if confirm {
        feature::confirm_edit(app, message_id);
        return;
    }
    if close || (modal.should_close() && !busy) {
        if pending {
            feature::cancel_edit(app, message_id);
        }
        app.assistant.ui.review = None;
    }
}

// MARK: Playlist preview

fn editor(app: &mut App, ctx: &egui::Context) {
    let Some(mut state) = app.assistant.ui.editor.take() else { return };
    let busy = feature::is_busy(app) || app.library.changing_playlist;
    let can_save =
        !busy && !state.songs.is_empty() && !state.title.trim().is_empty() && app.session.has_library() && !app.settings.downloaded_only;
    let mut close = false;
    let mut save = false;
    let mut add_existing = false;
    let modal = egui::Modal::new(egui::Id::unique("assistant-playlist-editor")).show(ctx, |ui| {
        ui.set_width(560.0);
        title(ui, &t("Playlist preview"));
        ui.add_enabled_ui(!busy, |ui| {
            ui.add(egui::TextEdit::singleline(&mut state.title).hint_text(t("Playlist name")).desired_width(f32::INFINITY).margin(vec2(10.0, 8.0)));
            ui.add_space(6.0);
            reorder_list(ui, &mut state.songs, "assistant-playlist-editor-list");
        });
        ui.add_space(4.0);
        ui.label(theme::secondary(
            ui,
            t("This proposal uses a sample of your library, based on titles, artists and albums. Edit its name, remove or reorder tracks, then save it to your server."),
            Font::Footnote,
        ));
        if busy {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(theme::secondary(ui, t("Saving…"), Font::Callout));
            });
        }
        if let Some(error) = &state.error {
            ui.label(theme::text(error, Font::Callout).color(Palette::of_ui(ui).danger));
        }
        footer(ui, |ui| {
            if ui.add_enabled(can_save, egui::Button::new(theme::text(t("Save playlist"), Font::Headline))).clicked() {
                save = true;
            }
            if ui.add_enabled(!busy, egui::Button::new(t("Cancel"))).clicked() {
                close = true;
            }
            if ui.add_enabled(can_save, egui::Button::new(format!("{}  {}", icons::LIST_PLUS, t("Add to playlist")))).clicked() {
                add_existing = true;
            }
        });
    });
    if close || (modal.should_close() && !busy) {
        return;
    }
    if add_existing {
        crate::features::library::open_playlist_picker(app, state.songs.clone());
        return;
    }
    let request = save.then(|| (state.conversation, state.message, state.title.clone(), state.songs.clone()));
    if save {
        state.error = None;
    }
    app.assistant.ui.editor = Some(state);
    if let Some((conversation, message, title, songs)) = request {
        feature::save_playlist(app, conversation, message, title, songs);
    }
}

/// Tracks that can be reordered (drag or arrows) and removed.
pub fn reorder_list(ui: &mut Ui, songs: &mut Vec<Song>, salt: &str) {
    let palette = Palette::of_ui(ui);
    let mut action: Option<(usize, Option<usize>)> = None;
    let count = songs.len();
    let pointer = ui.ctx().pointer_interact_pos();
    let released = ui.input(|i| i.pointer.any_released());
    egui::ScrollArea::vertical().id_salt(salt).max_height(380.0).auto_shrink([false, true]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        for index in 0..count {
            let song = &songs[index];
            let id = egui::Id::unique((salt, "row", index, song.stream_identifier()));
            let row = ui.dnd_drag_source(id, index, |ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::hover());
                ui.painter().rect_filled(rect, CornerRadius::same(8), palette.surface_raised.gamma_multiply(0.6));
                let mut child = ui
                    .new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(10.0, 3.0))).layout(Layout::left_to_right(Align::Center)));
                child.label(theme::icon(icons::DOTS_SIX_VERTICAL, 16.0).color(palette.secondary));
                child.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if widgets::icon_button_tinted(ui, icons::MINUS_CIRCLE, &t("Remove"), 16.0, Some(palette.danger)).clicked() {
                        action = Some((index, None));
                    }
                    ui.add_enabled_ui(index + 1 < count, |ui| {
                        if widgets::icon_button(ui, icons::ARROW_DOWN, &t("Move down"), 15.0).clicked() {
                            action = Some((index, Some(index + 2)));
                        }
                    });
                    ui.add_enabled_ui(index > 0, |ui| {
                        if widgets::icon_button(ui, icons::ARROW_UP, &t("Move up"), 15.0).clicked() {
                            action = Some((index, Some(index - 1)));
                        }
                    });
                    ui.with_layout(Layout::top_down(Align::Min), |ui| {
                        ui.add(egui::Label::new(theme::text(&song.title, Font::Body)).truncate().selectable(false));
                        ui.add(
                            egui::Label::new(theme::secondary(ui, song.artist.clone().unwrap_or_default(), Font::Footnote))
                                .truncate()
                                .selectable(false),
                        );
                    });
                });
            });
            if let (Some(pointer), true) = (pointer, released)
                && let Some(from) = row.response.dnd_release_payload::<usize>()
                && row.response.rect.contains(pointer)
            {
                let to = if pointer.y > row.response.rect.center().y { index + 1 } else { index };
                action = Some((*from, Some(to)));
            }
        }
    });
    match action {
        Some((index, None)) if index < songs.len() => {
            songs.remove(index);
        }
        Some((from, Some(to))) if from < songs.len() => {
            let song = songs.remove(from);
            let to = if to > from { to - 1 } else { to };
            songs.insert(to.min(songs.len()), song);
        }
        _ => {}
    }
}

// MARK: About this assistant

fn privacy(app: &mut App, ctx: &egui::Context) {
    if !app.assistant.ui.privacy {
        return;
    }
    let mut close = false;
    let modal = egui::Modal::new(egui::Id::unique("assistant-privacy")).show(ctx, |ui| {
        ui.set_width(520.0);
        title(ui, &t("About this assistant"));
        egui::ScrollArea::vertical().max_height(460.0).auto_shrink([false, true]).show(ui, |ui| {
            let section = |ui: &mut Ui, heading: String, body: String| {
                ui.add_space(6.0);
                ui.label(theme::text(heading, Font::Headline));
                ui.add(egui::Label::new(theme::text(body, Font::Callout)).wrap());
            };
            section(
                ui,
                t("Chat history"),
                t("Conversations are saved on this computer, separately for each music library. Switching AI provider keeps the chat. Only a limited recent context is included in each request."),
            );
            section(
                ui,
                t("Local model"),
                t("Requests go only to the local model server you configured. Library searches and streaming still use your music server. No automatic switch to ChatGPT."),
            );
            if !platform::IS_WEB {
                section(
                    ui,
                    "ChatGPT".into(),
                    t("ChatGPT receives your message, recent chat, the active collection summary, existing collection names, included track metadata, and up to 200 candidate songs for selections. Audio and server credentials are never sent. Your ChatGPT usage limits apply."),
                );
            }
            section(
                ui,
                t("Dictate message"),
                t("Dictation is not available in Discophile for computers: there is no on-device speech recognition that works on every system. Use your system's dictation feature in the message field instead."),
            );
            ui.add_space(6.0);
            ui.add(
                egui::Label::new(theme::secondary(
                    ui,
                    t("Ask to play a specific song, search your library, create a playlist for an activity, or learn about a track. AI answers may be inaccurate."),
                    Font::Callout,
                ))
                .wrap(),
            );
        });
        footer(ui, |ui| {
            if ui.button(t("Done")).clicked() {
                close = true;
            }
        });
    });
    if close || modal.should_close() {
        app.assistant.ui.privacy = false;
    }
}
