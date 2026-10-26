//! Audiobook playback on the shared audio output: one file at a time on the book's timeline,
//! with chapter-aware sleep, per-book repeat, skip intervals and position reporting.

use discophile_core::audiobook_reader::{
    AudiobookReaderPreferences, AudiobookTimeline, ListeningMeter, TrackEndAction, end_of_chapter_stop, track_end_action,
};
use discophile_core::audiobookshelf::{Audiobook, AudiobookPlaybackSession, AudiobookProgress, AudiobookshelfClient};
use discophile_core::l10n::{audiobook_error, t};
use discophile_core::{MusicError, MusicResult, now_millis};
use uuid::Uuid;

use crate::app::App;
use crate::audio::{AudioEvent, AudioSource, ContentKind, MediaItem};
use crate::player::{self, Mode, SleepTimer};
use crate::tasks::update;

/// Status positions further than this from a requested offset are stale (the output is still seeking).
const SETTLE_TOLERANCE: f64 = 3.0;
const SETTLE_TIMEOUT_MS: i64 = 5_000;

pub struct Playback {
    pub book: Audiobook,
    pub session: AudiobookPlaybackSession,
    /// `AudiobookshelfAccount::id`.
    pub account: String,
    pub connection: Uuid,
    /// Playing the downloaded files.
    pub offline: bool,
    pub timeline: AudiobookTimeline,
    /// Index of the file handed to the output.
    pub track: usize,
    /// Position on the book's timeline, in seconds.
    pub position: f64,
    /// A file URL is being resolved.
    pub loading: bool,
    /// The "end of current chapter" sleep position.
    pub sleep_stop: Option<f64>,
    pub repeating: bool,
    autoplay: bool,
    generation: u64,
    meter: ListeningMeter,
    was_playing: bool,
    /// A requested file offset the output has not reported yet, and when it was requested.
    settling: Option<(f64, i64)>,
    rate_before: f32,
}

impl Playback {
    pub fn duration(&self) -> f64 {
        self.session.duration
    }

    fn item_id(&self) -> String {
        format!("audiobook:{}:{}:{}", self.session.id, self.track, self.generation)
    }

    pub fn chapter_title(&self) -> Option<String> {
        self.timeline.chapter(self.position).map(|c| c.display_title())
    }
}

/// Starts playing a book from `position`, replacing whatever played before.
#[allow(clippy::too_many_arguments)]
pub fn begin(
    app: &mut App,
    book: Audiobook,
    session: AudiobookPlaybackSession,
    account: String,
    connection: Uuid,
    offline: bool,
    position: f64,
    autoplay: bool,
) {
    if app.player.mode == Mode::Audiobook {
        super::on_replaced(app);
        app.player.mode = Mode::Idle;
    }
    player::stop(app);
    if app.player.sleep == Some(SleepTimer::EndOfTrack) {
        app.player.sleep = None;
    }
    app.player.mode = Mode::Audiobook;
    let rate_before = app.player.rate;
    let rate = app.audiobooks.reader.preferences.rate as f32;
    player::set_rate(app, rate);
    let timeline = AudiobookTimeline::for_session(&session, book.media.chapters.as_deref());
    let repeating = app.audiobooks.reader.is_repeating(&account, &book.id);
    let duration = session.duration;
    app.audiobooks.playback = Some(Playback {
        book,
        session,
        account,
        connection,
        offline,
        timeline,
        track: 0,
        position: 0.0,
        loading: false,
        sleep_stop: None,
        repeating,
        autoplay,
        generation: 0,
        meter: ListeningMeter::start(now_millis()),
        was_playing: false,
        settling: None,
        rate_before,
    });
    let start = if position.is_finite() { position.clamp(0.0, duration) } else { 0.0 };
    load_track(app, start, autoplay);
}

