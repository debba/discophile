//! The macOS menu bar (Discophile, File, Controls, Go, Window, Help) and reopening the window from the Dock.
//!
//! Item IDs arrive through `menu_events` and become [`MenuCommand`]s applied by the app. Transport items have
//! no key equivalents: Space and ⌘← / ⌘→ belong to text fields while typing, and the app already handles them
//! otherwise. Labels follow the interface language; the whole menu is rebuilt when it changes.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use discophile_core::l10n::{Language, t};
use discophile_core::queue::RepeatMode;
use tray_icon::menu::accelerator::{Accelerator, Code, Modifiers};
use tray_icon::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};

use crate::nav::Route;

use super::{GITHUB, WEBSITE};
pub const ISSUES: &str = "https://github.com/debba/discophile/issues";

#[derive(Clone, Debug, PartialEq)]
pub enum MenuCommand {
    About,
    Settings,
    Search,
    Quit,
    TogglePlayback,
    Next,
    Previous,
    Shuffle,
    Repeat(RepeatMode),
    VolumeUp,
    VolumeDown,
    NowPlaying,
    Queue,
    Lyrics,
    Assistant,
    Go(Route),
    Back,
    Forward,
    Open(&'static str),
}

/// What the menu reflects this frame.
pub struct MenuState {
    pub playing: bool,
    pub has_content: bool,
    pub can_skip: bool,
    pub music: bool,
    pub shuffled: bool,
    pub repeat: RepeatMode,
    pub has_library: bool,
    pub has_audiobooks: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
}

const SECTIONS: [(&str, &str, Code); 9] = [
    ("go.listen-now", "Listen Now", Code::Digit1),
    ("go.search", "Search", Code::Digit2),
    ("go.albums", "Albums", Code::Digit3),
    ("go.artists", "Artists", Code::Digit4),
    ("go.songs", "Songs", Code::Digit5),
    ("go.playlists", "Playlists", Code::Digit6),
    ("go.favorites", "Favorites", Code::Digit7),
    ("go.podcasts", "Podcasts", Code::Digit8),
    ("go.audiobooks", "Audiobooks", Code::Digit9),
];

fn section_route(id: &str) -> Option<Route> {
    Some(match id {
        "go.listen-now" => Route::ListenNow,
        "go.search" => Route::Search,
        "go.albums" => Route::Albums,
        "go.artists" => Route::Artists,
        "go.songs" => Route::Songs,
        "go.playlists" => Route::Playlists,
        "go.favorites" => Route::Favorites,
        "go.podcasts" => Route::Podcasts,
        "go.audiobooks" => Route::Audiobooks,
        "go.radio" => Route::Radio,
        _ => return None,
    })
}

/// The command for a menu item ID, or `None` for items owned by someone else (such as the tray).
pub fn command(id: &str) -> Option<MenuCommand> {
    if let Some(route) = section_route(id) {
        return Some(MenuCommand::Go(route));
    }
    Some(match id {
        "app.about" => MenuCommand::About,
        "app.settings" => MenuCommand::Settings,
        "app.quit" => MenuCommand::Quit,
        "file.search" => MenuCommand::Search,
        "controls.toggle" => MenuCommand::TogglePlayback,
        "controls.next" => MenuCommand::Next,
        "controls.previous" => MenuCommand::Previous,
        "controls.shuffle" => MenuCommand::Shuffle,
        "controls.repeat-off" => MenuCommand::Repeat(RepeatMode::Off),
        "controls.repeat-all" => MenuCommand::Repeat(RepeatMode::All),
        "controls.repeat-one" => MenuCommand::Repeat(RepeatMode::One),
        "controls.volume-up" => MenuCommand::VolumeUp,
        "controls.volume-down" => MenuCommand::VolumeDown,
        "view.now-playing" => MenuCommand::NowPlaying,
        "view.queue" => MenuCommand::Queue,
        "view.lyrics" => MenuCommand::Lyrics,
        "view.assistant" => MenuCommand::Assistant,
        "go.back" => MenuCommand::Back,
        "go.forward" => MenuCommand::Forward,
        "help.website" => MenuCommand::Open(WEBSITE),
        "help.github" => MenuCommand::Open(GITHUB),
        "help.issues" => MenuCommand::Open(ISSUES),
        _ => return None,
    })
}

pub struct MenuBar {
    _menu: Menu,
    language: Language,
    toggle: MenuItem,
    next: MenuItem,
    previous: MenuItem,
    shuffle: CheckMenuItem,
    repeat: [(RepeatMode, CheckMenuItem); 3],
    volume: [MenuItem; 2],
    library: Vec<MenuItem>,
    audiobooks: MenuItem,
    search: MenuItem,
    back: MenuItem,
    forward: MenuItem,
    music_views: [MenuItem; 2],
    assistant: MenuItem,
}

fn command_key(code: Code) -> Option<Accelerator> {
    Some(Accelerator::new(Modifiers::META, code))
}

impl MenuBar {
    /// Builds the menu bar and makes it the application's main menu.
    pub fn new(language: Language) -> Option<Self> {
        match Self::build(language) {
            Ok(bar) => Some(bar),
            Err(error) => {
                log::warn!("Menu bar unavailable: {error}");
                None
            }
        }
    }

