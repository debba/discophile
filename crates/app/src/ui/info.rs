//! The About side panel: the assistant's overview of an artist, album, song, podcast or episode, follow-up
//! questions, and the sources it was written from (server notes, feed descriptions, Wikipedia, MusicBrainz).
//! It looks and works like the assistant chat: the model menu in the header, replies under the model's
//! name, the same composer, and sources under each reply as in ChatGPT.

use discophile_core::l10n::t;
use discophile_core::music_info::{InfoSource, InfoSourceKind, MAX_QUESTION_CHARACTERS, prefix};
use egui::{Align, Align2, Color32, CornerRadius, FontId, Frame, Layout, Margin, Sense, Stroke, Ui, pos2, vec2};

use crate::app::App;
use crate::features::info::{self, AnswerStatus};
use crate::nav::{Route, SettingsPage};
use crate::theme::{self, Font, Palette, icons};
use crate::ui::chat_kit as kit;
use crate::ui::widgets::{self, space};

/// Characters of a source shown before "Show more".
const SNIPPET: usize = 220;

pub fn panel(app: &mut App, ui: &mut Ui) {
    show(app, ui, "info-panel", true);
}

/// About inside Now Playing, beside the player.
pub fn inline(app: &mut App, ui: &mut Ui) {
    show(app, ui, "info-inline", false);
}

fn show(app: &mut App, ui: &mut Ui, id: &'static str, closable: bool) {
    egui::Panel::top((id, "header"))
        .frame(Frame::new().inner_margin(Margin { bottom: 6, ..Default::default() }))
        .show_separator_line(false)
        .show(ui, |ui| header(app, ui, closable));
    let can_ask = app.info.view.as_ref().is_some_and(|v| v.sources.is_some() && v.unavailable.is_none());
    if can_ask {
        egui::Panel::bottom((id, "composer"))
            .resizable(false)
            .frame(Frame::new().inner_margin(Margin { top: 6, ..Default::default() }))
            .show_separator_line(false)
            .show(ui, |ui| composer(app, ui));
    }
    let Some(view) = app.info.view.as_mut() else {
        widgets::empty_state(ui, icons::INFO, &t("Nothing selected"), &t("Choose About in a menu to learn more."));
        return;
    };
    let scroll_to_bottom = std::mem::take(&mut view.scroll_to_bottom);
    egui::ScrollArea::vertical().id_salt(id).auto_shrink([false, false]).stick_to_bottom(scroll_to_bottom).show(ui, |ui| {
        // Keeps text clear of the floating scroll bar.
        Frame::new().inner_margin(Margin { left: 0, right: 10, top: 0, bottom: 0 }).show(ui, |ui| {
            ui.set_width(ui.available_width());
            subject(app, ui);
            body(app, ui);
        });
        if scroll_to_bottom {
            ui.scroll_to_cursor(Some(Align::BOTTOM));
        }
    });
}

// MARK: Header

/// As in the assistant chat: the model menu on the left, the actions on the right.
fn header(app: &mut App, ui: &mut Ui, closable: bool) {
    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if closable && widgets::icon_button(ui, icons::X, &t("Close"), 16.0).clicked() {
                info::close(app);
            }
            let busy = app.info.view.as_ref().is_none_or(info::is_answering);
            if app.info.view.is_some() && !busy && widgets::icon_button(ui, icons::ARROW_CLOCKWISE, &t("Look up again"), 17.0).clicked() {
                info::retry(app);
            }
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                let Some((provider, model)) = app.info.view.as_ref().map(|v| (v.provider, v.model.clone())) else {
                    ui.label(theme::text(t("About"), Font::Headline));
                    return;
                };
                if let Some(pick) = kit::model_menu(app, ui, provider, &model) {
                    info::pick_model(app, pick);
                }
            });
        });
    });
}

/// What the panel is about: its kind, title and subtitle.
fn subject(app: &App, ui: &mut Ui) {
    let Some(view) = &app.info.view else { return };
    ui.add_space(space::XS);
    ui.label(theme::secondary(ui, view.subject.kind_label().to_uppercase(), Font::Caption).strong());
    ui.add(egui::Label::new(theme::text(view.subject.title(), Font::Title3)).wrap());
    if let Some(subtitle) = view.subject.subtitle() {
        ui.add(egui::Label::new(theme::secondary(ui, subtitle, Font::Subheadline)).wrap());
    }
    ui.add_space(space::L);
}

// MARK: Conversation

