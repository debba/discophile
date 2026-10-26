//! The shared player model: queue, transport, sleep timer, resume state and listening accounting.
//! Music, podcasts and radio are driven here; audiobooks drive the same output through
//! `features::audiobooks`, which receives the output's events while `mode` is `Audiobook`.

use discophile_core::queue::{PlaybackQueue, RepeatMode};
use discophile_core::{InternetRadioStation, MusicError, Song, l10n};
use serde::{Deserialize, Serialize};

use crate::app::App;
use crate::audio::{AudioEvent, AudioOutput, AudioSource, ContentKind, MediaItem, OutputStatus};
use crate::features;
use crate::platform::storage;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Idle,
    Music,
    Audiobook,
    Radio,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SleepTimer {
    /// Unix milliseconds.
    At(i64),
    EndOfTrack,
    /// Audiobooks only; handled by `features::audiobooks`.
    EndOfChapter,
}

/// Who controls the queue's continuation, shown above the queue and in the player.
#[derive(Clone, Debug, PartialEq)]
pub enum PlayContext {
    /// Personal radio started from a track; `features::discovery` refills the queue.
    PersonalRadio { seed: Song, explore: bool },
    /// "Choose a record for tonight": the complete album in order, stopping at the end.
    AlbumFocus { album_id: String, title: String },
    /// A discovery mix or curated selection, by title.
    Mix { title: String },
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ResumeState {
    queue: PlaybackQueue,
    position: f64,
}

pub struct Player {
    /// Changes whenever a transport action invalidates an outstanding Connect handoff.
    pub control_generation: u64,
    pub output: Box<dyn AudioOutput>,
    pub queue: PlaybackQueue,
    pub mode: Mode,
    pub status: OutputStatus,
    pub volume: f32,
    pub rate: f32,
    pub shuffled: bool,
    pub radio: Option<InternetRadioStation>,
    pub sleep: Option<SleepTimer>,
    pub context: Option<PlayContext>,
    pub error: Option<String>,
    /// Item id currently handed to the output with `set_next`.
    prepared_next: Option<String>,
    /// Seconds of real listening on the current item; seeking and buffering don't count.
    listened: f64,
    last_position: Option<f64>,
    qualified: bool,
    last_save_ms: i64,
    /// Restored queues start paused at their saved position.
    pending_resume: Option<f64>,
}

impl Player {
    pub fn new(output: Box<dyn AudioOutput>, volume: f32) -> Self {
        let mut output = output;
        output.set_volume(volume);
        Self {
            control_generation: 0,
            output,
            queue: PlaybackQueue::new(),
            mode: Mode::Idle,
            status: OutputStatus::default(),
            volume,
            rate: 1.0,
            shuffled: false,
            radio: None,
            sleep: None,
            context: None,
            error: None,
            prepared_next: None,
            listened: 0.0,
            last_position: None,
            qualified: false,
            last_save_ms: 0,
            pending_resume: None,
        }
    }

    pub fn is_playing(&self) -> bool {
        self.status.playing
    }

    /// The current music, podcast or radio item (audiobooks expose their own state).
    pub fn current(&self) -> Option<Song> {
        match self.mode {
            Mode::Music => self.queue.current().cloned(),
            Mode::Radio => self.radio.as_ref().map(InternetRadioStation::song),
            _ => None,
        }
    }

    pub fn position(&self) -> f64 {
        self.pending_resume.unwrap_or(self.status.position)
    }

    pub fn duration(&self) -> Option<f64> {
        self.status.duration.or_else(|| self.current().and_then(|s| s.duration)).filter(|d| d.is_finite() && *d > 0.0)
    }

