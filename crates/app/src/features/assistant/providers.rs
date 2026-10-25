//! Assistant providers: persisted preferences, the ChatGPT connection (secret vault, sign-in through a
//! loopback redirect, model catalog, sign-out) and the local OpenAI-compatible model server.
//!
//! The ChatGPT vault (host identifier, previous registrations and the OAuth credential) is one JSON
//! document in `platform::secrets` (the OS keychain on desktop). The web build cannot receive the OAuth
//! redirect (browsers cannot listen on a loopback port), so it offers only the local model.

use discophile_core::assistant::chatgpt::{self, ChatGptClient, ChatGptCredential, ChatGptModel, ChatGptRegistration, ChatGptVault};
use discophile_core::assistant::local_model::{LocalModelClient, LocalModelSettings};
use discophile_core::assistant::turn::{ChatGptResponder, ProviderResponder};
use discophile_core::assistant::{AssistantError, AssistantProvider, assistant_error_message};
use discophile_core::l10n::{t, tf};
use serde::{Deserialize, Serialize};

use crate::app::App;
use crate::platform::{self, secrets, storage};
use crate::tasks::{Tasks, update};

const CONFIG_KEY: &str = "assistant";
const VAULT_SECRET: &str = "chatgpt";
const LOCAL_KEY_SECRET: &str = "assistant:local-api-key";

/// Assistant preferences shared by every library.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AssistantConfig {
    /// Provider of new chats: the one chosen last.
    pub provider: AssistantProvider,
    /// ChatGPT model used by chats that have not chosen one.
    pub chatgpt_model: String,
    /// Local model server. The optional API key lives in secrets.
    pub local: LocalModelSettings,
}

#[derive(Default)]
pub struct ChatGptState {
    /// `None` when the stored vault could not be read; it is then never overwritten.
    pub vault: Option<ChatGptVault>,
    pub models: Vec<ChatGptModel>,
    pub models_loading: bool,
    /// Signing in, exchanging the code or signing out.
    pub busy: bool,
    pub sign_in: Option<SignIn>,
    pub error: Option<String>,
    generation: u64,
}

impl ChatGptState {
    pub fn credential(&self) -> Option<&ChatGptCredential> {
        self.vault.as_ref()?.credential.as_ref()
    }

    pub fn registrations(&self) -> Vec<ChatGptRegistration> {
        self.vault.as_ref().map(|v| v.registrations.clone()).unwrap_or_default()
    }
}

