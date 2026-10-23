//! Settings > Agents: the MCP server switch, how to connect an agent, recent agent activity, and the dialog
//! that asks before an agent removes songs from a playlist or deletes one.

use discophile_core::l10n::t;
#[cfg(not(target_arch = "wasm32"))]
use discophile_core::l10n::tf;
#[cfg(not(target_arch = "wasm32"))]
use egui::{Align, Layout, vec2};
use egui::Ui;

use crate::app::App;
use crate::theme::{self, Font};
#[cfg(not(target_arch = "wasm32"))]
use crate::theme::{Palette, icons};
use crate::ui::settings::group;
#[cfg(not(target_arch = "wasm32"))]
use crate::ui::settings::{footer, toggle_row};
#[cfg(not(target_arch = "wasm32"))]
use crate::ui::widgets;

#[cfg(target_arch = "wasm32")]
pub fn settings(_app: &mut App, ui: &mut Ui) {
    group(ui, |ui| {
        ui.add(
            egui::Label::new(theme::text(
                t("Agents connect to Discophile for computers: the web player cannot accept connections."),
                Font::Body,
            ))
            .wrap(),
        );
    });
}

#[cfg(not(target_arch = "wasm32"))]
pub fn settings(app: &mut App, ui: &mut Ui) {
    use crate::features::mcp::{self, Status};

    let palette = Palette::of_ui(ui);
    group(ui, |ui| {
        let mut enabled = app.settings.mcp_enabled;
        if toggle_row(ui, &mut enabled, &t("Allow agents to use Discophile"), Some(&t("Model Context Protocol server on this computer"))) {
            mcp::set_enabled(app, enabled);
        }
        ui.separator();
        let (icon, text, color) = match &app.mcp.status {
            Status::Off => (icons::PLUG, t("Off"), palette.secondary),
            Status::Running { .. } => (icons::PLUGS_CONNECTED, t("Running"), palette.accent),
            Status::Failed(message) => (icons::WARNING, message.clone(), palette.accent),
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(theme::icon(icon, 16.0).color(color));
            ui.label(theme::text(text, Font::Body).color(color));
        });
        ui.horizontal(|ui| {
            ui.label(theme::text(t("Port"), Font::Body));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let mut port = app.settings.mcp_port;
                let response = ui.add(egui::DragValue::new(&mut port).range(1024..=65535).speed(0.0));
                if response.lost_focus() || (response.changed() && !response.has_focus()) {
                    mcp::set_port(app, port);
                }
            });
        });
    });
    footer(
        ui,
        &t("Agents such as Claude can search your library, read music information, create and edit playlists, manage favorites and control playback. Discophile asks you before an agent removes songs from a playlist or deletes one. Only apps on this computer that know the access token can connect."),
    );

    widgets::section_heading(ui, &t("Connect an agent"), None);
    let endpoint = mcp::endpoint(app);
    let token = mcp::token(app);
    group(ui, |ui| {
        copy_row(ui, &t("Address"), &endpoint, &endpoint);
        ui.separator();
        let shown = ui.data(|d| d.get_temp::<bool>(egui::Id::unique("mcp-token-shown")).unwrap_or(false));
        let masked = if token.is_empty() { t("Created when you turn on the server") } else if shown { token.clone() } else { "•".repeat(24) };
        ui.horizontal(|ui| {
            ui.label(theme::text(t("Access token"), Font::Body));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_enabled_ui(!token.is_empty(), |ui| {
                    if widgets::icon_button(ui, icons::COPY, &t("Copy"), 16.0).clicked() {
                        ui.ctx().copy_text(token.clone());
                        app.toast(t("Copied"));
                    }
                    let (glyph, label) = if shown { (icons::EYE_SLASH, t("Hide")) } else { (icons::EYE, t("Show")) };
                    if widgets::icon_button(ui, glyph, &label, 16.0).clicked() {
                        ui.data_mut(|d| d.insert_temp(egui::Id::unique("mcp-token-shown"), !shown));
                    }
                });
                ui.add(egui::Label::new(theme::secondary(ui, masked, Font::Callout).monospace()).truncate());
            });
        });
        ui.add_space(4.0);
        if ui.button(format!("{}  {}", icons::KEY, t("New token"))).on_hover_text(t("Agents set up with the current token stop working.")).clicked() {
            mcp::regenerate_token(app);
            app.toast(t("New token created"));
        }
    });
    footer(ui, &t("Keep the token private: it gives access to your library and playback."));

    if !token.is_empty() {
        let claude_code = format!("claude mcp add --transport http discophile {endpoint} --header \"Authorization: Bearer {token}\"");
        let json = serde_json::to_string_pretty(&serde_json::json!({
            "mcpServers": {"discophile": {"type": "http", "url": endpoint, "headers": {"Authorization": format!("Bearer {token}")}}}
        }))
        .unwrap_or_default();
        let bridge = serde_json::to_string_pretty(&serde_json::json!({
            "mcpServers": {"discophile": {
                "command": "npx",
                "args": ["-y", "mcp-remote", endpoint, "--header", "Authorization:${DISCOPHILE_TOKEN}"],
                "env": {"DISCOPHILE_TOKEN": format!("Bearer {token}")}
            }}
        }))
        .unwrap_or_default();
        group(ui, |ui| {
            snippet(app, ui, "Claude Code", &t("Run in a terminal."), &claude_code);
            ui.separator();
            snippet(app, ui, &t("Clients with HTTP servers"), &t("Streamable HTTP configuration, as used by Cursor, VS Code and others."), &json);
            ui.separator();
            snippet(app, ui, "Claude Desktop", &t("Through the mcp-remote bridge (needs Node.js), in claude_desktop_config.json."), &bridge);
        });
    }

    widgets::section_heading(ui, &t("Recent activity"), None);
    let activity = mcp::activity(app);
    group(ui, |ui| {
        if activity.is_empty() {
            ui.label(theme::secondary(ui, t("No agent has used Discophile yet."), Font::Callout));
        }
        for entry in activity.iter().take(12) {
            let (glyph, outcome) = match &entry.outcome {
                mcp::Outcome::Done => (icons::CHECK_CIRCLE, String::new()),
                mcp::Outcome::Declined => (icons::PROHIBIT, t("Declined")),
                mcp::Outcome::Failed(message) => (icons::X_CIRCLE, message.clone()),
            };
            let time = chrono::DateTime::from_timestamp_millis(entry.time)
                .map(|d| d.with_timezone(&chrono::Local).format("%H:%M:%S").to_string())
                .unwrap_or_default();
            ui.horizontal(|ui| {
                ui.label(theme::icon(glyph, 15.0).color(palette.secondary));
                ui.label(theme::text(&entry.tool, Font::Callout).monospace());
                ui.label(theme::secondary(ui, format!("{} · {time}", entry.client), Font::Footnote));
            });
            if !outcome.is_empty() {
                ui.add(egui::Label::new(theme::secondary(ui, outcome, Font::Footnote)).wrap());
            }
        }
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn copy_row(ui: &mut Ui, label: &str, value: &str, copy: &str) {
    ui.horizontal(|ui| {
        ui.label(theme::text(label, Font::Body));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if widgets::icon_button(ui, icons::COPY, &t("Copy"), 16.0).clicked() {
                ui.ctx().copy_text(copy.to_string());
            }
            ui.add(egui::Label::new(theme::secondary(ui, value, Font::Callout).monospace()).truncate());
        });
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn snippet(app: &mut App, ui: &mut Ui, title: &str, detail: &str, text: &str) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(theme::text(title, Font::Headline));
            ui.add(egui::Label::new(theme::secondary(ui, detail, Font::Footnote)).wrap());
        });
        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            if ui.button(format!("{}  {}", icons::COPY, t("Copy"))).clicked() {
                ui.ctx().copy_text(text.to_string());
                app.toast(t("Copied"));
            }
        });
    });
    let palette = Palette::of_ui(ui);
    egui::Frame::new().fill(palette.surface).corner_radius(8).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let lines: Vec<String> = text.lines().map(|l| if l.contains("Bearer ") { mask(l) } else { l.to_string() }).collect();
        ui.add(egui::Label::new(theme::text(lines.join("\n"), Font::Footnote).monospace()).wrap());
    });
}

