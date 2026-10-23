//! Shared HTTP client construction. Authenticated API and artwork requests reject redirects.

use std::time::Duration;

/// A client for authenticated server requests: no redirects, bounded timeouts on native.
pub fn server_client() -> reqwest::Client {
    build(false, Duration::from_secs(30))
}

/// A client for unauthenticated public services (LRCLIB, iTunes, AutoEq, ChatGPT).
pub fn public_client() -> reqwest::Client {
    build(true, Duration::from_secs(30))
}

#[cfg(not(target_arch = "wasm32"))]
fn build(follow_redirects: bool, timeout: Duration) -> reqwest::Client {
    let policy = if follow_redirects { reqwest::redirect::Policy::limited(5) } else { reqwest::redirect::Policy::none() };
    reqwest::Client::builder()
        .redirect(policy)
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(timeout)
        .user_agent(concat!("Discophile/", env!("CARGO_PKG_VERSION")))
        .build()
        .unwrap_or_default()
}

#[cfg(target_arch = "wasm32")]
fn build(_follow_redirects: bool, _timeout: Duration) -> reqwest::Client {
    // Browsers own redirects, timeouts and the user agent.
    reqwest::Client::new()
}

/// Appends query parameters to a URL, percent-encoding values.
pub fn with_query(base: &url::Url, path: &str, params: &[(&str, String)]) -> url::Url {
    let mut url = base.clone();
    let joined = format!("{}/{}", url.path().trim_end_matches('/'), path.trim_start_matches('/'));
    url.set_path(&joined);
    {
        let mut pairs = url.query_pairs_mut();
        for (k, v) in params {
            pairs.append_pair(k, v);
        }
    }
    if url.query() == Some("") {
        url.set_query(None);
    }
    url
}