/// A pending browser sign-in.
pub struct SignIn {
    /// The authorization page, offered again in case the browser did not open.
    pub url: String,
    #[cfg(not(target_arch = "wasm32"))]
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

#[derive(Default)]
pub struct LocalState {
    /// Model IDs offered by the server, from the last successful check.
    pub models: Vec<String>,
    pub loading: bool,
    /// Result of Test connection: a confirmation or an error.
    pub status: Option<Result<String, String>>,
    /// API key being edited in Settings.
    pub api_key: String,
    generation: u64,
}

pub(super) fn load(app: &mut App) {
    let mut config: AssistantConfig = storage::load(CONFIG_KEY).unwrap_or_default();
    config.local.api_key = secrets::get(LOCAL_KEY_SECRET).filter(|k| !k.is_empty());
    if platform::IS_WEB {
        config.provider = AssistantProvider::Local;
    }
    app.assistant.local.api_key = config.local.api_key.clone().unwrap_or_default();
    app.assistant.config = config;
    if platform::IS_WEB {
        return;
    }
    let chatgpt = &mut app.assistant.chatgpt;
    match secrets::get(VAULT_SECRET) {
        // Nothing is written to the keychain until the first sign-in.
        None => chatgpt.vault = Some(ChatGptVault::new()),
        Some(text) => match serde_json::from_str::<ChatGptVault>(&text) {
            Ok(vault) => chatgpt.vault = Some(vault),
            Err(_) => {
                chatgpt.vault = None;
                chatgpt.error = Some(t(
                    "Your ChatGPT connection could not be read. Check that your system keychain is unlocked, then restart Discophile.",
                ));
            }
        },
    }
    if app.assistant.chatgpt.credential().is_some() {
        load_models(app);
    }
}

pub fn save_config(app: &mut App) {
    storage::save(CONFIG_KEY, &app.assistant.config);
}

fn save_vault(app: &mut App) {
    if let Some(vault) = &app.assistant.chatgpt.vault {
        match serde_json::to_string(vault) {
            Ok(text) => secrets::set(VAULT_SECRET, &text),
            Err(error) => log::error!("Unable to encode the ChatGPT vault: {error}"),
        }
    }
}

/// Saves a new or refreshed credential. A different account clears the cached models.
fn store_credential(app: &mut App, credential: ChatGptCredential) {
    let Some(vault) = app.assistant.chatgpt.vault.as_mut() else { return };
    if vault.store(credential) {
        app.assistant.chatgpt.models.clear();
        app.assistant.config.chatgpt_model.clear();
        save_config(app);
    }
    save_vault(app);
}

// MARK: Availability and responders

/// Why a provider cannot answer right now, or `None` when it can.
pub fn unavailable_reason(app: &App, provider: AssistantProvider) -> Option<String> {
    match provider {
        AssistantProvider::Local => app
            .assistant
            .config
            .local
            .model
            .trim()
            .is_empty()
            .then(|| t("Choose a local model in Settings > Music assistant to use this assistant.")),
        AssistantProvider::ChatGpt if platform::IS_WEB => Some(t("ChatGPT is not available in the web player. Choose the local model.")),
        AssistantProvider::ChatGpt => app.assistant.chatgpt.credential().is_none().then(|| t("Connect ChatGPT to use this assistant.")),
    }
}

/// The name shown with replies and in the provider menu.
pub fn provider_name(app: &App, provider: AssistantProvider, model_id: &str) -> String {
    match provider {
        AssistantProvider::Local => {
            let model = app.assistant.config.local.model.trim();
            if model.is_empty() { t("Local model") } else { format!("{} · {model}", t("Local model")) }
        }
        AssistantProvider::ChatGpt => {
            let chosen = if model_id.is_empty() { &app.assistant.config.chatgpt_model } else { model_id };
            chatgpt::model_display_name(&app.assistant.chatgpt.models, chosen)
        }
    }
}

/// What a background task needs to build a responder; prepared on the interface thread.
pub enum ResponderPlan {
    Local { client: LocalModelClient, source: String },
    ChatGpt { credential: ChatGptCredential, model: String, default_model: String, models: Vec<ChatGptModel> },
}

pub fn plan_responder(app: &App, provider: AssistantProvider, model_id: &str) -> Result<ResponderPlan, AssistantError> {
    match provider {
        AssistantProvider::Local => {
            let settings = &app.assistant.config.local;
            if settings.model.trim().is_empty() {
                return Err(AssistantError::LocalUnavailable);
            }
            Ok(ResponderPlan::Local { client: LocalModelClient::new(settings)?, source: settings.model.trim().to_string() })
        }
        AssistantProvider::ChatGpt => {
            if platform::IS_WEB {
                return Err(AssistantError::SignInRequired);
            }
            let credential = app.assistant.chatgpt.credential().cloned().ok_or(AssistantError::SignInRequired)?;
            Ok(ResponderPlan::ChatGpt {
                credential,
                model: model_id.to_string(),
                default_model: app.assistant.config.chatgpt_model.clone(),
                models: app.assistant.chatgpt.models.clone(),
            })
        }
    }
}

impl ResponderPlan {
    /// The reply source used when the responder cannot be built.
    pub fn source(&self) -> String {
        match self {
            Self::Local { source, .. } => source.clone(),
            Self::ChatGpt { .. } => "ChatGPT".into(),
        }
    }

