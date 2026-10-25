# Developing Discophile

One Rust codebase builds the native desktop app (Linux, Windows, macOS) and the web player (WebAssembly).
The interface follows the Discophile iPhone app's design, adapted to the desktop: a sidebar, a persistent player
bar, a resizable side panel for the queue, lyrics and assistant, and windows instead of sheets. English is the
source language; Italian is included throughout.

## Commands

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

`DISCOPHILE_DATA_DIR` and `DISCOPHILE_CACHE_DIR` override where settings, history, downloads and caches are
stored, which is useful for isolated testing against the synthetic servers in `../ios-app/Tests/Fixtures`.

Debug builds keep credentials in `secrets-local.json` (readable only by you) in the data directory instead of the
keychain, because macOS asks for keychain access again after every rebuild. Release builds store all credentials
in a single keychain item. `DISCOPHILE_SECRETS=keychain` or `DISCOPHILE_SECRETS=file` overrides the choice.

## Layout

- `crates/core` (`discophile-core`): portable logic, compiled for desktop and WebAssembly.
  - Clients for Subsonic, Navidrome, Jellyfin and Audiobookshelf.
  - Models, the queue and offline catalogs.
  - Discovery, statistics and personal radio.
  - Lyrics and LRCLIB, and the podcast directory.
  - Audio effects DSP, the assistant, MCP, updates and the Connect protocol.
- `crates/app` (`discophile`): the egui/eframe interface and platform services.
  - Storage, keychain, media controls, tray and the macOS menu bar.
  - The native audio engine (symphonia, cpal, rubato) and the web audio backend.
  - Downloads.
