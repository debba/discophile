//! Music downloads for offline listening (desktop only) and automatic downloads.
//!
//! Files live in `platform::storage::data_dir()/downloads/<account>/`: a `catalog.json` of
//! `OfflineRecord`s, audio files named by `OfflineRecord::filename`, and downloaded artwork. Up to three
//! transfers run at once on the tokio runtime; queued transfers resume on the next launch. Removing a
//! download never touches the server. In the web build every function is a harmless no-op:
//! `is_downloaded` is false, `download_button` draws nothing and `settings_ui` explains that offline
//! listening needs the desktop app.
//!
//! Public API (all functions act on the active music library):
//! - `download(app, songs)`: download songs (`Vec<Song>`, `&[Song]` or `&Vec<Song>`); already
//!   downloaded songs are skipped, failed ones restart, automatic ones become permanent ("Download all").
//! - `remove(app, song)`: remove a download or cancel its transfer. `remove_matching(app, |song| …)`
//!   removes several (for example a deleted podcast's episodes).
//! - `keep(app, song)`: "Keep download", turning an automatic download into a manual one.
//! - `is_downloaded(app, song)`, `download_state(app, song) -> DownloadState`, `progress(app, song)`.
//! - `download_button(ui, app, song)`: the trailing control on track rows (click to download, cancel or
//!   retry; right-click for Keep download / Remove download).
//! - `song_menu(ui, app, song)`: Download / Keep download / Cancel download / Remove download entries for
//!   a track's context menu.
//! - `downloaded_songs(app)`, `downloaded_albums(app)`, `downloaded_count(app, songs)`: the "Downloaded only"
//!   filter's content. `revision(app)` changes whenever downloads change, to cache derived lists.
//! - `total_bytes(app)`, `is_transferring(app)`, `retry_failed(app)`, `remove_all(app)`.
//! - `local_file(app, song)` / `local_artwork(app, cover)` (desktop): files used by the player and artwork.
//! - Hooks: `on_library_refreshed(app)` after the library reloads from the server, `on_mixes_changed(app)`
//!   after a curated mix is saved; both start an automatic-downloads sync.
//! - `settings_ui(app, ui)`: Settings > Offline listening (automatic downloads, storage, removal).

use std::borrow::Borrow;

use discophile_core::{Album, Song};

use crate::app::App;

#[cfg(not(target_arch = "wasm32"))]
mod automatic;
mod format;
#[cfg(not(target_arch = "wasm32"))]
mod manager;
#[cfg(not(target_arch = "wasm32"))]
mod store;
#[cfg(not(target_arch = "wasm32"))]
mod transfer;
mod view;

pub use format::bytes as format_bytes;

#[cfg(not(target_arch = "wasm32"))]
pub use manager::{DownloadState, Manager as Downloads};

#[cfg(target_arch = "wasm32")]
#[derive(Default)]
pub struct Downloads {
    pub(super) confirm_remove_all: bool,
}

/// What a track's download control shows.
#[cfg(target_arch = "wasm32")]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DownloadState {
    NotDownloaded,
    /// Waiting or transferring, with the fraction transferred so far.
    Downloading(f32),
    Downloaded {
        automatic: bool,
    },
    Failed,
}

pub fn start(app: &mut App) {
    #[cfg(not(target_arch = "wasm32"))]
    manager::start(app);
    #[cfg(target_arch = "wasm32")]
    let _ = app;
}

pub fn tick(app: &mut App) {
    #[cfg(not(target_arch = "wasm32"))]
    manager::tick(app);
    #[cfg(target_arch = "wasm32")]
    let _ = app;
}

pub fn windows(app: &mut App, ctx: &egui::Context) {
    view::windows(app, ctx);
}

pub fn on_library_changed(app: &mut App) {
    #[cfg(not(target_arch = "wasm32"))]
    manager::on_library_changed(app);
    #[cfg(target_arch = "wasm32")]
    let _ = app;
}

/// The library reloaded from the server: syncs automatic downloads.
pub fn on_library_refreshed(app: &mut App) {
    #[cfg(not(target_arch = "wasm32"))]
    automatic::refresh(app, true);
    #[cfg(target_arch = "wasm32")]
    let _ = app;
}

/// A curated mix was saved or removed: syncs automatic downloads.
pub fn on_mixes_changed(app: &mut App) {
    on_library_refreshed(app);
}

/// Removes a library's downloads, folder and automatic-download settings (when its account is removed).
pub fn remove_account(app: &mut App, account: &str) {
    #[cfg(not(target_arch = "wasm32"))]
    manager::remove_account(app, account);
    #[cfg(target_arch = "wasm32")]
    let _ = (app, account);
}

/// The downloaded file for a song in the active library, if complete.
#[cfg(not(target_arch = "wasm32"))]
pub fn local_file(app: &App, song: &Song) -> Option<std::path::PathBuf> {
    app.downloads.local_file(song)
}

/// Downloaded artwork for a cover id in the active library (desktop).
#[cfg(not(target_arch = "wasm32"))]
pub fn local_artwork(app: &App, cover: &str) -> Option<std::path::PathBuf> {
    app.downloads.local_artwork(cover)
}

pub fn is_downloaded(app: &App, song: &Song) -> bool {
    matches!(download_state(app, song), DownloadState::Downloaded { .. })
}

