# Discophile for desktop and the web

A player for your music libraries, audiobooks and internet radio, ported from the Discophile iPhone app
(`../ios-app`) to Rust. One codebase builds a native desktop app for Linux, Windows and macOS and a
standalone web player (WebAssembly). The interface follows the iOS app's design, adapted to the desktop.
The sidebar replaces the tab bar and a persistent player bar runs along the bottom. The queue, lyrics and
the assistant open in a resizable side panel, and sheets become windows. English is the source and
fallback language; Italian is included throughout.

There is no Discophile backend, tracking or analytics. Discophile talks only to the servers you configure
and to the optional public services described below.

## Features

- **Libraries and accounts**: Subsonic/OpenSubsonic, Navidrome and Jellyfin music libraries with named
  aliases, a library switcher, separate caches and downloads, connection verification and password or API
  key changes. Audiobookshelf connects independently. Internet radio stations need no account.
  Credentials live in the operating system keychain (Secret Service, Keychain, Credential Manager).
- **Listen Now**: discovery mixes (Worth another listen, Still to discover, Have 25 minutes?), resuming
  the saved queue, listening statistics, Ask your curator and Choose a record for tonight. Listening
  history stays on this computer, per library.
- **Library**: albums (sortable, paged), artists, songs, playlists (create, rename, edit, delete, add
  without duplicates), favorites and server podcasts. Podcasts can be added from Apple Podcasts links,
  Spotify show links or RSS feeds. Albums, artists, playlists and podcasts have detail pages. Search has
  filters and an on-request Apple Podcasts directory lookup. Metadata is cached for offline browsing.
- **Player**: queue with reordering, shuffle, repeat, sleep timer, personal radio (Stay with this style /
  Let me explore, with "Why this song?"), album focus, and a full Now Playing view tinted by the artwork.
  The operating system media controls work too: MPRIS on Linux, SMTC on Windows, Now Playing on macOS,
  and Media Session in browsers.
- **Lyrics**: server lyrics (OpenSubsonic structured, Subsonic legacy, Jellyfin) with synchronized
  highlighting, automatic scrolling and click-to-seek. Optional LRCLIB lookups, off by default.
- **Equalizer and sound**: 10-band equalizer and AutoEq headphone profiles, with ParametricEQ import.
  ReplayGain in Track, Album or Automatic mode with a preamp and clipping prevention. Volume limit with a
  peak limiter, gapless playback, crossfade (1 to 12 seconds) and Audio details.
- **Offline listening** (desktop): download songs, albums, playlists and podcasts, filter everything with
  Downloaded only, and keep automatic downloads of mixes and selected playlists within a space limit.
- **Audiobooks**: Audiobookshelf libraries, search, chapters, book and chapter timelines, skips, 0.5× to 4×
  speed, sleep timer including end of chapter, bookmarks, per-book repeat and complete-book downloads
  (desktop). Progress syncs with newer server positions taking precedence.
- **Music assistant**: a chat with history, song results, playlist proposals and confirmed edits to
  playlists and favorites. It can use ChatGPT (Sign in with ChatGPT, desktop only) or a local
  OpenAI-compatible model such as Ollama, which replaces Apple's on-device model.
- **About**: on request, from the menus of artists, albums, songs, podcasts and episodes, a side panel with
  an overview written by the music assistant and room for follow-up questions. It is grounded in your
  server's biographies and album notes, feed descriptions and, if you enable Online music information,
  MusicBrainz and Wikipedia (in your language, English as fallback). Sources are listed with links; without
  an assistant they are shown on their own.
- **Agents (MCP, desktop)**: an optional Model Context Protocol server on `http://127.0.0.1:47321/mcp`
  (Settings > Agents) lets agents such as Claude search the library, read music information, create and edit
  playlists, manage favorites and control playback. Requests need a bearer token kept in the keychain.
  Discophile asks you before an agent removes songs from a playlist or deletes one. Settings shows ready-made
  setups for Claude Code, Claude Desktop (via `mcp-remote`) and other Streamable HTTP clients, and the
  recent agent activity.
- **Settings**: language, appearance (system, light, dark), keep the computer awake while playing,
  streaming quality, scrobbling, an introduction and an About page with optional GitHub Sponsors support.

