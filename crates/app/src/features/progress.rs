//! Listening positions of podcast episodes and long tracks (see `discophile_core::playback_progress`).
//! Positions are saved per library under `library/<account>/progress`, uploaded in order (at most once a
//! minute while playing, at once on pause, stop and the end of an item), and read back from the server
//! when a library or podcast opens and when such an item starts. Failed uploads stay pending and retry.

use discophile_core::audiobookshelf::SyncBackoff;
use discophile_core::playback_progress::{PlaybackProgress, ProgressArchive, remembers_position};
use discophile_core::{MusicError, Song, now_millis};

use crate::app::App;
use crate::platform::storage;
use crate::player::{self, Mode};
use crate::tasks::update;

/// Seconds of playback between local saves.
const RECORD_INTERVAL_MS: i64 = 15_000;
/// Pending positions are uploaded at most this often while playing.
const UPLOAD_INTERVAL_MS: i64 = 60_000;
/// Server positions are read again only after this long, unless a page asks explicitly.
const REFRESH_INTERVAL_MS: i64 = 30_000;
/// A newer position from another device moves playback only this soon after the item started.
const CORRECTION_WINDOW_MS: i64 = 30_000;

#[derive(Default)]
pub struct ProgressState {
    /// The library `archive` belongs to.
    account: String,
    archive: ProgressArchive,
    /// Bumped when the library changes, so late results for the previous one are ignored.
    generation: u64,
    backoff: SyncBackoff,
    busy: bool,
    refresh_requested: bool,
    upload_requested: bool,
    last_refresh_ms: i64,
    last_upload_ms: i64,
    /// The server keeps no positions for this library (no bookmarks, or not allowed).
    unsupported: bool,
    tracking: Option<Tracking>,
}

/// The item playing now, when it remembers its position.
struct Tracking {
    /// The queue item, as the output reports it.
    item: String,
    /// The stream identifier positions are saved under.
    id: String,
    duration: Option<f64>,
    started_ms: i64,
    last_record_ms: i64,
    /// The item actually played. Until then (or a seek) its saved position is left alone, so skipping
    /// through a queue does not reset episodes to their start.
    played: bool,
    /// The listener moved the position, so a position from another device no longer applies.
    seeked: bool,
}

pub fn start(app: &mut App) {
    load(app);
}

pub fn on_library_changed(app: &mut App) {
    load(app);
}

fn storage_key(account: &str) -> String {
    format!("library/{account}/progress")
}

fn load(app: &mut App) {
    let account = app.session.account_key();
    let state = &mut app.progress;
    state.generation += 1;
    state.archive = if account.is_empty() {
        ProgressArchive::default()
    } else {
        storage::read(&storage_key(&account))
            .and_then(|text| ProgressArchive::decode(text.as_bytes()).map_err(|_| log::warn!("Ignoring unreadable listening positions")).ok())
            .unwrap_or_default()
    };
    state.account = account;
    state.backoff.reset();
    state.busy = false;
    state.unsupported = false;
    state.tracking = None;
    state.last_refresh_ms = 0;
    state.refresh_requested = true;
    state.upload_requested = !state.archive.pending.is_empty();
    pump(app);
}

fn persist(app: &App) {
    let state = &app.progress;
    if state.account.is_empty() {
        return;
    }
    match state.archive.encode() {
        Ok(bytes) => storage::write(&storage_key(&state.account), &String::from_utf8_lossy(&bytes)),
        Err(_) => log::error!("Unable to encode listening positions"),
    }
}

/// Whether playback of `song` saves and resumes its position.
pub fn remembers(song: &Song) -> bool {
    remembers_position(song)
}

/// The saved position of a podcast episode or long track in the active library.
pub fn saved(app: &App, song: &Song) -> Option<PlaybackProgress> {
    if !remembers(song) || app.progress.account != app.session.account_key() {
        return None;
    }
    app.progress.archive.get(song.stream_identifier()).copied()
}

/// Where playback of `song` starts when nothing else asks for a position.
pub fn start_position(app: &App, song: &Song) -> f64 {
    saved(app, song).and_then(|p| p.resume_position()).unwrap_or(0.0)
}

/// Reads the server's positions again; `force` ignores the refresh interval (a page was opened).
pub fn refresh(app: &mut App, force: bool) {
    let state = &mut app.progress;
    if force || now_millis() - state.last_refresh_ms >= REFRESH_INTERVAL_MS {
        state.refresh_requested = true;
        pump(app);
    }
}

