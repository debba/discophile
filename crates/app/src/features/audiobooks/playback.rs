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

/// Allow decoder rounding before the requested offset once the backend acknowledges readiness.
const SETTLE_TOLERANCE: f64 = 3.0;

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
    /// Playback intent is waiting for a file, its initial seek, or ready audio.
    pub loading: bool,
    resolving: bool,
    identity: Uuid,
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
    stream_url: Option<url::Url>,
    recovering: bool,
    stream_failed: bool,
    buffering_since: Option<i64>,
    recovery_after: i64,
}

impl Playback {
    pub fn duration(&self) -> f64 {
        self.session.duration
    }

    fn item_id(&self) -> String {
        format!("audiobook:{}:{}:{}", self.identity, self.track, self.generation)
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
        resolving: false,
        identity: Uuid::new_v4(),
        sleep_stop: None,
        repeating,
        autoplay,
        generation: 0,
        meter: ListeningMeter::start(now_millis()),
        was_playing: false,
        settling: None,
        rate_before,
        stream_url: None,
        recovering: false,
        stream_failed: false,
        buffering_since: None,
        recovery_after: 0,
    });
    if let Some(pb) = app.audiobooks.playback.as_ref() {
        let (account, book) = (pb.account.clone(), pb.book.clone());
        app.audiobooks.sync.remember_book(&account, book);
    }
    let start = if position.is_finite() { position.clamp(0.0, duration) } else { 0.0 };
    load_track(app, start, autoplay);
    if let Some(pb) = app.audiobooks.playback.as_ref() {
        let (account, item, progress) = (pb.account.clone(), pb.book.id.clone(), AudiobookProgress::new(pb.position, pb.duration()));
        app.audiobooks.sync.cache_position(&account, &item, progress);
    }
}

/// Hands the file containing `position` to the output, resolving a signed URL when streaming.
pub fn load_track(app: &mut App, position: f64, autoplay: bool) {
    let Some(pb) = app.audiobooks.playback.as_mut() else { return };
    let Some(location) = pb.session.location(position) else { return };
    pb.generation += 1;
    pb.track = location.index;
    pb.position = position.clamp(0.0, pb.duration());
    pb.loading = autoplay;
    pb.resolving = true;
    pb.autoplay = autoplay;
    pb.settling = None;
    pb.stream_url = None;
    pb.recovering = false;
    pb.stream_failed = false;
    pb.buffering_since = None;
    pb.recovery_after = 0;
    let item_id = pb.item_id();
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
            if app.audiobooks.playback.as_ref().is_none_or(|pb| pb.item_id() != item_id) {
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
    pb.stream_url = match &source {
        AudioSource::Url(value) => url::Url::parse(value).ok(),
        #[cfg(not(target_arch = "wasm32"))]
        AudioSource::File(_) => None,
    };
    let item = MediaItem {
        id: pb.item_id(),
        source,
        kind: ContentKind::Audiobook,
        duration_hint: Some(duration),
        replay_gain: None,
        album_key: None,
        server_format: pb.session.audio_tracks.get(pb.track).and_then(|t| t.mime_type.clone()),
    };
    pb.resolving = false;
    pb.settling = (offset > 0.0).then(|| (offset, now_millis()));
    pb.meter.resume(now_millis());
    let autoplay = pb.autoplay;
    let id = item.id.clone();
    app.player.output.set_rate(app.player.rate);
    app.player.output.load(item, offset, autoplay);
    app.player.status.item_id = Some(id);
    app.player.status.playing = false;
    app.player.status.buffering = autoplay;
    app.player.error = None;
}

fn fail(app: &mut App, message: String) {
    if let Some(pb) = app.audiobooks.playback.as_mut() {
        pb.loading = false;
        pb.resolving = false;
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
            let latest = AudiobookProgress::resuming(latest, app.audiobooks.sync.saved(&request.account, &request.book.id));
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
    super::cancel_pending_start(app);
    if let Some(pb) = app.audiobooks.playback.as_mut() { pb.autoplay = false; pb.loading = false; }
    app.player.status.buffering = false;
    player::output_pause(app);
}

/// Plays or pauses, restarting a finished book and reloading after a failure.
pub fn toggle(app: &mut App) {
    let Some(pb) = app.audiobooks.playback.as_mut() else { return };
    if pb.loading {
        pause(app);
        return;
    }
    let playing = app.player.status.playing;
    let position = pb.position;
    let finished = pb.duration() > 0.0 && position >= pb.duration() - 0.5;
    let loaded = app.player.output.status().item_id.as_deref() == Some(pb.item_id().as_str());
    if playing {
        pause(app);
    } else if finished {
        load_track(app, 0.0, true);
    } else if !loaded {
        load_track(app, position, true);
    } else {
        pb.autoplay = true;
        player::output_play(app);
    }
}

/// Moves to a position on the book's timeline.
pub fn seek(app: &mut App, target: f64) {
    if !target.is_finite() {
        return;
    }
    if let Some((id, _)) = app.audiobooks.starting.clone() {
        super::start_book(app, &id, Some(target));
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
    if pb.loading || pb.resolving || location.index != pb.track || !loaded {
        load_track(app, target, playing);
    } else {
        pb.position = target;
        pb.loading = playing;
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
        pb.autoplay = false;
        pb.generation += 1;
        pb.loading = false;
        pb.resolving = false;
        pb.recovering = false;
        pb.stream_url = None;
        pb.buffering_since = None;
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
                    pb.autoplay = false;
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
                    pb.autoplay = false;
                    app.player.status.playing = false;
                    save(app, false);
                }
            }
        }
        AudioEvent::Failed { item_id, message } if item_id == current => {
            let _ = message;
            recover_stream(app, true);
        }
        _ => {}
    }
}

