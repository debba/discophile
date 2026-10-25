# Repository Guidelines

Discophile desktop and web: a Rust port of `../ios-app` (SwiftUI). English is the source language; Italian is included.

## Layout

- `crates/core` (`discophile-core`): portable logic. Must compile for native and `wasm32-unknown-unknown`.
  No tokio, threads, `std::fs`, `std::time::{Instant, SystemTime}::now` (use `discophile_core::now_millis()` or chrono).
  HTTP goes through `reqwest` built by `http::server_client()` (no redirects) or `http::public_client()`.
  No file IO: expose serde types plus pure functions; the app crate persists them.
- `crates/app` (`discophile`): egui/eframe interface for desktop (glow) and web (WASM via trunk), platform services
  (storage, secrets, audio output, media keys, downloads).
- `site`: the discophile.app website (Next.js on Vercel, its own `package.json`); see `site/README.md`.
- `Localization/*.json`: `{"English key": "Italian text"}`. `strings.json` comes from the iOS app; add new keys to
  the file owned by your area (`core`, `desktop`, `library`, `player`, `settings`, `assistant`, `audiobooks`,
  `discovery`, `connect`). Plural forms go in `plurals.json`. Use `t("English text")`, `tf(key, &[&arg])`, `count(key, n)`.

## Commands

```sh
cargo test -p discophile-core
cargo check -p discophile-core --target wasm32-unknown-unknown
cargo run -p discophile                      # desktop
cargo check -p discophile --target wasm32-unknown-unknown
trunk serve crates/app/index.html            # web player
```

Use `CARGO_BUILD_JOBS=4` when several builds may run at once; this machine has limited free memory.

## Style

Follow the Swift original's behavior and limits (bounds, timeouts, cache sizes) unless a desktop equivalent is
documented. Keep comments sparse and explanatory. Port the matching Swift tests from
`../ios-app/Tests/DiscophileCoreTests` as Rust unit tests. Never embed real credentials.
