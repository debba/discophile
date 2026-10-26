//! Application root: state, the per-frame loop and the desktop layout
//! (sidebar · content · optional side panel, with the player bar along the bottom).

use std::sync::mpsc;

use discophile_core::client::MusicClient;
use discophile_core::l10n;
use egui::{CornerRadius, Frame, Margin};

use crate::artwork::ArtworkCache;
use crate::features::{self, assistant, audiobooks, connect, discovery, downloads, effects, info, library, lyrics, radio};
use crate::nav::{Nav, Route, SettingsPage, SidePanel};
use crate::player::{self, Player};
use crate::session::Session;
use crate::settings::Settings;
use crate::tasks::{Tasks, Update};
use crate::theme::{self, Appearance, Font, Palette};
use crate::ui;

/// Sheets and dialogs owned by the shell.
#[derive(Default)]
pub struct Sheets {
    pub add_account: bool,
    /// Pre-selects a provider in Add account ("Connect Audiobookshelf" on the welcome screen).
    pub add_account_provider: Option<String>,
}

pub struct App {
    pub tasks: Tasks,
    updates: mpsc::Receiver<Update>,
    pub settings: Settings,
    pub session: Session,
    pub player: Player,
    pub artwork: ArtworkCache,
    pub nav: Nav,
    pub sheets: Sheets,
    /// "Something needs your attention" alert.
    pub alert: Option<String>,
    toast: Option<(String, f64)>,
    pub library: library::LibraryState,
    pub downloads: downloads::Downloads,
    pub discovery: discovery::DiscoveryState,
    pub audiobooks: audiobooks::AudiobooksState,
    pub radio: radio::RadioState,
    pub assistant: assistant::AssistantState,
    pub effects: effects::EffectsState,
    pub lyrics: lyrics::LyricsState,
    pub info: info::InfoState,
    /// The optional Discophile Connect account.
    pub connect: connect::ConnectState,
    #[cfg(not(target_arch = "wasm32"))]
    pub mcp: features::mcp::McpState,
    #[cfg(not(target_arch = "wasm32"))]
    pub software_update: features::software_update::UpdatesState,
    pub media_controls: Option<crate::platform::media_controls::MediaControls>,
    dark: Option<bool>,
    scale_applied: bool,
    /// Holds a sleep inhibitor while music plays with "Keep the computer awake" on.
    #[cfg(not(target_arch = "wasm32"))]
    awake: Option<keepawake::KeepAwake>,
    #[cfg(not(target_arch = "wasm32"))]
    pub tray: Option<crate::platform::tray::Tray>,
    #[cfg(target_os = "macos")]
    menu_bar: Option<crate::platform::menu_bar::MenuBar>,
    #[cfg(not(target_arch = "wasm32"))]
    pub can_hide_window: bool,
    #[cfg(not(target_arch = "wasm32"))]
    quitting: bool,
}