fn recover_stream(app: &mut App, failing: bool) {
    let Some(pb) = app.audiobooks.playback.as_mut() else { return };
    pb.stream_failed |= failing;
    if pb.recovering { return; }
    let fallback = t("This audiobook file could not be played. Check your connection and audio format, then try again.");
    let Some((current, client)) = pb.stream_url.clone().zip(app.audiobooks.client.clone()).filter(|(_, client)| !pb.offline && !pb.resolving && client.account().connection_id == pb.connection) else {
        if failing { fail(app, fallback); save(app, false); }
        return;
    };
    let item = pb.item_id();
    let path = pb.session.audio_tracks[pb.track].content_url.clone();
    pb.recovering = true;
    pb.recovery_after = now_millis() + 20_000;
    app.tasks.spawn(async move {
        let renewed = client.renewed_media_url(&current, &path).await;
        update(move |app| {
            let Some(pb) = app.audiobooks.playback.as_mut().filter(|pb| pb.item_id() == item) else { return };
            pb.recovering = false;
            if matches!(renewed, Ok(Some(_))) {
                let (position, autoplay) = (pb.position, pb.autoplay);
                load_track(app, position, autoplay);
            } else if pb.stream_failed {
                fail(app, fallback);
                save(app, false);
            }
        })
    });
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
    let output = app.player.output.status();
    let Some(pb) = app.audiobooks.playback.as_mut() else { return };
    let current = pb.item_id();
    let live = !pb.resolving && output.item_id.as_deref() == Some(current.as_str());
    if live {
        let offset = output.position;
        let settled = match pb.settling {
            Some((target, _)) => output.ready && offset >= target - SETTLE_TOLERANCE,
            None => true,
        };
        if settled && output.ready {
            pb.settling = None;
            if let Some(track) = pb.session.audio_tracks.get(pb.track) {
                pb.position = (track.start_offset + offset.max(0.0)).clamp(0.0, pb.duration());
            }
        }
    }
    if live && output.ready && pb.settling.is_none() && output.playing && !output.buffering {
        pb.loading = false;
    }
    let playing = live && output.playing && output.ready && !output.buffering && !pb.loading && pb.autoplay;
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
    let (position, duration, loading, autoplay) = (pb.position, pb.duration(), pb.loading, pb.autoplay);
    let stalled = if !reached_stop && live && output.buffering && pb.stream_url.is_some() && pb.autoplay {
        let since = *pb.buffering_since.get_or_insert(now);
        now - since >= 5_000 && now >= pb.recovery_after
    } else { pb.buffering_since = None; false };
    if reached_stop {
        pb.autoplay = false;
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
    status.buffering = (output.buffering || loading) && autoplay;
    if stalled { recover_stream(app, false); }
    if loading {
        app.ctx().request_repaint_after(std::time::Duration::from_millis(250));
    }
}


#[cfg(all(test, not(target_arch = "wasm32")))]
mod token_recovery_tests {
    use super::*;
    use discophile_core::audiobookshelf::{AudiobookshelfAccount, AudiobookshelfTokens};
    use std::sync::{Arc, atomic::{AtomicBool, Ordering}, mpsc::Receiver};
    use std::io::{Read, Write};

    struct ControlledOutput(Arc<std::sync::Mutex<crate::audio::OutputStatus>>);
    impl crate::audio::AudioOutput for ControlledOutput {
        fn load(&mut self, item: MediaItem, start: f64, autoplay: bool) {
            *self.0.lock().unwrap() = crate::audio::OutputStatus { item_id: Some(item.id), position: start, playing: autoplay, buffering: autoplay, ready: false, ..Default::default() };
        }
        fn set_next(&mut self, _: Option<MediaItem>) {}
        fn play(&mut self) { self.0.lock().unwrap().playing = true; }
        fn pause(&mut self) { let mut s = self.0.lock().unwrap(); s.playing = false; s.buffering = false; }
        fn stop(&mut self) { *self.0.lock().unwrap() = Default::default(); }
        fn seek(&mut self, target: f64) { let mut s = self.0.lock().unwrap(); s.position = target; s.ready = false; }
        fn set_volume(&mut self, _: f32) {}
        fn set_rate(&mut self, _: f32) {}
        fn set_transitions(&mut self, _: crate::audio::TransitionSettings) {}
        fn set_effects(&mut self, _: &discophile_core::audio_effects::AudioEffectsSettings) {}
        fn status(&self) -> crate::audio::OutputStatus { self.0.lock().unwrap().clone() }
        fn poll_events(&mut self) -> Vec<AudioEvent> { vec![] }
    }

    #[test]
    fn readiness_keeps_seek_until_playing_and_pause_cancels_autoplay() {
        let f = Fixture::new();
        let (mut app, updates) = setup(&f);
        load_track(&mut app, 30.0, true);
        pump(&mut app, &updates);
        let status = Arc::new(std::sync::Mutex::new(app.player.output.status()));
        app.player.output = Box::new(ControlledOutput(status.clone()));
        // An intermediate paused event and stale zero must not finish startup, even after 8s.
        app.audiobooks.playback.as_mut().unwrap().settling = Some((30.0, now_millis() - 8_000));
        { let mut s = status.lock().unwrap(); s.position = 0.0; s.playing = false; s.ready = false; s.buffering = false; }
        tick(&mut app);
        assert_eq!(app.audiobooks.playback.as_ref().unwrap().position, 30.0);
        assert!(app.player.status.buffering);
        { let mut s = status.lock().unwrap(); s.position = 30.0; s.ready = true; }
        tick(&mut app);
        assert!(app.player.status.buffering);
        status.lock().unwrap().playing = true;
        tick(&mut app);
        assert!(!app.player.status.buffering);
        assert!(app.player.status.playing);
        seek(&mut app, 40.0);
        { let mut s = status.lock().unwrap(); s.position = 0.0; s.ready = false; s.playing = false; }
        pause(&mut app);
        tick(&mut app);
        assert_eq!(app.audiobooks.playback.as_ref().unwrap().position, 40.0);
        assert!(!app.player.status.buffering);
        assert!(!app.player.status.playing);
        { let mut s = status.lock().unwrap(); s.position = 40.0; s.ready = true; }
        tick(&mut app);
        assert!(!app.player.status.playing);
    }

    #[test]
    fn a_new_seek_supersedes_startup_and_stop_invalidates_completion() {
        let f = Fixture::new();
        let (mut app, updates) = setup(&f);
        let status = Arc::new(std::sync::Mutex::new(app.player.output.status()));
        app.player.output = Box::new(ControlledOutput(status.clone()));
        seek(&mut app, 40.0);
        pump(&mut app, &updates);
        let current = app.audiobooks.playback.as_ref().unwrap().item_id();
        assert_eq!(app.audiobooks.playback.as_ref().unwrap().position, 40.0);
        { let mut s = status.lock().unwrap(); s.position = 0.0; s.ready = false; s.playing = false; }
        tick(&mut app);
        assert_eq!(app.audiobooks.playback.as_ref().unwrap().position, 40.0);
        assert!(app.player.status.buffering);
        { let mut s = status.lock().unwrap(); s.position = 48.0; s.ready = true; s.playing = true; s.buffering = false; }
        tick(&mut app);
        assert!(!app.player.status.buffering);
        assert_eq!(app.audiobooks.playback.as_ref().unwrap().position, 48.0);
        load_track(&mut app, 30.0, true);
        player::stop(&mut app);
        while let Ok(update) = updates.recv_timeout(std::time::Duration::from_millis(100)) { update(&mut app); }
        assert!(app.audiobooks.playback.is_none());
        assert!(app.player.output.status().item_id.is_none());
        on_output_event(&mut app, AudioEvent::Ended { item_id: current });
        assert_eq!(app.player.mode, Mode::Idle);
    }

    #[test]
    fn canceled_or_superseded_sessions_cannot_start_and_replaced_books_ignore_old_urls() {
        let f = Fixture::new();
        let (mut app, updates) = setup(&f);
        let pb = app.audiobooks.playback.as_ref().unwrap();
        let (book, session, account, connection) = (pb.book.clone(), pb.session.clone(), pb.account.clone(), pb.connection);
        player::stop(&mut app);
        let request = super::super::sync::StartRequest { account: account.clone(), connection, book: book.clone(), position: Some(30.0), generation: 100, autoplay: true };
        app.audiobooks.starting = Some((book.id.clone(), 100));
        player::pause(&mut app);
        prepared(&mut app, request.clone(), None, Ok((None, session.clone())));
        assert!(app.audiobooks.playback.is_none());
        app.audiobooks.starting = Some((book.id.clone(), 101));
        prepared(&mut app, request.clone(), None, Ok((None, session.clone())));
        assert!(app.audiobooks.playback.is_none());
        let mut latest = request;
        latest.generation = 101;
        latest.position = Some(40.0);
        prepared(&mut app, latest, None, Ok((None, session.clone())));
        pump(&mut app, &updates);
        assert_eq!(app.audiobooks.playback.as_ref().unwrap().position, 40.0);
        load_track(&mut app, 30.0, true);
        let old = app.audiobooks.playback.as_ref().unwrap().item_id();
        let mut other = book;
        other.id = "different-book".into();
        // The server may reuse a session id: identity must still differ across Playback instances.
        begin(&mut app, other, session, account, connection, false, 40.0, true);
        pump(&mut app, &updates);
        let pb = app.audiobooks.playback.as_ref().unwrap();
        assert_eq!(pb.book.id, "different-book");
        assert_eq!(pb.position, 40.0);
        assert_ne!(pb.item_id(), old);
        assert_eq!(app.player.output.status().item_id, Some(pb.item_id()));
    }

    struct Fixture {
        origin: url::Url,
        expired: Arc<AtomicBool>,
        stop: Arc<AtomicBool>,
        server: Option<std::thread::JoinHandle<()>>,
    }
    impl Fixture {
        fn new() -> Self {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let origin = url::Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
            let expired = Arc::new(AtomicBool::new(false));
            let flag = expired.clone();
            let stop = Arc::new(AtomicBool::new(false));
            let done = stop.clone();
            let server = std::thread::spawn(move || {
                while !done.load(Ordering::SeqCst) {
                    let Ok((mut socket, _)) = listener.accept() else { std::thread::sleep(std::time::Duration::from_millis(2)); continue };
                    socket.set_read_timeout(Some(std::time::Duration::from_secs(3))).unwrap();
                    let mut request = Vec::new();
                    loop {
                        let mut bytes = [0; 4096];
                        let n = socket.read(&mut bytes).unwrap_or(0);
                        if n == 0 { break; }
                        request.extend_from_slice(&bytes[..n]);
                        if request.windows(4).any(|w| w == b"\r\n\r\n") { break; }
                    }
                    let request = String::from_utf8_lossy(&request);
                    let (status, body) = if request.starts_with("POST /auth/refresh ") {
                        (200, r#"{"user":{"id":"user","username":"fixture","accessToken":"fresh","refreshToken":"refresh-new"}}"#)
                    } else if request.starts_with("GET /api/me ") && flag.load(Ordering::SeqCst) && request.to_ascii_lowercase().contains("authorization: bearer old") {
                        (401, "{}")
                    } else { (200, "{}") };
                    let _ = write!(socket, "HTTP/1.1 {status} Status\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                }
            });
            Self { origin, expired, stop, server: Some(server) }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            self.server.take().unwrap().join().unwrap();
        }
    }
    fn setup(f: &Fixture) -> (App, Receiver<crate::tasks::Update>) {
        let mut app = App::connect_test_app();
        let (tasks, updates) = crate::tasks::Tasks::new(egui::Context::default());
        app.tasks = tasks;
        let account = AudiobookshelfAccount::new(f.origin.clone(), "user", "fixture", None);
        app.audiobooks.client = Some(AudiobookshelfClient::new(account.clone(), AudiobookshelfTokens {
            access_token: "old".into(), refresh_token: Some("refresh-old".into()),
        }, |_, _| Ok(())));
        app.audiobooks.account = Some(account.clone());
        let book = serde_json::from_str(r#"{"id":"book","libraryId":"library","mediaType":"book","media":{"metadata":{"title":"Synthetic book"},"duration":180}}"#).unwrap();
        let session = serde_json::from_str(r#"{"id":"session","libraryItemId":"book","duration":180,"currentTime":0,"audioTracks":[{"startOffset":0,"duration":180,"contentUrl":"/api/items/book/file/1","mimeType":"audio/mpeg"}]}"#).unwrap();
        begin(&mut app, book, session, account.id(), account.connection_id, false, 75.0, true);
        pump(&mut app, &updates);
        (app, updates)
    }
    fn pump(app: &mut App, updates: &Receiver<crate::tasks::Update>) {
        while app.audiobooks.playback.as_ref().is_some_and(|pb| pb.resolving || pb.recovering) {
            updates.recv_timeout(std::time::Duration::from_secs(5)).expect("playback update")(app);
        }
    }
    #[test]
    fn expired_stream_recovers_at_its_position_and_respects_pause_during_refresh() {
        let f = Fixture::new();
        let (mut app, updates) = setup(&f);
        let old = app.audiobooks.playback.as_ref().unwrap().item_id();
        f.expired.store(true, Ordering::SeqCst);
        on_output_event(&mut app, AudioEvent::Failed { item_id: old.clone(), message: "synthetic 401".into() });
        pause(&mut app);
        pump(&mut app, &updates);
        let pb = app.audiobooks.playback.as_ref().unwrap();
        assert_eq!(pb.position, 75.0);
        assert_eq!(pb.stream_url.as_ref().unwrap().query(), Some("token=fresh"));
        assert!(!app.player.output.status().playing);
        assert!(app.player.error.is_none());
        on_output_event(&mut app, AudioEvent::Failed { item_id: old, message: "late failure".into() });
        assert!(!app.audiobooks.playback.as_ref().unwrap().recovering);
        let current = app.audiobooks.playback.as_ref().unwrap().item_id();
        on_output_event(&mut app, AudioEvent::Failed { item_id: current.clone(), message: "invalid format".into() });
        pump(&mut app, &updates);
        assert!(app.player.error.is_some());
        assert_eq!(app.audiobooks.playback.as_ref().unwrap().item_id(), current);
    }
    #[test]
    fn a_stalled_stream_renews_without_a_failure_event() {
        let f = Fixture::new();
        let (mut app, updates) = setup(&f);
        f.expired.store(true, Ordering::SeqCst);
        app.audiobooks.playback.as_mut().unwrap().buffering_since = Some(now_millis() - 6_000);
        let mut output = app.player.output.status();
        output.playing = false;
        output.buffering = true;
        app.player.output = Box::new(ControlledOutput(std::sync::Arc::new(std::sync::Mutex::new(output))));
        tick(&mut app);
        assert!(app.audiobooks.playback.as_ref().unwrap().recovering);
        pump(&mut app, &updates);
        let pb = app.audiobooks.playback.as_ref().unwrap();
        assert_eq!(pb.position, 75.0);
        assert_eq!(pb.stream_url.as_ref().unwrap().query(), Some("token=fresh"));
        assert!(app.player.output.status().playing);
        assert!(app.player.error.is_none());
    }
}
