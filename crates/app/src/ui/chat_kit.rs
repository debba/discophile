//! Pieces shared by the music assistant chat and About, so both look and behave alike: the provider and
//! model menu, the composer, the question bubble, the reply label and the disclosure under the composer.

use discophile_core::assistant::AssistantProvider;
use discophile_core::l10n::t;
use discophile_core::mentions::MentionKind;
use egui::{Align, Color32, CornerRadius, Frame, Key, Layout, Margin, Modifiers, Sense, Stroke, Ui, UiBuilder, vec2};

use crate::app::App;
use crate::features::assistant::{self as feature, ModelPick};
use crate::nav::{Route, SettingsPage};
use crate::platform;
use crate::theme::{self, Font, Palette, icons};
use crate::ui::widgets;

/// The header's provider and model menu: the model name and a caret, a menu with Local model, ChatGPT,
/// the models of the chosen provider and a link to the settings. `chatgpt_model` is the chat's ChatGPT
/// model, empty for the default.
pub fn model_menu(app: &mut App, ui: &mut Ui, provider: AssistantProvider, chatgpt_model: &str) -> Option<ModelPick> {
    let name = feature::provider_name(app, provider, chatgpt_model);
    // The disclosure under the composer names the provider: a local chat shows just its model.
    let local_model = app.assistant.config.local.model.trim().to_string();
    let short = if provider == AssistantProvider::Local && !local_model.is_empty() { local_model.clone() } else { name.clone() };
    let palette = Palette::of_ui(ui);
    let label = theme::text(short, Font::Headline).color(palette.text);
    let caret = theme::icon(icons::CARET_DOWN, 13.0).color(palette.text);
    let button = egui::Button::new((label, caret)).frame(false).truncate();
    let mut pick = None;
    let (response, _) = egui::containers::menu::MenuButton::from_button(button).ui(ui, |ui| {
        ui.set_min_width(240.0);
        let check = |selected: bool| if selected { icons::CHECK } else { " " };
        let local = provider == AssistantProvider::Local;
        if ui.button(format!("{}  {}", check(local), t("Local model"))).clicked() {
            pick = Some(ModelPick::Provider(AssistantProvider::Local));
            ui.close();
        }
        if !platform::IS_WEB && ui.button(format!("{}  ChatGPT", check(!local))).clicked() {
            pick = Some(ModelPick::Provider(AssistantProvider::ChatGpt));
            ui.close();
        }
        ui.separator();
        ui.label(theme::secondary(ui, t("Model"), Font::Footnote));
        if local {
            // The server's models come from the last check; ask once when the menu opens without them.
            let state = &app.assistant.local;
            if state.models.is_empty() && !state.loading && state.status.is_none() && !local_model.is_empty() {
                feature::test_local(app);
            }
            let mut models = app.assistant.local.models.clone();
            if !local_model.is_empty() && !models.contains(&local_model) {
                models.insert(0, local_model.clone());
            }
            if models.is_empty() {
                if app.assistant.local.loading {
                    ui.spinner();
                } else {
                    ui.label(theme::secondary(ui, t("Choose a model in Settings."), Font::Footnote));
                }
            }
            for model in models {
                if ui.button(format!("{}  {model}", check(model == local_model))).clicked() {
                    feature::set_local_model(app, &model);
                    pick = Some(ModelPick::Local(model));
                    ui.close();
                }
            }
        } else if !platform::IS_WEB {
            let chosen = if chatgpt_model.is_empty() { app.assistant.config.chatgpt_model.clone() } else { chatgpt_model.to_string() };
            let models = app.assistant.chatgpt.models.clone();
            if models.is_empty() && app.assistant.chatgpt.models_loading {
                ui.spinner();
            }
            for item in models {
                if ui.button(format!("{}  {}", check(item.slug == chosen), item.display_name)).clicked() {
                    pick = Some(ModelPick::ChatGpt(item.slug));
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
    response.on_hover_text(format!("{}\n{name}", t("AI provider and model")));
    pick
}

pub enum ComposerEvent {
    None,
    Send,
    Stop,
}

/// The message field with its send (or stop) button. The field grows up to 120 points, then scrolls;
/// Enter sends and Shift+Enter adds a line. Returns the event and whether the text changed.
pub fn composer(
    ui: &mut Ui,
    draft: &mut String,
    editor_id: egui::Id,
    hint: &str,
    can_send: bool,
    answering: bool,
) -> (ComposerEvent, egui::Response) {
    let palette = Palette::of_ui(ui);
    let focused = ui.memory(|m| m.has_focus(editor_id));
    let mut event = if focused && can_send && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
        ComposerEvent::Send
    } else {
        ComposerEvent::None
    };
    let response = Frame::new()
        .fill(palette.surface)
        .corner_radius(CornerRadius::same(22))
        .stroke(Stroke::new(1.0, palette.separator))
        .inner_margin(Margin { left: 16, right: 4, top: 4, bottom: 4 })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // Laid out by hand: a horizontal row would cap the field at one control's height.
            let button_side = 40.0;
            let origin = ui.cursor().min;
            let width = ui.available_width();
            let field = egui::Rect::from_min_size(origin, vec2(width - button_side, 120.0));
            let mut field_ui = ui.new_child(UiBuilder::new().max_rect(field).layout(Layout::top_down(Align::Min)));
            let edit = egui::TextEdit::multiline(draft)
                .id(editor_id)
                .hint_text(hint)
                // As tall as the send button, so a single line sits on its center.
                .frame(Frame::NONE.inner_margin(Margin::symmetric(0, 11)))
                .desired_rows(1)
                .desired_width(field.width())
                .font(Font::Body.id());
            let response = egui::ScrollArea::vertical()
                .id_salt(editor_id.with("scroll"))
                .max_height(120.0)
                .min_scrolled_height(0.0)
                .show(&mut field_ui, |ui| ui.add(edit))
                .inner;
            let row = egui::Rect::from_min_size(origin, vec2(width, field_ui.min_rect().height().max(button_side)));
            let button = egui::Rect::from_min_size(row.right_bottom() - vec2(button_side, button_side), egui::Vec2::splat(button_side));
            let mut button_ui = ui.new_child(UiBuilder::new().max_rect(button).layout(Layout::right_to_left(Align::Center)));
            if answering {
                if widgets::icon_button_tinted(&mut button_ui, icons::STOP_CIRCLE, &t("Stop response"), 24.0, Some(palette.accent)).clicked() {
                    event = ComposerEvent::Stop;
                }
            } else {
                button_ui.add_enabled_ui(can_send, |ui| {
                    let send = widgets::icon_button_tinted(ui, icons::ARROW_CIRCLE_UP, &t("Send request"), 24.0, Some(palette.accent));
                    if send.on_hover_text(t("Enter sends, Shift+Enter adds a line.")).clicked() {
                        event = ComposerEvent::Send;
                    }
                });
            }
            ui.advance_cursor_after_rect(row);
            response
        })
        .inner;
    (event, response)
}

/// "Local model · AI can make mistakes" under the composer; it opens About this assistant.
pub fn disclosure(ui: &mut Ui, provider: AssistantProvider) -> egui::Response {
    let palette = Palette::of_ui(ui);
    let text = t(if provider == AssistantProvider::Local {
        "Local model · AI can make mistakes"
    } else {
        "ChatGPT · Requests, recent chat and music metadata shared with OpenAI"
    });
    ui.vertical_centered(|ui| {
        ui.add(egui::Label::new(theme::text(text, Font::Caption).color(palette.secondary)).wrap().sense(Sense::click()))
            .on_hover_cursor(egui::CursorIcon::PointingHand)
    })
    .inner
}

/// A question on the right, its lines reading from the left.
pub fn question_bubble(ui: &mut Ui, text: &str) -> egui::Response {
    let palette = Palette::of_ui(ui);
    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
        let width = (ui.available_width() * 0.85).max(120.0);
        Frame::new()
            .fill(palette.surface)
            .corner_radius(CornerRadius::same(18))
            .inner_margin(Margin::symmetric(14, 10))
            .show(ui, |ui| {
                ui.set_max_width(width);
                ui.with_layout(Layout::top_down(Align::Min), |ui| ui.add(egui::Label::new(theme::text(text, Font::Body)).wrap())).inner
            })
            .inner
    })
    .inner
}

