use std::sync::mpsc::{self, Receiver};
#[cfg(target_os = "linux")]
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use discophile_core::l10n::t;
use tray_icon::menu::{Menu, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Show,
    Toggle,
    Previous,
    Next,
    Quit,
}

impl Command {
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "tray.show" => Some(Self::Show),
            "tray.toggle" => Some(Self::Toggle),
            "tray.previous" => Some(Self::Previous),
            "tray.next" => Some(Self::Next),
            "tray.quit" => Some(Self::Quit),
            _ => None,
        }
    }
}

pub struct Tray {
    _icon: TrayIcon,
    commands: Receiver<Command>,
    show: MenuItem,
    toggle: MenuItem,
    previous: MenuItem,
    next: MenuItem,
    quit: MenuItem,
    pub hidden: bool,
    #[cfg(target_os = "linux")]
    available: Arc<AtomicBool>,
    #[cfg(target_os = "linux")]
    watcher: tokio::task::JoinHandle<()>,
}

impl Tray {
    pub fn new(ctx: &egui::Context) -> Option<Self> {
        match Self::build(ctx) {
            Ok(tray) => Some(tray),
            Err(error) => {
                log::warn!("System tray unavailable: {error}");
                None
            }
        }
    }

    fn build(ctx: &egui::Context) -> Result<Self, Box<dyn std::error::Error>> {
        let show = MenuItem::with_id("tray.show", t("Open Discophile"), true, None);
        let toggle = MenuItem::with_id("tray.toggle", t("Play"), false, None);
        let previous = MenuItem::with_id("tray.previous", t("Previous track"), false, None);
        let next = MenuItem::with_id("tray.next", t("Next track"), false, None);
        let quit = MenuItem::with_id("tray.quit", t("Quit Discophile"), true, None);
        let menu = Menu::new();
        menu.append_items(&[&show, &PredefinedMenuItem::separator(), &toggle, &previous, &next, &PredefinedMenuItem::separator(), &quit])?;
        let builder = TrayIconBuilder::new().with_id("discophile").with_tooltip("Discophile");
        // The macOS menu bar expects a monochrome template that the system tints for light, dark and
        // highlighted states; other desktops show the full-colour app icon.
        #[cfg(target_os = "macos")]
        let builder = builder.with_icon_templated(template_icon()?);
        #[cfg(not(target_os = "macos"))]
        let builder = builder.with_icon(app_icon()?);
        let icon = builder
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .build()?;

        // Menu items arrive through `menu_events`; only icon clicks come through this channel.
        super::menu_events::install(ctx);
        let (send, commands) = mpsc::channel();
        let icon_ctx = ctx.clone();
        TrayIconEvent::set_event_handler(Some(move |event| {
            if matches!(event, TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }) {
                let _ = send.send(Command::Show);
                icon_ctx.request_repaint();
            }
        }));

        #[cfg(target_os = "linux")]
        let available = Arc::new(AtomicBool::new(false));
        #[cfg(target_os = "linux")]
        let watcher = {
            let (available, ctx) = (available.clone(), ctx.clone());
            super::runtime().spawn(async move {
                loop {
                    let present = tray_host_available().await;
                    if available.swap(present, Ordering::Relaxed) != present {
                        ctx.request_repaint();
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                }
            })
        };
        Ok(Self {
            _icon: icon,
            commands,
            show,
            toggle,
            previous,
            next,
            quit,
            hidden: false,
            #[cfg(target_os = "linux")]
            available,
            #[cfg(target_os = "linux")]
            watcher,
        })
    }

    pub fn available(&self) -> bool {
        #[cfg(target_os = "linux")]
        {
            self.available.load(Ordering::Relaxed)
        }
        #[cfg(not(target_os = "linux"))]
        {
            true
        }
    }

    /// Updates the menu and returns the commands chosen in it (`menu` holds this frame's menu item IDs).
    pub fn poll(&mut self, menu: &[String], playing: bool, has_content: bool, can_skip: bool) -> Vec<Command> {
        for (item, label) in [
            (&self.show, t("Open Discophile")),
            (&self.toggle, t(if playing { "Pause" } else { "Play" })),
            (&self.previous, t("Previous track")),
            (&self.next, t("Next track")),
            (&self.quit, t("Quit Discophile")),
        ] {
            if item.text() != label {
                item.set_text(label);
            }
        }
        for (item, enabled) in [(&self.toggle, has_content), (&self.previous, can_skip), (&self.next, can_skip)] {
            if item.is_enabled() != enabled {
                item.set_enabled(enabled);
            }
        }
        let mut commands: Vec<_> = self.commands.try_iter().collect();
        commands.extend(menu.iter().filter_map(|id| Command::from_id(id)));
        // A desktop extension can disappear while the window is hidden.
        if self.hidden && !self.available() {
            commands.push(Command::Show);
        }
        if commands.contains(&Command::Show) {
            self.hidden = false;
        }
        commands
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        #[cfg(target_os = "linux")]
        self.watcher.abort();
        TrayIconEvent::set_event_handler(None::<fn(TrayIconEvent)>);
    }
}

#[cfg(not(target_os = "macos"))]
fn app_icon() -> Result<Icon, Box<dyn std::error::Error>> {
    let image = image::load_from_memory(include_bytes!("../../assets/AppIcon.png"))?
        .resize(32, 32, image::imageops::FilterType::Lanczos3)
        .into_rgba8();
    Ok(Icon::from_rgba(image.into_raw(), 32, 32)?)
}

/// 44 px is the 22 pt menu bar height at 2x; only the alpha channel is drawn.
#[cfg(target_os = "macos")]
fn template_icon() -> Result<Icon, Box<dyn std::error::Error>> {
    const SIZE: u32 = 44;
    let image = crate::artwork::render_svg(include_bytes!("../../assets/TrayTemplate.svg"), SIZE)
        .ok_or("could not render the tray template")?;
    Ok(Icon::from_rgba(image.as_raw().to_vec(), SIZE, SIZE)?)
}

pub fn show_window(ctx: &egui::Context) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
}

#[cfg(target_os = "linux")]
async fn tray_host_available() -> bool {
    let query = async {
        let connection = zbus::Connection::session().await.ok()?;
        let proxy =
            zbus::Proxy::new(&connection, "org.kde.StatusNotifierWatcher", "/StatusNotifierWatcher", "org.kde.StatusNotifierWatcher")
                .await
                .ok()?;
        proxy.get_property::<bool>("IsStatusNotifierHostRegistered").await.ok()
    };
    tokio::time::timeout(std::time::Duration::from_secs(2), query).await.ok().flatten().unwrap_or(false)
}
