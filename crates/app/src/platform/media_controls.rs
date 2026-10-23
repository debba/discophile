//! System media controls: MPRIS on Linux, the System Media Transport Controls on Windows and Now
//! Playing on macOS (through souvlaki), and the Media Session API in the browser.
//!
//! Artwork URLs authenticate themselves, so on desktop the cover is downloaded into the cache and
//! published as a `file://` URL instead of exposing the server URL to other processes.

use crate::app::App;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MediaCommand {
    Play,
    Pause,
    Toggle,
    Next,
    Previous,
    Stop,
    /// Relative seek in seconds.
    SeekBy(f64),
    SetPosition(f64),
}

/// Seconds for "seek forward/backward" without an amount.
const DEFAULT_SEEK: f64 = 10.0;

/// What the system shows; published again only when it changes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Metadata {
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    /// An image URL the system can load (`file://` on desktop).
    pub cover_url: Option<String>,
    pub duration: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Playback {
    playing: bool,
    position: f64,
    at_millis: i64,
}

impl Playback {
    /// Where a system that extrapolates from the last report thinks playback is now.
    fn expected(&self, now: i64) -> f64 {
        if self.playing { self.position + (now - self.at_millis) as f64 / 1000.0 } else { self.position }
    }
}

pub struct MediaControls {
    backend: backend::Backend,
    metadata: Option<Metadata>,
    playback: Option<Playback>,
}

impl MediaControls {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Option<Self> {
        match backend::Backend::new(cc) {
            Ok(backend) => Some(Self { backend, metadata: None, playback: None }),
            Err(error) => {
                log::info!("System media controls unavailable: {error}");
                None
            }
        }
    }

    /// Commands received since the last call.
    pub fn poll(&mut self) -> Vec<MediaCommand> {
        self.backend.poll()
    }

    pub fn set_metadata(&mut self, metadata: Option<Metadata>) {
        if self.metadata == metadata {
            return;
        }
        self.backend.set_metadata(metadata.as_ref());
        self.metadata = metadata;
        // Systems reset the position with new metadata.
        self.playback = None;
    }

    /// `position` is ignored for live streams (`None`).
    pub fn set_playback(&mut self, playing: bool, position: Option<f64>) {
        let now = discophile_core::now_millis();
        let position = position.filter(|p| p.is_finite()).map(|p| p.max(0.0));
        let changed = match (&self.playback, position) {
            (None, _) => true,
            (Some(last), _) if last.playing != playing => true,
            (Some(last), Some(position)) => (last.expected(now) - position).abs() > 1.5,
            (Some(_), None) => false,
        };
        if !changed {
            return;
        }
        self.backend.set_playback(playing, position);
        self.playback = Some(Playback { playing, position: position.unwrap_or(0.0), at_millis: now });
    }

    pub fn clear(&mut self) {
        if self.metadata.is_some() || self.playback.is_some() {
            self.backend.set_metadata(None);
            self.backend.stop();
            self.metadata = None;
            self.playback = None;
        }
    }
}

/// Applies a command from the system to the player.
pub fn apply(app: &mut App, command: MediaCommand) {
    use crate::player;
    match command {
        MediaCommand::Play => player::play(app),
        MediaCommand::Pause => player::pause(app),
        MediaCommand::Toggle => player::toggle(app),
        MediaCommand::Next => player::next(app),
        MediaCommand::Previous => player::previous(app),
        MediaCommand::Stop => player::pause(app),
        MediaCommand::SeekBy(delta) => {
            let position = app.player.position() + delta;
            player::seek(app, position)
        }
        MediaCommand::SetPosition(position) => player::seek(app, position),
    }
}

/// Publishes the current item and state to the system when they change. Called every frame.
pub fn sync(app: &App, controls: &mut MediaControls) {
    use crate::player::Mode;
    if app.player.mode == Mode::Audiobook
        && let Some((title, author, position, duration)) = crate::features::audiobooks::now_playing(app) {
            let album = crate::features::audiobooks::current_chapter(app);
            controls.set_metadata(Some(Metadata { title, artist: Some(author), album, cover_url: None, duration: Some(duration) }));
            let status = &app.player.status;
            controls.set_playback(status.playing || status.buffering, Some(position));
            return;
        }
    let Some(song) = app.player.current() else {
        if app.player.mode == Mode::Idle {
            controls.clear();
        } else {
            let status = &app.player.status;
            controls.set_playback(status.playing || status.buffering, Some(app.player.position()));
        }
        return;
    };
    let live = app.player.is_live();
    let cover = song.cover_art.clone().filter(|c| !c.is_empty());
    let cover_url = cover.and_then(|cover| {
        let url = app.session.client.as_ref().map(|client| client.artwork_url(&cover, 600));
        backend::cover_url(app.ctx(), &cover, url)
    });
    let metadata = Metadata {
        title: song.title.clone(),
        artist: song.artist.clone(),
        album: song.album.clone(),
        cover_url,
        duration: if live { None } else { app.player.duration() },
    };
    controls.set_metadata(Some(metadata));
    let status = &app.player.status;
    controls.set_playback(status.playing || status.buffering, (!live).then(|| app.player.position()));
}

