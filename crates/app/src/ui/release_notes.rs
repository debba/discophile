//! Release Markdown with real font weights, underlined links and semantic headings/lists.

use crate::theme::{self, Font, Palette};
use egui::{
    FontId, RichText, Ui,
    text::{LayoutJob, TextFormat},
};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::collections::VecDeque;

#[derive(Clone, Debug, Default, PartialEq)]
struct Style {
    strong: bool,
    emphasis: bool,
    code: bool,
    strike: bool,
    link: Option<String>,
}
#[derive(Clone, Debug)]
struct Run {
    text: String,
    style: Style,
}
#[derive(Debug)]
enum Block {
    Text(Vec<Run>),
    Heading(u8, Vec<Run>),
    List(Option<u64>, Vec<Vec<Block>>),
    Quote(Vec<Block>),
    Code(String),
    Rule,
}

#[derive(Default)]
pub struct Cache {
    source: String,
    blocks: Vec<Block>,
}

impl Cache {
    pub fn show(&mut self, ui: &mut Ui, source: &str) {
        if self.source != source {
            self.source = source.into();
            let mut events = Parser::new_ext(source, Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS).collect();
            self.blocks = blocks(&mut events, None);
        }
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            render(ui, &self.blocks);
        });
    }
}

fn is_inline(event: &Event<'_>) -> bool {
    matches!(
        event,
        Event::Text(_)
            | Event::Code(_)
            | Event::SoftBreak
            | Event::HardBreak
            | Event::InlineHtml(_)
            | Event::Html(_)
            | Event::TaskListMarker(_)
            | Event::FootnoteReference(_)
            | Event::Start(Tag::Strong | Tag::Emphasis | Tag::Strikethrough | Tag::Link { .. } | Tag::Image { .. })
            | Event::End(TagEnd::Strong | TagEnd::Emphasis | TagEnd::Strikethrough | TagEnd::Link | TagEnd::Image)
    )
}

fn inline(events: &mut VecDeque<Event<'_>>) -> Vec<Run> {
    let mut style = Style::default();
    let mut runs: Vec<Run> = vec![];
    while events.front().is_some_and(is_inline) {
        let text = match events.pop_front().unwrap() {
            Event::Start(Tag::Strong) => {
                style.strong = true;
                continue;
            }
            Event::End(TagEnd::Strong) => {
                style.strong = false;
                continue;
            }
            Event::Start(Tag::Emphasis) => {
                style.emphasis = true;
                continue;
            }
            Event::End(TagEnd::Emphasis) => {
                style.emphasis = false;
                continue;
            }
            Event::Start(Tag::Strikethrough) => {
                style.strike = true;
                continue;
            }
            Event::End(TagEnd::Strikethrough) => {
                style.strike = false;
                continue;
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                style.link = Some(dest_url.into_string());
                continue;
            }
            Event::End(TagEnd::Link) => {
                style.link = None;
                continue;
            }
            // Images are represented by their alternative text, without remote downloads.
            Event::Start(Tag::Image { .. }) | Event::End(TagEnd::Image) => continue,
            Event::SoftBreak => " ".into(),
            Event::HardBreak => "\n".into(),
            Event::TaskListMarker(checked) => if checked { "[x] " } else { "[ ] " }.into(),
            Event::Text(text) | Event::FootnoteReference(text) | Event::Html(text) | Event::InlineHtml(text) => text.into_string(),
            Event::Code(text) => {
                runs.push(Run { text: text.into_string(), style: Style { code: true, ..style.clone() } });
                continue;
            }
            _ => unreachable!(),
        };
        if text.is_empty() {
            continue;
        }
        if let Some(last) = runs.last_mut().filter(|last| last.style == style) {
            last.text.push_str(&text);
        } else {
            runs.push(Run { text, style: style.clone() });
        }
    }
    runs
}