Differences from the iPhone app: CarPlay, Apple Watch, Sport mode, Siri and dictation are not available.
Desktop playback changes pitch with speed. HLS and Opus streams are not supported by the desktop engine,
and the browser plays only what it supports. The web player cannot download, cannot sign in to ChatGPT,
keeps credentials in the site's local storage, and needs CORS enabled on your servers.

On Linux, playback prefers PipeWire, then PulseAudio, so it follows the output selected in the desktop
sound settings. Plain ALSA is the fallback when neither sound server is available.

The desktop tray menu provides Open, Play/Pause, Previous, Next and Quit. In Settings > General > Window
behavior, enable closing to the tray to keep listening with the window hidden. If no tray is available,
the window is minimized instead so it can still be reopened.
On macOS, clicking the Dock icon reopens a window closed to the tray, and the menu bar offers Discophile,
File, Controls, Go, Window and Help menus (playback, sections with ⌘1 to ⌘0, panels, back and forward with
⌘[ and ⌘], Settings with ⌘,).
On Linux, X11/XWayland is preferred when available because the window backend cannot hide native Wayland
windows. Native Wayland keeps the tray controls and minimizes on close.

## Privacy

- **LRCLIB**: when online lyrics are enabled, opening lyrics sends the title, artist, album and duration
  to [LRCLIB](https://lrclib.net/docs).
- **Apple Podcasts**: directory lookups send only the search text or link.
- **AutoEq**: the index and profiles are downloaded from GitHub only when you browse headphones.
- **ChatGPT**: sends your request, bounded recent conversation and limited song metadata (title, artist,
  album, genre, year for up to 200 candidates under temporary IDs). Requests use `store: false`.
- **Local model**: stays on your machine apart from the address you configure.
- **Online music information** (off by default): About sends the name, title and artist of the item to
  MusicBrainz and Wikipedia/Wikidata. The assistant receives the item's public metadata and the source
  excerpts, never server identifiers.
- **Agents**: off by default and reachable only from this computer. A connected agent sees library metadata
  and IDs, and what it sends to its own model provider follows that agent's policy.

## Discophile Connect (planned)

`crates/core/src/connect.rs` defines the protocol for moving playback between devices, Spotify Connect
style. Devices find each other with mDNS (`_discophile._tcp`) and exchange JSON messages over a WebSocket.
A playback snapshot carries the queue, position and play state. It never carries credentials: the
receiving device must already have the same library. See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Development

Requirements: Rust 1.98 or later. On Linux, also the ALSA and D-Bus development packages. For the web
player you also need the `wasm32-unknown-unknown` target and [trunk](https://trunkrs.dev).

```sh
cargo run -p discophile                                  # desktop app
cargo test --workspace                                   # core and app tests
cargo clippy --workspace
cargo check -p discophile --target wasm32-unknown-unknown
python3 scripts/verify-localization.py                   # every interface key has an Italian translation
cargo run -p discophile --example play -- a.flac b.mp3   # audio engine smoke test (gapless, --crossfade 5)

cd crates/app && trunk serve                             # web player at http://127.0.0.1:8080
cd crates/app && trunk build --release                   # static site in dist/
```

`DISCOPHILE_DATA_DIR` and `DISCOPHILE_CACHE_DIR` override where settings, history, downloads and caches
are stored, which is useful for isolated testing against the synthetic servers in
`../ios-app/Tests/Fixtures`.

## Organization

- `crates/core`: portable logic, compiled for desktop and WebAssembly.
  - Clients for Subsonic, Navidrome, Jellyfin and Audiobookshelf.
  - Models, the queue and offline catalogs.
  - Discovery, statistics and personal radio.
  - Lyrics and LRCLIB, and the podcast directory.
  - Audio effects DSP, the assistant and the Connect protocol.
- `crates/app`: the egui/eframe interface and platform services.
  - Storage, keychain and media controls.
  - The native audio engine (symphonia, cpal, rubato) and the web audio backend.
  - Downloads.
- `Localization`: English keys with Italian translations, one file per area, plus plural forms.
- `docs/ARCHITECTURE.md`: structure, patterns and the Connect design.

Fonts: [Inter](https://rsms.me/inter/) (SIL Open Font License) and [Phosphor](https://phosphoricons.com)
icons (MIT), in `crates/app/assets/fonts`.