/// Hands the file containing `position` to the output, resolving a signed URL when streaming.
pub fn load_track(app: &mut App, position: f64, autoplay: bool) {
    let Some(pb) = app.audiobooks.playback.as_mut() else { return };
    let Some(location) = pb.session.location(position) else { return };
    pb.generation += 1;
    pb.track = location.index;
    pb.position = position.clamp(0.0, pb.duration());
    pb.loading = true;
    pb.autoplay = autoplay;
    pb.settling = None;
    let generation = pb.generation;
    let track = pb.session.audio_tracks[location.index].clone();
    let offline = pb.offline;
    let book_id = pb.book.id.clone();
    app.player.output.stop();
    app.player.status.playing = false;
    if offline {
        #[cfg(not(target_arch = "wasm32"))]
        {
            match super::downloads::local_path(app, &book_id, location.index) {
                Some(path) => start_output(app, AudioSource::File(path), location.offset, track.duration),
                None => fail(app, audiobook_error(MusicError::NotFound)),
            }
        }
        #[cfg(target_arch = "wasm32")]
        fail(app, audiobook_error(MusicError::Unsupported));
        let _ = book_id;
        return;
    }
    let Some(client) = app.audiobooks.client.clone() else {
        fail(app, audiobook_error(MusicError::Authentication));
        return;
    };
    app.tasks.spawn(async move {
        let result = client.media_url(&track.content_url).await;
        update(move |app| {
            if app.audiobooks.playback.as_ref().is_none_or(|pb| pb.generation != generation) {
                return;
            }
            match result {
                Ok(url) => start_output(app, AudioSource::Url(url.to_string()), location.offset, track.duration),
                Err(error) => fail(app, audiobook_error(error)),
            }
        })
    });
}

fn start_output(app: &mut App, source: AudioSource, offset: f64, duration: f64) {
    let Some(pb) = app.audiobooks.playback.as_mut() else { return };
    let item = MediaItem {
        id: pb.item_id(),
        source,
        kind: ContentKind::Audiobook,
        duration_hint: Some(duration),
        replay_gain: None,
        album_key: None,
        server_format: pb.session.audio_tracks.get(pb.track).and_then(|t| t.mime_type.clone()),
    };
    pb.loading = false;
    pb.settling = (offset > 0.0).then(|| (offset, now_millis()));
    pb.meter.resume(now_millis());
    let autoplay = pb.autoplay;
    let id = item.id.clone();
    app.player.output.set_rate(app.player.rate);
    app.player.output.load(item, offset, autoplay);
    app.player.status.item_id = Some(id);
    app.player.status.playing = autoplay;
    app.player.error = None;
}

fn fail(app: &mut App, message: String) {
    if let Some(pb) = app.audiobooks.playback.as_mut() {
        pb.loading = false;
        pb.autoplay = false;
    }
    app.player.status.playing = false;
    app.player.status.buffering = false;
    app.player.error = Some(message);
}

/// The result of preparing a streaming session (see `sync::prepare`).
pub fn prepared(
    app: &mut App,
    request: super::sync::StartRequest,
    client: Option<AudiobookshelfClient>,
    result: MusicResult<(Option<AudiobookProgress>, AudiobookPlaybackSession)>,
) {
    let current = app.audiobooks.starting.as_ref().is_some_and(|(_, g)| *g == request.generation);
    #[cfg(not(target_arch = "wasm32"))]
    let current = current && crate::features::connect::services::audiobook_start_allowed(app);
    if current {
        app.audiobooks.starting = None;
    }
    let connected = app.audiobooks.account.as_ref().is_some_and(|a| a.connection_id == request.connection);
    match result {
        Ok((latest, session)) => {
            if !current || !connected {
                // Close the server session nobody will use.
                if let Some(client) = client {
                    let progress = AudiobookProgress::new(session.current_time, session.duration);
                    app.tasks.spawn(async move {
                        let _ = client.sync(&session.id, &progress, 0.0, true).await;
                        update(|_| {})
                    });
                }
                return;
            }
            let resume = request
                .position
                .or_else(|| latest.map(|p| if p.is_finished { 0.0 } else { p.current_time }))
                .unwrap_or(session.current_time);
            let book = merged_book(app, request.book);
            begin(app, book, session, request.account, request.connection, false, resume, request.autoplay);
            #[cfg(not(target_arch = "wasm32"))]
            crate::features::connect::services::audiobook_prepared(app);
        }
        Err(error) => {
            if current {
                #[cfg(not(target_arch = "wasm32"))]
                crate::features::connect::services::audiobook_failed(app);
                let message = audiobook_error(error);
                match app.audiobooks.detail.as_mut() {
                    Some(detail) if detail.id == request.book.id => detail.error = Some(message),
                    _ => app.show_error(message),
                }
            }
        }
    }
}

/// The most complete copy of a book we know (the expanded item when the detail page loaded it).
fn merged_book(app: &App, book: Audiobook) -> Audiobook {
    match &app.audiobooks.detail {
        Some(detail) if detail.id == book.id => detail.book.clone().unwrap_or(book),
        _ => book,
    }
}

/// Cancels autoplay even before the output has loaded its file.
pub fn pause(app: &mut App) {
    if let Some(pb) = app.audiobooks.playback.as_mut() { pb.autoplay = false; }
    player::output_pause(app);
}