impl App {
    #[cfg(test)]
    pub(crate) fn connect_test_app() -> Self {
        let ctx = egui::Context::default();
        let (tasks, updates) = Tasks::new(ctx);
        Self {
            tasks,
            updates,
            settings: Settings::default(),
            session: Session::default(),
            player: Player::new(Box::new(crate::audio::NullOutput::default()), 1.0),
            artwork: ArtworkCache::default(),
            nav: Nav::default(),
            sheets: Sheets::default(),
            alert: None,
            toast: None,
            library: Default::default(),
            downloads: Default::default(),
            discovery: Default::default(),
            audiobooks: Default::default(),
            radio: Default::default(),
            assistant: Default::default(),
            effects: Default::default(),
            lyrics: Default::default(),
            info: Default::default(),
            connect: Default::default(),
            #[cfg(not(target_arch = "wasm32"))]
            mcp: Default::default(),
            #[cfg(not(target_arch = "wasm32"))]
            software_update: Default::default(),
            media_controls: None,
            dark: None,
            scale_applied: false,
            #[cfg(not(target_arch = "wasm32"))]
            awake: None,
            #[cfg(not(target_arch = "wasm32"))]
            tray: None,
            #[cfg(target_os = "macos")]
            menu_bar: None,
            #[cfg(not(target_arch = "wasm32"))]
            can_hide_window: false,
            #[cfg(not(target_arch = "wasm32"))]
            quitting: false,
        }
    }

    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let ctx = cc.egui_ctx.clone();
        theme::install_fonts(&ctx);
        let settings = Settings::load();
        l10n::set_language(settings.effective_language());
        let (tasks, updates) = Tasks::new(ctx.clone());
        let output = crate::audio::create_output(&ctx);
        let player = Player::new(output, settings.volume);
        let media_controls = crate::platform::media_controls::MediaControls::new(cc);
        let mut app = Self {
            tasks,
            updates,
            session: Session::load(),
            player,
            settings,
            artwork: ArtworkCache::default(),
            nav: Nav::default(),
            sheets: Sheets::default(),
            alert: None,
            toast: None,
            library: Default::default(),
            downloads: Default::default(),
            discovery: Default::default(),
            audiobooks: Default::default(),
            radio: Default::default(),
            assistant: Default::default(),
            effects: Default::default(),
            lyrics: Default::default(),
            info: Default::default(),
            connect: Default::default(),
            #[cfg(not(target_arch = "wasm32"))]
            mcp: Default::default(),
            #[cfg(not(target_arch = "wasm32"))]
            software_update: Default::default(),
            media_controls,
            dark: None,
            scale_applied: false,
            #[cfg(not(target_arch = "wasm32"))]
            awake: None,
            #[cfg(not(target_arch = "wasm32"))]
            tray: crate::platform::tray::Tray::new(&ctx),
            #[cfg(target_os = "macos")]
            menu_bar: {
                crate::platform::menu_events::install(&ctx);
                crate::platform::menu_bar::install_reopen_handler(&ctx);
                crate::platform::menu_bar::MenuBar::new(l10n::language())
            },
            #[cfg(not(target_arch = "wasm32"))]
            can_hide_window: crate::platform::window::can_hide(cc),
            #[cfg(not(target_arch = "wasm32"))]
            quitting: false,
        };
        app.player.output.set_transitions(app.settings.transitions());
        features::start(&mut app);
        player::restore(&mut app);
        if !app.session.has_library() {
            app.nav.reset(if audiobooks::has_account(&app) { Route::Audiobooks } else { Route::Radio });
        }
        app
    }

    pub fn ctx(&self) -> &egui::Context {
        self.tasks.ctx()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn request_quit(&mut self) {
        self.quitting = true;
        self.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        self.ctx().request_repaint();
    }

    /// A clone of the active music client, for background tasks.
    pub fn client(&self) -> Option<MusicClient> {
        self.session.client.clone()
    }

    /// Whether any listening source is configured.
    pub fn has_any_source(&self) -> bool {
        self.session.has_library() || audiobooks::has_account(self) || radio::has_stations(self)
    }

    /// Artwork from the active music library, preferring downloaded artwork.
    pub fn artwork_texture(&mut self, cover: &str) -> Option<egui::TextureHandle> {
        let client = self.session.client.as_ref()?;
        let key = format!("{}:{cover}", self.session.account_key());
        let url = client.artwork_url(cover, 600);
        #[cfg(not(target_arch = "wasm32"))]
        let local = downloads::local_artwork(self, cover);
        #[cfg(target_arch = "wasm32")]
        let local = None;
        self.artwork.texture(&self.tasks, &key, Some(url), local)
    }

    /// Shows the attention alert.
    pub fn show_error(&mut self, message: impl Into<String>) {
        self.alert = Some(message.into());
    }

    /// A short confirmation at the bottom of the window.
    pub fn toast(&mut self, message: impl Into<String>) {
        let now = self.ctx().input(|i| i.time);
        self.toast = Some((message.into(), now + 3.0));
    }

    /// Called after the active library changes or is removed.
    pub fn on_library_changed(&mut self) {
        self.session.generation += 1;
        if self.player.mode != player::Mode::Audiobook && self.player.mode != player::Mode::Radio {
            player::stop(self);
            self.player.queue = Default::default();
        }
        self.nav.reset(if self.session.has_library() {
            Route::ListenNow
        } else if audiobooks::has_account(self) {
            Route::Audiobooks
        } else {
            Route::Radio
        });
        features::on_library_changed(self);
        player::restore(self);
    }

    fn apply_appearance(&mut self, ctx: &egui::Context) {
        let dark = match self.settings.appearance {
            Appearance::Dark => true,
            Appearance::Light => false,
            Appearance::System => ctx.system_theme().is_none_or(|t| t == egui::Theme::Dark),
        };
        if self.dark != Some(dark) {
            self.dark = Some(dark);
            theme::apply(ctx, dark);
            #[cfg(not(target_arch = "wasm32"))]
            ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(if dark { egui::SystemTheme::Dark } else { egui::SystemTheme::Light }));
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        #[cfg(not(target_arch = "wasm32"))]
        if ui::software_update::modal_open(self) { return; }
        let typing = ctx.egui_wants_keyboard_input();
        let (space, back, forward, find, settings) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::Space),
                (i.modifiers.alt && i.key_pressed(egui::Key::ArrowLeft)) || i.pointer.button_pressed(egui::PointerButton::Extra1),
                (i.modifiers.alt && i.key_pressed(egui::Key::ArrowRight)) || i.pointer.button_pressed(egui::PointerButton::Extra2),
                i.modifiers.command && i.key_pressed(egui::Key::F),
                i.modifiers.command && i.key_pressed(egui::Key::Comma),
            )
        });
        if space && !typing {
            player::toggle(self);
        }
        if back {
            if self.nav.now_playing { self.nav.now_playing = false } else { self.nav.back() }
        }
        if forward {
            self.nav.forward();
        }
        if find && self.session.has_library() {
            self.nav.select(Route::Search);
            ctx.memory_mut(|m| m.request_focus(egui::Id::unique("search-field")));
        }
        if settings {
            self.nav.push(Route::Settings(SettingsPage::General));
        }
        let (next, previous) = ctx.input(|i| {
            (i.modifiers.command && i.key_pressed(egui::Key::ArrowRight), i.modifiers.command && i.key_pressed(egui::Key::ArrowLeft))
        });
        if !typing && next {
            player::next(self);
        }
        if !typing && previous {
            player::previous(self);
        }
    }

    /// Applies the saved interface scale, and remembers changes made with Ctrl + / Ctrl −.
    fn sync_interface_scale(&mut self, ctx: &egui::Context) {
        let zoom = ctx.zoom_factor();
        let saved = self.settings.interface_scale.clamp(0.75, 2.0);
        if !self.scale_applied {
            self.scale_applied = true;
            if (zoom - saved).abs() > 0.001 {
                ctx.set_zoom_factor(saved);
            }
        } else if (zoom - saved).abs() > 0.001 {
            self.settings.interface_scale = zoom.clamp(0.75, 2.0);
            self.settings.save();
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn update_keep_awake(&mut self) {
        let wanted = self.settings.keep_awake && self.player.status.playing;
        if wanted && self.awake.is_none() {
            self.awake = keepawake::Builder::default()
                .display(true)
                .idle(true)
                .reason("Playing music")
                .app_name("Discophile")
                .app_reverse_domain("app.discophile.desktop")
                .create()
                .map_err(|error| log::warn!("Unable to keep the computer awake: {error}"))
                .ok();
        } else if !wanted {
            self.awake = None;
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn update_keep_awake(&mut self) {}

    fn sync_media_controls(&mut self) {
        let Some(mut controls) = self.media_controls.take() else { return };
        for command in controls.poll() {
            crate::platform::media_controls::apply(self, command);
        }
        crate::platform::media_controls::sync(self, &mut controls);
        self.media_controls = Some(controls);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn sync_tray(&mut self) {
        use crate::platform::tray::{Command, show_window};
        let playing = features::connect::services::remote_playback(self).map_or_else(|| self.player.is_playing(), |r| r.online && r.state.playing);
        let has_content = ui::player_bar::controls::has_content(self);
        let can_skip = features::connect::services::remote_playback(self).is_some() || matches!(self.player.mode, player::Mode::Music | player::Mode::Audiobook);
        let menu = crate::platform::menu_events::drain();
        let commands = self.tray.as_mut().map(|tray| tray.poll(&menu, playing, has_content, can_skip)).unwrap_or_default();
        #[cfg(target_os = "macos")]
        self.sync_menu_bar(&menu, playing, has_content, can_skip);
        for command in commands {
            match command {
                Command::Show => show_window(self.ctx()),
                Command::Toggle => player::toggle(self),
                Command::Previous => player::previous(self),
                Command::Next => player::next(self),
                Command::Quit => self.request_quit(),
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn sync_menu_bar(&mut self, menu: &[String], playing: bool, has_content: bool, can_skip: bool) {
        use crate::platform::menu_bar::{self, MenuBar, MenuCommand, MenuState};
        use crate::platform::tray::show_window;
        if menu_bar::take_reopen() {
            show_window(self.ctx());
            if let Some(tray) = &mut self.tray {
                tray.hidden = false;
            }
        }
        let state = MenuState {
            playing,
            has_content,
            can_skip,
            music: self.player.mode == player::Mode::Music,
            shuffled: self.player.shuffled,
            repeat: player::repeat_mode(self),
            has_library: self.session.has_library(),
            has_audiobooks: audiobooks::has_account(self),
            can_go_back: self.nav.can_go_back(),
            can_go_forward: self.nav.can_go_forward(),
            can_check_updates: self.software_update.status != features::software_update::UpdateStatus::Checking && !self.software_update.status.busy(),
        };
        MenuBar::sync(&mut self.menu_bar, self.settings.effective_language(), &state);
        for command in menu.iter().filter_map(|id| menu_bar::command(id)) {
            let shows_window = matches!(
                command,
                MenuCommand::About | MenuCommand::CheckForUpdates | MenuCommand::Settings | MenuCommand::Search | MenuCommand::Go(_) | MenuCommand::NowPlaying
                    | MenuCommand::Queue | MenuCommand::Lyrics | MenuCommand::Assistant
            );
            if shows_window {
                show_window(self.ctx());
                if let Some(tray) = &mut self.tray {
                    tray.hidden = false;
                }
            }
            match command {
                MenuCommand::About => self.nav.push(Route::Settings(SettingsPage::About)),
                MenuCommand::CheckForUpdates => {
                    self.nav.now_playing = false;
                    self.nav.push(Route::Settings(SettingsPage::About));
                    features::software_update::check_now(self);
                }
                MenuCommand::Settings => self.nav.push(Route::Settings(SettingsPage::General)),
                MenuCommand::Search | MenuCommand::Go(Route::Search) => {
                    self.nav.now_playing = false;
                    self.nav.select(Route::Search);
                    self.ctx().memory_mut(|m| m.request_focus(egui::Id::unique("search-field")));
                }
                MenuCommand::Go(route) => {
                    self.nav.now_playing = false;
                    self.nav.select(route);
                }
                MenuCommand::Quit => self.request_quit(),
                MenuCommand::TogglePlayback => player::toggle(self),
                MenuCommand::Next => player::next(self),
                MenuCommand::Previous => player::previous(self),
                MenuCommand::Shuffle => player::toggle_shuffle(self),
                MenuCommand::Repeat(mode) => {
                    for _ in 0..3 {
                        if player::repeat_mode(self) == mode {
                            break;
                        }
                        player::cycle_repeat(self);
                    }
                }
                MenuCommand::VolumeUp => player::set_volume(self, (ui::player_bar::actions::volume(self) + 0.1).min(1.0)),
                MenuCommand::VolumeDown => player::set_volume(self, (ui::player_bar::actions::volume(self) - 0.1).max(0.0)),
                MenuCommand::NowPlaying => self.nav.now_playing = has_content && !self.nav.now_playing,
                MenuCommand::Queue => {
                    if self.nav.now_playing {
                        ui::now_playing::show_inline(self, ui::now_playing::Inline::Queue);
                    } else {
                        self.nav.toggle_panel(SidePanel::Queue);
                    }
                }
                MenuCommand::Lyrics => ui::player_bar::actions::open_lyrics(self),
                MenuCommand::Assistant => assistant::open(self),
                MenuCommand::Back => {
                    if self.nav.now_playing { self.nav.now_playing = false } else { self.nav.back() }
                }
                MenuCommand::Forward => self.nav.forward(),
                MenuCommand::Open(url) => self.ctx().open_url(egui::OpenUrl::new_tab(url)),
            }
        }
    }

    fn content(&mut self, ui: &mut egui::Ui) {
        let route = self.nav.current.clone();
        match &route {
            Route::ListenNow => ui::listen_now::show(self, ui),
            Route::Statistics => ui::statistics::show(self, ui),
            Route::CuratedSelection(id) => ui::curator::show_selection(self, ui, *id),
            Route::Search => ui::search::show(self, ui),
            Route::Albums | Route::Artists | Route::Songs | Route::Playlists | Route::Favorites | Route::Podcasts => ui::library::show(self, ui, &route),
            Route::Album(id) => ui::detail::album(self, ui, id),
            Route::Artist(id) => ui::detail::artist(self, ui, id),
            Route::Playlist(id) => ui::detail::playlist(self, ui, id),
            Route::Podcast(id) => ui::detail::podcast(self, ui, id),
            Route::Audiobooks => ui::audiobooks::show(self, ui),
            Route::Audiobook(id) => ui::audiobooks::book(self, ui, id),
            Route::Radio => ui::radio::show(self, ui),
            Route::Assistant => ui::assistant::page(self, ui),
            Route::Settings(page) => ui::settings::show(self, ui, *page),
        }
        self.nav.changed = false;
    }

    fn alert_window(&mut self, ctx: &egui::Context) {
        if self.alert.is_none()
            && let Some(error) = self.player.error.take() {
                self.alert = Some(error);
            }
        let Some(message) = self.alert.clone() else { return };
        let modal = crate::ui::widgets::modal(ctx, egui::Id::unique("alert")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.label(theme::text(l10n::t("Something needs your attention"), Font::Title3));
            ui.add_space(6.0);
            ui.label(theme::text(message, Font::Body));
            ui.add_space(12.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui::widgets::action_button(ui, l10n::t("OK"), false).clicked())
                .inner
        });
        if modal.inner || modal.should_close() {
            self.alert = None;
        }
    }

    fn toast_overlay(&mut self, ctx: &egui::Context) {
        let Some((message, until)) = self.toast.clone() else { return };
        let now = ctx.input(|i| i.time);
        if now > until {
            self.toast = None;
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
        let palette = Palette::of(ctx);
        egui::Area::new(egui::Id::unique("toast")).anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -110.0)).interactable(false).show(ctx, |ui| {
            Frame::new().fill(palette.surface_raised).corner_radius(CornerRadius::same(14)).inner_margin(Margin::symmetric(16, 10)).shadow(egui::Shadow {
                offset: [0, 4],
                blur: 16,
                spread: 0,
                color: egui::Color32::from_black_alpha(60),
            }).show(ui, |ui| {
                ui.set_max_width((ctx.content_rect().width() - 80.0).max(100.0));
                let response = ui.add(egui::Label::new(theme::text(message, Font::Callout)).wrap());
                ctx.accesskit_node_builder(response.id, |node| {
                    node.set_role(egui::Role::Status);
                    node.set_live(egui::accesskit::Live::Polite);
                    node.set_live_atomic();
                });
            });
        });
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(update) = self.updates.try_recv() {
            update(self);
        }
        l10n::set_language(self.settings.effective_language());
        self.artwork.begin_frame();
        player::tick(self);
        features::tick(self);
        self.sync_media_controls();
        #[cfg(not(target_arch = "wasm32"))]
        self.sync_tray();
        self.update_keep_awake();
        self.sync_interface_scale(ctx);
    }

    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        #[cfg(not(target_arch = "wasm32"))]
        {
            let tray_available = self.can_hide_window && self.tray.as_ref().is_some_and(|tray| tray.available());
            if crate::platform::window::handle_close(&ctx, self.settings.minimize_on_close, tray_available, self.quitting)
                && let Some(tray) = &mut self.tray {
                    tray.hidden = true;
                }
        }
        self.apply_appearance(&ctx);
        self.shortcuts(&ctx);
        let palette = Palette::of(&ctx);

        let background = root.scope(|root| {
            #[cfg(not(target_arch = "wasm32"))]
            {
                let profile_toolbar = self.settings.introduction_completed
                    && !ui::welcome::choosing_account(&ctx)
                    && ((!self.has_any_source() && !matches!(self.nav.current, Route::Settings(_))) || self.nav.now_playing);
                if profile_toolbar || ((!self.settings.introduction_completed || !self.has_any_source() || self.nav.now_playing)
                    && self.software_update.available.is_some()) {
                    egui::Panel::top("account-toolbar")
                        .frame(Frame::new().fill(palette.background).inner_margin(Margin::symmetric(ui::widgets::PAGE_MARGIN as i8, 8)))
                        .show(root, |ui| {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if profile_toolbar {
                                    ui::connect::toolbar(self, ui);
                                    ui.add_space(ui::widgets::space::S);
                                }
                                ui::software_update::toolbar(self, ui);
                            });
                            ui::software_update::launch_notice(self, ui);
                        });
                }
            }

            if ui::welcome::choosing_account(&ctx) {
                egui::CentralPanel::default().frame(Frame::new().fill(palette.background)).show(root, |ui| ui::welcome::account_choice(self, ui));
            } else if !self.settings.introduction_completed {
                egui::CentralPanel::default().frame(Frame::new().fill(palette.background)).show(root, |ui| ui::welcome::introduction(self, ui));
            } else if !self.has_any_source() && !matches!(self.nav.current, Route::Settings(_)) {
                egui::CentralPanel::default().frame(Frame::new().fill(palette.background)).show(root, |ui| ui::welcome::welcome(self, ui));
            } else {
                egui::Panel::bottom("player-bar")
                    .exact_size(ui::player_bar::height(self))
                    .frame(Frame::new().fill(palette.surface).stroke(egui::Stroke::new(1.0, palette.separator)).inner_margin(Margin::symmetric(24, 14)))
                    .show(root, |ui| ui::player_bar::show(self, ui));
                let collapsed = ui::sidebar::collapsed(self);
                egui::Panel::left("sidebar")
                    .resizable(false)
                    .exact_size(if collapsed { 72.0 } else { 244.0 })
                    .frame(Frame::new().fill(palette.sidebar).inner_margin(Margin::symmetric(14, 20)))
                    .show(root, |ui| ui::sidebar::show(self, ui));
                if self.nav.side_panel != SidePanel::None && !self.nav.now_playing {
                    egui::Panel::right("side-panel")
                        .resizable(true)
                        .default_size(360.0)
                        .size_range(300.0..=560.0)
                        .frame(Frame::new().fill(palette.sidebar).inner_margin(Margin::symmetric(14, 12)))
                        .show(root, |ui| match self.nav.side_panel {
                            SidePanel::Queue => ui::queue::panel(self, ui),
                            SidePanel::Lyrics => ui::lyrics::panel(self, ui),
                            SidePanel::Assistant => ui::assistant::panel(self, ui),
                            SidePanel::Info => ui::info::panel(self, ui),
                            SidePanel::None => {}
                        });
                }
                egui::CentralPanel::default().frame(Frame::new().fill(palette.background)).show(root, |ui| {
                    if self.nav.now_playing {
                        ui::now_playing::show(self, ui);
                    } else {
                        ui::sidebar::toolbar(self, ui);
                        #[cfg(not(target_arch = "wasm32"))]
                        ui::software_update::launch_notice(self, ui);
                        self.content(ui);
                    }
                });
            }

        });
        #[cfg(not(target_arch = "wasm32"))]
        ui::software_update::background_accessibility(&ctx, background.response.id, ui::software_update::modal_open(self));
        #[cfg(target_arch = "wasm32")]
        let _ = background;

        features::windows(self, &ctx);
        ui::accounts::add_account_window(self, &ctx);
        self.alert_window(&ctx);
        self.toast_overlay(&ctx);
    }

    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        self.settings.save();
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.settings.save();
        if self.player.mode == player::Mode::Audiobook {
            audiobooks::on_replaced(self);
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(&mut *self)
    }
}
