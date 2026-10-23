//! Plays files or URLs through the desktop audio engine, for manual testing.
//!
//! ```sh
//! cargo run -p discophile --example play -- first.flac [second.mp3] [--crossfade 5] [--rate 1.5] [--start 30] [--seek 60] [--radio]
//! ```
//!
//! The second item is handed over with `set_next`, so it starts gaplessly (or crossfades).
//! `--seek` jumps there two seconds after starting. Prints status twice a second and every event;
//! exits when playback ends or fails.

use std::time::Duration;

use discophile::audio::native::NativeOutput;
use discophile::audio::{AudioEvent, AudioOutput, AudioSource, ContentKind, MediaItem, TransitionSettings};

fn item(arg: &str, kind: ContentKind) -> MediaItem {
    let source = if arg.starts_with("http://") || arg.starts_with("https://") {
        AudioSource::Url(arg.to_string())
    } else {
        AudioSource::File(arg.into())
    };
    MediaItem { id: arg.to_string(), source, kind, duration_hint: None, replay_gain: None, album_key: None, server_format: None }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let mut paths = Vec::new();
    let (mut crossfade, mut rate, mut start, mut seek, mut kind) = (None, 1.0f32, 0.0f64, None, ContentKind::Music);
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().and_then(|v| v.parse::<f64>().ok());
        match arg.as_str() {
            "--crossfade" => crossfade = value().map(|v| v as f32),
            "--rate" => rate = value().unwrap_or(1.0) as f32,
            "--start" => start = value().unwrap_or(0.0),
            "--seek" => seek = value(),
            "--radio" => kind = ContentKind::Radio,
            _ => paths.push(arg),
        }
    }
    let Some(first) = paths.first() else {
        eprintln!(
            "usage: play <file-or-url> [next-file-or-url] [--crossfade SECONDS] [--rate SPEED] [--start SECONDS] [--seek SECONDS] [--radio]"
        );
        std::process::exit(2);
    };

    let ctx = egui::Context::default();
    let mut output = match NativeOutput::new(&ctx) {
        Ok(output) => output,
        Err(error) => {
            eprintln!("audio engine failed to start: {error}");
            std::process::exit(1);
        }
    };
    output.set_transitions(TransitionSettings { gapless: true, crossfade, keep_album_gapless: true });
    output.set_rate(rate);
    output.load(item(first, kind), start, true);
    if let Some(second) = paths.get(1) {
        output.set_next(Some(item(second, kind)));
    }

    let mut last_details = None;
    for tick in 0u64.. {
        std::thread::sleep(Duration::from_millis(500));
        if tick == 4
            && let Some(seconds) = seek
        {
            println!("seek to {seconds}");
            output.seek(seconds);
        }
        for event in output.poll_events() {
            println!("event: {event:?}");
            match event {
                AudioEvent::Ended { .. } => return,
                AudioEvent::Failed { .. } => std::process::exit(1),
                AudioEvent::Advanced { .. } => {}
            }
        }
        let status = output.status();
        if status.details != last_details {
            println!("details: {:?}", status.details);
            last_details = status.details.clone();
        }
        let duration = status.duration.map_or("live".to_string(), |d| format!("{d:.1}"));
        println!(
            "{} {:.1}/{} {}{}",
            status.item_id.as_deref().unwrap_or("-"),
            status.position,
            duration,
            if status.playing { "playing" } else { "paused" },
            if status.buffering { " (buffering)" } else { "" }
        );
    }
}
