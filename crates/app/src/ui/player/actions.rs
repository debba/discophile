//! Player actions shared by the player bar and Now Playing: favorites, mute, the sleep timer, speed and the
//! More menu.

use discophile_core::l10n::{self, t};
use discophile_core::{Song, StreamingQuality};
use egui::{Id, Ui};

use crate::app::App;
use crate::nav::{Route, SettingsPage, SidePanel};
use crate::player::{self, Mode, SleepTimer};
use crate::theme::icons;

pub fn is_favorite(app: &App, song: &Song) -> bool {
    crate::features::library::is_favorite(app, song)
}

/// Stars or unstars a song on the server (the library keeps the favorites list).
pub fn toggle_favorite(app: &mut App, song: &Song) {
    crate::features::library::toggle_favorite(app, song);
}

fn unmuted_id() -> Id {
    Id::unique("player-unmuted-volume")
}

pub fn is_muted(app: &App) -> bool {
    app.player.volume <= 0.001
}

/// Mutes, or restores the volume from before muting.
pub fn toggle_mute(app: &mut App) {
    if is_muted(app) {
        let previous = app.ctx().data(|d| d.get_temp::<f32>(unmuted_id())).filter(|v| *v > 0.01).unwrap_or(0.6);
        player::set_volume(app, previous);
    } else {
        let current = app.player.volume;
        app.ctx().data_mut(|d| d.insert_temp(unmuted_id(), current));
        player::set_volume(app, 0.0);
    }
}

pub fn volume_icon(volume: f32) -> &'static str {
    if volume <= 0.001 {
        icons::SPEAKER_SLASH
    } else if volume < 0.34 {
        icons::SPEAKER_NONE
    } else if volume < 0.67 {
        icons::SPEAKER_LOW
    } else {
        icons::SPEAKER_HIGH
    }
}

pub fn repeat_label(app: &App) -> String {
    use discophile_core::queue::RepeatMode;
    t(match player::repeat_mode(app) {
        RepeatMode::Off => "Repeat off",
        RepeatMode::All => "Repeat all",
        RepeatMode::One => "Repeat one",
    })
}

pub fn repeat_icon(app: &App) -> &'static str {
    if player::repeat_mode(app) == discophile_core::queue::RepeatMode::One { icons::REPEAT_ONCE } else { icons::REPEAT }
}

/// The countdown of a timed sleep timer as `m:ss`, or a short label for end of track.
pub fn sleep_caption(app: &App) -> Option<String> {
    match app.player.sleep {
        Some(SleepTimer::At(_)) => player::sleep_remaining(app).map(l10n::time),
        Some(SleepTimer::EndOfTrack) => Some(t("Track")),
        Some(SleepTimer::EndOfChapter) => Some(t("Chapter")),
        None => None,
    }
}

/// Sleep timer choices, as in the iOS player: off, presets, end of track and a custom duration.
pub fn sleep_menu(ui: &mut Ui, app: &mut App) {
    if let Some(caption) = sleep_caption(app) {
        ui.label(crate::theme::secondary(ui, l10n::tf("Pauses in %@", &[&caption]), crate::theme::Font::Footnote));
        ui.separator();
    }
    if ui.button(t("Off")).clicked() {
        player::set_sleep_minutes(app, None);
    }
    for minutes in [5u32, 10, 15, 30, 45, 60] {
        if ui.button(l10n::count("%lld minutes", minutes as i64)).clicked() {
            player::set_sleep_minutes(app, Some(minutes));
        }
    }
    if app.player.mode == Mode::Music {
        let selected = app.player.sleep == Some(SleepTimer::EndOfTrack);
        if ui.add(egui::Button::selectable(selected, t("End of track"))).clicked() {
            app.player.sleep = Some(SleepTimer::EndOfTrack);
        }
    }
    if ui.button(t("Custom timer")).clicked() {
        ui.ctx().data_mut(|d| d.insert_temp(custom_timer_id(), Some(20u32)));
    }
}

fn custom_timer_id() -> Id {
    Id::unique("player-custom-timer")
}

