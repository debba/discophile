//! Prepares a demo profile for the README screenshots against the servers in `scripts/screenshots`:
//! both accounts, playlists, favorites and two months of listening history. Run it with an isolated
//! data folder, for example:
//!
//! ```sh
//! DISCOPHILE_DATA_DIR=.screenshots/profile DISCOPHILE_SECRETS=file cargo run -p discophile --example demo_profile
//! ```

use chrono::{Duration, Local, TimeZone};
use discophile::platform::{secrets, storage};
use discophile_core::audiobookshelf::AudiobookshelfClient;
use discophile_core::client::MusicClient;
use discophile_core::listening_history::DiscoveryArchive;
use discophile_core::queue::PlaybackQueue;
use discophile_core::{Credentials, MusicProvider, ServerAccount, Song};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

const MUSIC: &str = "http://localhost:4542";
const BOOKS: &str = "http://localhost:13382";
const USER: &str = "demo";
const PASSWORD: &str = "demo-password";

/// Artists the history favors, most played first.
const FAVORITE_ARTISTS: [&str; 6] = ["Mina Sol", "Cherry Vale", "Kiri", "Isla Wren", "Nova Reyes", "Daisy Cole"];

const PLAYLISTS: [(&str, &[&str]); 5] = [
    ("Rainy Sunday", &["Kiri", "Isla Wren", "Ivy Hale", "June Ash", "Inés Mar"]),
    ("Late Night Drive", &["The Night Shift", "Lena Vale", "Gloria Veil", "Zara Moon"]),
    ("Summer Rotation", &["Mina Sol", "Daisy Cole", "Bailey Shore", "Palm Society", "Nova Reyes"]),
    ("Focus", &["Mirage Garden", "Delphine", "Theo Lane", "Desert Radio"]),
    ("Crate Finds", &["Cherry Vale", "Tavo Rey", "Mara Flor", "Jules Okafor", "Odell Kingsley"]),
];

fn main() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    runtime.block_on(run());
}

async fn run() {
    let credentials = Credentials {
        server: url::Url::parse(MUSIC).unwrap(),
        username: USER.into(),
        password: PASSWORD.into(),
        authentication: Default::default(),
        provider: MusicProvider::Navidrome,
        user_id: None,
        device_id: None,
    };
    let account = ServerAccount::new(credentials.clone(), "Home");
    let id = account.id();
    let mut stored = account.clone();
    stored.credentials.password.clear();
    storage::save("accounts", &serde_json::json!({ "accounts": [stored], "active": id }));
    secrets::set(&format!("account:{id}"), PASSWORD);

    let client = MusicClient::new(credentials);
    let mut songs: Vec<Song> = Vec::new();
    for album in client.albums("alphabeticalByName", 0, 500).await.expect("albums") {
        songs.extend(client.album(&album.id).await.expect("album").song.unwrap_or_default());
    }
    println!("{} songs", songs.len());
    let by_artist = |artist: &str| songs.iter().filter(|s| s.artist.as_deref() == Some(artist)).cloned().collect::<Vec<_>>();

    let mut rng = StdRng::seed_from_u64(7);
    if client.playlists().await.map(|p| p.is_empty()).unwrap_or(true) {
        for (name, artists) in PLAYLISTS {
            let mut picks: Vec<Song> = artists.iter().flat_map(|a| by_artist(a).into_iter().take(4)).collect();
            picks.sort_by_key(|_| rng.random::<u32>());
            client.create_playlist(name, &picks).await.expect("playlist");
        }
    }
    for song in by_artist("Mina Sol").iter().take(6).chain(by_artist("Kiri").iter().take(3)).chain(by_artist("Cherry Vale").iter().take(3)) {
        let _ = client.favorite(&song.id, true).await;
    }

    // Two months of evenings, weighted towards the favorite artists, ending with Discophile by Mina Sol.
    let mut archive = DiscoveryArchive::default();
    let now = Local::now();
    let mut queue = PlaybackQueue::new();
    for days_ago in (0..60).rev() {
        let sessions = if days_ago % 7 == 3 { 0 } else { rng.random_range(4..14) };
        let evening = Local.from_local_datetime(&(now - Duration::days(days_ago)).date_naive().and_hms_opt(19, 0, 0).unwrap()).unwrap();
        for n in 0..sessions {
            let song = if rng.random_bool(0.55) {
                let artist = FAVORITE_ARTISTS[(rng.random::<f64>().powi(2) * FAVORITE_ARTISTS.len() as f64) as usize];
                let pool = by_artist(artist);
                pool[rng.random_range(0..pool.len())].clone()
            } else {
                songs[rng.random_range(0..songs.len())].clone()
            };
            let duration = song.duration.unwrap_or(200.0);
            let when = evening + Duration::minutes(n as i64 * 4);
            if when > now {
                break;
            }
            queue.replace(vec![song.clone()], 0, false);
            archive.record_playback(&song, uuid::Uuid::new_v4(), duration, duration, duration, &queue, true, false, &when);
        }
    }
    let discophile: Vec<Song> = songs.iter().filter(|s| s.album.as_deref() == Some("Discophile")).cloned().collect();
    queue.replace(discophile.clone(), 0, false);
    let first = &discophile[0];
    archive.record_playback(first, uuid::Uuid::new_v4(), 14.0, 14.0, first.duration.unwrap_or(66.0), &queue, false, false, &now);
    storage::save(&format!("library/{id}/resume"), &serde_json::json!({ "queue": queue, "position": 14.0 }));
    archive.set_candidates(&songs, &[]);
    storage::save(&format!("library/{id}/discovery"), &archive);

    let login = AudiobookshelfClient::connect(BOOKS, USER, PASSWORD, true, "Audiobooks").await.expect("audiobookshelf");
    secrets::set("audiobookshelf-tokens", &serde_json::to_string(&login.tokens).unwrap());
    storage::save("audiobooks/account", &login.account);

    let mut settings: serde_json::Value = storage::load("settings").unwrap_or_else(|| serde_json::json!({}));
    settings["language"] = "English".into();
    settings["introduction_completed"] = true.into();
    settings["appearance"] = "Dark".into();
    settings["check_for_updates"] = false.into();
    storage::save("settings", &settings);
    println!("profile ready for {id}");
}
