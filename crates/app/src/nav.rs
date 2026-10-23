//! Navigation: a desktop sidebar selects a section; detail pages push onto a back/forward history.

use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Route {
    ListenNow,
    Statistics,
    /// Saved assistant selection (Ask your curator).
    CuratedSelection(Uuid),
    Search,
    Albums,
    Artists,
    Songs,
    Playlists,
    Favorites,
    Podcasts,
    Album(String),
    Artist(String),
    Playlist(String),
    Podcast(String),
    Audiobooks,
    Audiobook(String),
    Radio,
    Assistant,
    Settings(SettingsPage),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum SettingsPage {
    #[default]
    General,
    Accounts,
    Playback,
    Sound,
    Lyrics,
    Offline,
    Rediscover,
    Assistant,
    Agents,
    Advanced,
    About,
}

/// The panel at the right of the content, toggled from the player bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SidePanel {
    #[default]
    None,
    Queue,
    Lyrics,
    Assistant,
    /// About an artist, album, song, podcast or episode.
    Info,
}

pub struct Nav {
    back: Vec<Route>,
    forward: Vec<Route>,
    pub current: Route,
    pub side_panel: SidePanel,
    /// Full-window Now Playing view.
    pub now_playing: bool,
    /// Text in the sidebar search field; Search reads it.
    pub search_text: String,
    /// Set when the route changed, so pages can reset scroll or start loading.
    pub changed: bool,
}

impl Default for Nav {
    fn default() -> Self {
        Self {
            back: Vec::new(),
            forward: Vec::new(),
            current: Route::ListenNow,
            side_panel: SidePanel::None,
            now_playing: false,
            search_text: String::new(),
            changed: true,
        }
    }
}

impl Nav {
    /// Opens a page, keeping history for Back.
    pub fn push(&mut self, route: Route) {
        if route == self.current {
            return;
        }
        let previous = std::mem::replace(&mut self.current, route);
        self.back.push(previous);
        if self.back.len() > 100 {
            self.back.remove(0);
        }
        self.forward.clear();
        self.now_playing = false;
        self.changed = true;
    }

    /// Selects a top-level section from the sidebar, clearing detail history.
    pub fn select(&mut self, route: Route) {
        if route != self.current {
            self.back.push(std::mem::replace(&mut self.current, route));
            self.forward.clear();
            self.changed = true;
        }
        self.now_playing = false;
    }

    pub fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }
    pub fn can_go_forward(&self) -> bool {
        !self.forward.is_empty()
    }
    pub fn back(&mut self) {
        if let Some(route) = self.back.pop() {
            self.forward.push(std::mem::replace(&mut self.current, route));
            self.changed = true;
        }
    }
    pub fn forward(&mut self) {
        if let Some(route) = self.forward.pop() {
            self.back.push(std::mem::replace(&mut self.current, route));
            self.changed = true;
        }
    }

    /// Resets to a section after switching libraries; stale ids from the old library are dropped.
    pub fn reset(&mut self, route: Route) {
        self.back.clear();
        self.forward.clear();
        self.current = route;
        self.changed = true;
    }

    pub fn toggle_panel(&mut self, panel: SidePanel) {
        self.side_panel = if self.side_panel == panel { SidePanel::None } else { panel };
    }

    /// The sidebar entry that should look selected for the current page.
    pub fn section(&self) -> Route {
        match &self.current {
            Route::Album(_) => Route::Albums,
            Route::Artist(_) => Route::Artists,
            Route::Playlist(_) => Route::Playlists,
            Route::Podcast(_) => Route::Podcasts,
            Route::Audiobook(_) => Route::Audiobooks,
            Route::Statistics | Route::CuratedSelection(_) => Route::ListenNow,
            Route::Settings(_) => Route::Settings(SettingsPage::General),
            other => other.clone(),
        }
    }
}