    pub fn is_live(&self) -> bool {
        self.mode == Mode::Radio
    }
}

/// Builds the output item for a song: the downloaded file when present, else the server stream.
pub fn media_item(app: &App, song: &Song) -> Option<MediaItem> {
    let kind = if song.is_podcast() { ContentKind::Podcast } else { ContentKind::Music };
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(path) = features::downloads::local_file(app, song) {
        return Some(MediaItem {
            id: song.id.clone(),
            source: AudioSource::File(path),
            kind,
            duration_hint: song.duration,
            replay_gain: song.replay_gain.clone(),
            album_key: song.album_id.clone(),
            server_format: song.suffix.clone(),
        });
    }
    let client = app.session.client.as_ref()?;
    let url = client.stream_url(song.stream_identifier(), app.settings.streaming_quality);
    Some(MediaItem {
        id: song.id.clone(),
        source: AudioSource::Url(url.to_string()),
        kind,
        duration_hint: song.duration,
        replay_gain: song.replay_gain.clone(),
        album_key: song.album_id.clone(),
        server_format: song.suffix.clone(),
    })
}

/// Replaces the queue and starts playing `songs[index]`.
pub fn play_songs(app: &mut App, songs: Vec<Song>, index: usize, shuffled: bool) {
    play_songs_with_context(app, songs, index, shuffled, None);
}

pub fn play_songs_with_context(app: &mut App, songs: Vec<Song>, index: usize, shuffled: bool, context: Option<PlayContext>) {
    #[cfg(not(target_arch = "wasm32"))]
    features::connect::services::take_local_control(app);
    app.player.control_generation += 1;
    if songs.is_empty() {
        app.player.error = Some(l10n::music_error(MusicError::EmptyQueue));
        return;
    }
    leave_current_content(app);
    app.player.context = context;
    app.player.queue.replace(songs, index, shuffled);
    app.player.shuffled = shuffled;
    app.player.mode = Mode::Music;
    load_current(app, 0.0, true);
}

/// Stops audiobook or radio state before other content starts.
fn leave_current_content(app: &mut App) {
    if app.player.mode == Mode::Audiobook {
        features::audiobooks::on_replaced(app);
    }
    app.player.radio = None;
    app.player.context = None;
    app.player.pending_resume = None;
    if app.player.sleep == Some(SleepTimer::EndOfChapter) {
        app.player.sleep = None;
    }
}

fn load_current(app: &mut App, start_at: f64, autoplay: bool) {
    let Some(song) = app.player.queue.current().cloned() else {
        app.player.output.stop();
        app.player.mode = Mode::Idle;
        return;
    };
    let Some(item) = media_item(app, &song) else {
        app.player.error = Some(l10n::music_error(MusicError::ServerUnavailable));
        return;
    };
    app.player.prepared_next = None;
    app.player.listened = 0.0;
    app.player.last_position = None;
    app.player.qualified = false;
    app.player.output.set_rate(if song.is_podcast() { app.player.rate } else { 1.0 });
    app.player.output.load(item, start_at, autoplay);
    app.player.status.item_id = Some(song.id.clone());
    app.player.status.position = start_at;
    app.player.status.playing = autoplay;
    if autoplay && app.settings.scrobble && !song.is_podcast() {
        scrobble(app, &song, false);
    }
    features::on_track_started(app, &song);
    save_resume(app, true);
}

pub fn play(app: &mut App) {
    #[cfg(not(target_arch = "wasm32"))]
    if features::connect::services::route_command(app, discophile_core::connect::RemoteCommand::Play) { return; }
    app.player.control_generation += 1;
    if app.player.mode == Mode::Audiobook {
        if !app.player.status.playing {
            features::audiobooks::toggle(app);
        }
        return;
    }
    if let Some(position) = app.player.pending_resume.take() {
        load_current(app, position, true);
        return;
    }
    output_play(app);
}

/// Resumes the output directly. Audiobook playback uses this from its own toggle.
pub fn output_play(app: &mut App) {
    app.player.output.play();
    app.player.status.playing = true;
}

pub fn pause(app: &mut App) {
    #[cfg(not(target_arch = "wasm32"))]
    if features::connect::services::route_command(app, discophile_core::connect::RemoteCommand::Pause) { return; }
    app.player.control_generation += 1;
    if app.player.mode == Mode::Audiobook {
        features::audiobooks::pause(app);
        return;
    }
    output_pause(app);
}

/// Pauses the output directly. Audiobook playback uses this from its own toggle.
pub fn output_pause(app: &mut App) {
    app.player.output.pause();
    app.player.status.playing = false;
    save_resume(app, true);
}

pub fn toggle(app: &mut App) {
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(remote) = features::connect::services::remote_playback(app) {
        let command = if remote.state.playing { discophile_core::connect::RemoteCommand::Pause } else { discophile_core::connect::RemoteCommand::Play };
        features::connect::services::route_command(app, command);
        return;
    }
    if app.player.mode == Mode::Audiobook {
        return features::audiobooks::toggle(app);
    }
    if app.player.mode == Mode::Idle && app.player.pending_resume.is_none() {
        return;
    }
    if app.player.status.playing { pause(app) } else { play(app) }
}

pub fn stop(app: &mut App) {
    #[cfg(not(target_arch = "wasm32"))]
    features::connect::services::take_local_control(app);
    app.player.control_generation += 1;
    if app.player.mode == Mode::Audiobook {
        features::audiobooks::on_replaced(app);
    }
    app.player.output.stop();
    app.player.status = OutputStatus::default();
    app.player.mode = Mode::Idle;
    app.player.radio = None;
    app.player.context = None;
    app.player.pending_resume = None;
}

pub fn next(app: &mut App) {
    #[cfg(not(target_arch = "wasm32"))]
    if features::connect::services::route_command(app, discophile_core::connect::RemoteCommand::Next) { return; }
    app.player.control_generation += 1;
    match app.player.mode {
        Mode::Music => {
            if app.player.queue.advance(false) {
                load_current(app, 0.0, true);
            }
        }
        Mode::Audiobook => features::audiobooks::skip_forward(app),
        _ => {}
    }
}

/// Restarts the song after three seconds, otherwise goes to the previous one.
pub fn previous(app: &mut App) {
    #[cfg(not(target_arch = "wasm32"))]
    if features::connect::services::route_command(app, discophile_core::connect::RemoteCommand::Previous) { return; }
    app.player.control_generation += 1;
    match app.player.mode {
        Mode::Music => {
            if app.player.position() > 3.0 || !app.player.queue.previous() {
                seek(app, 0.0);
            } else {
                load_current(app, 0.0, true);
            }
        }
        Mode::Audiobook => features::audiobooks::skip_backward(app),
        _ => {}
    }
}

pub fn seek(app: &mut App, seconds: f64) {
    #[cfg(not(target_arch = "wasm32"))]
    if features::connect::services::route_command(app, discophile_core::connect::RemoteCommand::Seek { position: seconds }) { return; }
    app.player.control_generation += 1;
    if app.player.mode == Mode::Audiobook {
        return features::audiobooks::seek(app, seconds);
    }
    if app.player.is_live() {
        return;
    }
    if let Some(pending) = app.player.pending_resume.as_mut() {
        *pending = seconds.max(0.0);
        return;
    }
    app.player.output.seek(seconds.max(0.0));
    app.player.status.position = seconds.max(0.0);
    // A seek is not listening time.
    app.player.last_position = None;
    app.player.prepared_next = None;
    app.player.output.set_next(None);
}

/// Starts an already validated Connect queue directly at its transferred offset.
pub fn play_transferred(
    app: &mut App,
    songs: Vec<Song>,
    index: usize,
    repeat: RepeatMode,
    position: f64,
    playing: bool,
) {
    app.player.control_generation += 1;
    leave_current_content(app);
    app.player.queue.replace(songs, index, false);
    app.player.queue.repeat_mode = repeat;
    app.player.shuffled = false;
    app.player.mode = Mode::Music;
    load_current(app, position, playing);
}

pub fn set_volume(app: &mut App, volume: f32) {
    #[cfg(not(target_arch = "wasm32"))]
    if features::connect::services::route_command(app, discophile_core::connect::RemoteCommand::SetVolume { volume: volume.clamp(0.0, 1.0) }) { return; }
    let volume = volume.clamp(0.0, 1.0);
    app.player.volume = volume;
    app.player.output.set_volume(volume);
    app.settings.volume = volume;
}

/// Podcast and audiobook speed.
pub fn set_rate(app: &mut App, rate: f32) {
    app.player.control_generation += 1;
    app.player.rate = rate.clamp(0.5, 4.0);
    let applies = match app.player.mode {
        Mode::Audiobook => true,
        Mode::Music => app.player.queue.current().is_some_and(Song::is_podcast),
        _ => false,
    };
    if applies {
        app.player.output.set_rate(app.player.rate);
    }
}

pub fn cycle_repeat(app: &mut App) {
    app.player.queue.repeat_mode = app.player.queue.repeat_mode.next();
    refresh_next(app);
}

pub fn toggle_shuffle(app: &mut App) {
    app.player.shuffled = !app.player.shuffled;
    if app.player.shuffled {
        app.player.queue.shuffle_upcoming();
    }
    refresh_next(app);
}

pub fn play_next(app: &mut App, songs: Vec<Song>) {
    if app.player.mode != Mode::Music || app.player.queue.is_empty() {
        return play_songs(app, songs, 0, false);
    }
    for song in songs.into_iter().rev() {
        app.player.queue.play_next(song);
    }
    refresh_next(app);
}

pub fn add_to_queue(app: &mut App, songs: Vec<Song>) {
    if app.player.mode != Mode::Music || app.player.queue.is_empty() {
        return play_songs(app, songs, 0, false);
    }
    for song in songs {
        app.player.queue.append(song);
    }
    refresh_next(app);
}

pub fn select_queue_index(app: &mut App, index: usize) {
    app.player.queue.select(index);
    load_current(app, 0.0, true);
}

pub fn remove_from_queue(app: &mut App, index: usize) {
    app.player.queue.remove(index);
    refresh_next(app);
}

pub fn move_in_queue(app: &mut App, from: usize, to: usize) {
    app.player.queue.move_item(from, to);
    refresh_next(app);
}

pub fn play_radio(app: &mut App, station: InternetRadioStation) {
    #[cfg(not(target_arch = "wasm32"))]
    features::connect::services::take_local_control(app);
    app.player.control_generation += 1;
    leave_current_content(app);
    app.player.queue = PlaybackQueue::new();
    app.player.mode = Mode::Radio;
    let item = MediaItem {
        id: station.song().id,
        source: AudioSource::Url(station.stream_url.to_string()),
        kind: ContentKind::Radio,
        duration_hint: None,
        replay_gain: None,
        album_key: None,
        server_format: None,
    };
    app.player.radio = Some(station);
    app.player.output.set_rate(1.0);
    app.player.output.load(item, 0.0, true);
    app.player.status.playing = true;
}

/// Drops the next-item preparation so it is recomputed (queue or settings changed).
pub fn refresh_next(app: &mut App) {
    app.player.prepared_next = None;
    app.player.output.set_next(None);
}

/// Called every frame.
pub fn tick(app: &mut App) {
    app.player.status = app.player.output.status();
    if app.player.pending_resume.is_some() {
        app.player.status.playing = false;
    }
    let events = app.player.output.poll_events();
    for event in events {
        if app.player.mode == Mode::Audiobook {
            features::audiobooks::on_output_event(app, event);
            continue;
        }
        handle_event(app, event);
    }
    match app.player.mode {
        Mode::Music => {
            account_listening(app);
            prepare_next(app);
            features::on_music_tick(app);
        }
        Mode::Audiobook => features::audiobooks::tick(app),
        _ => {}
    }
    check_sleep_timer(app);
    save_resume(app, false);
    if app.player.status.playing || app.player.status.buffering {
        app.ctx().request_repaint_after(std::time::Duration::from_millis(250));
    }
}

fn handle_event(app: &mut App, event: AudioEvent) {
    match event {
        AudioEvent::Ended { .. } => {
            if app.player.sleep == Some(SleepTimer::EndOfTrack) {
                app.player.sleep = None;
                app.player.output.pause();
                if app.player.queue.advance(true) {
                    load_current(app, 0.0, false);
                }
                return;
            }
            if matches!(app.player.context, Some(PlayContext::AlbumFocus { .. })) && app.player.queue.upcoming().is_empty() {
                app.player.output.stop();
                app.player.status.playing = false;
                return;
            }
            if app.player.mode == Mode::Music && app.player.queue.advance(true) {
                load_current(app, 0.0, true);
            } else {
                app.player.status.playing = false;
                features::on_queue_finished(app);
            }
        }
        AudioEvent::Advanced { to, .. } => {
            // The output already plays the prepared item; move the queue without reloading.
            if app.player.queue.advance(true) && app.player.queue.current().is_some_and(|s| s.id == to) {
                let song = app.player.queue.current().cloned();
                app.player.prepared_next = None;
                app.player.listened = 0.0;
                app.player.last_position = None;
                app.player.qualified = false;
                if let Some(song) = song {
                    if app.settings.scrobble && !song.is_podcast() {
                        scrobble(app, &song, false);
                    }
                    features::on_track_started(app, &song);
                }
                if app.player.sleep == Some(SleepTimer::EndOfTrack) {
                    app.player.sleep = None;
                    pause(app);
                }
            } else {
                load_current(app, 0.0, true);
            }
        }
        AudioEvent::Failed { message, .. } => {
            log::warn!("Playback failed: {message}");
            app.player.error = Some(if app.player.mode == Mode::Radio {
                l10n::t("This station could not be played. Check the stream URL and your connection.")
            } else {
                l10n::t("This song could not be played. Check your connection or try another quality.")
            });
            app.player.status.playing = false;
        }
    }
}

/// Hands the following song to the output so it can start without a gap or crossfade into it.
fn prepare_next(app: &mut App) {
    if app.player.prepared_next.is_some() || matches!(app.player.sleep, Some(SleepTimer::EndOfTrack)) {
        return;
    }
    if matches!(app.player.context, Some(PlayContext::AlbumFocus { .. })) && app.player.queue.upcoming().is_empty() {
        return;
    }
    let Some(next) = app.player.queue.peek_next(true).cloned() else { return };
    let Some(item) = media_item(app, &next) else { return };
    app.player.prepared_next = Some(item.id.clone());
    app.player.output.set_next(Some(item));
}

/// Listening counts after 30 seconds or half of a shorter track; seeking and buffering don't add time.
fn account_listening(app: &mut App) {
    let status = &app.player.status;
    if !status.playing || status.buffering {
        app.player.last_position = None;
        return;
    }
    let position = status.position;
    if let Some(last) = app.player.last_position {
        let delta = position - last;
        if delta > 0.0 && delta < 2.0 {
            app.player.listened += delta;
            if let Some(song) = app.player.queue.current().cloned() {
                features::discovery::record_listening(app, &song, delta);
                let threshold = song.duration.map(|d| (d / 2.0).min(30.0)).unwrap_or(30.0);
                if !app.player.qualified && app.player.listened >= threshold {
                    app.player.qualified = true;
                    if app.settings.scrobble && !song.is_podcast() {
                        scrobble(app, &song, true);
                    }
                    features::discovery::on_qualified_listen(app, &song);
                }
            }
        }
    }
    app.player.last_position = Some(position);
}

fn scrobble(app: &mut App, song: &Song, submission: bool) {
    let Some(client) = app.session.client.clone() else { return };
    let id = song.stream_identifier().to_string();
    app.tasks.spawn(async move {
        let _ = client.scrobble(&id, submission).await;
        crate::tasks::update(|_| {})
    });
}

fn check_sleep_timer(app: &mut App) {
    if let Some(SleepTimer::At(deadline)) = app.player.sleep {
        if discophile_core::now_millis() >= deadline {
            app.player.sleep = None;
            app.player.output.pause();
            app.player.status.playing = false;
            save_resume(app, true);
        } else {
            app.ctx().request_repaint_after(std::time::Duration::from_secs(1));
        }
    }
}

/// Seconds left on a timed sleep timer.
pub fn sleep_remaining(app: &App) -> Option<f64> {
    match app.player.sleep {
        Some(SleepTimer::At(deadline)) => Some(((deadline - discophile_core::now_millis()) as f64 / 1000.0).max(0.0)),
        _ => None,
    }
}

pub fn set_sleep_minutes(app: &mut App, minutes: Option<u32>) {
    app.player.sleep = minutes.map(|m| SleepTimer::At(discophile_core::now_millis() + m as i64 * 60_000));
}

/// Saves the music queue (up to 200 tracks) and position per library, at most every 10 seconds.
fn save_resume(app: &mut App, force: bool) {
    if app.player.mode != Mode::Music {
        return;
    }
    let now = discophile_core::now_millis();
    if !force && now - app.player.last_save_ms < 10_000 {
        return;
    }
    let account = app.session.account_key();
    if account.is_empty() {
        return;
    }
    app.player.last_save_ms = now;
    let state = ResumeState { queue: app.player.queue.truncated(200), position: app.player.status.position };
    storage::save(&format!("library/{account}/resume"), &state);
}

/// Restores the saved queue for the active library, paused at its position.
pub fn restore(app: &mut App) {
    let account = app.session.account_key();
    if account.is_empty() {
        return;
    }
    let Some(state) = storage::load::<ResumeState>(&format!("library/{account}/resume")) else { return };
    if state.queue.current().is_none() {
        return;
    }
    app.player.queue = state.queue;
    app.player.mode = Mode::Music;
    app.player.pending_resume = Some(state.position);
    app.player.status = OutputStatus { item_id: app.player.queue.current().map(|s| s.id.clone()), position: state.position, ..Default::default() };
}

/// Whether a saved queue is waiting to be resumed (Listen Now's "Continue" card).
pub fn has_pending_resume(app: &App) -> bool {
    app.player.pending_resume.is_some()
}

pub fn repeat_mode(app: &App) -> RepeatMode {
    app.player.queue.repeat_mode
}

/// Forgets the saved resume queue for the active library (after clearing listening history).
pub fn clear_resume(app: &mut App) {
    let account = app.session.account_key();
    if !account.is_empty() {
        storage::remove(&format!("library/{account}/resume"));
    }
    if app.player.pending_resume.take().is_some() {
        app.player.queue = PlaybackQueue::new();
        app.player.mode = Mode::Idle;
        app.player.status = OutputStatus::default();
    }
}