fn blocks(events: &mut VecDeque<Event<'_>>, end: Option<TagEnd>) -> Vec<Block> {
    let mut result = vec![];
    while let Some(event) = events.pop_front() {
        match event {
            Event::End(tag) if Some(tag) == end => break,
            Event::Start(Tag::Heading { level, .. }) => result.push(Block::Heading(level as u8, inline(events))),
            Event::Start(Tag::Paragraph) => result.push(Block::Text(inline(events))),
            Event::Start(Tag::BlockQuote(kind)) => result.push(Block::Quote(blocks(events, Some(TagEnd::BlockQuote(kind))))),
            Event::Start(Tag::List(start)) => {
                let mut items = vec![];
                while let Some(event) = events.pop_front() {
                    match event {
                        Event::Start(Tag::Item) => items.push(blocks(events, Some(TagEnd::Item))),
                        Event::End(TagEnd::List(_)) => break,
                        _ => {}
                    }
                }
                result.push(Block::List(start, items));
            }
            Event::Start(Tag::CodeBlock(_)) => {
                let mut code = String::new();
                while let Some(event) = events.pop_front() {
                    match event {
                        Event::Text(text) => code.push_str(&text),
                        Event::End(TagEnd::CodeBlock) => break,
                        _ => {}
                    }
                }
                result.push(Block::Code(code));
            }
            Event::Rule => result.push(Block::Rule),
            event if is_inline(&event) => {
                events.push_front(event);
                result.push(Block::Text(inline(events)));
            }
            _ => {}
        }
    }
    result
}

fn text_job(ui: &Ui, runs: &[Run], size: f32, heading: bool) -> LayoutJob {
    let palette = Palette::of_ui(ui);
    let mut job = LayoutJob::default();
    for run in runs {
        let font = if run.style.code {
            egui::FontFamily::Monospace
        } else if heading || run.style.strong {
            theme::semibold()
        } else {
            egui::FontFamily::Proportional
        };
        let color = if run.style.link.is_some() { palette.accent } else { palette.text };
        job.append(
            &run.text,
            0.0,
            TextFormat {
                font_id: FontId::new(size, font),
                line_height: Some((size * 1.4).round()),
                color,
                italics: run.style.emphasis,
                background: if run.style.code { palette.surface_raised } else { egui::Color32::TRANSPARENT },
                strikethrough: if run.style.strike { egui::Stroke::new(1.0, color) } else { egui::Stroke::NONE },
                underline: if run.style.link.is_some() { egui::Stroke::new(1.0, color) } else { egui::Stroke::NONE },
                ..Default::default()
            },
        );
    }
    job
}

fn span_rects(galley: &egui::Galley, origin: egui::Pos2, range: std::ops::Range<usize>) -> Vec<egui::Rect> {
    let mut cursor = 0;
    let mut rects = vec![];
    for row in &galley.rows {
        let start = range.start.saturating_sub(cursor).min(row.glyphs.len());
        let end = range.end.saturating_sub(cursor).min(row.glyphs.len());
        if start < end {
            rects.push(egui::Rect::from_min_max(
                origin + row.pos.to_vec2() + egui::vec2(row.glyphs[start].pos.x, 0.0),
                origin + row.pos.to_vec2() + egui::vec2(row.glyphs[end - 1].max_x(), row.size.y),
            ));
        }
        cursor += row.glyphs.len() + usize::from(row.ends_with_newline);
    }
    rects
}