/// Hides the token in previews; Copy still copies the full text.
#[cfg(not(target_arch = "wasm32"))]
fn mask(line: &str) -> String {
    match line.find("Bearer ") {
        Some(start) => {
            let token_start = start + "Bearer ".len();
            let end = line[token_start..].find(|c: char| !c.is_ascii_hexdigit()).map_or(line.len(), |i| token_start + i);
            format!("{}••••••{}", &line[..token_start], &line[end..])
        }
        None => line.to_string(),
    }
}

/// Asks before an agent's destructive change, one at a time.
#[cfg(not(target_arch = "wasm32"))]
pub fn confirmation(app: &mut App, ctx: &egui::Context) {
    let Some(pending) = app.mcp.pending.front() else { return };
    let (client, title, detail) = (pending.client.clone(), pending.title.clone(), pending.detail.clone());
    let mut decision = None;
    widgets::modal(ctx, egui::Id::unique("mcp-confirmation")).show(ctx, |ui| {
        ui.set_width(420.0);
        ui.horizontal(|ui| {
            ui.label(theme::icon(icons::ROBOT, 22.0).color(Palette::of_ui(ui).accent));
            ui.label(theme::secondary(ui, tf("%@ is asking", &[&client]), Font::Subheadline));
        });
        ui.add_space(4.0);
        ui.add(egui::Label::new(theme::text(&title, Font::Title3)).wrap());
        if !detail.is_empty() {
            ui.add_space(4.0);
            ui.add(egui::Label::new(theme::secondary(ui, &detail, Font::Callout)).wrap());
        }
        ui.add_space(6.0);
        ui.add(egui::Label::new(theme::secondary(ui, t("This change is made on your music server and cannot be undone."), Font::Footnote)).wrap());
        ui.add_space(14.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 0.0);
            if widgets::action_button(ui, t("Allow"), false).clicked() {
                decision = Some(true);
            }
            if widgets::action_button(ui, t("Don't allow"), true).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                decision = Some(false);
            }
        });
    });
    if let Some(allow) = decision {
        crate::features::mcp::decide(app, allow);
    }
}
