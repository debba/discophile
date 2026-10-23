//! Audio details: the server file, how it reaches this computer, the decoded format, the output and the
//! active effects.

use discophile_core::audio_effects::ReplayGainMode;
use discophile_core::l10n::{self, t};

use crate::app::App;
use crate::nav::{Route, SettingsPage};
use crate::player::Mode;
use crate::theme::{self, Font};
use crate::ui::player_bar::controls::{decibels, sample_rate, value_row};

pub fn window(app: &mut App, ctx: &egui::Context) {
    let Some(song) = app.effects.details.clone() else { return };
    let mut open = true;
    let mut sound_settings = false;
    egui::Window::new(t("Audio details"))
        .id(egui::Id::unique("audio-details"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(400.0)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| {
            ui.set_width(400.0);
            let details = app.player.status.details.clone().unwrap_or_default();
            let music = app.player.mode == Mode::Music && !song.id.is_empty();
            if music {
                ui.label(theme::text(&song.title, Font::Title3));
                if let Some(artist) = &song.artist {
                    ui.label(theme::secondary(ui, artist, Font::Subheadline));
                }
                section(ui, &t("File on the server"));
                let format =
                    song.suffix.as_ref().map(|s| s.to_uppercase()).or_else(|| song.content_type.clone()).unwrap_or_else(|| t("Unknown"));
                value_row(ui, &t("Format"), &format);
                if let Some(rate) = song.bit_rate.filter(|r| *r > 0) {
                    value_row(ui, &t("Bit rate"), &format!("{rate} kbps"));
                }
                if let Some(rate) = song.sampling_rate.filter(|r| *r > 0) {
                    value_row(ui, &t("Sample rate"), &sample_rate(rate as f64));
                }
                if let Some(depth) = song.bit_depth.filter(|d| *d > 0) {
                    value_row(ui, &t("Bit depth"), &format!("{depth} bit"));
                }
                if let Some(channels) = song.channel_count.filter(|c| *c > 0) {
                    value_row(ui, &t("Channels"), &channels.to_string());
                }
            }
            section(ui, &t("Playback"));
            let local = music && crate::features::downloads::is_downloaded(app, &song) && !details.streamed;
            value_row(ui, &t("Source"), &t(if local { "Downloaded file" } else { "Streaming" }));
            if music {
                value_row(ui, &t("Audio sent"), &crate::ui::player_bar::actions::audio_sent(app, local));
            }
            let mut decoded = Vec::new();
            if let Some(codec) = &details.codec {
                decoded.push(codec.to_uppercase());
            }
            if let Some(rate) = details.sample_rate {
                decoded.push(sample_rate(rate as f64));
            }
            if let Some(depth) = details.bit_depth {
                decoded.push(format!("{depth} bit"));
            }
            if let Some(channels) = details.channels {
                decoded.push(l10n::count("%lld channels", channels as i64));
            }
            if let Some(rate) = details.bit_rate {
                decoded.push(format!("{} kbps", rate / 1000));
            }
            if !decoded.is_empty() {
                value_row(ui, &t("Decoded"), &decoded.join(" · "));
            }
            let mut output = Vec::new();
            if let Some(device) = &details.output_device {
                output.push(device.clone());
            }
            if let Some(rate) = details.output_sample_rate {
                output.push(sample_rate(rate as f64));
            }
            if !output.is_empty() {
                value_row(ui, &t("Output"), &output.join(" · "));
            }
            section(ui, &t("Sound"));
            let settings = &app.effects.settings;
            value_row(ui, &t("Equalizer"), &settings.equalizer_name());
            let replay = if settings.replay_gain == ReplayGainMode::Off {
                t("Off")
            } else if !music {
                t(settings.replay_gain.title())
            } else {
                let album = matches!(app.player.context, Some(crate::player::PlayContext::AlbumFocus { .. }));
                match settings.replay_gain_decibels(song.replay_gain.as_ref(), album) {
                    Some(value) => format!("{} · {}", t(settings.replay_gain.title()), decibels(value)),
                    None => t("No tags"),
                }
            };
            value_row(ui, &t("ReplayGain"), &replay);
            let limit = if settings.volume_limit == 0.0 { t("No limit") } else { decibels(settings.volume_limit) };
            value_row(ui, &t("Volume limit"), &limit);
            let transitions = app.settings.transitions();
            let crossfade = match transitions.crossfade {
                Some(seconds) => l10n::count("%lld seconds", seconds.round() as i64),
                None => t("Off"),
            };
            value_row(ui, &t("Crossfade"), &crossfade);
            value_row(ui, &t("Gapless playback"), &t(if transitions.gapless { "On" } else { "Off" }));
            ui.add_space(10.0);
            if crate::ui::widgets::action_button(ui, t("Sound settings"), true).clicked() {
                sound_settings = true;
            }
        });
    if sound_settings {
        app.nav.push(Route::Settings(SettingsPage::Sound));
        open = false;
    }
    if !open || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.effects.details = None;
    }
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(10.0);
    ui.label(theme::accent(ui, title, Font::Headline));
    ui.separator();
}