/// The Custom timer dialog, drawn every frame by the player bar.
pub fn custom_timer_window(app: &mut App, ctx: &egui::Context) {
    let Some(mut minutes) = ctx.data(|d| d.get_temp::<Option<u32>>(custom_timer_id())).flatten() else { return };
    let mut close = false;
    let modal = crate::ui::widgets::modal(ctx, Id::unique("custom-timer-modal")).show(ctx, |ui| {
        ui.set_width(340.0);
        ui.label(crate::theme::text(t("Custom timer"), crate::theme::Font::Title3));
        ui.add_space(8.0);
        ui.label(crate::theme::text(t("Pause playback after"), crate::theme::Font::Body));
        ui.add(egui::Slider::new(&mut minutes, 1..=180).custom_formatter(|v, _| l10n::count("%lld minutes", v as i64)));
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if crate::ui::widgets::action_button(ui, t("Start sleep timer"), false).clicked() {
                player::set_sleep_minutes(app, Some(minutes));
                close = true;
            }
            if crate::ui::widgets::action_button(ui, t("Cancel"), true).clicked() {
                close = true;
            }
        });
    });
    if close || modal.should_close() {
        ctx.data_mut(|d| d.remove::<Option<u32>>(custom_timer_id()));
    } else {
        ctx.data_mut(|d| d.insert_temp(custom_timer_id(), Some(minutes)));
    }
}

/// Podcast and audiobook speeds.
pub const SPEEDS: [f32; 9] = [0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0];

pub fn speed_label(rate: f32) -> String {
    let digits = if (rate * 10.0).fract().abs() > 0.01 {
        2
    } else if rate.fract().abs() > 0.01 {
        1
    } else {
        0
    };
    format!("{}×", crate::ui::player_bar::controls::decimal(rate as f64, digits))
}

/// Whether speed applies to what is playing (podcasts and audiobooks).
pub fn speed_applies(app: &App) -> bool {
    match app.player.mode {
        Mode::Audiobook => true,
        Mode::Music => app.player.current().is_some_and(|s| s.is_podcast()),
        _ => false,
    }
}

pub fn speed_menu(ui: &mut Ui, app: &mut App) {
    for rate in SPEEDS {
        let selected = (app.player.rate - rate).abs() < 0.01;
        if ui.add(egui::Button::selectable(selected, speed_label(rate))).clicked() {
            player::set_rate(app, rate);
        }
    }
}