fn body(app: &mut App, ui: &mut Ui) {
    let Some(view) = &app.info.view else { return };
    let Some(sources) = view.sources.clone() else {
        ui.vertical_centered(|ui| {
            ui.add_space(space::XXL);
            ui.spinner();
            ui.add_space(space::S);
            ui.label(theme::secondary(ui, t("Looking for information…"), Font::Subheadline));
        });
        return;
    };
    let view_id = egui::Id::unique(("info-view", view.subject.cache_key()));
    let overview = view.overview.clone();
    let unavailable = view.unavailable.clone();
    let provider = view.provider_name.clone();
    let exchanges = view.exchanges.clone();
    let mut regenerate = false;
    match (&overview, &unavailable) {
        (_, Some(reason)) => {
            notice(app, ui, reason);
            if !sources.is_empty() {
                ui.add_space(space::L);
                ui.label(theme::text(t("Sources"), Font::Headline));
                ui.add_space(space::XS);
                source_cards(ui, view_id.with("all"), &sources, &(0..sources.len()).collect::<Vec<_>>());
            }
        }
        (Some(AnswerStatus::Loading), _) => {
            kit::reply_label(ui, &provider);
            progress(ui, &t("Writing an overview…"));
        }
        (Some(AnswerStatus::Done(answer)), _) => {
            kit::reply_label(ui, &provider);
            answer_text(app, ui, &answer.text);
            regenerate = reply_footer(ui, view_id.with("overview"), &answer.text, &sources, &answer.sources, true);
        }
        (Some(AnswerStatus::Failed(message)), _) => {
            error_text(ui, message);
            ui.add_space(space::S);
            regenerate = small_button(ui, icons::ARROW_CLOCKWISE, &t("Try again")).clicked();
        }
        (None, None) => {}
    }
    if regenerate {
        info::regenerate(app);
    }
    for (index, exchange) in exchanges.iter().enumerate() {
        ui.add_space(space::L);
        kit::question_bubble(ui, &exchange.question);
        ui.add_space(space::M);
        kit::reply_label(ui, &provider);
        match &exchange.answer {
            AnswerStatus::Loading => progress(ui, &t("Answering…")),
            AnswerStatus::Done(answer) => {
                answer_text(app, ui, &answer.text);
                reply_footer(ui, view_id.with(("exchange", index)), &answer.text, &sources, &answer.sources, false);
            }
            AnswerStatus::Failed(message) => error_text(ui, message),
        }
    }
    if sources.is_empty() {
        ui.add_space(space::L);
        let message = if app.settings.online_music_info {
            t("No information was found for this item.")
        } else {
            t("Your server has no notes for this item. Turn on Online music information to search Wikipedia and MusicBrainz.")
        };
        ui.add(egui::Label::new(reading(message, Font::Subheadline).color(Palette::of_ui(ui).secondary)).wrap());
    }
    if !app.settings.online_music_info {
        ui.add_space(space::M);
        if small_button(ui, icons::GLOBE, &t("Search Wikipedia and MusicBrainz too")).clicked() {
            app.nav.push(Route::Settings(SettingsPage::Assistant));
        }
    }
    ui.add_space(space::L);
}

/// Reading text: a comfortable line height for paragraphs.
fn reading(text: impl Into<String>, font: Font) -> egui::RichText {
    let size = font.id().size;
    theme::text(text, font).line_height(Some((size * 1.45).round()))
}

/// Paragraphs separated by blank lines; lines starting with "- " become bullets. Library names are links.
fn answer_text(app: &mut App, ui: &mut Ui, text: &str) {
    let color = Palette::of_ui(ui).text;
    let line_height = Some((Font::Body.id().size * 1.45).round());
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = space::M;
        for paragraph in text.split("\n\n").map(str::trim).filter(|p| !p.is_empty()) {
            let lines: Vec<String> = paragraph
                .lines()
                .map(|line| match line.trim_start().strip_prefix("- ") {
                    Some(item) => format!("•  {item}"),
                    None => line.to_string(),
                })
                .collect();
            kit::linked_text(app, ui, &lines.join("\n"), Font::Body, color, line_height);
        }
    });
}

fn error_text(ui: &mut Ui, message: &str) {
    let palette = Palette::of_ui(ui);
    ui.horizontal_top(|ui| {
        ui.label(theme::icon(icons::WARNING, 16.0).color(palette.accent));
        ui.add(egui::Label::new(reading(message, Font::Callout)).wrap());
    });
}

fn progress(ui: &mut Ui, text: &str) {
    ui.horizontal(|ui| {
        ui.spinner();
        ui.label(theme::secondary(ui, text, Font::Subheadline));
    });
}

/// A compact secondary action: focusable, with a visible label and a 30-point target.
fn small_button(ui: &mut Ui, icon: &str, label: &str) -> egui::Response {
    let palette = Palette::of_ui(ui);
    let text = theme::text(format!("{icon}  {label}"), Font::Callout).color(palette.accent);
    // Wraps so a long label never makes the resizable panel wider.
    ui.add(egui::Button::new(text).wrap().fill(palette.surface_raised).corner_radius(CornerRadius::same(12)).min_size(vec2(0.0, widgets::CONTROL_SMALL)))
}