fn seek_command_from_direction(forward: bool, seconds: Option<f64>) -> MediaCommand {
    let seconds = seconds.filter(|s| s.is_finite() && *s > 0.0).unwrap_or(DEFAULT_SEEK);
    MediaCommand::SeekBy(if forward { seconds } else { -seconds })
}

#[cfg(not(target_arch = "wasm32"))]
mod backend {
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::Duration;

    use souvlaki::{MediaControlEvent, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig, SeekDirection};

    use super::{MediaCommand, Metadata, seek_command_from_direction};

    pub struct Backend {
        controls: souvlaki::MediaControls,
        commands: Arc<Mutex<Vec<MediaCommand>>>,
    }

    #[cfg(target_os = "windows")]
    fn window_handle(cc: &eframe::CreationContext<'_>) -> Option<*mut std::ffi::c_void> {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        match cc.window_handle().ok()?.as_raw() {
            RawWindowHandle::Win32(handle) => Some(handle.hwnd.get() as *mut std::ffi::c_void),
            _ => None,
        }
    }

    #[cfg(not(target_os = "windows"))]
    fn window_handle(_cc: &eframe::CreationContext<'_>) -> Option<*mut std::ffi::c_void> {
        None
    }

    impl Backend {
        pub fn new(cc: &eframe::CreationContext<'_>) -> Result<Self, String> {
            let hwnd = window_handle(cc);
            if cfg!(target_os = "windows") && hwnd.is_none() {
                return Err("no window handle for the Windows media controls".into());
            }
            let config = PlatformConfig { display_name: "Discophile", dbus_name: "discophile", hwnd };
            let mut controls = souvlaki::MediaControls::new(config).map_err(|e| format!("{e:?}"))?;
            let commands = Arc::new(Mutex::new(Vec::new()));
            let (sink, ctx) = (commands.clone(), cc.egui_ctx.clone());
            controls
                .attach(move |event| {
                    let command = match event {
                        MediaControlEvent::Play => Some(MediaCommand::Play),
                        MediaControlEvent::Pause => Some(MediaCommand::Pause),
                        MediaControlEvent::Toggle => Some(MediaCommand::Toggle),
                        MediaControlEvent::Next => Some(MediaCommand::Next),
                        MediaControlEvent::Previous => Some(MediaCommand::Previous),
                        MediaControlEvent::Stop => Some(MediaCommand::Stop),
                        MediaControlEvent::Seek(direction) => Some(seek_command_from_direction(direction == SeekDirection::Forward, None)),
                        MediaControlEvent::SeekBy(direction, amount) => {
                            Some(seek_command_from_direction(direction == SeekDirection::Forward, Some(amount.as_secs_f64())))
                        }
                        MediaControlEvent::SetPosition(MediaPosition(position)) => Some(MediaCommand::SetPosition(position.as_secs_f64())),
                        MediaControlEvent::Raise => {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                            None
                        }
                        MediaControlEvent::Quit => {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            None
                        }
                        MediaControlEvent::SetVolume(_) | MediaControlEvent::OpenUri(_) => None,
                    };
                    if let Some(command) = command {
                        sink.lock().unwrap_or_else(|e| e.into_inner()).push(command);
                        ctx.request_repaint();
                    }
                })
                .map_err(|e| format!("{e:?}"))?;
            Ok(Self { controls, commands })
        }

        pub fn poll(&mut self) -> Vec<MediaCommand> {
            std::mem::take(&mut *self.commands.lock().unwrap_or_else(|e| e.into_inner()))
        }

        pub fn set_metadata(&mut self, metadata: Option<&Metadata>) {
            let result = match metadata {
                Some(m) => self.controls.set_metadata(MediaMetadata {
                    title: Some(&m.title),
                    artist: m.artist.as_deref(),
                    album: m.album.as_deref(),
                    cover_url: m.cover_url.as_deref(),
                    duration: m.duration.filter(|d| d.is_finite() && *d > 0.0).map(Duration::from_secs_f64),
                }),
                None => self.controls.set_metadata(MediaMetadata::default()),
            };
            if let Err(error) = result {
                log::debug!("Media controls metadata: {error:?}");
            }
        }

        pub fn set_playback(&mut self, playing: bool, position: Option<f64>) {
            let progress = position.map(|p| MediaPosition(Duration::from_secs_f64(p)));
            let playback = if playing { MediaPlayback::Playing { progress } } else { MediaPlayback::Paused { progress } };
            if let Err(error) = self.controls.set_playback(playback) {
                log::debug!("Media controls playback: {error:?}");
            }
        }

        pub fn stop(&mut self) {
            let _ = self.controls.set_playback(MediaPlayback::Stopped);
        }
    }

    enum Cover {
        Loading,
        Ready(String),
        Missing,
    }

    fn covers() -> &'static Mutex<HashMap<String, Cover>> {
        static COVERS: OnceLock<Mutex<HashMap<String, Cover>>> = OnceLock::new();
        COVERS.get_or_init(Mutex::default)
    }

    fn cover_path(cover: &str) -> PathBuf {
        crate::platform::storage::cache_dir().join("media-controls").join(discophile_core::md5_hex(cover.as_bytes()))
    }

    /// `file:///home/...` on Unix; on Windows souvlaki strips `file://` and expects `C:\...` after it.
    fn file_url(path: &std::path::Path) -> String {
        format!("file://{}", path.display())
    }

    /// A `file://` URL of the cached cover, or `None` while it downloads (the interface repaints
    /// when it is ready).
    pub fn cover_url(ctx: &egui::Context, cover: &str, url: Option<url::Url>) -> Option<String> {
        let mut cache = covers().lock().unwrap_or_else(|e| e.into_inner());
        match cache.get(cover) {
            Some(Cover::Ready(file)) => return Some(file.clone()),
            Some(Cover::Loading | Cover::Missing) => return None,
            None => {}
        }
        let path = cover_path(cover);
        if path.is_file() {
            let file = file_url(&path);
            cache.insert(cover.to_string(), Cover::Ready(file.clone()));
            return Some(file);
        }
        let url = url?;
        if cache.len() > 256 {
            cache.retain(|_, c| matches!(c, Cover::Ready(_)));
        }
        cache.insert(cover.to_string(), Cover::Loading);
        drop(cache);
        let (cover, ctx) = (cover.to_string(), ctx.clone());
        crate::platform::spawn(async move {
            let saved = async {
                let response = discophile_core::http::server_client().get(url).send().await.ok()?;
                if !response.status().is_success() {
                    return None;
                }
                let bytes = response.bytes().await.ok()?;
                if bytes.is_empty() || bytes.len() > 8 * 1024 * 1024 {
                    return None;
                }
                let path = cover_path(&cover);
                tokio::fs::create_dir_all(path.parent()?).await.ok()?;
                tokio::fs::write(&path, &bytes).await.ok()?;
                Some(file_url(&path))
            }
            .await;
            let state = saved.map_or(Cover::Missing, Cover::Ready);
            covers().lock().unwrap_or_else(|e| e.into_inner()).insert(cover, state);
            ctx.request_repaint();
        });
        None
    }
}

