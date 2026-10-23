//! Discophile for desktop and the web.

pub mod app;
pub mod artwork;
pub mod audio;
pub mod features;
pub mod icons;
pub mod nav;
pub mod platform;
pub mod player;
pub mod session;
pub mod settings;
pub mod tasks;
pub mod theme;
pub mod ui;

pub use app::App;
#[cfg(target_arch = "wasm32")]
pub use web_entry::start_web;

#[cfg(target_arch = "wasm32")]
mod web_entry {
    use wasm_bindgen::JsCast;

    /// Starts the web player on `<canvas id="discophile">`.
    pub fn start_web() {
        console_error_panic_hook::set_once();
        let _ = log::set_max_level(log::LevelFilter::Info);
        wasm_bindgen_futures::spawn_local(async {
            let Some(document) = web_sys::window().and_then(|w| w.document()) else { return };
            let Some(canvas) = document.get_element_by_id("discophile").and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok())
            else {
                log::error!("Missing <canvas id=\"discophile\">");
                return;
            };
            let result = eframe::WebRunner::new()
                .start(canvas, eframe::WebOptions::default(), Box::new(|cc| Ok(Box::new(crate::App::new(cc)))))
                .await;
            if let Some(loading) = document.get_element_by_id("loading") {
                loading.remove();
            }
            if let Err(error) = result {
                log::error!("Discophile failed to start: {error:?}");
            }
        });
    }
}
