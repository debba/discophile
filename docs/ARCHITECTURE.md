# Architecture

Discophile desktop/web is a Rust port of the iOS app in `../ios-app`. The look follows the iOS app
(palette, rounded surfaces, bold headings, pink accents) adapted to a desktop layout: a sidebar instead
of the tab bar, a persistent player bar instead of the mini player, a resizable right panel for the
queue, lyrics and the assistant, and modal windows instead of sheets.

## Crates

- `crates/core` (`discophile-core`): portable logic (providers, models, queue, discovery, lyrics,
  podcasts, audio effects DSP, assistant, Connect protocol). No IO besides HTTP.
- `crates/app` (`discophile`): egui/eframe interface and platform services, one codebase for desktop
  (glow renderer) and the web (WASM).

## App crate map

| Path | Purpose |
| --- | --- |
| `app.rs` | `App` struct (all state), per-frame loop, layout, alert and toast. |
| `tasks.rs` | `app.tasks.spawn(async move { …; update(move \|app\| …) })`: background work that returns a closure applied on the UI thread. |
| `platform/` | `spawn`, `sleep_ms`, `storage::{load, save, remove, remove_group}` (JSON documents; files on desktop, localStorage on web), `secrets::{get,set,delete}` (OS keychain), `media_controls`. |
| `session.rs` | Music accounts and the active `MusicClient` (`app.session.client`, `app.client()`), add/switch/rename/remove/verify. |
| `player.rs` | Shared player: `player::play_songs(app, songs, index, shuffled)`, `play_songs_with_context`, `toggle`, `next`, `previous`, `seek`, `set_volume`, `set_rate`, `cycle_repeat`, `toggle_shuffle`, `play_next`, `add_to_queue`, `play_radio`, `stop`, sleep timer helpers. `app.player.current()`, `.status`, `.queue`, `.mode`, `.context`. |
| `audio/` | `AudioOutput` trait and engines (native symphonia/cpal, web HTMLAudio + Web Audio). |
| `artwork.rs` | Texture cache. Use `ui::widgets::artwork(ui, app, seed, cover, size, radius)` for library covers or `artwork_url` for other URLs. |
| `nav.rs` | `Route`, `SettingsPage`, `SidePanel`; `app.nav.push(route)` for detail pages, `app.nav.select(route)` for sections, `app.nav.now_playing`, `app.nav.toggle_panel(..)`. |
| `settings.rs` | Shared preferences (`app.settings`, call `app.settings.save()` after changing). |
| `theme.rs` | `Palette::of_ui(ui)`, `theme::text(s, Font::Title2)`, `theme::secondary(ui, s, font)`, `theme::icon(icons::X, size)`, `theme::icon_filled`. |
| `icons.rs` | Phosphor glyph constants (`crate::theme::icons::PLAY` etc.). |
| `ui/widgets.rs` | `page`, `page_header`, `section_heading`, `card`, `clickable_card`, `action_button`, `pill_button`, `icon_button`, `round_play_button`, `chip`, `tile_grid`, `album_tile`, `track_row`, `artwork`, `empty_state`, `loading`, `error_banner`, `search_field`, `songs_summary`, `count_badge`. |
| `features/*.rs` | Feature state (a field on `App`) and hooks called by the shell and player. Cross-feature entry points: `library::{open_playlist_picker, open_playlist_editor}`, `downloads::{download, is_downloaded, download_button, song_menu}`, `discovery::{start_personal_radio, why_this_song, save_mix}`, `assistant::{open, ask, edit_playlist_with_assistant, edit_selection_with_assistant, unread_count}`, `audiobooks::{now_playing, seek, toggle}`. |
| `ui/*.rs` | Pages and panels called by the shell. |

## Patterns

- Immediate mode: pages read state and draw every frame. Start loads when data is missing (keep a
  `loading` flag to avoid duplicate requests) or when `app.nav.changed` is set.
- Async: clone what the task needs (`app.client()`), spawn, return `update(move |app| …)`. Compare
  `app.session.generation` captured before spawning with the current one to drop results that belong
  to a previous library.
- Errors: inline `widgets::error_banner` for page loads; `app.show_error(msg)` for actions;
  `app.toast(msg)` for confirmations ("Added: 3").
- Persist feature data with `platform::storage` under `library/<account_key>/<name>` for per-library
  data, so removing an account clears it.
- Strings: `t("English")`, `tf("Chapter %1$lld of %2$lld", &[&a, &b])`, `count("%lld songs", n)`.
  Reuse keys from `Localization/strings.json`; add new keys with Italian translations to your area's
  file in `Localization/`. Desktop wording replaces iPhone wording ("on this computer").
- Accessibility: icon buttons carry labels (tooltips + AccessKit names); keep keyboard access
  (Enter/double-click plays, Space toggles playback, Alt+←/→ navigates).
- Platform differences: use `#[cfg(target_arch = "wasm32")]` or `crate::platform::IS_WEB`. Downloads,
  keychain, file dialogs and the native audio engine exist only on desktop; the web build hides those
  controls and explains why where the iOS app offered them.

## Discophile Connect (planned)

`discophile_core::connect` defines the device handoff protocol (Spotify Connect style). Devices
exchange `ConnectMessage` JSON over a WebSocket after discovery via mDNS (`_discophile._tcp`).
A `PlaybackSnapshot` carries the queue, position and play state but never credentials; the target
must have the same library configured (`account_key`). Transfer: the active device sends `Transfer`,
the target starts playing at `position_at(now)`, replies `TransferComplete`, and the sender pauses.
