//! Launch notification, persistent toolbar action and post-restart Markdown changelog.

use crate::app::App;
use crate::features::software_update::{self as updates, UpdateStatus};
use crate::theme::{self, Font, Palette, icons};
use crate::ui::widgets;
use discophile_core::l10n::{t, tf};
use egui::{Align, Layout, Ui};

pub fn toolbar(app: &mut App, ui: &mut Ui) {
    if app.software_update.available.is_none() && !app.software_update.status.busy() {
        return;
    }
    let busy = app.software_update.status.busy();
    let (icon, label) = match app.software_update.status {
        UpdateStatus::Downloading { received, total } => (
            icons::ARROW_CIRCLE_DOWN,
            if total > 0 {
                format!("{} {:.0}%", t("Downloading the update…"), received as f64 / total as f64 * 100.0)
            } else {
                t("Downloading the update…")
            },
        ),
        UpdateStatus::Installing => (icons::ARROWS_CLOCKWISE, t("Verifying and installing the update…")),
        UpdateStatus::Restarting => (icons::POWER, t("Restarting Discophile…")),
        _ => {
            let version = &app.software_update.available.as_ref().unwrap().version;
            (icons::ARROW_CIRCLE_UP, format!("{}\n{}", tf("Discophile %@ is available", &[version]), t("Install and restart")))
        }
    };
    let color = Palette::of_ui(ui).accent;
    let response = widgets::icon_button_tinted(ui, icon, &label, 20.0, Some(color));
    app.software_update.notice_anchor = Some(response.rect);
    if !busy {
        ui.painter().circle_filled(response.rect.right_top() + egui::vec2(-5.0, 5.0), 3.0, color);
    }
    if response.clicked() {
        if busy {
            app.software_update.dialog = true;
        } else {
            updates::install(app);
        }
    }
    ui.add_space(widgets::space::S);
}

