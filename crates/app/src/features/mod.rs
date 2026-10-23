//! Feature state and behavior. Each module owns its state struct (a field of `App`) and exposes
//! hooks that the app shell and player call at fixed points.

pub mod assistant;
pub mod audiobooks;
pub mod discovery;
pub mod downloads;
pub mod effects;
pub mod library;
pub mod lyrics;
pub mod radio;

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
}

/// Every frame, before drawing.
pub fn tick(app: &mut App) {
    library::tick(app);
    downloads::tick(app);
    discovery::tick(app);
    audiobooks::tick_always(app);
    assistant::tick(app);
    lyrics::tick(app);
    effects::tick(app);
}

/// Modal windows and sheets, drawn after the main layout.
pub fn windows(app: &mut App, ctx: &egui::Context) {
    library::windows(app, ctx);
    downloads::windows(app, ctx);
    discovery::windows(app, ctx);
    audiobooks::windows(app, ctx);
    radio::windows(app, ctx);
    assistant::windows(app, ctx);
    effects::windows(app, ctx);
}

/// The active music library changed (or was removed). Playback has already stopped.
pub fn on_library_changed(app: &mut App) {
    library::on_library_changed(app);
    downloads::on_library_changed(app);
    discovery::on_library_changed(app);
    assistant::on_library_changed(app);
    lyrics::on_library_changed(app);
}

/// A music or podcast item started (including gapless and crossfade advances).
pub fn on_track_started(app: &mut App, song: &Song) {
    discovery::on_track_started(app, song);
    lyrics::on_track_started(app, song);
}

/// Every frame while music plays.
pub fn on_music_tick(app: &mut App) {
    discovery::on_music_tick(app);
}

/// The music queue played to its end.
pub fn on_queue_finished(app: &mut App) {
    discovery::on_queue_finished(app);
}
