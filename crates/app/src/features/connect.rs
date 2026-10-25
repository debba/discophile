//! Discophile Connect: the optional account on a Connect instance (protocol version 1). The official
//! instance is the default; the person can choose another address and return to the official one.
//! Changing instance signs out (revoking on the old instance) and nothing is sent to the new one.
//!
//! The chosen instance and its last discovery document are a storage document. The credential (tokens,
//! origin, instance and account identifiers) is one JSON entry in `platform::secrets`. Refresh tokens
//! rotate, so one operation runs at a time (`busy`) and a rotated credential is stored on the interface
//! thread before the account is read with it. Results of operations that were cancelled, signed out or
//! replaced by a newer one (`generation`) are ignored, and tokens they obtained are revoked.
//!
//! The web build cannot receive the sign-in redirect (browsers cannot listen on a loopback port), so it
//! only shows the instance and explains that signing in needs Discophile for computers.

use std::future::Future;

use discophile_core::connect::client::check_binding;
use discophile_core::connect::{
    AccountInfo, ConnectClient, ConnectCredential, ConnectError, InstanceOrigin, InstanceUrlError, ServerInfo,
};
use discophile_core::l10n::{t, tf};
use serde::{Deserialize, Serialize};

use crate::app::App;
use crate::platform::{self, MaybeSend, secrets, storage};
use crate::tasks::update;

const CONFIG_KEY: &str = "connect";
const CREDENTIAL_SECRET: &str = "connect";

/// The persisted instance choice.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct ConnectConfig {
    /// `None` for the official instance.
    instance: Option<InstanceOrigin>,
    /// The last validated discovery document of the instance.
    server_info: Option<ServerInfo>,
}

#[derive(Default)]
pub struct ConnectState {
    pub origin: InstanceOrigin,
    /// The last validated discovery document of `origin`.
    pub server_info: Option<ServerInfo>,
    pub credential: Option<ConnectCredential>,
    /// The account read with the credential during this session.
    pub account: Option<AccountInfo>,
    /// Text of the custom address field.
    pub address: String,
    pub checking_instance: bool,
    pub instance_error: Option<String>,
    /// Signing in or reading the account.
    pub busy: bool,
    pub sign_in: Option<SignIn>,
    pub error: Option<String>,
    /// What happened after signing out or changing instance, such as an unconfirmed revocation.
    pub notice: Option<String>,
    generation: u64,
    instance_generation: u64,
}