pub fn launch_notice(app: &mut App, ui: &mut Ui) {
    if modal_open(app) {
        return;
    }
    let Some(release) = updates::notice(app).cloned() else { return };
    let ctx = ui.ctx().clone();
    let viewport = ctx.content_rect();
    let anchor = app.software_update.notice_anchor.map(|rect| rect.right_bottom()).unwrap_or(viewport.right_top() + egui::vec2(-24.0, 64.0));
    egui::Area::new(egui::Id::unique("launch-update-notice"))
        .order(egui::Order::Foreground)
        .movable(false)
        .sense(egui::Sense::click())
        .pivot(egui::Align2::RIGHT_TOP)
        .fixed_pos(anchor + egui::vec2(0.0, 8.0))
        .accessible_name(t("Update available"))
        .role(egui::Role::Status)
        .fade_in(false)
        .show(&ctx, |ui| {
            ctx.accesskit_node_builder(egui::Id::unique("launch-update-notice"), |node| node.set_live(egui::accesskit::Live::Polite));
            let palette = Palette::of_ui(ui);
            egui::Frame::popup(ui.style()).fill(palette.surface).inner_margin(16).corner_radius(12).show(ui, |ui| {
                ui.set_width((viewport.width() - 64.0).clamp(180.0, 300.0));
                ui.spacing_mut().item_spacing = egui::vec2(8.0, 0.0);
                let width = ui.available_width();
                let text_width = (width - 96.0).max(60.0);
                let title = t("Update available");
                let subtitle = format!("Discophile {}", release.version);
                let title_height = ui.painter().layout(title.clone(), Font::Headline.id(), palette.text, text_width).size().y;
                let subtitle_height = ui.painter().layout(subtitle.clone(), Font::Callout.id(), palette.secondary, text_width).size().y;
                let text_height = title_height + 4.0 + subtitle_height;
                let (header, _) = ui.allocate_exact_size(egui::vec2(width, text_height.max(44.0)), egui::Sense::hover());
                let badge = egui::Rect::from_center_size(egui::pos2(header.left() + 16.0, header.center().y), egui::vec2(32.0, 32.0));
                ui.painter().rect_filled(badge, 9, palette.accent.gamma_multiply(0.12));
                ui.painter().text(badge.center(), egui::Align2::CENTER_CENTER, icons::ARROW_CIRCLE_UP, egui::FontId::proportional(20.0), palette.accent);
                let text_rect = egui::Rect::from_min_size(egui::pos2(header.left() + 44.0, header.center().y - text_height / 2.0), egui::vec2(text_width, text_height));
                let mut text_ui = ui.new_child(egui::UiBuilder::new().max_rect(text_rect).layout(Layout::top_down(Align::Min)));
                text_ui.label(theme::text(title, Font::Headline));
                text_ui.add_space(4.0);
                text_ui.label(theme::secondary(&text_ui, subtitle, Font::Callout));
                let close_rect = egui::Rect::from_center_size(egui::pos2(header.right() - 22.0, header.center().y), egui::vec2(44.0, 44.0));
                let dismiss = ui.put(close_rect, egui::Button::new(theme::icon(icons::X, 14.0).color(palette.secondary)).frame(false).min_size(egui::vec2(44.0, 44.0)));
                dismiss.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, t("Dismiss")));
                widgets::focus_ring(ui, &dismiss, 7);
                if dismiss.on_hover_text(t("Dismiss")).clicked() {
                    app.software_update.notice_dismissed = true;
                }
                ui.add_space(12.0);
                ui.with_layout(Layout::right_to_left(Align::Center).with_main_wrap(true), |ui| {
                    if action_button(ui, &t("Install and restart"), true).clicked() {
                        updates::install(app);
                    }
                    ui.spacing_mut().button_padding.x = 12.0;
                    let later = ui.add(egui::Button::new(theme::secondary(ui, t("Later"), Font::Callout)).frame(false).min_size(egui::vec2(0.0, 40.0)));
                    widgets::focus_ring(ui, &later, 7);
                    if later.clicked() {
                        app.software_update.notice_dismissed = true;
                    }
                });
            });
        });
}

pub fn notes(app: &mut App, ui: &mut Ui, markdown: &str) {
    ui.style_mut().url_in_tooltip = true;
    app.software_update.markdown.show(ui, markdown);
}

pub fn modal_open(app: &App) -> bool {
    (!app.software_update.status.busy() && app.software_update.changelog.is_some()) || app.software_update.dialog
}

pub(crate) fn background_accessibility(ctx: &egui::Context, id: egui::Id, modal: bool) {
    ctx.accesskit_node_builder(id, |node| {
        if modal {
            node.set_hidden();
        } else {
            node.clear_hidden();
        }
    });
}

fn escape_pressed(response: &egui::ModalResponse<()>) -> bool {
    response.is_top_modal && !response.any_popup_open && response.response.ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
}

fn action_button(ui: &mut Ui, label: &str, primary: bool) -> egui::Response {
    let palette = Palette::of_ui(ui);
    let text = theme::text(label, Font::Headline).color(if primary { egui::Color32::WHITE } else { palette.text });
    let response = ui
        .scope(|ui| {
            ui.spacing_mut().button_padding = egui::vec2(16.0, 8.0);
            ui.add(egui::Button::new(text).fill(if primary { palette.action } else { palette.surface_raised }).corner_radius(8).min_size(egui::vec2(0.0, 40.0)))
        })
        .inner;
    widgets::focus_ring(ui, &response, 7);
    response
}

fn heading(ui: &mut Ui, title: &str) {
    let response = ui.add(egui::Label::new(theme::text(title, Font::Title2)).wrap());
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_role(egui::Role::Heading);
        node.set_level(1);
    });
}

