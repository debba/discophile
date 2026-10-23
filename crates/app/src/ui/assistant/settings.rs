//! Settings > Music assistant: the provider of new chats, the ChatGPT connection (desktop only) and the
//! local model server, with the privacy explanations of the iOS app adapted to a computer.

use discophile_core::assistant::AssistantProvider;
use discophile_core::assistant::local_model::DEFAULT_BASE_URL;
use discophile_core::l10n::t;
use egui::{Ui, vec2};

use crate::app::App;
use crate::features::assistant as feature;
use crate::platform;
use crate::theme::{self, Font, Palette, icons};
use crate::ui::settings::{choice_row, footer, group};
use crate::ui::widgets;

pub fn show(app: &mut App, ui: &mut Ui) {
    provider_section(app, ui);
    chatgpt_section(app, ui);
    local_section(app, ui);
    privacy_section(ui);
}

fn provider_section(app: &mut App, ui: &mut Ui) {
    widgets::section_heading(ui, &t("AI provider and model"), None);
    let current = app.assistant.config.provider;
    let mut chosen = None;
    group(ui, |ui| {
        if choice_row(
            ui,
            current == AssistantProvider::Local,
            &t("Local model"),
            Some(&t("An OpenAI-compatible server you run, such as Ollama or LM Studio.")),
        ) {
            chosen = Some(AssistantProvider::Local);
        }
        if !platform::IS_WEB
            && choice_row(ui, current == AssistantProvider::ChatGpt, "ChatGPT", Some(&t("The models available to your ChatGPT account.")))
        {
            chosen = Some(AssistantProvider::ChatGpt);
        }
    });
    footer(
        ui,
        &t("New chats use this provider. The menu at the top of a chat switches provider and model while keeping the conversation."),
    );
    if let Some(provider) = chosen {
        app.assistant.config.provider = provider;
        feature::save_config(app);
        if provider == AssistantProvider::ChatGpt && app.assistant.chatgpt.models.is_empty() {
            feature::load_models(app);
        }
    }
}

fn error_text(ui: &mut Ui, text: &str) {
    ui.add(egui::Label::new(theme::text(text, Font::Callout).color(Palette::of_ui(ui).accent)).wrap());
}

fn chatgpt_section(app: &mut App, ui: &mut Ui) {
    widgets::section_heading(ui, &t("ChatGPT connection"), None);
    #[cfg(target_arch = "wasm32")]
    {
        let _ = app;
        group(ui, |ui| {
            ui.add(
                egui::Label::new(theme::text(
                    t("Signing in with ChatGPT needs Discophile for computers: the web player cannot receive the sign-in redirect. Use a local model instead."),
                    Font::Body,
                ))
                .wrap(),
            );
        });
    }
    #[cfg(not(target_arch = "wasm32"))]
    chatgpt_connection(app, ui);
}