#[cfg(target_arch = "wasm32")]
mod backend {
    //! `navigator.mediaSession` through reflection: web-sys only exposes it behind `web_sys_unstable_apis`.
    use std::cell::RefCell;
    use std::rc::Rc;

    use js_sys::{Array, Function, Object, Reflect};
    use wasm_bindgen::JsCast;
    use wasm_bindgen::prelude::*;

    use super::{MediaCommand, Metadata, seek_command_from_direction};

    pub struct Backend {
        session: JsValue,
        commands: Rc<RefCell<Vec<MediaCommand>>>,
        duration: Option<f64>,
        _handlers: Vec<Closure<dyn FnMut(JsValue)>>,
    }

    fn set(target: &JsValue, key: &str, value: &JsValue) {
        let _ = Reflect::set(target, &key.into(), value);
    }

    fn number(target: &JsValue, key: &str) -> Option<f64> {
        Reflect::get(target, &key.into()).ok()?.as_f64()
    }

    impl Backend {
        pub fn new(cc: &eframe::CreationContext<'_>) -> Result<Self, String> {
            let window = web_sys::window().ok_or("no window")?;
            let session = Reflect::get(&window.navigator(), &"mediaSession".into()).map_err(|e| format!("{e:?}"))?;
            if session.is_undefined() || session.is_null() {
                return Err("the Media Session API is not available".into());
            }
            let set_handler = Reflect::get(&session, &"setActionHandler".into())
                .ok()
                .and_then(|f| f.dyn_into::<Function>().ok())
                .ok_or("no setActionHandler")?;
            let commands = Rc::new(RefCell::new(Vec::new()));
            let mut handlers = Vec::new();
            let actions = ["play", "pause", "stop", "nexttrack", "previoustrack", "seekforward", "seekbackward", "seekto"];
            for action in actions {
                let (sink, ctx) = (commands.clone(), cc.egui_ctx.clone());
                let handler = Closure::<dyn FnMut(JsValue)>::new(move |details: JsValue| {
                    let command = match action {
                        "play" => MediaCommand::Play,
                        "pause" => MediaCommand::Pause,
                        "stop" => MediaCommand::Stop,
                        "nexttrack" => MediaCommand::Next,
                        "previoustrack" => MediaCommand::Previous,
                        "seekforward" => seek_command_from_direction(true, number(&details, "seekOffset")),
                        "seekbackward" => seek_command_from_direction(false, number(&details, "seekOffset")),
                        "seekto" => match number(&details, "seekTime") {
                            Some(time) => MediaCommand::SetPosition(time),
                            None => return,
                        },
                        _ => return,
                    };
                    sink.borrow_mut().push(command);
                    ctx.request_repaint();
                });
                // Browsers throw for actions they do not support; skip those.
                if set_handler.call2(&session, &action.into(), handler.as_ref().unchecked_ref()).is_ok() {
                    handlers.push(handler);
                }
            }
            Ok(Self { session, commands, duration: None, _handlers: handlers })
        }