    fn build(language: Language) -> Result<Self, tray_icon::menu::Error> {
        let item = |id: &str, label: &str, accelerator: Option<Accelerator>| MenuItem::with_id(id, t(label), true, accelerator);
        let separator = PredefinedMenuItem::separator;

        let about = item("app.about", "About Discophile", None);
        let settings = item("app.settings", "Settings…", command_key(Code::Comma));
        let quit = item("app.quit", "Quit Discophile", command_key(Code::KeyQ));
        let app_menu = Submenu::with_items(
            "Discophile",
            true,
            &[
                &about,
                &separator(),
                &settings,
                &separator(),
                &PredefinedMenuItem::services(Some(&t("Services"))),
                &separator(),
                &PredefinedMenuItem::hide(Some(&t("Hide Discophile"))),
                &PredefinedMenuItem::hide_others(Some(&t("Hide Others"))),
                &PredefinedMenuItem::show_all(Some(&t("Show All"))),
                &separator(),
                &quit,
            ],
        )?;

        let search = item("file.search", "Search", command_key(Code::KeyF));
        let file = Submenu::with_items(
            t("File"),
            true,
            &[&search, &separator(), &PredefinedMenuItem::close_window(Some(&t("Close Window")))],
        )?;

        let toggle = item("controls.toggle", "Play", None);
        let next = item("controls.next", "Next track", None);
        let previous = item("controls.previous", "Previous track", None);
        let shuffle = CheckMenuItem::with_id("controls.shuffle", t("Shuffle"), true, false, None);
        let repeat = [
            (RepeatMode::Off, CheckMenuItem::with_id("controls.repeat-off", t("Repeat off"), true, true, None)),
            (RepeatMode::All, CheckMenuItem::with_id("controls.repeat-all", t("Repeat all"), true, false, None)),
            (RepeatMode::One, CheckMenuItem::with_id("controls.repeat-one", t("Repeat one"), true, false, None)),
        ];
        let repeat_menu = Submenu::with_items(t("Repeat"), true, &[&repeat[0].1, &repeat[1].1, &repeat[2].1])?;
        let volume = [item("controls.volume-up", "Increase volume", None), item("controls.volume-down", "Decrease volume", None)];
        let now_playing = item("view.now-playing", "Now Playing", None);
        let controls = Submenu::with_items(
            t("Controls"),
            true,
            &[
                &toggle,
                &next,
                &previous,
                &separator(),
                &shuffle,
                &repeat_menu,
                &separator(),
                &volume[0],
                &volume[1],
                &separator(),
                &now_playing,
            ],
        )?;

        let library: Vec<MenuItem> = SECTIONS
            .iter()
            .filter(|(id, _, _)| *id != "go.audiobooks")
            .map(|(id, label, code)| item(id, label, command_key(*code)))
            .collect();
        let audiobooks = item("go.audiobooks", "Audiobooks", command_key(Code::Digit9));
        let radio = item("go.radio", "Radio", command_key(Code::Digit0));
        let back = item("go.back", "Back", command_key(Code::BracketLeft));
        let forward = item("go.forward", "Forward", command_key(Code::BracketRight));
        let music_views = [item("view.queue", "Playing next", None), item("view.lyrics", "Lyrics", None)];
        let assistant = item("view.assistant", "Music assistant", None);
        let go = Submenu::with_id(t("Go"), t("Go"), true);
        for entry in &library {
            go.append(entry)?;
        }
        go.append_items(&[
            &audiobooks,
            &radio,
            &separator(),
            &music_views[0],
            &music_views[1],
            &assistant,
            &separator(),
            &back,
            &forward,
        ])?;

        let window = Submenu::with_items(
            t("Window"),
            true,
            &[
                &PredefinedMenuItem::minimize(Some(&t("Minimize"))),
                &PredefinedMenuItem::maximize(Some(&t("Zoom"))),
                &PredefinedMenuItem::fullscreen(Some(&t("Enter Full Screen"))),
                &separator(),
                &PredefinedMenuItem::bring_all_to_front(Some(&t("Bring All to Front"))),
            ],
        )?;
        let help = Submenu::with_items(
            t("Help"),
            true,
            &[
                &item("help.website", "Discophile website", None),
                &item("help.github", "Discophile on GitHub", None),
                &item("help.issues", "Report a problem", None),
            ],
        )?;

        let menu = Menu::new();
        menu.append_items(&[&app_menu, &file, &controls, &go, &window, &help])?;
        menu.init_for_nsapp();
        window.set_as_windows_menu_for_nsapp();
        help.set_as_help_menu_for_nsapp();
        Ok(Self {
            _menu: menu,
            language,
            toggle,
            next,
            previous,
            shuffle,
            repeat,
            volume,
            library,
            audiobooks,
            search,
            back,
            forward,
            music_views,
            assistant,
        })
    }