fn render_inline(ui: &mut Ui, runs: &[Run]) -> egui::Rect {
    // Lay out the entire paragraph once. Separate wrapping widgets used to insert an empty
    // first line in list items with links and move their bullet above the text.
    ui.scope_builder(egui::UiBuilder::new().accessibility_role(egui::Role::Paragraph), |ui| {
        let mut job = text_job(ui, runs, Font::Body.id().size, false);
        job.wrap.max_width = ui.available_width().max(1.0);
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        let paragraph = ui.add(egui::Label::new(galley.clone()));
        // Paint/selection belong to the paragraph; semantic spans stay in reading order.
        ui.ctx().accesskit_node_builder(paragraph.id, |node| node.set_hidden());
        let mut first = 0;
        let mut char_start = 0;
        while first < runs.len() {
            let url = &runs[first].style.link;
            let end = (first + 1..runs.len()).find(|&index| &runs[index].style.link != url).unwrap_or(runs.len());
            let label: String = runs[first..end].iter().map(|run| run.text.as_str()).collect();
            let char_end = char_start + label.chars().count();
            let rects = span_rects(&galley, paragraph.rect.min, char_start..char_end);
            if let Some(bounds) = rects.iter().copied().reduce(|first, next| first.union(next)) {
                let link = url.as_ref().filter(|url| url::Url::parse(url).is_ok_and(|url| matches!(url.scheme(), "http" | "https")));
                let sense = if link.is_some() { egui::Sense::click() } else { egui::Sense::hover() };
                let response = ui.interact(bounds, paragraph.id.with(first), sense);
                response.widget_info(|| egui::WidgetInfo::labeled(if link.is_some() { egui::Role::Link } else { egui::Role::Label }, true, &label));
                if let Some(url) = link {
                    let over_text = ui.input(|input| input.pointer.hover_pos().is_some_and(|point| rects.iter().any(|rect| rect.contains(point))));
                    if response.clicked() && (over_text || !ui.input(|input| input.pointer.any_click())) {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                    }
                    if over_text {
                        response.clone().on_hover_text(url).on_hover_cursor(egui::CursorIcon::PointingHand);
                    }
                    if response.gained_focus() {
                        response.scroll_to_me(None);
                    }
                    if response.has_focus() {
                        for rect in rects {
                            ui.painter().rect_stroke(rect.expand(2.0), 2, egui::Stroke::new(1.5, Palette::of_ui(ui).accent), egui::StrokeKind::Outside);
                        }
                    }
                }
            }
            char_start = char_end;
            first = end;
        }
    })
    .response
    .rect
}

