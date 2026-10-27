//! Feature state and behavior. Each module owns its state struct (a field of `App`) and exposes
//! hooks that the app shell and player call at fixed points.

pub mod assistant;
pub mod audiobooks;
pub mod connect;
pub mod discovery;
pub mod downloads;
pub mod effects;
pub mod info;
pub mod library;
pub mod lyrics;
pub mod mentions;
#[cfg(not(target_arch = "wasm32"))]
pub mod mcp;
pub mod playlist_resume;
pub mod progress;
pub mod radio;
#[cfg(not(target_arch = "wasm32"))]
pub mod software_update;

use discophile_core::Song;

use crate::app::App;

/// Once, after the app state is created.
pub fn start(app: &mut App) {
    library::start(app);
    downloads::start(app);
    discovery::start(app);
    audiobooks::start(app);
    radio::start(app);
    assistant::start(app);
    effects::start(app);
    lyrics::start(app);
    info::start(app);
    progress::start(app);
    playlist_resume::start(app);
    connect::start(app);
    #[cfg(not(target_arch = "wasm32"))]
    mcp::start(app);
    #[cfg(not(target_arch = "wasm32"))]
    software_update::start(app);
}

/// Every frame, before drawing.
pub fn tick(app: &mut App) {
    #[cfg(not(target_arch = "wasm32"))]
    connect::services::tick(app);
    library::tick(app);
    downloads::tick(app);
    discovery::tick(app);
    audiobooks::tick_always(app);
    assistant::tick(app);
    lyrics::tick(app);
    progress::tick(app);
    effects::tick(app);
    #[cfg(not(target_arch = "wasm32"))]
    mcp::tick(app);
}

/// Modal windows and sheets, drawn after the main layout.
pub fn windows(app: &mut App, ctx: &egui::Context) {
    #[cfg(not(target_arch = "wasm32"))]
    crate::ui::software_update::windows(app, ctx);
    library::windows(app, ctx);
    downloads::windows(app, ctx);
    discovery::windows(app, ctx);
    audiobooks::windows(app, ctx);
    radio::windows(app, ctx);
    assistant::windows(app, ctx);
    effects::windows(app, ctx);
    #[cfg(not(target_arch = "wasm32"))]
    crate::ui::agents::confirmation(app, ctx);
}

/// The active music library changed (or was removed). Playback has already stopped.
pub fn on_library_changed(app: &mut App) {
    library::on_library_changed(app);
    downloads::on_library_changed(app);
    discovery::on_library_changed(app);
    assistant::on_library_changed(app);
    lyrics::on_library_changed(app);
    info::on_library_changed(app);
    progress::on_library_changed(app);
    playlist_resume::on_library_changed(app);
    #[cfg(not(target_arch = "wasm32"))]
    mcp::on_library_changed(app);
}

/// A music or podcast item started (including gapless and crossfade advances).
pub fn on_track_started(app: &mut App, song: &Song) {
    discovery::on_track_started(app, song);
    lyrics::on_track_started(app, song);
    progress::on_track_started(app, song);
    playlist_resume::on_track_started(app);
}

/// Every frame while music plays.
pub fn on_music_tick(app: &mut App) {
    discovery::on_music_tick(app);
    progress::on_music_tick(app);
    playlist_resume::on_music_tick(app);
}

/// The music queue played to its end.
pub fn on_queue_finished(app: &mut App) {
    discovery::on_queue_finished(app);
    playlist_resume::on_queue_finished(app);
}