/// Under a reply, as in ChatGPT: copy, write again (for the overview) and the Sources pill, which opens
/// the cards of the sources the reply relied on (all of them when it names none). Returns true for
/// Write again.
fn reply_footer(ui: &mut Ui, id: egui::Id, text: &str, sources: &[InfoSource], cited: &[usize], rewrite: bool) -> bool {
    let cited: Vec<usize> = if cited.is_empty() { (0..sources.len()).collect() } else { cited.iter().copied().filter(|i| *i < sources.len()).collect() };
    let mut open = ui.data(|d| d.get_temp::<bool>(id).unwrap_or(false));
    let mut again = false;
    ui.add_space(space::XS);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        if widgets::icon_button(ui, icons::COPY, &t("Copy"), 16.0).clicked() {
            ui.ctx().copy_text(text.to_string());
        }
        if rewrite && widgets::icon_button(ui, icons::ARROW_CLOCKWISE, &t("Write again"), 16.0).clicked() {
            again = true;
        }
        if !cited.is_empty() {
            ui.add_space(6.0);
            let kinds: Vec<InfoSourceKind> = cited.iter().map(|i| sources[*i].kind).collect();
            if sources_pill(ui, &kinds, open).clicked() {
                open = !open;
                ui.data_mut(|d| d.insert_temp(id, open));
            }
        }
    });
    if open && !cited.is_empty() {
        ui.add_space(space::S);
        source_cards(ui, id, sources, &cited);
    }
    again
}

// MARK: Sources

/// "Sources" with the providers' badges overlapping, like ChatGPT's sources button.
fn sources_pill(ui: &mut Ui, kinds: &[InfoSourceKind], open: bool) -> egui::Response {
    let palette = Palette::of_ui(ui);
    let mut unique: Vec<InfoSourceKind> = Vec::new();
    for kind in kinds {
        if !unique.contains(kind) {
            unique.push(*kind);
        }
    }
    let shown = unique.len().min(3);
    let badge = 18.0;
    let overlap = 6.0;
    let label = ui.painter().layout_no_wrap(t("Sources"), Font::Footnote.id(), palette.text);
    let badges_width = badge + (shown.saturating_sub(1)) as f32 * (badge - overlap);
    let size = vec2(8.0 + badges_width + 6.0 + label.size().x + 8.0, 28.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let fill = if open {
        palette.accent.gamma_multiply(0.16)
    } else if response.hovered() {
        palette.surface_raised
    } else {
        palette.surface
    };
    ui.painter().rect(rect, CornerRadius::same(14), fill, Stroke::new(1.0, palette.separator), egui::StrokeKind::Inside);
    let mut x = rect.left() + 8.0 + badge / 2.0;
    for kind in unique.iter().take(shown) {
        let center = pos2(x, rect.center().y);
        ui.painter().circle_filled(center, badge / 2.0 + 1.5, fill);
        paint_badge(ui, center, badge, *kind);
        x += badge - overlap;
    }
    let text_pos = pos2(rect.left() + 8.0 + badges_width + 6.0, rect.center().y - label.size().y / 2.0);
    ui.painter().galley(text_pos, label, palette.text);
    widgets::focus_ring(ui, &response, 14);
    let names = unique.iter().map(|k| kind_name(*k)).collect::<Vec<_>>().join(", ");
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, open, format!("{}: {names}", t("Sources"))));
    response.on_hover_text(names).on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn kind_name(kind: InfoSourceKind) -> String {
    match kind {
        InfoSourceKind::Server => t("Your library"),
        InfoSourceKind::Feed => t("Podcast feed"),
        InfoSourceKind::MusicBrainz => "MusicBrainz".into(),
        InfoSourceKind::Wikipedia => "Wikipedia".into(),
    }
}

/// A round mark for each provider, standing in for a site icon.
fn paint_badge(ui: &Ui, center: egui::Pos2, diameter: f32, kind: InfoSourceKind) {
    let palette = Palette::of_ui(ui);
    let painter = ui.painter();
    let radius = diameter / 2.0;
    match kind {
        InfoSourceKind::Wikipedia => {
            painter.circle(center, radius, Color32::WHITE, Stroke::new(1.0, Color32::from_gray(200)));
            painter.text(center, Align2::CENTER_CENTER, "W", FontId::new(diameter * 0.58, theme::bold()), Color32::BLACK);
        }
        InfoSourceKind::MusicBrainz => {
            painter.circle_filled(center, radius, theme::hex(0xBA478F));
            painter.text(center, Align2::CENTER_CENTER, "MB", FontId::new(diameter * 0.42, theme::bold()), Color32::WHITE);
        }
        InfoSourceKind::Server => {
            painter.circle_filled(center, radius, palette.action);
            widgets::paint_icon(painter, center, icons::MUSIC_NOTE, FontId::new(diameter * 0.6, theme::icon_fill()), Color32::WHITE);
        }
        InfoSourceKind::Feed => {
            painter.circle_filled(center, radius, theme::hex(0xD9822B));
            widgets::paint_icon(painter, center, icons::RSS_SIMPLE, FontId::proportional(diameter * 0.62), Color32::WHITE);
        }
    }
}