fn close_button(ui: &mut Ui) -> egui::Response {
    let response = ui.add(egui::Button::new(theme::icon(icons::X, 18.0)).frame(false).min_size(egui::vec2(44.0, 44.0)));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, t("Close release notes")));
    widgets::focus_ring(ui, &response, 7);
    response.on_hover_text(t("Close release notes"))
}

fn dialog(ctx: &egui::Context, id: egui::Id, title: &str) -> egui::Modal {
    let area = egui::Modal::default_area(id).accessible_name(title).role(egui::Role::Dialog).fade_in(false);
    widgets::modal(ctx, id).area(area)
}

fn begin_dialog(app: &mut App, id: egui::Id) -> bool {
    if app.software_update.active_dialog == Some(id) {
        return false;
    }
    app.software_update.return_focus = app.ctx().memory(|memory| memory.focused());
    app.software_update.active_dialog = Some(id);
    app.software_update.notes_scroll = 0.0;
    true
}

fn restore_focus(app: &mut App) {
    app.software_update.active_dialog = None;
    if let Some(id) = app.software_update.return_focus.take() {
        app.ctx().memory_mut(|memory| memory.request_focus(id));
    }
}

fn notes_height(viewport: egui::Vec2, header: f32) -> f32 {
    // Reserve both footer rows when actions wrap. Never force a minimum taller than the viewport.
    (viewport.y - 48.0 - 32.0 - header - 100.0).max(24.0)
}

fn changelog_footer(ui: &mut Ui) -> bool {
    let links = |ui: &mut Ui| {
        if widgets::icon_button(ui, icons::GLOBE, &t("Discophile website"), 18.0).clicked() {
            ui.ctx().open_url(egui::OpenUrl::new_tab(crate::platform::WEBSITE));
        }
        if widgets::icon_button(ui, icons::GITHUB_LOGO, &t("Discophile on GitHub"), 18.0).clicked() {
            ui.ctx().open_url(egui::OpenUrl::new_tab(crate::platform::GITHUB));
        }
    };
    let mut close = false;
    if ui.available_width() < 360.0 {
        ui.vertical(|ui| {
            close = action_button(ui, &t("Back to the music"), true).clicked();
            ui.add_space(12.0);
            ui.horizontal(links);
        });
    } else {
        ui.horizontal(|ui| {
            links(ui);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                close = action_button(ui, &t("Back to the music"), true).clicked();
            });
        });
    }
    close
}

fn scroll_keys(ui: &mut Ui, offset: f32, page: f32) -> f32 {
    ui.input_mut(|input| {
        let mut offset = offset;
        for (key, delta) in [(egui::Key::ArrowDown, 32.0), (egui::Key::ArrowUp, -32.0), (egui::Key::PageDown, page * 0.8), (egui::Key::PageUp, -page * 0.8)] {
            if input.consume_key(egui::Modifiers::NONE, key) {
                offset += delta;
            }
        }
        if input.consume_key(egui::Modifiers::NONE, egui::Key::Home) {
            offset = 0.0;
        }
        if input.consume_key(egui::Modifiers::NONE, egui::Key::End) {
            offset = f32::MAX;
        }
        offset.max(0.0)
    })
}