#[cfg(not(target_arch = "wasm32"))]
fn chatgpt_connection(app: &mut App, ui: &mut Ui) {
    use crate::ui::settings::value_row;
    let state = &app.assistant.chatgpt;
    let credential = state.credential().cloned();
    let registrations = state.registrations();
    let busy = state.busy;
    let unreadable = state.vault.is_none();
    let sign_in_url = state.sign_in.as_ref().map(|s| s.url.clone());
    let models = state.models.clone();
    let models_loading = state.models_loading;
    let error = state.error.clone();
    group(ui, |ui| {
        if unreadable {
            if let Some(error) = &error {
                error_text(ui, error);
            }
            return;
        }
        match &credential {
            Some(credential) => {
                value_row(ui, &t("Connected account"), &credential.email.clone().unwrap_or_else(|| t("ChatGPT account")));
                ui.add_space(4.0);
                if !models.is_empty() {
                    ui.horizontal(|ui| {
                        ui.label(theme::text(t("Model"), Font::Body));
                        let selected = app.assistant.config.chatgpt_model.clone();
                        let shown = models
                            .iter()
                            .find(|m| m.slug == selected)
                            .map(|m| m.display_name.clone())
                            .unwrap_or_else(|| t("Default model"));
                        egui::ComboBox::from_id_salt("assistant-chatgpt-model").selected_text(shown).width(220.0).show_ui(ui, |ui| {
                            for model in &models {
                                if ui.selectable_label(model.slug == selected, &model.display_name).clicked() {
                                    feature::set_default_model(app, &model.slug);
                                }
                            }
                        });
                    });
                } else if models_loading {
                    ui.spinner();
                }
                ui.horizontal_wrapped(|ui| {
                    if ui.add_enabled(!busy && !models_loading, egui::Button::new(t("Refresh models"))).clicked() {
                        app.assistant.chatgpt.error = None;
                        feature::load_models(app);
                    }
                    if ui.add_enabled(!busy, egui::Button::new(t("Reconnect ChatGPT"))).clicked() {
                        let registration = registrations.iter().find(|r| r.client_id == credential.client_id).cloned();
                        feature::sign_in(app, registration);
                    }
                    let palette = Palette::of_ui(ui);
                    let disconnect = egui::Button::new(theme::text(t("Disconnect ChatGPT"), Font::Body).color(palette.danger));
                    if ui.add_enabled(!busy, disconnect).clicked() {
                        feature::sign_out(app);
                    }
                });
            }
            None => {
                let button = widgets::action_button(ui, t("Continue with ChatGPT"), false);
                if button.clicked() && !busy {
                    feature::sign_in(app, registrations.last().cloned());
                }
            }
        }
        if !registrations.is_empty() && ui.add_enabled(!busy, egui::Button::new(t("Use another ChatGPT account"))).clicked() {
            feature::sign_in(app, None);
        }
        if busy {
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.spinner();
                if let Some(url) = &sign_in_url {
                    ui.label(theme::secondary(
                        ui,
                        t("Finish signing in in your browser. Discophile receives the answer on this computer."),
                        Font::Callout,
                    ));
                    if ui.link(t("Open the sign-in page again")).clicked() {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                    }
                    if ui.button(t("Cancel")).clicked() {
                        feature::cancel_sign_in(app);
                    }
                }
            });
        }
        if let Some(error) = &error {
            ui.add_space(4.0);
            error_text(ui, error);
        }
    });
    footer(
        ui,
        &t(
            "Sign in on OpenAI's website and choose whether to share your ChatGPT plan's model usage with Discophile. Availability depends on your account. Access tokens are stored only in this computer's keychain.",
        ),
    );
    ui.hyperlink_to(format!("{}  {}", icons::ARROW_SQUARE_OUT, t("Manage ChatGPT usage")), "https://chatgpt.com/settings/usage");
    footer(
        ui,
        &t(
            "ChatGPT receives only requests you send with ChatGPT selected, plus the metadata described in the assistant. The local model runs on a server you choose and does not need a ChatGPT connection.",
        ),
    );
}