fn render(ui: &mut Ui, blocks: &[Block]) {
    for (index, block) in blocks.iter().enumerate() {
        if index > 0 {
            ui.add_space(if matches!(block, Block::Heading(..)) { 16.0 } else { 8.0 });
        }
        match block {
            Block::Text(runs) => {
                render_inline(ui, runs);
            }
            Block::Heading(level, runs) => {
                let size = match level {
                    1 => 20.0,
                    2 => 18.0,
                    _ => 16.0,
                };
                let response = ui.add(egui::Label::new(text_job(ui, runs, size, true)).wrap());
                ui.ctx().accesskit_node_builder(response.id, |node| {
                    node.set_role(egui::Role::Heading);
                    node.set_level(u32::from((*level + 1).min(6)) as _);
                });
            }
            Block::List(start, items) => {
                let group = ui.scope(|ui| {
                    for (index, item) in items.iter().enumerate() {
                        if index > 0 {
                            ui.add_space(8.0);
                        }
                        let row = ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, (Font::Body.id().size * 1.4).round()), egui::Sense::hover());
                            let marker = start.map_or_else(|| "•".into(), |start| format!("{}.", start + index as u64));
                            ui.painter().text(rect.right_center(), egui::Align2::RIGHT_CENTER, marker, Font::Body.id(), Palette::of_ui(ui).text);
                            ui.vertical(|ui| {
                                ui.set_width(ui.available_width());
                                render(ui, item);
                            });
                        });
                        ui.ctx().accesskit_node_builder(row.response.id, |node| node.set_role(egui::Role::ListItem));
                    }
                });
                ui.ctx().accesskit_node_builder(group.response.id, |node| node.set_role(egui::Role::List));
            }
            Block::Quote(blocks) => {
                let response = egui::Frame::new().inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| render(ui, blocks)).response;
                ui.painter().vline(response.rect.left(), response.rect.y_range(), egui::Stroke::new(2.0, Palette::of_ui(ui).secondary));
                ui.ctx().accesskit_node_builder(response.id, |node| node.set_role(egui::Role::Blockquote));
            }
            Block::Code(code) => {
                egui::Frame::new().fill(Palette::of_ui(ui).surface_raised).inner_margin(10).corner_radius(6).show(ui, |ui| {
                    ui.add(egui::Label::new(RichText::new(code).monospace().color(Palette::of_ui(ui).text)).wrap());
                });
            }
            Block::Rule => {
                ui.separator();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_preserves_nested_structure_and_inline_styles() {
        let source =
            "## Fixes\n\n- **Bold** and `code`, [PR #12](https://github.com/debba/discophile/pull/12)\n  - Nested\n\n> Quote\n\n```rust\nlet x = 1;\n```";
        let mut events = Parser::new(source).collect();
        let tree = blocks(&mut events, None);
        assert!(matches!(&tree[0], Block::Heading(2, _)));
        let Block::List(None, items) = &tree[1] else { panic!("missing list") };
        let Block::Text(runs) = &items[0][0] else { panic!("missing paragraph") };
        assert!(runs.iter().any(|run| run.text == "Bold" && run.style.strong));
        assert!(runs.iter().any(|run| run.text == "code" && run.style.code));
        assert!(runs.iter().any(|run| run.text == "PR #12" && run.style.link.is_some()));
        assert!(matches!(&items[0][1], Block::List(None, _)));
        assert!(matches!(&tree[2], Block::Quote(_)));
        assert!(matches!(&tree[3], Block::Code(code) if code == "let x = 1;\n"));
    }

    #[test]
    fn accessibility_tree_exposes_headings_lists_and_named_links() {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        ctx.enable_accesskit();
        let mut cache = Cache::default();
        let mut output =
            ctx.run_ui(Default::default(), |ui| cache.show(ui, "## Fixes\n\n- **Readable text** and [PR #12](https://github.com/debba/discophile/pull/12)"));
        output.textures_delta.clear();
        let tree = output.platform_output.accesskit_update.unwrap();
        assert!(tree.nodes.iter().any(|(_, node)| node.role() == egui::Role::Heading && node.level() == Some(3)));
        assert!(tree.nodes.iter().any(|(_, node)| node.role() == egui::Role::List));
        assert!(tree.nodes.iter().any(|(_, node)| node.role() == egui::Role::ListItem));
        assert!(tree.nodes.iter().any(|(_, node)| node.role() == egui::Role::Link && node.label() == Some("PR #12")));
    }

    #[test]
    fn links_do_not_insert_a_blank_first_line_or_change_paragraph_height() {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        let mut heights = vec![];
        let mut output = ctx.run_ui(Default::default(), |ui| {
            ui.set_width(300.0);
            for source in ["Changelog con **grassetto**, `codice` e PR #12.", "Changelog con **grassetto**, `codice` e [PR #12](https://example.com)."] {
                let mut events = Parser::new(source).collect();
                let Block::Text(runs) = blocks(&mut events, None).remove(0) else { panic!("missing paragraph") };
                heights.push(render_inline(ui, &runs).height());
            }
        });
        output.textures_delta.clear();
        assert!((heights[0] - heights[1]).abs() < 0.5, "{heights:?}");
        assert!(heights[0] < 45.0);
    }

    #[test]
    fn inline_link_hit_area_opens_its_destination() {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        ctx.enable_accesskit();
        let mut cache = Cache::default();
        let source = "Testo con [PR #12](https://github.com/debba/discophile/pull/12).";
        let mut output = ctx.run_ui(Default::default(), |ui| cache.show(ui, source));
        output.textures_delta.clear();
        let tree = output.platform_output.accesskit_update.unwrap();
        let bounds = tree.nodes.iter().find(|(_, node)| node.role() == egui::Role::Link).unwrap().1.bounds().unwrap();
        let point = egui::pos2(((bounds.x0 + bounds.x1) / 2.0) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32);
        for pressed in [true, false] {
            let input = egui::RawInput {
                events: vec![
                    egui::Event::PointerMoved(point),
                    egui::Event::PointerButton { pos: point, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE },
                ],
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| cache.show(ui, source));
            output.textures_delta.clear();
            if !pressed {
                assert!(
                    output.platform_output.commands.iter().any(|command| matches!(command, egui::OutputCommand::OpenUrl(url) if url.url.ends_with("/pull/12")))
                );
            }
        }
    }
}