/// A pending browser sign-in.
pub struct SignIn {
    /// The authorization page once discovery succeeded, offered again in case the browser did not open.
    pub url: Option<String>,
    #[cfg(not(target_arch = "wasm32"))]
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

pub(super) fn start(app: &mut App) {
    let config: ConnectConfig = storage::load(CONFIG_KEY).unwrap_or_default();
    let state = &mut app.connect;
    state.origin = config.instance.unwrap_or_default();
    state.server_info = config.server_info.filter(|info| info.origin == state.origin && info.is_consistent());
    if platform::IS_WEB {
        return;
    }
    let Some(text) = secrets::get(CREDENTIAL_SECRET) else { return };
    match serde_json::from_str::<ConnectCredential>(&text) {
        Ok(credential) if credential.origin == app.connect.origin => {
            app.connect.credential = Some(credential);
            refresh_account(app);
        }
        // Tokens of another instance are never used here; revoke them where they came from.
        Ok(credential) => {
            secrets::delete(CREDENTIAL_SECRET);
            revoke_in_background(app, credential, None, false);
        }
        Err(_) => {
            app.connect.error = Some(t(
                "Your Discophile Connect sign-in could not be read. Check that your system keychain is unlocked, or sign in again.",
            ))
        }
    }
}

fn save_config(app: &App) {
    let state = &app.connect;
    let config = ConnectConfig {
        instance: (!state.origin.is_official()).then(|| state.origin.clone()),
        server_info: state.server_info.clone(),
    };
    storage::save(CONFIG_KEY, &config);
}

fn remember_server_info(app: &mut App, info: ServerInfo) {
    if info.origin == app.connect.origin && app.connect.server_info.as_ref() != Some(&info) {
        app.connect.server_info = Some(info);
        save_config(app);
    }
}

fn store_credential(app: &mut App, credential: ConnectCredential) {
    match serde_json::to_string(&credential) {
        Ok(text) => secrets::set(CREDENTIAL_SECRET, &text),
        Err(error) => log::error!("Unable to encode the Discophile Connect credential: {error}"),
    }
    app.connect.credential = Some(credential);
}

/// Forgets the credential and account on this computer and returns the credential for revocation.
fn clear_credential(app: &mut App) -> Option<ConnectCredential> {
    app.connect.account = None;
    let previous = app.connect.credential.take();
    if previous.is_some() {
        secrets::delete(CREDENTIAL_SECRET);
    }
    previous
}

/// Stops the pending sign-in or account operation; its results will be ignored.
fn stop_operations(app: &mut App) {
    let state = &mut app.connect;
    state.generation += 1;
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(sign_in) = &state.sign_in {
        sign_in.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    state.sign_in = None;
    state.busy = false;
}

pub fn is_signed_in(app: &App) -> bool {
    app.connect.credential.is_some()
}

// MARK: Instance

/// Checks the address in the custom address field and switches to it when it is a compatible instance.
pub fn apply_instance(app: &mut App) {
    let origin = match InstanceOrigin::parse(&app.connect.address) {
        Ok(origin) => origin,
        Err(error) => return app.connect.instance_error = Some(instance_error_message(error)),
    };
    let state = &mut app.connect;
    state.instance_error = None;
    if origin == state.origin {
        state.address.clear();
        return;
    }
    state.instance_generation += 1;
    let generation = state.instance_generation;
    state.checking_instance = true;
    app.tasks.spawn(async move {
        let result = ConnectClient::new().server_info(&origin).await.and_then(|info| info.check_desktop_sign_in().map(|_| info));
        update(move |app: &mut App| {
            if app.connect.instance_generation != generation {
                return;
            }
            app.connect.checking_instance = false;
            match result {
                Ok(info) => switch_instance(app, origin, Some(info)),
                Err(error) => app.connect.instance_error = Some(error_message(&error)),
            }
        })
    });
}

/// Returns to the official instance.
pub fn use_official_instance(app: &mut App) {
    if !app.connect.origin.is_official() {
        switch_instance(app, InstanceOrigin::official(), None);
    }
}

/// Signs out of the current instance (revoking there) and selects `origin`. Nothing is sent to the new one.
fn switch_instance(app: &mut App, origin: InstanceOrigin, info: Option<ServerInfo>) {
    stop_operations(app);
    let previous = clear_credential(app);
    let state = &mut app.connect;
    state.instance_generation += 1;
    state.checking_instance = false;
    state.instance_error = None;
    state.error = None;
    state.notice = None;
    state.address.clear();
    let previous_info = state.server_info.take();
    state.origin = origin;
    state.server_info = info;
    save_config(app);
    if let Some(credential) = previous {
        revoke_in_background(app, credential, previous_info, true);
    }
}

// MARK: Sign-in and sign-out

/// Stops a pending sign-in.
pub fn cancel_sign_in(app: &mut App) {
    stop_operations(app);
}

/// Reads the instance's discovery document, then opens its sign-in page and waits up to five minutes for
/// the redirect to a loopback port.
#[cfg(not(target_arch = "wasm32"))]
pub fn sign_in(app: &mut App) {
    if app.connect.busy {
        return;
    }
    stop_operations(app);
    let state = &mut app.connect;
    state.error = None;
    state.notice = None;
    state.busy = true;
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    state.sign_in = Some(SignIn { url: None, cancel });
    let generation = state.generation;
    let origin = state.origin.clone();
    app.tasks.spawn(async move {
        let result = ConnectClient::new().server_info(&origin).await.and_then(|info| info.check_desktop_sign_in().map(|_| info));
        update(move |app: &mut App| open_sign_in(app, generation, result))
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn open_sign_in(app: &mut App, generation: u64, result: Result<ServerInfo, ConnectError>) {
    use discophile_core::connect::AuthorizationRequest;
    use discophile_core::connect::authorization::SIGN_IN_TIMEOUT_SECS;

    use crate::platform::loopback;

    if app.connect.generation != generation {
        return;
    }
    let fail = |app: &mut App, error: ConnectError| {
        stop_operations(app);
        app.connect.error = Some(error_message(&error));
    };
    let info = match result {
        Ok(info) if info.origin == app.connect.origin => info,
        Ok(_) => return fail(app, ConnectError::OriginMismatch),
        Err(error) => return fail(app, error),
    };
    remember_server_info(app, info.clone());
    let listener = match std::net::TcpListener::bind(("127.0.0.1", 0)) {
        Ok(listener) => listener,
        Err(error) => {
            log::warn!("Discophile Connect sign-in could not listen on the loopback interface: {error}");
            return fail(app, ConnectError::Unavailable);
        }
    };
    let request = match listener.local_addr().map_err(|_| ConnectError::Unavailable).and_then(|a| AuthorizationRequest::new(&info, a.port())) {
        Ok(request) => request,
        Err(error) => return fail(app, error),
    };
    let Some(sign_in) = app.connect.sign_in.as_mut() else { return };
    sign_in.url = Some(request.url.to_string());
    let cancel = sign_in.cancel.clone();
    app.ctx().open_url(egui::OpenUrl::new_tab(request.url.as_str()));
    let tasks = app.tasks.clone();
    let spawned = std::thread::Builder::new().name("connect-sign-in".into()).spawn(move || {
        let timeout = std::time::Duration::from_secs(SIGN_IN_TIMEOUT_SECS);
        let end = loopback::wait(listener, &request.redirect, timeout, &cancel, |url| match request.callback(url) {
            Err(ConnectError::InvalidCallback) => None,
            result => Some(result),
        });
        tasks.send(update(move |app: &mut App| on_redirect(app, generation, end, info, request)));
    });
    if spawned.is_err() {
        fail(app, ConnectError::Unavailable);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn on_redirect(
    app: &mut App,
    generation: u64,
    end: crate::platform::loopback::Wait<Result<String, ConnectError>>,
    info: ServerInfo,
    request: discophile_core::connect::AuthorizationRequest,
) {
    use discophile_core::connect::discovery::DESKTOP_CLIENT_ID;

    use crate::platform::loopback::Wait;

    if app.connect.generation != generation {
        return;
    }
    app.connect.sign_in = None;
    let code = match end {
        Wait::Done(Ok(code)) => code,
        Wait::Done(Err(ConnectError::Cancelled)) | Wait::Stopped => return app.connect.busy = false,
        Wait::Done(Err(error)) => {
            app.connect.busy = false;
            return app.connect.error = Some(error_message(&error));
        }
        Wait::TimedOut => {
            app.connect.busy = false;
            return app.connect.error = Some(t("Sign-in timed out. Try again."));
        }
        Wait::Failed => {
            app.connect.busy = false;
            return app.connect.error = Some(error_message(&ConnectError::Unavailable));
        }
    };
    app.tasks.spawn(async move {
        let client = ConnectClient::new();
        let result = async {
            let tokens = client.exchange_code(&info, &request, &code).await?;
            match client.account(&info, &tokens.access_token).await {
                Ok(account) => {
                    let credential = ConnectCredential::new(
                        info.origin.clone(),
                        info.instance_id.clone(),
                        account.account.id.clone(),
                        DESKTOP_CLIENT_ID.to_string(),
                        tokens,
                    );
                    Ok((credential, account))
                }
                // Without a readable account there is no namespace to keep the tokens in.
                Err(error) => {
                    let _ = client.revoke_token(&info, &tokens.refresh_token, DESKTOP_CLIENT_ID).await;
                    Err(error)
                }
            }
        }
        .await;
        update(move |app: &mut App| {
            if app.connect.generation != generation {
                if let Ok((credential, _)) = result {
                    revoke_in_background(app, credential, Some(info), false);
                }
                return;
            }
            app.connect.busy = false;
            match result {
                Ok((credential, account)) => {
                    store_credential(app, credential);
                    app.connect.account = Some(account);
                }
                Err(ConnectError::Cancelled) => {}
                // The account named another instance: an inconsistent instance, not a change since sign-in.
                Err(ConnectError::InstanceMismatch) => app.connect.error = Some(error_message(&ConnectError::InvalidResponse)),
                Err(error) => app.connect.error = Some(error_message(&error)),
            }
        })
    });
}

/// Forgets the credential on this computer, then revokes it on its instance.
pub fn sign_out(app: &mut App) {
    stop_operations(app);
    app.connect.error = None;
    app.connect.notice = None;
    let Some(credential) = clear_credential(app) else { return };
    let cached = app.connect.server_info.clone();
    revoke_in_background(app, credential, cached, true);
}

/// Revokes a credential on the instance that issued it, best effort. Fresh discovery is preferred; the
/// cached document is used when the instance cannot describe itself. With `report`, a failure is shown.
fn revoke_in_background(app: &mut App, credential: ConnectCredential, cached: Option<ServerInfo>, report: bool) {
    let generation = app.connect.generation;
    app.tasks.spawn(async move {
        let client = ConnectClient::new();
        let result = async {
            let info = match client.server_info(&credential.origin).await {
                Ok(info) => info,
                Err(error) => cached.filter(|info| info.origin == credential.origin).ok_or(error)?,
            };
            client.revoke(&info, &credential).await
        }
        .await;
        update(move |app: &mut App| {
            let Err(error) = result else { return };
            log::warn!("Discophile Connect revocation on {} failed: {error}", credential.origin);
            if report && app.connect.generation == generation {
                app.connect.notice = Some(tf(
                    "Signed out on this computer. Revocation on %@ could not be confirmed. You can remove Discophile's access from your account page on that instance.",
                    &[&credential.origin],
                ));
            }
        })
    });
}

// MARK: Account

/// One step of reading the account.
enum Step {
    /// The tokens were rotated: store them, then read the account with them.
    Rotated { info: ServerInfo, credential: ConnectCredential },
    /// The access token was refused before it expired: rotate once, then read the account again.
    Refused { info: ServerInfo },
    Account { info: ServerInfo, account: AccountInfo },
}

fn spawn_step<F>(app: &mut App, generation: u64, retried: bool, future: F)
where
    F: Future<Output = Result<Step, ConnectError>> + MaybeSend + 'static,
{
    app.tasks.spawn(async move {
        let result = future.await;
        update(move |app: &mut App| on_step(app, generation, retried, result))
    });
}

/// Reads the discovery document and the account, refreshing the tokens first when they expire within a
/// minute. Does nothing while another operation runs.
pub fn refresh_account(app: &mut App) {
    let state = &mut app.connect;
    let Some(credential) = state.credential.clone() else { return };
    if state.busy {
        return;
    }
    state.busy = true;
    state.error = None;
    let generation = state.generation;
    spawn_step(app, generation, false, async move {
        let client = ConnectClient::new();
        let info = client.server_info(&credential.origin).await?;
        check_binding(&info, &credential)?;
        if credential.needs_refresh(discophile_core::now_millis()) {
            let credential = client.refresh(&info, &credential).await?;
            return Ok(Step::Rotated { info, credential });
        }
        match client.account_for(&info, &credential).await {
            Ok(account) => Ok(Step::Account { info, account }),
            Err(ConnectError::Unauthorized) => Ok(Step::Refused { info }),
            Err(error) => Err(error),
        }
    });
}

fn on_step(app: &mut App, generation: u64, retried: bool, result: Result<Step, ConnectError>) {
    if app.connect.generation != generation {
        // Signed out or changed instance meanwhile: rotated tokens must not stay valid.
        if let Ok(Step::Rotated { info, credential }) = result {
            revoke_in_background(app, credential, Some(info), false);
        }
        return;
    }
    let Some(current) = app.connect.credential.clone() else {
        app.connect.busy = false;
        return;
    };
    match result {
        Ok(Step::Rotated { info, credential }) => {
            remember_server_info(app, info.clone());
            store_credential(app, credential.clone());
            spawn_step(app, generation, true, async move {
                match ConnectClient::new().account_for(&info, &credential).await {
                    Ok(account) => Ok(Step::Account { info, account }),
                    Err(error) => Err(error),
                }
            });
        }
        Ok(Step::Refused { info }) if !retried => {
            spawn_step(app, generation, true, async move {
                let credential = ConnectClient::new().refresh(&info, &current).await?;
                Ok(Step::Rotated { info, credential })
            });
        }
        Ok(Step::Refused { info }) => {
            // Refused even after a rotation: the delegation is unusable, so end it.
            let previous = clear_credential(app);
            app.connect.busy = false;
            app.connect.error = Some(error_message(&ConnectError::Unauthorized));
            if let Some(credential) = previous {
                revoke_in_background(app, credential, Some(info), false);
            }
        }
        Ok(Step::Account { info, account }) => {
            remember_server_info(app, info);
            app.connect.busy = false;
            app.connect.account = Some(account);
        }
        // Refused after a rotation, or the instance identifier changed at the same origin (a different
        // instance): delete the tokens, revoke them there best effort, and ask to sign in again.
        Err(error @ (ConnectError::Unauthorized | ConnectError::InstanceMismatch)) => {
            let previous = clear_credential(app);
            app.connect.busy = false;
            app.connect.error = Some(error_message(&error));
            if let Some(credential) = previous {
                revoke_in_background(app, credential, app.connect.server_info.clone(), false);
            }
        }
        Err(error) if error.ends_delegation() => {
            clear_credential(app);
            app.connect.busy = false;
            app.connect.error = Some(error_message(&error));
        }
        // Network errors, 5xx and the rest keep the tokens.
        Err(error) => {
            app.connect.busy = false;
            app.connect.error = Some(error_message(&error));
        }
    }
}

// MARK: Messages

pub fn instance_error_message(error: InstanceUrlError) -> String {
    t(match error {
        InstanceUrlError::Empty => "Enter the address of a Discophile Connect instance.",
        InstanceUrlError::Invalid => "Enter a complete address, such as https://connect.example.org.",
        InstanceUrlError::Insecure => "Use an HTTPS address. HTTP is allowed only for localhost, 127.0.0.1 and [::1].",
        InstanceUrlError::Credentials => "Remove the user name and password from the address.",
        InstanceUrlError::NotOrigin => "Enter only the instance's address, without a path, query or fragment.",
    })
}

pub fn error_message(error: &ConnectError) -> String {
    use ConnectError::*;
    t(match error {
        Address(error) => return instance_error_message(*error),
        Network => "Unable to reach the Connect instance. Check your connection and the address, then try again.",
        Redirected => "The instance tried to redirect Discophile to another address. For your security, the redirect was not followed.",
        InvalidResponse | InvalidCallback => {
            "The instance returned an unexpected response. Check that the address belongs to a Discophile Connect instance."
        }
        Unsupported => "This address does not offer a version of Discophile Connect supported by this app.",
        OriginMismatch => "The instance identified itself with a different address, so Discophile did not use it.",
        InstanceMismatch => "This instance's identity changed since you signed in, so you were signed out. Sign in again.",
        NotOffered => "This instance does not offer accounts for Discophile for computers.",
        SignInRequired | Unauthorized | RefreshNotRotated => "Your Discophile Connect session has ended. Sign in again.",
        InsufficientScope => "Discophile is not allowed to read your account. Sign in again and allow access.",
        RateLimited { .. } => "The instance received too many requests. Try again in a few minutes.",
        ServerUnavailable(_) => "The instance is temporarily unavailable. Try again later.",
        Rejected(_) => "The instance refused the sign-in request. Try again, or check that it supports Discophile for computers.",
        Cancelled => "Sign-in was cancelled.",
        IssuerMismatch => "The sign-in answer did not come from this instance, so it was ignored. Try again.",
        Unavailable => "Sign-in could not start on this computer. Try again.",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use discophile_core::l10n::has_translation;

    #[test]
    fn every_message_has_a_translation() {
        let errors = [
            ConnectError::Network,
            ConnectError::Redirected,
            ConnectError::InvalidResponse,
            ConnectError::Unsupported,
            ConnectError::OriginMismatch,
            ConnectError::InstanceMismatch,
            ConnectError::NotOffered,
            ConnectError::SignInRequired,
            ConnectError::RefreshNotRotated,
            ConnectError::Unauthorized,
            ConnectError::InsufficientScope,
            ConnectError::RateLimited { retry_after: None },
            ConnectError::ServerUnavailable(503),
            ConnectError::Rejected("invalid_client".into()),
            ConnectError::Cancelled,
            ConnectError::IssuerMismatch,
            ConnectError::InvalidCallback,
            ConnectError::Unavailable,
            ConnectError::Address(InstanceUrlError::Insecure),
        ];
        let addresses = [
            InstanceUrlError::Empty,
            InstanceUrlError::Invalid,
            InstanceUrlError::Insecure,
            InstanceUrlError::Credentials,
            InstanceUrlError::NotOrigin,
        ];
        // English is the source language, so each message is its own key.
        let messages = errors.iter().map(error_message).chain(addresses.into_iter().map(instance_error_message));
        for message in messages {
            assert!(has_translation(&message), "{message}");
        }
    }
}
