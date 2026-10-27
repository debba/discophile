//! Where listening stopped in each playlist (see `discophile_core::playlist_resume`), saved on this computer under
//! `library/<account>/playlist-resume`. A queue counts as a playlist while it plays that playlist's tracks as listed
//! (so Downloaded only works too), in order and outside radio or mixes. The point moves when a track starts, every
//! 15 seconds of playback and on pause or stop, and is cleared when the playlist plays to its end.

use discophile_core::playlist_resume::{PlaylistResume, PlaylistResumes, plays};
use discophile_core::{Song, now_millis};

use crate::app::App;
use crate::features::{library, progress};
use crate::platform::storage;
use crate::player::{self, Mode};

const SAVE_INTERVAL_MS: i64 = 15_000;

#[derive(Default)]
pub struct PlaylistResumeState {
    /// The library `resumes` belongs to.
    account: String,
    resumes: PlaylistResumes,
    /// The playlist the queue plays now.
    playing: Option<String>,
    last_save_ms: i64,
}

pub fn start(app: &mut App) {
    load(app);
}

pub fn on_library_changed(app: &mut App) {
    load(app);
}

fn storage_key(account: &str) -> String {
    format!("library/{account}/playlist-resume")
}

fn load(app: &mut App) {
    let account = app.session.account_key();
    let state = &mut app.playlist_resume;
    state.resumes = if account.is_empty() { PlaylistResumes::default() } else { storage::load(&storage_key(&account)).unwrap_or_default() };
    state.account = account;
    state.playing = None;
}

fn persist(app: &App) {
    let state = &app.playlist_resume;
    if !state.account.is_empty() {
        storage::save(&storage_key(&state.account), &state.resumes);
    }
}

/// Where listening stopped in `playlist`, with the track's index in `songs` (the playlist as listed now), when
/// continuing differs from playing it from the start.
pub fn resume_point(app: &App, playlist: &str, songs: &[Song]) -> Option<(usize, PlaylistResume)> {
    if app.playlist_resume.account != app.session.account_key() {
        return None;
    }
    let resume = app.playlist_resume.resumes.get(playlist)?;
    let index = resume.locate(songs)?;
    resume.is_useful(index).then(|| (index, resume.clone()))
}

/// Plays `songs` (the playlist as listed) from where listening stopped. Podcast episodes and long tracks continue
/// from their own saved position.
pub fn continue_playlist(app: &mut App, playlist: &str, songs: Vec<Song>) {
    let Some((index, resume)) = resume_point(app, playlist, &songs) else { return };
    let position = if progress::remembers(&songs[index]) { 0.0 } else { resume.resume_position().unwrap_or(0.0) };
    player::play_songs_at(app, songs, index, position);
}

/// The playlist the queue plays, if any: the one already followed, otherwise any loaded playlist it matches.
fn queue_playlist(app: &mut App) -> Option<String> {
    if app.player.mode != Mode::Music || app.player.shuffled || app.player.context.is_some() || app.player.queue.is_empty() {
        return None;
    }
    let queue = app.player.queue.songs().to_vec();
    let mut candidates: Vec<String> = app.playlist_resume.playing.iter().cloned().collect();
    candidates.extend(app.library.snapshot.playlist_contents.keys().filter(|id| app.playlist_resume.playing.as_ref() != Some(*id)).cloned());
    candidates.into_iter().find(|id| library::playlist_tracks(app, id).is_some_and(|songs| plays(&queue, &songs)))
}

/// A music item started (including gapless and crossfade advances).
pub fn on_track_started(app: &mut App) {
    app.playlist_resume.playing = queue_playlist(app);
    save(app);
}

/// Every frame while music plays.
pub fn on_music_tick(app: &mut App) {
    if app.playlist_resume.playing.is_some() && app.player.status.playing && now_millis() - app.playlist_resume.last_save_ms >= SAVE_INTERVAL_MS {
        save(app);
    }
}

/// Saves the queue's current track and position for the playlist it plays (on pause and stop too).
pub fn save(app: &mut App) {
    if app.playlist_resume.playing.is_none() {
        return;
    }
    let Some(song) = app.player.queue.current().cloned() else { return };
    // While the output still describes the previous item, its position does not belong to this track.
    if app.player.status.item_id.as_deref() != Some(song.id.as_str()) {
        return;
    }
    // The queue may have been edited since the track started.
    let Some(playlist) = queue_playlist(app) else {
        app.playlist_resume.playing = None;
        return;
    };
    let now = now_millis();
    let resume = PlaylistResume { index: app.player.queue.index(), id: song.stream_identifier().to_string(), position: app.player.position(), updated: now };
    app.playlist_resume.resumes.record(&playlist, resume);
    app.playlist_resume.playing = Some(playlist);
    app.playlist_resume.last_save_ms = now;
    persist(app);
}

/// The queue played to its end: a playlist starts from the beginning next time.
pub fn on_queue_finished(app: &mut App) {
    if let Some(playlist) = queue_playlist(app) {
        app.playlist_resume.resumes.finish(&playlist);
        persist(app);
    }
    app.playlist_resume.playing = None;
}

#[cfg(test)]
mod tests {
    use discophile_core::client::MusicClient;
    use discophile_core::{Credentials, MusicProvider};

    use super::*;

    fn fixture() -> (App, String) {
        let mut app = App::connect_test_app();
        let account = format!("playlist-resume-test-{}", uuid::Uuid::new_v4());
        let credentials = Credentials::simple("https://music.example.invalid", "fixture", "fixture", false, MusicProvider::Subsonic).unwrap();
        app.session.active = Some(account.clone());
        app.session.client = Some(MusicClient::new(credentials));
        crate::features::progress::start(&mut app);
        start(&mut app);
        (app, account)
    }

    fn songs(ids: &[&str]) -> Vec<Song> {
        ids.iter().map(|id| Song { duration: Some(200.0), ..Song::new(*id, *id) }).collect()
    }

    fn cleanup(account: &str) {
        storage::remove(&storage_key(account));
        storage::remove(&format!("library/{account}/progress"));
    }

    #[test]
    fn a_playlist_remembers_its_track_and_position_and_continues_there() {
        let (mut app, account) = fixture();
        let list = songs(&["a", "b", "c"]);
        app.library.snapshot.playlist_contents.insert("list".into(), list.clone());
        player::play_songs(&mut app, list.clone(), 1, false);
        player::seek(&mut app, 95.0);
        player::pause(&mut app);
        let (index, resume) = resume_point(&app, "list", &list).unwrap();
        assert_eq!((index, resume.id.as_str(), resume.position), (1, "b", 95.0));

        // Other music replaces the queue without touching the playlist's point.
        player::play_songs(&mut app, songs(&["x", "y"]), 0, false);
        assert!(app.playlist_resume.playing.is_none());
        continue_playlist(&mut app, "list", list.clone());
        assert_eq!(app.player.queue.index(), 1);
        assert_eq!(app.player.output.status().position, 95.0);
        assert_eq!(app.playlist_resume.playing.as_deref(), Some("list"));

        // A shuffled queue is not the playlist; playing it to the end starts it over.
        player::play_songs(&mut app, list.clone(), 0, true);
        assert!(app.playlist_resume.playing.is_none());
        player::play_songs(&mut app, list.clone(), 2, false);
        on_queue_finished(&mut app);
        assert!(resume_point(&app, "list", &list).is_none());
        cleanup(&account);
    }
}