/// A music item started; its position is followed only if it remembers one. Nothing is saved before it plays or
/// is moved, and the periodic save waits 15 seconds, so a newer position from another device can still apply.
pub fn on_track_started(app: &mut App, song: &Song) {
    app.progress.tracking = None;
    if !remembers(song) || app.progress.account != app.session.account_key() {
        return;
    }
    let now = now_millis();
    app.progress.tracking = Some(Tracking {
        item: song.id.clone(),
        id: song.stream_identifier().to_string(),
        duration: song.duration,
        started_ms: now,
        last_record_ms: now,
        played: false,
        seeked: false,
    });
    refresh(app, false);
}

/// Every frame while music plays: saves the position every 15 seconds of playback.
pub fn on_music_tick(app: &mut App) {
    let status = &app.player.status;
    let Some(tracking) = app.progress.tracking.as_mut() else { return };
    if !status.playing || status.buffering || status.item_id.as_deref() != Some(tracking.item.as_str()) {
        return;
    }
    tracking.played = true;
    if now_millis() - tracking.last_record_ms >= RECORD_INTERVAL_MS {
        record_current(app, false);
    }
}

/// Every frame: uploads pending positions once a minute and runs requests a failure postponed.
pub fn tick(app: &mut App) {
    let state = &mut app.progress;
    let now = now_millis();
    if state.busy || !state.backoff.can_attempt(now) {
        return;
    }
    if !state.archive.pending.is_empty() && now - state.last_upload_ms >= UPLOAD_INTERVAL_MS {
        state.upload_requested = true;
    }
    if state.upload_requested || state.refresh_requested {
        pump(app);
    }
}

/// Saves the position of the item the output plays, if it is the tracked one. `flush` uploads it at once.
/// The queue may already point at the next item: the output status still describes the outgoing one.
pub fn record_current(app: &mut App, flush: bool) {
    let Some(tracking) = &app.progress.tracking else { return };
    if !(tracking.played || tracking.seeked)
        || app.player.mode != Mode::Music
        || app.player.status.item_id.as_deref() != Some(tracking.item.as_str())
    {
        return;
    }
    let duration = app.player.status.duration.filter(|d| d.is_finite() && *d > 0.0).or(tracking.duration).unwrap_or(0.0);
    let id = tracking.id.clone();
    record(app, &id, PlaybackProgress::at(app.player.status.position, duration, now_millis()), flush);
}

fn record(app: &mut App, id: &str, progress: PlaybackProgress, flush: bool) {
    if app.progress.account.is_empty() {
        return;
    }
    app.progress.archive.record(id, progress);
    if let Some(tracking) = app.progress.tracking.as_mut().filter(|t| t.id == id) {
        tracking.last_record_ms = progress.updated;
    }
    persist(app);
    if flush {
        app.progress.upload_requested = true;
        pump(app);
    }
}

/// The item playing now is being replaced or stopped: its position is saved and uploaded.
pub fn leave(app: &mut App) {
    record_current(app, true);
    app.progress.tracking = None;
}

/// The output finished `item` (it may already play the next one). A stream that ended well before its known
/// duration keeps its position instead of counting as played.
pub fn finished(app: &mut App, item: &str) {
    if app.progress.tracking.as_ref().is_none_or(|t| t.item != item) || app.player.mode != Mode::Music {
        return;
    }
    let Some(tracking) = app.progress.tracking.take() else { return };
    let now = now_millis();
    let status = &app.player.status;
    let duration = status.duration.filter(|d| d.is_finite() && *d > 0.0).or(tracking.duration).unwrap_or(0.0);
    let reached = PlaybackProgress::at(status.position, duration, now);
    let progress = if status.item_id.as_deref() == Some(item) && duration > 0.0 && !reached.finished {
        reached
    } else {
        PlaybackProgress::played(duration, now)
    };
    record(app, &tracking.id, progress, true);
}

/// The listener moved the position of the item playing now.
pub fn on_seek(app: &mut App) {
    if let Some(tracking) = app.progress.tracking.as_mut() {
        tracking.seeked = true;
    }
    record_current(app, false);
}