/// Plays or pauses, restarting a finished book and reloading after a failure.
pub fn toggle(app: &mut App) {
    let Some(pb) = app.audiobooks.playback.as_mut() else { return };
    if pb.loading {
        pb.autoplay = !pb.autoplay;
        return;
    }
    let playing = app.player.status.playing;
    let position = pb.position;
    let finished = pb.duration() > 0.0 && position >= pb.duration() - 0.5;
    let loaded = app.player.output.status().item_id.as_deref() == Some(pb.item_id().as_str());
    if playing {
        player::output_pause(app);
    } else if finished {
        load_track(app, 0.0, true);
    } else if !loaded {
        load_track(app, position, true);
    } else {
        player::output_play(app);
    }
}

/// Moves to a position on the book's timeline.
pub fn seek(app: &mut App, target: f64) {
    if !target.is_finite() {
        return;
    }
    let playing = app.player.status.playing;
    let Some(pb) = app.audiobooks.playback.as_mut() else { return };
    let target = target.clamp(0.0, pb.duration());
    if discophile_core::audiobook_reader::seek_clears_sleep_stop(target, pb.sleep_stop) {
        pb.sleep_stop = None;
        if app.player.sleep == Some(SleepTimer::EndOfChapter) {
            app.player.sleep = None;
        }
    }
    let Some(location) = pb.session.location(target) else { return };
    let loaded = app.player.output.status().item_id.as_deref() == Some(pb.item_id().as_str());
    let playing = playing || (pb.loading && pb.autoplay);
    if pb.loading || location.index != pb.track || !loaded {
        load_track(app, target, playing);
    } else {
        pb.position = target;
        pb.settling = Some((location.offset, now_millis()));
        app.player.output.seek(location.offset);
        app.player.status.position = target;
    }
    save(app, false);
}

pub fn skip(app: &mut App, forward: bool) {
    let Some(pb) = app.audiobooks.playback.as_ref() else { return };
    let target = app.audiobooks.reader.preferences.skip_target(pb.position, forward);
    seek(app, target);
}

pub fn set_rate(app: &mut App, rate: f64) {
    let rate = AudiobookReaderPreferences::rate(rate);
    app.audiobooks.reader.preferences.rate = rate;
    app.audiobooks.reader.save_preferences();
    if app.player.mode == Mode::Audiobook {
        player::set_rate(app, rate as f32);
    }
}

/// Pauses at the end of the current chapter.
pub fn sleep_at_end_of_chapter(app: &mut App) {
    let Some(pb) = app.audiobooks.playback.as_mut() else { return };
    let Some(stop) = end_of_chapter_stop(&pb.timeline, pb.position) else { return };
    pb.sleep_stop = Some(stop);
    app.player.sleep = Some(SleepTimer::EndOfChapter);
}

pub fn set_repeating(app: &mut App, enabled: bool) {
    let Some(pb) = app.audiobooks.playback.as_mut() else { return };
    pb.repeating = enabled;
    let (account, book) = (pb.account.clone(), pb.book.id.clone());
    app.audiobooks.reader.set_repeating(&account, &book, enabled);
}

pub fn mark_finished(app: &mut App) {
    let Some(duration) = app.audiobooks.playback.as_ref().map(Playback::duration) else { return };
    player::output_pause(app);
    if let Some(pb) = app.audiobooks.playback.as_mut() {
        pb.position = duration;
        pb.sleep_stop = None;
    }
    app.player.output.stop();
    save(app, false);
}

/// Saves the position locally and queues its upload.
pub fn save(app: &mut App, close: bool) {
    let Some(pb) = app.audiobooks.playback.as_mut() else { return };
    let progress = AudiobookProgress::new(pb.position, pb.duration());
    let listened = pb.meter.take(now_millis());
    let (account, connection, item, session, offline) =
        (pb.account.clone(), pb.connection, pb.book.id.clone(), pb.session.id.clone(), pb.offline);
    super::sync::record(app, &account, connection, &item, &session, progress, listened, close, offline);
}

/// Other content replaces the book: report the position and close the session.
pub fn replaced(app: &mut App) {
    if app.audiobooks.playback.is_none() {
        return;
    }
    save(app, true);
    if let Some(pb) = app.audiobooks.playback.take() {
        app.player.rate = pb.rate_before;
    }
    if app.player.sleep == Some(SleepTimer::EndOfChapter) {
        app.player.sleep = None;
    }
}

