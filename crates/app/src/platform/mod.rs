//! Platform services with one API for desktop and the browser.

use std::future::Future;

pub mod media_controls;
pub mod secrets;
pub mod storage;
#[cfg(not(target_arch = "wasm32"))]
pub mod window;
#[cfg(not(target_arch = "wasm32"))]
pub mod tray;

#[cfg(not(target_arch = "wasm32"))]
pub trait MaybeSend: Send {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Send> MaybeSend for T {}
#[cfg(target_arch = "wasm32")]
pub trait MaybeSend {}
#[cfg(target_arch = "wasm32")]
impl<T> MaybeSend for T {}

pub const IS_WEB: bool = cfg!(target_arch = "wasm32");

#[cfg(not(target_arch = "wasm32"))]
pub fn runtime() -> &'static tokio::runtime::Runtime {
    use std::sync::OnceLock;
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(3)
            .thread_name("discophile-io")
            .enable_all()
            .build()
            .expect("tokio runtime")
    })
}

/// Runs a future in the background: a tokio worker on desktop, the browser event loop on the web.
pub fn spawn<F>(future: F)
where
    F: Future<Output = ()> + MaybeSend + 'static,
{
    #[cfg(not(target_arch = "wasm32"))]
    {
        runtime().spawn(future);
    }
    #[cfg(target_arch = "wasm32")]
    {
        wasm_bindgen_futures::spawn_local(future);
    }
}

/// Waits without blocking the interface.
pub async fn sleep_ms(ms: u64) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
    }
    #[cfg(target_arch = "wasm32")]
    {
        let promise = js_sys::Promise::new(&mut |resolve, _| {
            if let Some(window) = web_sys::window() {
                let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms.min(i32::MAX as u64) as i32);
            }
        });
        let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
    }
}

/// The preferred interface language from the operating system or browser.
pub fn system_language() -> discophile_core::l10n::Language {
    use discophile_core::l10n::Language;
    #[cfg(not(target_arch = "wasm32"))]
    {
        sys_locale::get_locale().map(|tag| Language::from_tag(&tag)).unwrap_or_default()
    }
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window().and_then(|w| w.navigator().language()).map(|tag| Language::from_tag(&tag)).unwrap_or_default()
    }
}

/// A human-readable name for this device, used by Discophile Connect and ChatGPT sign-in.
pub fn device_name() -> String {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::env::var("HOSTNAME")
            .ok()
            .or_else(|| std::fs::read_to_string("/etc/hostname").ok().map(|s| s.trim().to_string()))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Discophile Desktop".into())
    }
    #[cfg(target_arch = "wasm32")]
    {
        "Discophile Web".into()
    }
}
