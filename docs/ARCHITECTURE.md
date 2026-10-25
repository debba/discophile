# Architecture

Discophile desktop/web is a Rust port of the iOS app in `../ios-app`. The look follows the iOS app
(palette, rounded surfaces, bold headings, pink accents) adapted to a desktop layout: a sidebar instead
of the tab bar, a persistent player bar instead of the mini player, a resizable right panel for the
queue, lyrics and the assistant, and modal windows instead of sheets.

## Crates

- `crates/core` (`discophile-core`): portable logic (providers, models, queue, discovery, lyrics,
  podcasts, audio effects DSP, assistant, music information (`music_info`), MCP protocol (`mcp`), Connect
  protocol). No IO besides HTTP.
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

## Layout and accessibility rules

- **Spacing**: use `widgets::space::{XS, S, M, L, XL, XXL}` (4–32) and the control heights `CONTROL_SMALL` (30),
  `CONTROL` (36) and `CONTROL_LARGE` (44). Pages get `PAGE_MARGIN` through `widgets::page`.
- **Dialogs**: open them with `widgets::modal(ctx, id)`, never `egui::Modal::new`. Its frame has sheet padding
  (24) and corners (18); egui's default popup frame uses menu margins.
- **Icons**: paint glyphs with `widgets::paint_icon`, which centers the drawn shape, not the font box.
  Phosphor glyphs are not centered in their advance width.
- **Text fields**: single-line `TextEdit`s use `.align(egui::Align2::LEFT_CENTER)`. Border, padding and focus
  outline come from `theme::FieldStyle`, registered as the `TextEdit` style provider. Use
  `widgets::search_field` for search.
- **Keyboard**: every custom clickable control calls `widgets::focus_ring`. The ring shows only while
  navigating with Tab. Tiles made of artwork plus text are a single stop: build them with
  `widgets::tile_target`, which also sets the accessible name. Artwork itself is pointer-clickable but not
  focusable.
- **Screen readers**: give every icon-only control a label (`icon_button` does: tooltip and AccessKit name).
- **Contrast**: text and accents meet WCAG AA in both appearances. Field borders (`Palette::field_border`)
  and separators are opaque colors with at least 3:1 against their surface.
- **Scaling**: Settings > General > Text size and Ctrl + / Ctrl − change `Settings::interface_scale`
  (egui zoom). Layouts must reflow at 150%.

## About and agents

- `features/info.rs` + `ui/info.rs`: the About side panel (`SidePanel::Info`). `info::about_song`,
  `about_album`, `about_artist`, `about_podcast` open it from any menu. Sources come from
  `music_info::MusicInfoClient::gather`; overviews and follow-ups from `music_info::describe` with the
  assistant's default provider.
- `features/mcp.rs` + `platform/http_server.rs` + `ui/agents.rs` (desktop only): the MCP server. Library tools
  run on the tokio runtime with a clone of the active `MusicClient`; player tools and confirmations are sent
  to the interface thread with `Tasks::send` and answered through a oneshot channel. `ToolCall::is_destructive`
  calls wait for the dialog drawn by `ui::agents::confirmation`.

## Discophile Connect

`discophile_core::connect` implements the client side of the Connect protocol version 1: `instance`
(canonical origins, `https` only except the loopback `http` exception), `discovery` (`ServerInfo`, bound to the
origin), `authorization` (PKCE and the `/connect/callback` loopback redirect), `credential` (tokens with
redacted `Debug`, the origin and account namespace), `account` (plan and entitlements) and `client`
(`ConnectClient`, no redirects, bounded bodies). The shared loopback parsing is in `discophile_core::oauth`.
`features/connect.rs` keeps the chosen instance in storage and the credential in secrets; refreshes run one at
a time and a rotated credential is stored before the account is read with it. `ui/connect.rs` is
Settings > Discophile Connect; see [CONNECT.md](CONNECT.md).

`connect::handoff` keeps the draft playback handoff messages (`ConnectMessage`, `PlaybackSnapshot`). Their
transport is not implemented; it will follow the protocol's future `handoff` capability. A snapshot carries
the queue, position and play state but never credentials; the target must have the same library configured
(`account_key`).