/// The sources at `indices` as cards: provider and site, title, an excerpt that expands, and the page.
fn source_cards(ui: &mut Ui, id: egui::Id, sources: &[InfoSource], indices: &[usize]) {
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = space::S;
        for index in indices {
            if let Some(source) = sources.get(*index) {
                source_card(ui, id.with(index), source);
            }
        }
    });
}

fn source_card(ui: &mut Ui, id: egui::Id, source: &InfoSource) {
    let palette = Palette::of_ui(ui);
    let mut expanded = ui.data(|d| d.get_temp::<bool>(id).unwrap_or(false));
    let long = source.text.trim().chars().count() > SNIPPET;
    Frame::new()
        .fill(palette.surface)
        .stroke(Stroke::new(1.0, palette.separator))
        .corner_radius(CornerRadius::same(widgets::CARD_RADIUS))
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = space::XS;
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                paint_badge(ui, rect.center(), 16.0, source.kind);
                let site = source.link.as_deref().and_then(|l| url::Url::parse(l).ok()).and_then(|u| u.host_str().map(|h| h.trim_start_matches("www.").to_string()));
                let line = match site {
                    Some(site) => format!("{} · {site}", source.provider()),
                    None => source.provider(),
                };
                ui.add(egui::Label::new(theme::secondary(ui, line, Font::Caption)).truncate());
            });
            ui.add(egui::Label::new(theme::text(&source.title, Font::Headline)).wrap());
            let text = if expanded || !long { source.text.trim().to_string() } else { prefix(&source.text, SNIPPET) };
            ui.add(egui::Label::new(reading(text, Font::Subheadline).color(palette.secondary)).wrap().selectable(true));
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().interact_size.y = 0.0;
                if long {
                    let label = t(if expanded { "Show less" } else { "Show more" });
                    if ui.add(egui::Link::new(theme::text(label, Font::Footnote).color(palette.accent))).clicked() {
                        expanded = !expanded;
                        ui.data_mut(|d| d.insert_temp(id, expanded));
                    }
                }
                if let Some(link) = &source.link {
                    ui.hyperlink_to(theme::text(format!("{}  {}", t("Open page"), icons::ARROW_SQUARE_OUT), Font::Footnote).color(palette.accent), link);
                }
            });
        });
}

/// Why no overview is written, with a button to the assistant settings.
fn notice(app: &mut App, ui: &mut Ui, reason: &str) {
    let palette = Palette::of_ui(ui);
    Frame::new()
        .fill(palette.surface)
        .stroke(Stroke::new(1.0, palette.separator))
        .corner_radius(CornerRadius::same(widgets::CARD_RADIUS))
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = space::S;
                ui.label(theme::icon(icons::SPARKLE, 18.0).color(palette.accent));
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = space::XS;
                    ui.add(egui::Label::new(reading(t("An AI overview needs the music assistant."), Font::Callout)).wrap());
                    ui.add(egui::Label::new(reading(reason, Font::Subheadline).color(palette.secondary)).wrap());
                    ui.add_space(space::XS);
                    if small_button(ui, icons::GEAR_SIX, &t("Set up")).clicked() {
                        app.nav.push(Route::Settings(SettingsPage::Assistant));
                    }
                });
            });
        });
}

// MARK: Composer

fn composer(app: &mut App, ui: &mut Ui) {
    let palette = Palette::of_ui(ui);
    let Some(view) = app.info.view.as_mut() else { return };
    let busy = info::is_answering(view);
    let provider = view.provider;
    let editor_id = egui::Id::unique("info-question");
    let length = view.draft.trim().chars().count();
    let can_send = !busy && length > 0 && length <= MAX_QUESTION_CHARACTERS;
    ui.spacing_mut().item_spacing.y = 6.0;
    let (event, response) = kit::composer(ui, &mut view.draft, editor_id, &t("Ask about this…"), can_send, false);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::MultilineTextInput, true, t("Your question")));
    if length > MAX_QUESTION_CHARACTERS {
        ui.label(theme::text(t("Keep your question shorter."), Font::Caption).color(palette.accent));
    }
    if kit::disclosure(ui, provider).on_hover_text(t("About this assistant")).clicked() {
        app.assistant.ui.privacy = true;
    }
    if matches!(event, kit::ComposerEvent::Send) {
        info::ask(app);
        ui.memory_mut(|m| m.request_focus(editor_id));
    }
}