pub fn windows(app: &mut App, ctx: &egui::Context) {
    if !app.software_update.status.busy()
        && let Some(release) = app.software_update.changelog.clone()
    {
        let id = egui::Id::unique("installed-changelog");
        let opened = begin_dialog(app, id);
        let mut close = false;
        let response = dialog(ctx, id, &t("Discophile is updated")).show(ctx, |ui| {
            ctx.accesskit_node_builder(id, |node| node.set_modal());
            ui.spacing_mut().item_spacing.y = 0.0;
            let start = ui.cursor().min.y;
            ui.set_width((ctx.content_rect().width() - 80.0).clamp(120.0, 520.0));
            ui.horizontal_top(|ui| {
                let available = ui.available_width();
                ui.vertical(|ui| {
                    ui.set_width((available - 56.0).max(40.0));
                    heading(ui, &t("Discophile is updated"));
                    ui.add_space(4.0);
                    ui.label(theme::secondary(ui, tf("Welcome to version %@", &[&release.version]), Font::Callout));
                });
                let button = close_button(ui);
                if opened {
                    button.request_focus();
                }
                close |= button.clicked();
            });
            ui.add_space(16.0);
            ui.separator();
            ui.add_space(12.0);
            let height = notes_height(ctx.content_rect().size(), ui.cursor().min.y - start);
            let offset = scroll_keys(ui, app.software_update.notes_scroll, height);
            let output =
                egui::ScrollArea::vertical().id_salt("installed-changelog-notes").max_height(height).vertical_scroll_offset(offset).animated(false).show(
                    ui,
                    |ui| {
                        if release.notes.is_empty() {
                            ui.label(t("Release notes are available on GitHub."));
                        } else {
                            let prefix = format!("# Discophile {}\n\n", release.version);
                            notes(app, ui, release.notes.strip_prefix(&prefix).unwrap_or(&release.notes));
                        }
                    },
                );
            app.software_update.notes_scroll = output.state.clamped_offset().y;
            ui.add_space(12.0);
            ui.separator();
            ui.add_space(16.0);
            close |= changelog_footer(ui);
        });
        if close || escape_pressed(&response) {
            updates::dismiss_changelog(app);
            restore_focus(app);
        }
        return;
    }
    if !app.software_update.dialog {
        return;
    }
    let id = egui::Id::unique("install-update");
    let opened = begin_dialog(app, id);
    let status = app.software_update.status.clone();
    let busy = status.busy();
    let mut close = false;
    let title = match status {
        UpdateStatus::Restarting => t("Restarting Discophile…"),
        _ if busy => t("Updating Discophile…"),
        _ => t("Unable to install the update"),
    };
    let response = dialog(ctx, id, &title).show(ctx, |ui| {
        ctx.accesskit_node_builder(id, |node| node.set_modal());
        ui.set_width((ctx.content_rect().width() - 80.0).clamp(120.0, 440.0));
        // The close control stays available during a download; it hides the dialog, not the task.
        // The persistent toolbar icon reopens progress. No invisible keyboard trap while installing.
        ui.horizontal_top(|ui| {
            let width = ui.available_width();
            ui.vertical(|ui| {
                ui.set_width((width - 80.0).max(40.0));
                heading(ui, &title);
            });
            let button = action_button(ui, &t("Close"), false);
            if opened {
                button.request_focus();
            }
            close |= button.clicked();
        });
        if let Some(release) = &app.software_update.available {
            ui.label(theme::secondary(ui, format!("{} → {}", env!("CARGO_PKG_VERSION"), release.version), Font::Callout));
        }
        ui.add_space(16.0);
        let phase = match status {
            UpdateStatus::Downloading { .. } => t("Downloading the update…"),
            UpdateStatus::Installing => t("Verifying and installing the update…"),
            UpdateStatus::Restarting => t("Restarting Discophile…"),
            _ => t("The update was not completed."),
        };
        let phase_response = ui.label(&phase);
        ui.ctx().accesskit_node_builder(phase_response.id, |node| {
            node.set_role(egui::Role::Status);
            node.set_live(egui::accesskit::Live::Polite);
            node.set_live_atomic();
        });
        if busy {
            let (fraction, known) = match status {
                UpdateStatus::Downloading { received, total } => ((received as f32 / total.max(1) as f32).clamp(0.0, 1.0), total > 0),
                _ => (1.0, true),
            };
            let bar = ui.add(egui::ProgressBar::new(fraction).animate(false).text(if known {
                format!("{:.0}%", fraction * 100.0)
            } else {
                t("Downloading the update…")
            }));
            bar.widget_info(|| {
                let mut info = egui::WidgetInfo::labeled(egui::Role::ProgressIndicator, true, &phase);
                info.value = known.then_some((fraction * 100.0).round() as f64);
                info
            });
            ui.ctx().accesskit_node_builder(bar.id, |node| {
                if known {
                    node.set_min_numeric_value(0.0);
                    node.set_max_numeric_value(100.0);
                } else {
                    node.clear_numeric_value();
                }
            });
            ui.label(theme::secondary(ui, t("Discophile will install and restart automatically."), Font::Callout));
        }
        if let Some(error) = app.software_update.error.clone() {
            egui::ScrollArea::vertical().id_salt("update-error").max_height((ctx.content_rect().height() - 280.0).max(32.0)).show(ui, |ui| {
                ui.label(theme::text(t(&error), Font::Callout).color(Palette::of_ui(ui).text));
            });
            ui.add_space(12.0);
            ui.horizontal_wrapped(|ui| {
                if action_button(ui, &t("Try again"), true).clicked() {
                    updates::install(app);
                }
                if let Some(release) = &app.software_update.available {
                    ui.hyperlink_to(egui::RichText::new(t("Release notes")).color(Palette::of_ui(ui).accent).underline(), &release.page_url);
                }
            });
        }
    });
    if close || escape_pressed(&response) {
        app.software_update.dialog = false;
        restore_focus(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn large_text_keeps_room_for_dialog_actions() {
        for zoom in [1.0, 1.5, 2.0] {
            let viewport = egui::vec2(900.0 / zoom, 600.0 / zoom);
            let header = 78.0;
            let body = notes_height(viewport, header);
            assert!(body + header + 48.0 + 100.0 <= viewport.y);
        }
    }

    #[test]
    fn update_text_and_links_meet_minimum_contrast_in_both_themes() {
        fn luminance(color: egui::Color32) -> f32 {
            let linear = |channel: u8| {
                let value = channel as f32 / 255.0;
                if value <= 0.04045 { value / 12.92 } else { ((value + 0.055) / 1.055).powf(2.4) }
            };
            0.2126 * linear(color.r()) + 0.7152 * linear(color.g()) + 0.0722 * linear(color.b())
        }
        for palette in [Palette::light(), Palette::dark()] {
            for (foreground, background) in [
                (palette.text, palette.surface),
                (palette.secondary, palette.surface),
                (palette.accent, palette.surface),
                (palette.text, palette.surface_raised),
                (egui::Color32::WHITE, palette.action),
            ] {
                let (first, second) = (luminance(foreground), luminance(background));
                assert!((first.max(second) + 0.05) / (first.min(second) + 0.05) >= 4.5);
            }
        }
    }

    #[test]
    fn dialog_has_native_name_role_and_initial_keyboard_focus() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        crate::theme::install_fonts(&ctx);
        let id = egui::Id::unique("accessible-update-test");
        let mut close_id = None;
        let mut tree = None;
        for frame in 0..3 {
            let mut output = ctx.run_ui(Default::default(), |_| {
                dialog(&ctx, id, "Release notes").show(&ctx, |ui| {
                    ctx.accesskit_node_builder(id, |node| node.set_modal());
                    heading(ui, "Release notes");
                    let close = close_button(ui);
                    close_id = Some(close.id);
                    if frame == 0 {
                        close.request_focus();
                    }
                });
            });
            output.textures_delta.clear();
            tree = output.platform_output.accesskit_update;
        }
        let tree = tree.unwrap();
        assert!(
            tree.nodes.iter().any(|(_, node)| node.role() == egui::Role::Dialog && node.label() == Some("Release notes") && node.is_modal()),
            "{:?}",
            tree.nodes.iter().map(|(id, node)| (id, node.role(), node.label(), node.is_modal())).collect::<Vec<_>>()
        );
        assert_eq!(ctx.memory(|memory| memory.focused()), close_id);
        assert!(tree.nodes.iter().any(|(_, node)| node.role() == egui::Role::Button && node.label().is_some_and(|label| !label.contains(icons::X))));
    }
}