fn local_section(app: &mut App, ui: &mut Ui) {
    widgets::section_heading(ui, &t("Local model"), None);
    let mut base = app.assistant.config.local.base_url.clone();
    let mut model = app.assistant.config.local.model.clone();
    let models = app.assistant.local.models.clone();
    let loading = app.assistant.local.loading;
    let status = app.assistant.local.status.clone();
    let has_key = feature::has_local_api_key(app);
    group(ui, |ui| {
        ui.label(theme::text(t("Server address"), Font::Headline));
        let response =
            ui.add(egui::TextEdit::singleline(&mut base).align(egui::Align2::LEFT_CENTER).hint_text(DEFAULT_BASE_URL).desired_width(f32::INFINITY).margin(vec2(10.0, 8.0)));
        if response.changed() {
            app.assistant.config.local.base_url = base.clone();
            app.assistant.local.status = None;
        }
        if response.lost_focus() {
            feature::set_local_base_url(app, &base);
        }
        ui.add_space(8.0);
        ui.label(theme::text(t("Model"), Font::Headline));
        ui.horizontal(|ui| {
            let response =
                ui.add(egui::TextEdit::singleline(&mut model).align(egui::Align2::LEFT_CENTER).hint_text("llama3.2").desired_width(240.0).margin(vec2(10.0, 8.0)));
            if response.lost_focus() && model.trim() != app.assistant.config.local.model {
                feature::set_local_model(app, &model);
            }
            if !models.is_empty() {
                egui::ComboBox::from_id_salt("assistant-local-models").selected_text(t("Choose")).width(200.0).show_ui(ui, |ui| {
                    for item in &models {
                        if ui.selectable_label(item == &app.assistant.config.local.model, item).clicked() {
                            feature::set_local_model(app, item);
                        }
                    }
                });
            }
        });
        ui.add_space(8.0);
        ui.label(theme::text(t("API key (optional)"), Font::Headline));
        ui.horizontal(|ui| {
            let mut key = app.assistant.local.api_key.clone();
            let response = ui.add(
                egui::TextEdit::singleline(&mut key).align(egui::Align2::LEFT_CENTER)
                    .password(true)
                    .hint_text(t("Only if your server requires one"))
                    .desired_width(240.0)
                    .margin(vec2(10.0, 8.0)),
            );
            if response.changed() {
                app.assistant.local.api_key = key;
            }
            let edited = app.assistant.local.api_key.trim() != app.assistant.config.local.api_key.clone().unwrap_or_default();
            if ui.add_enabled(edited, egui::Button::new(t("Save"))).clicked() {
                feature::save_local_api_key(app);
            }
            if has_key && ui.button(t("Remove")).clicked() {
                app.assistant.local.api_key.clear();
                feature::save_local_api_key(app);
            }
        });
        ui.label(theme::secondary(
            ui,
            t(if platform::IS_WEB { "Stored in this browser's storage for this site." } else { "Stored in your system keychain." }),
            Font::Footnote,
        ));
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(!loading, egui::Button::new(format!("{}  {}", icons::PLUGS_CONNECTED, t("Test connection")))).clicked() {
                feature::set_local_base_url(app, &base);
                feature::test_local(app);
            }
            if loading {
                ui.spinner();
            }
            match &status {
                Some(Ok(text)) => {
                    ui.label(theme::text(format!("{}  {text}", icons::CHECK_CIRCLE), Font::Callout).color(Palette::of_ui(ui).positive));
                }
                Some(Err(text)) => error_text(ui, text),
                None => {}
            }
        });
    });
    footer(
        ui,
        &t(
            "The local model runs on an OpenAI-compatible server such as Ollama, LM Studio or llama.cpp. Requests go only to this address, and the server must support structured JSON output.",
        ),
    );
    if platform::IS_WEB {
        footer(
            ui,
            &t(
                "In the web player, the local model server must allow requests from this page (CORS). For Ollama, set OLLAMA_ORIGINS to this site's address.",
            ),
        );
    }
}

fn privacy_section(ui: &mut Ui) {
    widgets::section_heading(ui, &t("Privacy"), None);
    group(ui, |ui| {
        let paragraphs = [
            "Models see only bounded public metadata (title, artist, album, years, genre) with temporary IDs. Audio, server credentials and private library identifiers are never included. Library edits are proposals that you review and confirm.",
            "Sending a ChatGPT request shares its text, bounded recent conversation text and, if included, the current track's title, artist and album. This also applies to recent messages written with the local model if you then choose ChatGPT. Selections share metadata of at most 200 candidate songs. Requests use store: false.",
            "Chats, drafts, the chosen provider and model, and playlist results are saved on this computer for each music library. They are separate from conversations on the ChatGPT website.",
            "AI answers and mood recommendations can be inaccurate; the models have no web access or audio analysis. Copy request and open ChatGPT copies the request and included metadata for pasting into ChatGPT; that conversation cannot operate Discophile.",
        ];
        for (index, text) in paragraphs.iter().enumerate() {
            if index > 0 {
                ui.add_space(6.0);
            }
            ui.add(egui::Label::new(theme::text(t(text), Font::Callout)).wrap());
        }
    });
}