pub fn download_state(app: &App, song: &Song) -> DownloadState {
    #[cfg(not(target_arch = "wasm32"))]
    return app.downloads.state(song);
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (app, song);
        DownloadState::NotDownloaded
    }
}

/// The transferred fraction while a song is downloading.
pub fn progress(app: &App, song: &Song) -> Option<f32> {
    match download_state(app, song) {
        DownloadState::Downloading(fraction) => Some(fraction),
        _ => None,
    }
}

/// Downloads songs of the active library with the current streaming quality.
pub fn download<I>(app: &mut App, songs: I)
where
    I: IntoIterator,
    I::Item: Borrow<Song>,
{
    #[cfg(not(target_arch = "wasm32"))]
    manager::download(app, songs.into_iter().map(|s| s.borrow().clone()).collect(), false);
    #[cfg(target_arch = "wasm32")]
    let _ = (app, songs.into_iter().count());
}

/// Removes a song's download, or cancels it while transferring. Media on the server is unchanged.
pub fn remove(app: &mut App, song: &Song) {
    remove_matching(app, |s| s.id == song.id);
}

/// Removes the active library's downloads whose song matches.
pub fn remove_matching(app: &mut App, matches: impl Fn(&Song) -> bool) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let ids: Vec<uuid::Uuid> = app.downloads.account_records().filter(|r| matches(&r.song)).map(|r| r.id).collect();
        manager::remove_ids(app, &ids);
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (app, matches);
}

/// "Keep download": an automatic download becomes permanent.
pub fn keep(app: &mut App, song: &Song) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let account = app.downloads.account.clone();
        if let Some(id) = app.downloads.record(&account, &song.id).map(|r| r.id) {
            manager::keep_record(app, id);
        }
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (app, song);
}

/// Restarts failed downloads of the active library, keeping completed files.
pub fn retry_failed(app: &mut App) {
    #[cfg(not(target_arch = "wasm32"))]
    manager::retry_failed(app);
    #[cfg(target_arch = "wasm32")]
    let _ = app;
}

/// Removes every music download of the active library and turns off automatic downloads.
pub fn remove_all(app: &mut App) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if app.downloads.automatic.settings.enabled {
            automatic::update_settings(app, |s| s.enabled = false);
        }
        let current_is_local = app.player.mode == crate::player::Mode::Music
            && app.player.queue.current().is_some_and(|song| app.downloads.local_file(song).is_some());
        if current_is_local {
            crate::player::stop(app);
        }
        let ids: Vec<uuid::Uuid> = app.downloads.account_records().map(|r| r.id).collect();
        manager::remove_ids(app, &ids);
    }
    #[cfg(target_arch = "wasm32")]
    let _ = app;
}

/// Ready downloads of the active library, in download order.
pub fn downloaded_songs(app: &App) -> Vec<Song> {
    #[cfg(not(target_arch = "wasm32"))]
    return discophile_core::offline_catalog::OfflineCatalog::songs(app.downloads.records(), &app.downloads.account);
    #[cfg(target_arch = "wasm32")]
    {
        let _ = app;
        Vec::new()
    }
}

/// Albums with downloaded music (no podcasts), sorted by name.
pub fn downloaded_albums(app: &App) -> Vec<Album> {
    #[cfg(not(target_arch = "wasm32"))]
    return discophile_core::offline_catalog::OfflineCatalog::albums(app.downloads.records(), &app.downloads.account);
    #[cfg(target_arch = "wasm32")]
    {
        let _ = app;
        Vec::new()
    }
}

/// How many of `songs` are downloaded ("Download all" can hide when all are).
pub fn downloaded_count(app: &App, songs: &[Song]) -> usize {
    songs.iter().filter(|s| is_downloaded(app, s)).count()
}

/// Bytes of ready downloads in the active library.
pub fn total_bytes(app: &App) -> i64 {
    #[cfg(not(target_arch = "wasm32"))]
    return discophile_core::offline_catalog::OfflineCatalog::total_bytes(app.downloads.records(), &app.downloads.account);
    #[cfg(target_arch = "wasm32")]
    {
        let _ = app;
        0
    }
}

/// Whether transfers are queued or running.
pub fn is_transferring(app: &App) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    return app.downloads.is_transferring();
    #[cfg(target_arch = "wasm32")]
    {
        let _ = app;
        false
    }
}

/// Changes whenever the catalog changes or the library switches.
pub fn revision(app: &App) -> u64 {
    #[cfg(not(target_arch = "wasm32"))]
    return app.downloads.revision;
    #[cfg(target_arch = "wasm32")]
    {
        let _ = app;
        0
    }
}

/// The trailing download control on track rows.
pub fn download_button(ui: &mut egui::Ui, app: &mut App, song: &Song) {
    view::download_button(ui, app, song);
}

/// Download entries for a track's context menu (nothing on the web).
pub fn song_menu(ui: &mut egui::Ui, app: &mut App, song: &Song) {
    view::song_menu(ui, app, song);
}

/// Settings > Offline listening: automatic downloads, storage use and removal.
pub fn settings_ui(app: &mut App, ui: &mut egui::Ui) {
    view::settings_ui(app, ui);
}