/// Marks an episode or long track as played or unplayed. Playing it again saves new positions.
pub fn mark(app: &mut App, song: &Song, played: bool) {
    let id = song.stream_identifier().to_string();
    if app.progress.tracking.as_ref().is_some_and(|t| t.id == id) {
        app.progress.tracking = None;
    }
    let duration = song.duration.or_else(|| saved(app, song).map(|p| p.duration)).unwrap_or(0.0);
    let now = now_millis();
    let progress = if played { PlaybackProgress::played(duration, now) } else { PlaybackProgress::unplayed(duration, now) };
    record(app, &id, progress, true);
}

/// Runs one server job at a time for the library the positions belong to: uploads first, then a refresh.
fn pump(app: &mut App) {
    let state = &app.progress;
    if state.busy || state.account.is_empty() || state.unsupported || state.account != app.session.account_key() {
        return;
    }
    let Some(client) = app.session.client.clone() else { return };
    let now = now_millis();
    if !state.backoff.can_attempt(now) {
        return;
    }
    let generation = state.generation;
    if state.upload_requested {
        app.progress.upload_requested = false;
        let items = app.progress.archive.pending_items();
        if items.is_empty() {
            return pump(app);
        }
        app.progress.busy = true;
        app.progress.last_upload_ms = now;
        app.tasks.spawn(async move {
            let mut results = Vec::new();
            for (id, progress) in items {
                let result = client.save_progress(&id, &progress).await;
                let stop = result.is_err() && result != Err(MusicError::NotFound);
                results.push((id, progress, result));
                if stop {
                    break;
                }
            }
            update(move |app| {
                if app.progress.generation != generation {
                    return;
                }
                let state = &mut app.progress;
                state.busy = false;
                for (id, progress, result) in results {
                    match result {
                        // A deleted item has nothing left to keep.
                        Ok(()) | Err(MusicError::NotFound) => state.archive.sent(&id, &progress),
                        Err(MusicError::Unsupported | MusicError::Forbidden) => {
                            log::info!("The server keeps no listening positions; they stay on this computer");
                            state.unsupported = true;
                            state.archive.pending.clear();
                        }
                        Err(error) => {
                            log::warn!("Listening position upload failed: {error}");
                            state.backoff.failed(now_millis());
                        }
                    }
                }
                persist(app);
                pump(app);
            })
        });
    } else if app.progress.refresh_requested {
        app.progress.refresh_requested = false;
        app.progress.busy = true;
        app.progress.last_refresh_ms = now;
        app.tasks.spawn(async move {
            let result = client.remote_progress().await;
            update(move |app| {
                if app.progress.generation != generation {
                    return;
                }
                app.progress.busy = false;
                match result {
                    Ok(remote) => {
                        let changed = app.progress.archive.merge(&remote);
                        persist(app);
                        follow_remote(app, &changed);
                    }
                    Err(MusicError::Unsupported | MusicError::Forbidden) => app.progress.unsupported = true,
                    Err(error) => {
                        log::warn!("Listening positions could not be read: {error}");
                        app.progress.backoff.failed(now_millis());
                    }
                }
                pump(app);
            })
        });
    }
}

/// Moves the item that just started to a newer position saved on another device, unless the listener
/// already moved it or has listened for a while.
fn follow_remote(app: &mut App, changed: &[String]) {
    let Some(tracking) = &app.progress.tracking else { return };
    if tracking.seeked
        || now_millis() - tracking.started_ms > CORRECTION_WINDOW_MS
        || !changed.contains(&tracking.id)
        || app.player.mode != Mode::Music
        || app.player.status.item_id.as_deref() != Some(tracking.item.as_str())
    {
        return;
    }
    let Some(position) = app.progress.archive.resume_position(&tracking.id) else { return };
    if (position - app.player.status.position).abs() > 3.0 {
        player::seek(app, position);
    }
}

#[cfg(test)]
mod tests {
    use discophile_core::client::MusicClient;
    use discophile_core::playback_progress::RemoteProgress;
    use discophile_core::{Credentials, MusicProvider};

    use super::*;

    /// A library whose server is never reached: uploads and refreshes fail in the background, unapplied.
    fn fixture() -> (App, String) {
        let mut app = App::connect_test_app();
        let account = format!("progress-test-{}", uuid::Uuid::new_v4());
        let credentials = Credentials::simple("https://music.example.invalid", "fixture", "fixture", false, MusicProvider::Subsonic).unwrap();
        app.session.active = Some(account.clone());
        app.session.client = Some(MusicClient::new(credentials));
        start(&mut app);
        (app, account)
    }