    /// Rebuilds for a new interface language and updates labels, checkmarks and enabled states.
    pub fn sync(bar: &mut Option<Self>, language: Language, state: &MenuState) {
        if bar.as_ref().is_some_and(|b| b.language != language) {
            *bar = None;
            *bar = Self::new(language);
        }
        let Some(bar) = bar else { return };
        let label = t(if state.playing { "Pause" } else { "Play" });
        if bar.toggle.text() != label {
            bar.toggle.set_text(label);
        }
        let set = |item: &MenuItem, enabled: bool| {
            if item.is_enabled() != enabled {
                item.set_enabled(enabled);
            }
        };
        set(&bar.toggle, state.has_content);
        set(&bar.next, state.can_skip);
        set(&bar.previous, state.can_skip);
        for item in &bar.volume {
            set(item, state.has_content);
        }
        for item in &bar.music_views {
            set(item, state.music);
        }
        for item in &bar.library {
            set(item, state.has_library);
        }
        set(&bar.search, state.has_library);
        set(&bar.assistant, state.has_library);
        set(&bar.audiobooks, state.has_audiobooks);
        set(&bar.back, state.can_go_back);
        set(&bar.forward, state.can_go_forward);
        if bar.shuffle.is_checked() != state.shuffled {
            bar.shuffle.set_checked(state.shuffled);
        }
        if bar.shuffle.is_enabled() != state.music {
            bar.shuffle.set_enabled(state.music);
        }
        for (mode, item) in &bar.repeat {
            if item.is_checked() != (*mode == state.repeat) {
                item.set_checked(*mode == state.repeat);
            }
            if item.is_enabled() != state.music {
                item.set_enabled(state.music);
            }
        }
    }
}

// MARK: Dock

static REOPEN: AtomicBool = AtomicBool::new(false);
static REOPEN_CTX: OnceLock<egui::Context> = OnceLock::new();

/// True once after the Dock icon was clicked while no window was visible.
pub fn take_reopen() -> bool {
    REOPEN.swap(false, Ordering::Relaxed)
}

/// AppKit asks the application delegate before reopening: with the window hidden in the tray, nothing
/// would happen. Winit's delegate does not implement the method, so it is added to its class here.
pub fn install_reopen_handler(ctx: &egui::Context) {
    use std::ffi::CString;

    use objc2::encode::Encode;
    use objc2::runtime::{AnyClass, AnyObject, Bool, Imp, Sel};
    use objc2::{MainThreadMarker, sel};
    use objc2_app_kit::NSApplication;

    unsafe extern "C-unwind" fn should_handle_reopen(_this: &AnyObject, _cmd: Sel, _app: *mut AnyObject, visible: Bool) -> Bool {
        if visible.as_bool() {
            // Default behavior, such as restoring a minimized window.
            return Bool::YES;
        }
        REOPEN.store(true, Ordering::Relaxed);
        if let Some(ctx) = REOPEN_CTX.get() {
            ctx.request_repaint();
        }
        Bool::NO
    }

    if REOPEN_CTX.set(ctx.clone()).is_err() {
        return;
    }
    let Some(mtm) = MainThreadMarker::new() else { return };
    let app = NSApplication::sharedApplication(mtm);
    let Some(delegate) = app.delegate() else { return };
    let object: &AnyObject = delegate.as_ref();
    let class: &AnyClass = object.class();
    let selector = sel!(applicationShouldHandleReopen:hasVisibleWindows:);
    let types = format!("{}{}{}{}{}", Bool::ENCODING, <&AnyObject>::ENCODING, Sel::ENCODING, <*mut AnyObject>::ENCODING, Bool::ENCODING);
    let Ok(types) = CString::new(types) else { return };
    type Handler = unsafe extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject, Bool) -> Bool;
    // SAFETY: the implementation matches the selector's signature and encoding; the class outlives the app.
    let added = unsafe {
        let imp: Imp = std::mem::transmute::<Handler, Imp>(should_handle_reopen);
        objc2::ffi::class_addMethod(class as *const AnyClass as *mut AnyClass, selector, imp, types.as_ptr())
    };
    if !added.as_bool() {
        log::warn!("The application delegate already handles reopening from the Dock");
    }
    // AppKit caches which delegate methods exist when the delegate is set.
    app.setDelegate(Some(&delegate));
}