/// The player's More menu. `song` is the current music or podcast item.
pub fn more_menu(ui: &mut Ui, app: &mut App, song: Option<&Song>) {
    ui.set_min_width(220.0);
    let music = song.filter(|s| !s.is_podcast() && app.player.mode == Mode::Music);
    if let Some(song) = music {
        let radio_active = crate::features::discovery::is_radio_active(app);
        let label = t(if radio_active { "Why this song?" } else { "Start personal radio" });
        if ui.button(format!("{}  {label}", icons::RADIO)).clicked() {
            // Discovery owns personal radio; while it plays, its window explains the current song.
            if radio_active {
                crate::features::discovery::open_radio_window(app);
            } else {
                crate::features::discovery::start_personal_radio(app, song.clone());
            }
        }
        if ui.button(format!("{}  {}", icons::QUOTES, t("Lyrics"))).clicked() {
            open_lyrics(app);
        }
    }
    if ui.button(format!("{}  {}", icons::SLIDERS, t("Equalizer and sound"))).clicked() {
        app.nav.push(Route::Settings(SettingsPage::Sound));
    }
    if let Some(song) = song.filter(|_| app.player.mode == Mode::Music) {
        if ui.button(format!("{}  {}", icons::WAVEFORM, t("Audio details"))).clicked() {
            app.effects.details = Some(song.clone());
        }
    } else if app.player.mode != Mode::Idle && ui.button(format!("{}  {}", icons::WAVEFORM, t("Audio details"))).clicked() {
        app.effects.details = app.player.current().or_else(|| Some(Song::new("", t("Audiobook"))));
    }
    if let Some(song) = song.filter(|_| app.player.mode == Mode::Music)
        && ui.button(format!("{}  {}", icons::INFO, t("About"))).clicked()
    {
        crate::features::info::about_song(app, song);
    }
    if ui.button(format!("{}  {}", icons::SPARKLE, t("Music assistant"))).clicked() {
        app.nav.now_playing = false;
        if app.nav.side_panel != SidePanel::Assistant {
            app.nav.toggle_panel(SidePanel::Assistant);
        }
    }
    if let Some(song) = music {
        ui.separator();
        let can_edit = app.session.client.is_some();
        if ui.add_enabled(can_edit, egui::Button::new(format!("{}  {}", icons::LIST_PLUS, t("Add to playlist")))).clicked() {
            crate::features::library::open_playlist_picker(app, vec![song.clone()]);
        }
        let starred = is_favorite(app, song);
        let label = t(if starred { "Remove from favorites" } else { "Add to favorites" });
        if ui.add_enabled(can_edit, egui::Button::new(format!("{}  {label}", icons::HEART))).clicked() {
            toggle_favorite(app, song);
        }
        download_item(ui, app, song);
        if let Some(album) = song.album_id.clone()
            && ui.button(format!("{}  {}", icons::DISC, t("Go to album"))).clicked()
        {
            app.nav.push(Route::Album(album));
        }
        if let Some(artist) = song.artist_id.clone()
            && ui.button(format!("{}  {}", icons::MICROPHONE_STAGE, t("Go to artist"))).clicked()
        {
            app.nav.push(Route::Artist(artist));
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn download_item(ui: &mut Ui, app: &mut App, song: &Song) {
    use crate::features::downloads::{self, DownloadState};
    let (glyph, label, enabled) = match downloads::download_state(app, song) {
        DownloadState::Downloaded { .. } => (icons::CHECK_CIRCLE, t("Downloaded"), false),
        DownloadState::Downloading(_) => (icons::STOP_CIRCLE, t("Cancel download"), true),
        DownloadState::Failed => (icons::ARROW_CLOCKWISE, t("Retry download"), true),
        DownloadState::NotDownloaded => (icons::DOWNLOAD_SIMPLE, t("Download"), true),
    };
    if ui.add_enabled(enabled, egui::Button::new(format!("{glyph}  {label}"))).clicked() {
        match downloads::download_state(app, song) {
            DownloadState::Downloading(_) => downloads::remove(app, song),
            _ => downloads::download(app, [song.clone()]),
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn download_item(_ui: &mut Ui, _app: &mut App, _song: &Song) {}

/// Shows lyrics: inline in Now Playing when it is open, otherwise in the side panel.
pub fn open_lyrics(app: &mut App) {
    if app.nav.now_playing {
        crate::ui::now_playing::show_inline(app, crate::ui::now_playing::Inline::Lyrics);
    } else if app.nav.side_panel != SidePanel::Lyrics {
        app.nav.toggle_panel(SidePanel::Lyrics);
    }
}

/// Title and subtitle for what plays now, in any mode.
pub fn now_playing_text(app: &App) -> Option<(String, String)> {
    match app.player.mode {
        Mode::Radio => app.player.radio.as_ref().map(|station| (station.display_name(), t("Live radio"))),
        Mode::Audiobook => crate::features::audiobooks::now_playing(app).map(|(title, author, _, _)| {
            let subtitle = crate::features::audiobooks::current_chapter(app).filter(|c| !c.is_empty()).unwrap_or(author);
            (title, subtitle)
        }),
        _ => app.player.current().map(|song| (song.title.clone(), song.artist.clone().unwrap_or_else(|| t("Unknown artist")))),
    }
}

/// What the server sends for a stream at the chosen quality, for Audio details.
pub fn audio_sent(app: &App, local: bool) -> String {
    if local {
        return t("Original file");
    }
    t(match app.settings.streaming_quality {
        StreamingQuality::Original => "Original file",
        StreamingQuality::High => "MP3 up to 320 kbps, converted by the server",
        StreamingQuality::Compact => "MP3 up to 128 kbps, converted by the server",
    })
}

/// Position and duration on the timeline: the whole book for audiobooks, otherwise the current item.
pub fn timeline(app: &App) -> (f64, Option<f64>) {
    if app.player.mode == Mode::Audiobook
        && let Some((_, _, position, duration)) = crate::features::audiobooks::now_playing(app)
    {
        return (position, Some(duration).filter(|d| d.is_finite() && *d > 0.0));
    }
    (app.player.position(), app.player.duration())
}

pub fn seek(app: &mut App, seconds: f64) {
    if app.player.mode == Mode::Audiobook { crate::features::audiobooks::seek(app, seconds) } else { player::seek(app, seconds) }
}

/// Play/pause; audiobooks also restart a finished book or reload a failed file.
pub fn toggle(app: &mut App) {
    if app.player.mode == Mode::Audiobook { crate::features::audiobooks::toggle(app) } else { player::toggle(app) }
}
