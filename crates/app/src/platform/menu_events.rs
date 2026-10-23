//! One handler for native menu events (the tray menu and the macOS menu bar): `muda` accepts a single global
//! handler, so item IDs are queued here and dispatched by the app once per frame.

use std::sync::mpsc::{self, Receiver};
use std::sync::{Mutex, OnceLock};

use tray_icon::menu::MenuEvent;

static EVENTS: OnceLock<Mutex<Receiver<String>>> = OnceLock::new();

/// Routes menu events to [`drain`] and wakes the interface. Later calls do nothing.
pub fn install(ctx: &egui::Context) {
    if EVENTS.get().is_some() {
        return;
    }
    let (send, receive) = mpsc::channel();
    if EVENTS.set(Mutex::new(receive)).is_err() {
        return;
    }
    let ctx = ctx.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let _ = send.send(event.id.as_ref().to_string());
        ctx.request_repaint();
    }));
}

/// IDs of the menu items chosen since the last call.
pub fn drain() -> Vec<String> {
    EVENTS.get().and_then(|events| events.lock().ok().map(|r| r.try_iter().collect())).unwrap_or_default()
}