        pub fn poll(&mut self) -> Vec<MediaCommand> {
            std::mem::take(&mut *self.commands.borrow_mut())
        }

        pub fn set_metadata(&mut self, metadata: Option<&Metadata>) {
            self.duration = metadata.and_then(|m| m.duration);
            let Some(m) = metadata else {
                set(&self.session, "metadata", &JsValue::NULL);
                return;
            };
            let Some(window) = web_sys::window() else { return };
            let Some(constructor) = Reflect::get(&window, &"MediaMetadata".into()).ok().and_then(|c| c.dyn_into::<Function>().ok()) else {
                return;
            };
            let init = Object::new();
            set(&init, "title", &m.title.as_str().into());
            set(&init, "artist", &m.artist.as_deref().unwrap_or("").into());
            set(&init, "album", &m.album.as_deref().unwrap_or("").into());
            if let Some(url) = &m.cover_url {
                let image = Object::new();
                set(&image, "src", &url.as_str().into());
                set(&image, "sizes", &"600x600".into());
                set(&init, "artwork", &Array::of1(&image));
            }
            if let Ok(value) = Reflect::construct(&constructor, &Array::of1(&init)) {
                set(&self.session, "metadata", &value);
            }
            self.set_position_state(0.0);
        }

        fn set_position_state(&self, position: f64) {
            let Some(function) = Reflect::get(&self.session, &"setPositionState".into()).ok().and_then(|f| f.dyn_into::<Function>().ok())
            else {
                return;
            };
            match self.duration.filter(|d| d.is_finite() && *d > 0.0) {
                Some(duration) => {
                    let state = Object::new();
                    set(&state, "duration", &duration.into());
                    set(&state, "position", &position.clamp(0.0, duration).into());
                    set(&state, "playbackRate", &1.0.into());
                    let _ = function.call1(&self.session, &state);
                }
                None => {
                    let _ = function.call0(&self.session);
                }
            }
        }

        pub fn set_playback(&mut self, playing: bool, position: Option<f64>) {
            set(&self.session, "playbackState", &(if playing { "playing" } else { "paused" }).into());
            if let Some(position) = position {
                self.set_position_state(position);
            }
        }

        pub fn stop(&mut self) {
            set(&self.session, "playbackState", &"none".into());
        }
    }

    /// The browser loads the artwork URL itself.
    pub fn cover_url(_ctx: &egui::Context, _cover: &str, url: Option<url::Url>) -> Option<String> {
        url.map(|u| u.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playback_extrapolates_while_playing() {
        let playback = Playback { playing: true, position: 10.0, at_millis: 1_000 };
        assert_eq!(playback.expected(3_000), 12.0);
        let paused = Playback { playing: false, ..playback };
        assert_eq!(paused.expected(3_000), 10.0);
    }

    #[test]
    fn seek_defaults() {
        assert_eq!(seek_command_from_direction(true, None), MediaCommand::SeekBy(DEFAULT_SEEK));
        assert_eq!(seek_command_from_direction(false, Some(30.0)), MediaCommand::SeekBy(-30.0));
    }
}