    /// Refreshes the ChatGPT credential and model catalog when needed. Returns the responder and the
    /// name shown with its replies.
    pub async fn build(self, tasks: Tasks) -> Result<(ProviderResponder, String), AssistantError> {
        match self {
            Self::Local { client, source } => Ok((ProviderResponder::Local(client), source)),
            Self::ChatGpt { credential, model, default_model, mut models } => {
                let credential = fresh_credential(&tasks, credential).await?;
                let client = ChatGptClient::new();
                if models.is_empty() {
                    models = client.models(&credential.access_token).await?;
                    let catalog = models.clone();
                    tasks.send(update(move |app: &mut App| apply_models(app, catalog)));
                }
                let chosen = if model.is_empty() { chatgpt::choose_model(&models, &default_model) } else { model };
                if !models.iter().any(|m| m.slug == chosen) {
                    return Err(AssistantError::Restricted);
                }
                let source = chatgpt::model_display_name(&models, &chosen);
                Ok((ProviderResponder::ChatGpt(ChatGptResponder { client, token: credential.access_token, model: chosen }), source))
            }
        }
    }
}

/// A usable credential, refreshed (and stored again) when it is about to expire.
async fn fresh_credential(tasks: &Tasks, credential: ChatGptCredential) -> Result<ChatGptCredential, AssistantError> {
    let (credential, changed) = ChatGptClient::new().current_credential(Some(&credential)).await?;
    if changed {
        let stored = credential.clone();
        tasks.send(update(move |app: &mut App| {
            let same = app.assistant.chatgpt.credential().is_some_and(|c| c.client_id == stored.client_id && c.subject == stored.subject);
            if same {
                store_credential(app, stored);
            }
        }));
    }
    Ok(credential)
}

fn apply_models(app: &mut App, models: Vec<ChatGptModel>) {
    let state = &mut app.assistant.chatgpt;
    state.models_loading = false;
    let chosen = chatgpt::choose_model(&models, &app.assistant.config.chatgpt_model);
    state.models = models;
    if state.models.is_empty() {
        state.error = Some(assistant_error_message(AssistantError::Restricted));
    }
    if chosen != app.assistant.config.chatgpt_model {
        app.assistant.config.chatgpt_model = chosen;
        save_config(app);
    }
}

// MARK: ChatGPT models and connection

/// Loads the models offered to the connected account.
pub fn load_models(app: &mut App) {
    let state = &mut app.assistant.chatgpt;
    let Some(credential) = state.credential().cloned() else { return };
    if state.models_loading {
        return;
    }
    state.models_loading = true;
    let generation = state.generation;
    let tasks = app.tasks.clone();
    app.tasks.spawn(async move {
        let result = async {
            let credential = fresh_credential(&tasks, credential).await?;
            ChatGptClient::new().models(&credential.access_token).await
        }
        .await;
        update(move |app: &mut App| {
            if app.assistant.chatgpt.generation != generation {
                app.assistant.chatgpt.models_loading = false;
                return;
            }
            match result {
                Ok(models) => apply_models(app, models),
                Err(error) => {
                    app.assistant.chatgpt.models_loading = false;
                    app.assistant.chatgpt.error = Some(assistant_error_message(error));
                }
            }
        })
    });
}

pub fn set_default_model(app: &mut App, model: &str) {
    app.assistant.config.chatgpt_model = model.to_string();
    save_config(app);
}

/// Stops a pending sign-in.
pub fn cancel_sign_in(app: &mut App) {
    let state = &mut app.assistant.chatgpt;
    state.generation += 1;
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(sign_in) = state.sign_in.take() {
        sign_in.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    state.sign_in = None;
    state.busy = false;
}

/// Forgets the credential on this computer, then revokes it remotely.
pub fn sign_out(app: &mut App) {
    if app.assistant.chatgpt.busy {
        return;
    }
    cancel_sign_in(app);
    let state = &mut app.assistant.chatgpt;
    let Some(vault) = state.vault.as_mut() else { return };
    let previous = vault.sign_out();
    state.models.clear();
    state.error = None;
    save_vault(app);
    app.assistant.config.chatgpt_model.clear();
    save_config(app);
    super::reply::on_chatgpt_changed(app);
    let Some(previous) = previous else { return };
    app.assistant.chatgpt.busy = true;
    let generation = app.assistant.chatgpt.generation;
    app.tasks.spawn(async move {
        let result = ChatGptClient::new().revoke(&previous).await;
        update(move |app: &mut App| {
            if app.assistant.chatgpt.generation != generation {
                return;
            }
            app.assistant.chatgpt.busy = false;
            if result.is_err() {
                app.assistant.chatgpt.error = Some(t(
                    "Signed out on this computer. Remote revocation could not be confirmed. Remove Discophile's access in ChatGPT settings.",
                ));
            }
        })
    });
}

/// Opens OpenAI's sign-in page and waits up to five minutes for the redirect to a loopback port.
/// `registration` reconnects a previously used ChatGPT account.
#[cfg(not(target_arch = "wasm32"))]
pub fn sign_in(app: &mut App, registration: Option<ChatGptRegistration>) {
    use discophile_core::assistant::chatgpt::{PkceRequest, oauth};
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    if app.assistant.chatgpt.busy {
        return;
    }
    let Some(host_id) = app.assistant.chatgpt.vault.as_ref().map(|v| v.host_id.clone()) else { return };
    cancel_sign_in(app);
    let state = &mut app.assistant.chatgpt;
    state.error = None;
    let pkce = match PkceRequest::generate() {
        Ok(pkce) => pkce,
        Err(error) => return state.error = Some(assistant_error_message(error)),
    };
    let listener = match std::net::TcpListener::bind(("127.0.0.1", 0)) {
        Ok(listener) => listener,
        Err(error) => {
            log::warn!("ChatGPT sign-in could not listen on the loopback interface: {error}");
            return state.error = Some(assistant_error_message(AssistantError::Unavailable));
        }
    };
    let Ok(port) = listener.local_addr().map(|a| a.port()) else {
        return state.error = Some(assistant_error_message(AssistantError::Unavailable));
    };
    let redirect = oauth::loopback_redirect(port);
    let saved_client = registration.as_ref().map(|r| r.client_id.clone());
    let expected_subject = registration.map(|r| r.subject);
    let url = oauth::authorization_url(&host_id, &redirect, &pkce.state, &pkce.nonce, &pkce.challenge, saved_client.as_deref());
    let cancel = Arc::new(AtomicBool::new(false));
    state.generation += 1;
    let generation = state.generation;
    state.busy = true;
    state.sign_in = Some(SignIn { url: url.to_string(), cancel: cancel.clone() });
    app.ctx().open_url(egui::OpenUrl::new_tab(url.as_str()));
    let tasks = app.tasks.clone();
    let spawned = std::thread::Builder::new().name("chatgpt-sign-in".into()).spawn(move || {
        let end = loopback::wait(listener, &redirect, &pkce.state, saved_client.as_deref(), &cancel);
        tasks.send(update(move |app: &mut App| on_redirect(app, generation, end, pkce, redirect, expected_subject)));
    });
    if spawned.is_err() {
        cancel_sign_in(app);
        app.assistant.chatgpt.error = Some(assistant_error_message(AssistantError::Unavailable));
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn on_redirect(
    app: &mut App,
    generation: u64,
    end: loopback::End,
    pkce: discophile_core::assistant::chatgpt::PkceRequest,
    redirect: url::Url,
    expected_subject: Option<String>,
) {
    let state = &mut app.assistant.chatgpt;
    if state.generation != generation {
        return;
    }
    state.sign_in = None;
    let (code, client_id) = match end {
        loopback::End::Code { code, client_id } => (code, client_id),
        loopback::End::Denied | loopback::End::Stopped => return state.busy = false,
        loopback::End::TimedOut => {
            state.busy = false;
            return state.error = Some(t("Sign-in timed out. Try again."));
        }
        loopback::End::Failed => {
            state.busy = false;
            return state.error = Some(assistant_error_message(AssistantError::Unavailable));
        }
    };
    app.tasks.spawn(async move {
        let result = ChatGptClient::new().exchange_code(&code, &client_id, &pkce, &redirect, expected_subject.as_deref()).await;
        update(move |app: &mut App| {
            if app.assistant.chatgpt.generation != generation {
                return;
            }
            app.assistant.chatgpt.busy = false;
            match result {
                Ok(credential) => {
                    store_credential(app, credential);
                    super::reply::on_chatgpt_changed(app);
                    load_models(app);
                }
                Err(AssistantError::Cancelled) => {}
                Err(error) => app.assistant.chatgpt.error = Some(assistant_error_message(error)),
            }
        })
    });
}

/// The loopback listener of a sign-in (`platform::loopback`), with ChatGPT's callback validation.
#[cfg(not(target_arch = "wasm32"))]
mod loopback {
    use std::net::TcpListener;
    use std::sync::atomic::AtomicBool;
    use std::time::Duration;

    use discophile_core::assistant::AssistantError;
    use discophile_core::assistant::chatgpt::oauth;
    use url::Url;

    use crate::platform::loopback::{self as shared, Wait};

    pub enum End {
        Code {
            code: String,
            client_id: String,
        },
        /// The person declined on OpenAI's page.
        Denied,
        TimedOut,
        Stopped,
        Failed,
    }

    pub fn wait(listener: TcpListener, redirect: &Url, state: &str, saved_client: Option<&str>, cancel: &AtomicBool) -> End {
        let timeout = Duration::from_secs(oauth::SIGN_IN_TIMEOUT_SECS);
        let accept = |url: &Url| match oauth::callback(url, redirect, state, saved_client) {
            Ok((code, client_id)) => Some(End::Code { code, client_id }),
            Err(AssistantError::Cancelled) => Some(End::Denied),
            Err(_) => None,
        };
        match shared::wait(listener, redirect, timeout, cancel, accept) {
            Wait::Done(end) => end,
            Wait::TimedOut => End::TimedOut,
            Wait::Stopped => End::Stopped,
            Wait::Failed => End::Failed,
        }
    }
}

// MARK: Local model

pub fn set_local_base_url(app: &mut App, url: &str) {
    app.assistant.config.local.base_url = url.trim().to_string();
    app.assistant.local.status = None;
    save_config(app);
}

pub fn set_local_model(app: &mut App, model: &str) {
    app.assistant.config.local.model = model.trim().to_string();
    save_config(app);
    super::reply::on_local_changed(app);
}

/// Saves the edited API key (an empty key removes it).
pub fn save_local_api_key(app: &mut App) {
    let key = app.assistant.local.api_key.trim().to_string();
    if key.is_empty() {
        secrets::delete(LOCAL_KEY_SECRET);
        app.assistant.config.local.api_key = None;
    } else {
        secrets::set(LOCAL_KEY_SECRET, &key);
        app.assistant.config.local.api_key = Some(key);
    }
    app.assistant.local.status = None;
}

pub fn has_local_api_key(app: &App) -> bool {
    app.assistant.config.local.api_key.is_some()
}

/// Test connection: lists the server's models and picks the first when none is chosen.
pub fn test_local(app: &mut App) {
    let client = match LocalModelClient::new(&app.assistant.config.local) {
        Ok(client) => client,
        Err(error) => return app.assistant.local.status = Some(Err(assistant_error_message(error))),
    };
    let state = &mut app.assistant.local;
    state.generation += 1;
    let generation = state.generation;
    state.loading = true;
    state.status = None;
    app.tasks.spawn(async move {
        let result = client.models().await;
        update(move |app: &mut App| {
            if app.assistant.local.generation != generation {
                return;
            }
            app.assistant.local.loading = false;
            match result {
                Ok(models) => {
                    app.assistant.local.status = Some(Ok(tf("Connected. Models available: %lld", &[&models.len()])));
                    if app.assistant.config.local.model.trim().is_empty()
                        && let Some(first) = models.first().cloned()
                    {
                        set_local_model(app, &first);
                    }
                    app.assistant.local.models = models;
                }
                Err(error) => app.assistant.local.status = Some(Err(assistant_error_message(error))),
            }
        })
    });
}
