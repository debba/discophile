//! Internet radio: saved direct HTTP(S) audio streams, played by the shared player as live content.

use discophile_core::InternetRadioStation;
use discophile_core::l10n::{music_error, t};
use uuid::Uuid;

use crate::app::App;
use crate::platform::storage;

const STATIONS_KEY: &str = "radio/stations";

/// The add/edit station form.
#[derive(Clone, Default)]
pub struct StationForm {
    /// The station being edited; `None` adds a new one.
    pub id: Option<Uuid>,
    pub name: String,
    pub stream: String,
    pub allow_http: bool,
    pub error: Option<String>,
}

impl StationForm {
    pub fn editing(station: &InternetRadioStation) -> Self {
        Self {
            id: Some(station.id),
            name: station.name.clone(),
            stream: station.stream_url.to_string(),
            allow_http: station.allow_http,
            error: None,
        }
    }

    pub fn is_http(&self) -> bool {
        self.stream.trim().to_ascii_lowercase().starts_with("http:")
    }

    /// Playlist files and HLS streams, which the players here cannot open directly.
    pub fn looks_like_playlist(&self) -> bool {
        let path = url::Url::parse(self.stream.trim()).map(|u| u.path().to_ascii_lowercase()).unwrap_or_default();
        [".m3u", ".m3u8", ".pls", ".asx", ".xspf"].iter().any(|ext| path.ends_with(ext))
    }
}

#[derive(Default)]
pub struct RadioState {
    pub stations: Vec<InternetRadioStation>,
    pub error: Option<String>,
    /// The form embedded in the Add account sheet.
    pub form: StationForm,
    /// The station editor window (Radio page and Settings).
    pub editor: Option<StationForm>,
    /// Station waiting for removal confirmation.
    pub removing: Option<Uuid>,
}

pub fn start(app: &mut App) {
    if let Some(text) = storage::read(STATIONS_KEY) {
        match serde_json::from_str::<Vec<InternetRadioStation>>(&text) {
            Ok(stations) => app.radio.stations = stations,
            Err(_) => app.radio.error = Some(t("Saved radio stations could not be read.")),
        }
    }
}

pub fn tick(_app: &mut App) {}

pub fn windows(app: &mut App, ctx: &egui::Context) {
    crate::ui::radio::windows(app, ctx);
}

pub fn on_library_changed(_app: &mut App) {}

pub fn has_stations(app: &App) -> bool {
    !app.radio.stations.is_empty()
}

fn persist(app: &mut App, stations: &[InternetRadioStation]) {
    storage::save(STATIONS_KEY, &stations);
    app.radio.error = None;
}

/// Validates and saves a form. A playing station is renamed without restarting it.
pub fn save_form(app: &mut App, form: &mut StationForm) -> bool {
    let station = match InternetRadioStation::new(form.id, &form.name, &form.stream, form.allow_http) {
        Ok(station) => station,
        Err(error) => {
            form.error = Some(music_error(error));
            return false;
        }
    };
    let mut stations = app.radio.stations.clone();
    match stations.iter_mut().find(|s| s.id == station.id) {
        Some(existing) => *existing = station.clone(),
        None => stations.push(station.clone()),
    }
    persist(app, &stations);
    app.radio.stations = stations;
    if app.player.radio.as_ref().is_some_and(|s| s.id == station.id) {
        app.player.radio = Some(station);
    }
    form.error = None;
    true
}

pub fn remove(app: &mut App, id: Uuid) {
    let stations: Vec<InternetRadioStation> = app.radio.stations.iter().filter(|s| s.id != id).cloned().collect();
    persist(app, &stations);
    app.radio.stations = stations;
    if app.player.radio.as_ref().is_some_and(|s| s.id == id) {
        crate::player::stop(app);
    }
}

/// Plays a station, or pauses and resumes the one already playing.
pub fn play(app: &mut App, station: &InternetRadioStation) {
    if is_current(app, station.id) {
        crate::player::toggle(app);
    } else {
        crate::player::play_radio(app, station.clone());
    }
}

pub fn is_current(app: &App, id: Uuid) -> bool {
    app.player.mode == crate::player::Mode::Radio && app.player.radio.as_ref().is_some_and(|s| s.id == id)
}

/// The note under the stream URL field: what kind of URL works on this platform.
pub fn format_note() -> String {
    if crate::platform::IS_WEB {
        t("Use a direct audio stream URL, not a website or a playlist file. Available formats depend on your browser.")
    } else {
        t(
            "Use a direct audio stream URL (MP3, AAC, Ogg or FLAC), not a website. HLS streams and M3U or PLS playlist files are not supported on this computer.",
        )
    }
}