/// "✧ model" above a reply.
pub fn reply_label(ui: &mut Ui, source: &str) {
    ui.label(theme::secondary(ui, format!("{}  {source}", icons::SPARKLE), Font::Caption));
}

/// Reply text whose library names (artists, albums, songs) are links, as in a web page: tinted, underlined
/// on hover, and opening the artist or album page when clicked. Without names it is a plain selectable
/// label.
pub fn linked_text(app: &mut App, ui: &mut Ui, text: &str, font: Font, color: Color32, line_height: Option<f32>) {
    let found = crate::features::mentions::find(app, ui.ctx(), text);
    if found.is_empty() {
        let mut rich = theme::text(text, font).color(color);
        if line_height.is_some() {
            rich = rich.line_height(line_height);
        }
        ui.add(egui::Label::new(rich).wrap().selectable(true));
        return;
    }
    let palette = Palette::of_ui(ui);
    let hover_id = ui.next_auto_id().with("hovered-mention");
    let hovered: Option<usize> = ui.data(|d| d.get_temp(hover_id)).flatten();
    let format = |color: Color32, underline: bool| egui::TextFormat {
        font_id: font.id(),
        color,
        line_height,
        underline: if underline { Stroke::new(1.0, color) } else { Stroke::NONE },
        ..Default::default()
    };
    let mut job = egui::text::LayoutJob::default();
    let mut cursor = 0;
    for (index, mention) in found.iter().enumerate() {
        job.append(&text[cursor..mention.range.start], 0.0, format(color, false));
        job.append(&text[mention.range.clone()], 0.0, format(palette.accent, hovered == Some(index)));
        cursor = mention.range.end;
    }
    job.append(&text[cursor..], 0.0, format(color, false));
    let (position, galley, response) = egui::Label::new(job).wrap().selectable(false).sense(Sense::click()).layout_in_ui(ui);
    let over = response.hover_pos().and_then(|pointer| {
        let char_index = galley.cursor_from_pos(pointer - position).index.0;
        // The cursor lands between characters: take the one under the pointer.
        let rect = galley.pos_from_cursor(egui::text::CCursor::new(char_index));
        let char_index = if pointer.x < position.x + rect.min.x { char_index.saturating_sub(1) } else { char_index };
        let byte = text.char_indices().nth(char_index).map_or(text.len(), |(b, _)| b);
        found.iter().position(|m| m.range.contains(&byte))
    });
    if over != hovered {
        ui.data_mut(|d| d.insert_temp(hover_id, over));
        ui.ctx().request_repaint();
    }
    ui.painter().galley(position, galley, color);
    let Some(index) = over else { return };
    let mention = found[index].clone();
    let action = t(match mention.kind {
        MentionKind::Artist => "Go to artist",
        MentionKind::Album | MentionKind::Song => "Go to album",
    });
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(action);
    if response.clicked() {
        crate::features::mentions::open(app, &mention);
    }
}