/// Switches a streaming book to its downloaded files once they are ready.
pub fn use_downloaded_if_available(app: &mut App, book_id: &str) {
    let Some(pb) = app.audiobooks.playback.as_ref() else { return };
    if pb.offline || pb.book.id != book_id {
        return;
    }
    let Some(record) = super::downloads::ready_record(app, book_id) else { return };
    let playing = app.player.status.playing || (pb.loading && pb.autoplay);
    save(app, true);
    let Some(pb) = app.audiobooks.playback.as_mut() else { return };
    pb.timeline = AudiobookTimeline::for_session(&record.playback, record.book.media.chapters.as_deref());
    pb.book = record.book;
    pb.session = record.playback;
    pb.offline = true;
    let position = pb.position;
    load_track(app, position, playing);
}

pub fn on_output_event(app: &mut App, event: AudioEvent) {
    let Some(pb) = app.audiobooks.playback.as_mut() else { return };
    let current = pb.item_id();
    match event {
        AudioEvent::Ended { item_id } | AudioEvent::Advanced { from: item_id, .. } if item_id == current => {
            match track_end_action(&pb.session, pb.track, pb.sleep_stop, pb.repeating) {
                TrackEndAction::PauseAt(stop) => {
                    pb.position = stop;
                    pb.sleep_stop = None;
                    pb.was_playing = false;
                    app.player.sleep = None;
                    app.player.output.pause();
                    app.player.status.playing = false;
                    save(app, false);
                }
                TrackEndAction::Continue(start) => load_track(app, start, true),
                TrackEndAction::Restart => load_track(app, 0.0, true),
                TrackEndAction::Finish => {
                    pb.position = pb.duration();
                    pb.was_playing = false;
                    app.player.status.playing = false;
                    save(app, false);
                }
            }
        }
        AudioEvent::Failed { item_id, message } if item_id == current => {
            log::warn!("Audiobook playback failed: {message}");
            pb.was_playing = false;
            fail(app, t("This audiobook file could not be played. Check your connection and audio format, then try again."));
            save(app, false);
        }
        _ => {}
    }
}

/// Every frame while an audiobook is the player's content.
pub fn tick(app: &mut App) {
    let now = now_millis();
    // Speed changed outside the book page (player bar, media keys) is remembered for the next book.
    let rate = AudiobookReaderPreferences::rate(app.player.rate as f64);
    if app.audiobooks.playback.is_some() && (rate - app.audiobooks.reader.preferences.rate).abs() > 0.001 {
        app.audiobooks.reader.preferences.rate = rate;
        app.audiobooks.reader.save_preferences();
    }
    let sleep = app.player.sleep;
    let output = app.player.status.clone();
    let Some(pb) = app.audiobooks.playback.as_mut() else { return };
    let current = pb.item_id();
    let live = !pb.loading && output.item_id.as_deref() == Some(current.as_str());
    if live {
        let offset = output.position;
        let settled = match pb.settling {
            Some((target, since)) => (offset - target).abs() < SETTLE_TOLERANCE || now - since > SETTLE_TIMEOUT_MS,
            None => true,
        };
        if settled {
            pb.settling = None;
            if let Some(track) = pb.session.audio_tracks.get(pb.track) {
                pb.position = (track.start_offset + offset.max(0.0)).clamp(0.0, pb.duration());
            }
        }
    }
    let playing = live && output.playing;
    // A sleep timer chosen elsewhere replaces "end of chapter"; one set from the player bar starts it.
    match sleep {
        Some(SleepTimer::EndOfChapter) if pb.sleep_stop.is_none() => {
            pb.sleep_stop = end_of_chapter_stop(&pb.timeline, pb.position);
            if pb.sleep_stop.is_none() {
                app.player.sleep = None;
            }
        }
        Some(SleepTimer::EndOfChapter) => {}
        _ => pb.sleep_stop = None,
    }
    let reached_stop = playing && pb.sleep_stop.is_some_and(|stop| pb.position >= stop - 0.05);
    let due = pb.meter.tick(now, playing);
    let paused = pb.was_playing && !playing && !pb.loading;
    pb.was_playing = playing && !reached_stop;
    let (position, duration, loading) = (pb.position, pb.duration(), pb.loading);
    if reached_stop {
        if let Some(stop) = pb.sleep_stop.take() {
            pb.position = stop;
        }
        app.player.sleep = None;
        app.player.output.pause();
        save(app, false);
    } else if paused || due {
        save(app, false);
    }
    // The player bar shows book time; the output itself works on the current file.
    let status = &mut app.player.status;
    status.position = position;
    status.duration = Some(duration);
    status.playing = playing && !reached_stop;
    status.buffering = output.buffering || loading;
    if loading {
        app.ctx().request_repaint_after(std::time::Duration::from_millis(250));
    }
}