- `Localization`: English keys with Italian translations, one file per area, plus plural forms.
- `site`: the website at [discophile.app](https://discophile.app), a Next.js app deployed on Vercel. See
  [site/README.md](../site/README.md).
- `deploy`: the web player's container files and Helm chart. See [WEB.md](WEB.md).
- `docs/ARCHITECTURE.md`: structure, patterns and the Connect design.

## Platform notes

- **Linux**: playback prefers PipeWire, then PulseAudio, so it follows the output selected in the desktop sound
  settings; plain ALSA is the fallback. X11/XWayland is preferred when available because the window backend cannot
  hide native Wayland windows; native Wayland keeps the tray controls and minimizes on close.
- **Tray**: Open, Play/Pause, Previous, Next and Quit. With closing to the tray enabled (Settings > General >
  Window), the window hides; without a tray it is minimized so it can still be reopened.
- **macOS**: clicking the Dock icon reopens a window closed to the tray. The menu bar offers Discophile, File,
  Controls, Go, Window and Help (sections with ⌘1 to ⌘0, back and forward with ⌘[ and ⌘], Settings with ⌘,).
- **Differences from the iPhone app**: no CarPlay, Apple Watch, Sport mode, Siri or dictation. Desktop playback
  changes pitch with speed. HLS and Opus streams are not supported by the desktop engine, and the browser plays
  only what it supports.

## Releases

Pushing a `v*` tag runs `.github/workflows/release.yml`. It builds a universal macOS disk image (signed with
Developer ID and notarized), a Windows zip and Linux tarball, Debian and AppImage packages, then publishes them as a GitHub release. The app
compares that release with its own version and offers the matching download. The same run pushes the web player
image to `ghcr.io/<owner>/discophile-web` (tags `X.Y.Z`, `X.Y` and `latest`, for amd64 and arm64) and the Helm
chart to `oci://ghcr.io/<owner>/charts/discophile-web`, with the chart and app version set from the tag; the
chart archive is attached to the release too. Both use the workflow's `GITHUB_TOKEN`, so no extra secrets are
needed. The first time, check in the package settings on GitHub that both packages are public.

Linux packages use `scripts/package-linux.sh` on Ubuntu 22.04 (x86_64, glibc 2.35 baseline). Debian linked dependencies are generated with `dpkg-shlibdeps`; runtime window-system dependencies are explicit. AppImage dependencies are bundled with a version- and checksum-pinned linuxdeploy. CI installs the Debian package and launches both packaged apps under Xvfb before publishing. AppImage filenames omit `linux-` so pre-0.1.3 tarball updaters keep selecting their compatible archive; AppImage installations select and replace the complete image instead.

On Arch Linux, the AUR package [`discophile-bin`](https://aur.archlinux.org/packages/discophile-bin) repackages the
Linux tarball. Its PKGBUILD is `packaging/aur/discophile-bin/PKGBUILD`; for every tag the workflow sets the
version and checksum from the tarball it just built, test-builds the package with `makepkg`, and pushes it to the
AUR. This runs only while the repository is public (AUR users download from the GitHub release) and needs three
secrets: `AUR_SSH_PRIVATE_KEY` (the private half of an SSH key added to your AUR account), `AUR_USERNAME` and
`AUR_EMAIL` (the author of the AUR commits). The first push creates the package on the AUR. The `.desktop` file
and icon shipped in the tarball are in `packaging/linux`.

The container files are in `Dockerfile` and `deploy/docker`, the chart in `deploy/helm/discophile-web`; see
[WEB.md](WEB.md). Before changing the chart, run `helm lint deploy/helm/discophile-web`.

```sh
# bump [workspace.package] version in Cargo.toml, commit, then:
git tag v0.2.0 && git push origin main v0.2.0
```

The tag must match the workspace version. Running the workflow by hand builds the same files as artifacts
without publishing. Signing and notarization need these repository secrets: `MACOS_CERTIFICATE_P12` (base64 of
the Developer ID Application .p12), `MACOS_CERTIFICATE_PASSWORD`, `APPLE_API_KEY_P8` (the contents of the App
Store Connect API key), `APPLE_API_KEY_ID` and `APPLE_API_ISSUER_ID`. Without them the macOS build is ad-hoc
signed. `scripts/package-macos.sh` builds the same app bundle locally.

## Screenshots

The README screenshots show a fictional library: the artists, albums and audiobooks of the iPhone promo, with
covers generated in the same styles and the *Discophile* cover by Mina Sol. Everything except the generated
media lives in `scripts/screenshots`; working files go to `.screenshots/`, which Git ignores. You need Docker,
ffmpeg and Python with Pillow and NumPy.

```sh
scripts/screenshots/fetch_fonts.sh
python3 scripts/screenshots/covers.py .screenshots/covers
python3 scripts/screenshots/build_library.py .screenshots         # silent, fully tagged music and audiobooks
docker compose -f scripts/screenshots/compose.yml -p discophile-screens up -d
python3 scripts/screenshots/setup_servers.py                      # Navidrome on 4542, Audiobookshelf on 13382
python3 scripts/screenshots/book_progress.py

export DISCOPHILE_DATA_DIR=$PWD/.screenshots/profile DISCOPHILE_CACHE_DIR=$PWD/.screenshots/profile-cache
export DISCOPHILE_SECRETS=file
cargo run -p discophile --example demo_profile                    # accounts, playlists, favorites, history
cargo run -p discophile
```

The covers use OFL fonts from [Google Fonts](https://github.com/google/fonts): Instrument Serif, Archivo Black,
DM Serif Display, Unbounded, Bebas Neue and Inter. The demo profile is in English with the dark appearance.
Capture the window at 1280 × 820 points and save the images as WebP in `docs/screenshots`, once with the dark
appearance and once with `"appearance": "Light"` in the profile's `settings.json` for `docs/screenshots/light`. The
README and the website pick the set that matches the reader's theme; the website uses 1600-pixel copies in
`site/public/assets/screens/desktop` and `desktop/light`.

The website's iPhone screenshots come from the iOS app (`../ios-app`) in the simulator, driven by the UI test in
`scripts/screenshots/ios`, against the same demo servers and in both appearances. With the servers running, run
`scripts/screenshots/ios/capture.sh`: it adds the validation build's `fixture-user`, builds the app for the
simulator with ad-hoc signing (it needs a keychain identity for the Audiobookshelf token), captures 13 screens in
dark and light, and writes them to `site/public/assets/screens` and `screens/light`. The assistant screens use
Apple Intelligence in the simulator, so the Mac must support it; the answers vary from run to run. The web player shown on the Download page is
captured by `scripts/screenshots/web_player.py` from the published container, with the same demo profile.

## Discophile Connect

`crates/core/src/connect/` implements accounts on a Connect instance (protocol version 1); the mock-server tests
in `connect/client.rs` cover discovery, token exchange, refresh rotation, revocation and the account. To try a
local server, choose `http://localhost:<port>` as the custom address in Settings > Discophile Connect. See
[CONNECT.md](CONNECT.md) for the user guide and [ARCHITECTURE.md](ARCHITECTURE.md) for the structure.
`connect/handoff.rs` keeps the draft playback handoff messages; their transport is not implemented yet.
