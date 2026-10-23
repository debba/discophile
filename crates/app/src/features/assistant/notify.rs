//! "Reply ready" notifications for chats that are not on screen: a system notification on desktop,
//! the browser's Notification API on the web (permission is requested the first time).

/// Posts a notification for a finished reply. `body` is the reply text, shortened here.
pub fn reply_ready(chat_title: &str, body: &str) {
    let title = format!("{} · {chat_title}", discophile_core::l10n::t("Music assistant"));
    let mut body: String = body.chars().take(180).collect();
    if body.trim().is_empty() {
        body = discophile_core::l10n::t("Your reply is ready.");
    }
    post(title, body);
}

#[cfg(not(target_arch = "wasm32"))]
fn post(title: String, body: String) {
    // Showing a notification can block on the session bus, so it never runs on the interface thread.
    let _ = std::thread::Builder::new().name("assistant-notification".into()).spawn(move || {
        if let Err(error) = notify_rust::Notification::new().appname("Discophile").summary(&title).body(&body).show() {
            log::warn!("Unable to show the assistant notification: {error}");
        }
    });
}

#[cfg(target_arch = "wasm32")]
fn post(title: String, body: String) {
    use web_sys::{Notification, NotificationOptions, NotificationPermission};
    match Notification::permission() {
        NotificationPermission::Granted => {
            let options = NotificationOptions::new();
            options.set_body(&body);
            let _ = Notification::new_with_options(&title, &options);
        }
        NotificationPermission::Default => {
            // The next reply can notify once the person allows it.
            let _ = Notification::request_permission();
        }
        _ => {}
    }
}