    fn episode(id: &str) -> Song {
        Song { stream_id: Some(format!("file-{id}")), is_podcast: Some(true), duration: Some(3_000.0), ..Song::new(format!("podcast:channel:{id}"), id) }
    }

    fn sync_status(app: &mut App) {
        app.player.status = app.player.output.status();
    }

    #[test]
    fn episodes_continue_where_they_were_left_and_count_as_played_at_the_end() {
        let (mut app, account) = fixture();
        let (first, second) = (episode("a"), episode("b"));
        player::play_songs(&mut app, vec![first.clone(), second.clone()], 0, false);
        assert_eq!(app.player.output.status().position, 0.0);
        player::seek(&mut app, 1_200.0);
        player::pause(&mut app);
        assert_eq!(saved(&app, &first).and_then(|p| p.resume_position()), Some(1_200.0));
        assert!(app.progress.archive.is_pending("file-a"));

        // Another episode, then back: the first one continues where it was left. Skipping the second one before
        // it plays leaves its saved position alone.
        app.progress.archive.record("file-b", PlaybackProgress::at(600.0, 3_000.0, 1));
        player::select_queue_index(&mut app, 1);
        sync_status(&mut app);
        assert_eq!(app.player.output.status().position, 600.0);
        app.player.output.seek(0.0);
        sync_status(&mut app);
        player::select_queue_index(&mut app, 0);
        assert_eq!(app.progress.archive.resume_position("file-b"), Some(600.0));
        assert_eq!(app.player.output.status().position, 1_200.0);

        // A stream that stops early keeps its position; the end counts as played.
        app.player.output.seek(1_500.0);
        sync_status(&mut app);
        finished(&mut app, &first.id);
        assert_eq!(saved(&app, &first).and_then(|p| p.resume_position()), Some(1_500.0));
        on_track_started(&mut app, &first);
        app.player.output.seek(2_990.0);
        sync_status(&mut app);
        finished(&mut app, &first.id);
        assert!(saved(&app, &first).is_some_and(|p| p.finished));

        // Positions survive a restart, and a finished episode starts again from the beginning.
        load(&mut app);
        assert!(saved(&app, &first).is_some_and(|p| p.finished));
        assert_eq!(start_position(&app, &first), 0.0);
        storage::remove(&storage_key(&account));
    }

    #[test]
    fn short_songs_keep_no_position_and_marks_apply_to_episodes() {
        let (mut app, account) = fixture();
        let song = Song { duration: Some(240.0), ..Song::new("song", "Song") };
        player::play_songs(&mut app, vec![song.clone()], 0, false);
        player::seek(&mut app, 120.0);
        player::pause(&mut app);
        assert!(saved(&app, &song).is_none());
        assert!(app.progress.archive.saved.is_empty());

        let long = Song { duration: Some(1_500.0), ..Song::new("mix", "Mix") };
        mark(&mut app, &long, true);
        assert!(saved(&app, &long).is_some_and(|p| p.finished));
        mark(&mut app, &long, false);
        assert!(saved(&app, &long).is_some_and(|p| !p.finished && p.resume_position().is_none()));
        assert!(app.progress.archive.is_pending("mix"));
        storage::remove(&storage_key(&account));
    }

    #[test]
    fn a_newer_position_from_another_device_moves_an_episode_that_just_started() {
        let (mut app, account) = fixture();
        let first = episode("a");
        player::play_songs(&mut app, vec![first.clone()], 0, false);
        sync_status(&mut app);
        let changed = app.progress.archive.merge(&[RemoteProgress {
            id: "file-a".into(),
            position: 900.0,
            duration: Some(3_000.0),
            updated: Some(now_millis()),
        }]);
        follow_remote(&mut app, &changed);
        assert_eq!(app.player.output.status().position, 900.0);

        // Once the listener moves the position, other devices no longer change it.
        player::seek(&mut app, 100.0);
        let changed = app.progress.archive.merge(&[RemoteProgress {
            id: "file-a".into(),
            position: 2_000.0,
            duration: Some(3_000.0),
            updated: Some(now_millis() + 1_000),
        }]);
        follow_remote(&mut app, &changed);
        assert_eq!(app.player.output.status().position, 100.0);
        storage::remove(&storage_key(&account));
    }
}
