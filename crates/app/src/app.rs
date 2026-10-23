//! Application root: state, the per-frame loop and the desktop layout
//! (sidebar · content · optional side panel, with the player bar along the bottom).

use std::sync::mpsc;

use discophile_core::client::MusicClient;
use discophile_core::l10n;
use egui::{CornerRadius, Frame, Margin};

use crate::artwork::ArtworkCache;
use crate::features::{self, assistant, audiobooks, discovery, downloads, effects, library, lyrics, radio};
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
    pub media_controls: Option<crate::platform::media_controls::MediaControls>,
    dark: Option<bool>,
    /// Holds a sleep inhibitor while music plays with "Keep the computer awake" on.
    #[cfg(not(target_arch = "wasm32"))]
    awake: Option<keepawake::KeepAwake>,
}

impl App {
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
            media_controls,
            dark: None,
            #[cfg(not(target_arch = "wasm32"))]
            awake: None,
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
        let modal = egui::Modal::new(egui::Id::unique("alert")).show(ctx, |ui| {
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
                ui.label(theme::text(message, Font::Callout));
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
        self.update_keep_awake();
        let _ = ctx;
    }

    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        #[cfg(not(target_arch = "wasm32"))]
        crate::platform::window::handle_close(&ctx, self.settings.minimize_on_close);
        self.apply_appearance(&ctx);
        self.shortcuts(&ctx);
        let palette = Palette::of(&ctx);

        if !self.settings.introduction_completed {
            egui::CentralPanel::default().frame(Frame::new().fill(palette.background)).show(root, |ui| ui::welcome::introduction(self, ui));
        } else if !self.has_any_source() {
            egui::CentralPanel::default().frame(Frame::new().fill(palette.background)).show(root, |ui| ui::welcome::welcome(self, ui));
        } else {
            egui::Panel::bottom("player-bar")
                .exact_size(96.0)
                .frame(Frame::new().fill(palette.surface).stroke(egui::Stroke::new(1.0, palette.separator)).inner_margin(Margin::symmetric(24, 14)))
                .show(root, |ui| ui::player_bar::show(self, ui));
            let collapsed = self.settings.sidebar_collapsed;
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
                        SidePanel::None => {}
                    });
            }
            egui::CentralPanel::default().frame(Frame::new().fill(palette.background)).show(root, |ui| {
                if self.nav.now_playing {
                    ui::now_playing::show(self, ui);
                } else {
                    ui::sidebar::toolbar(self, ui);
                    self.content(ui);
                }
            });
        }

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
