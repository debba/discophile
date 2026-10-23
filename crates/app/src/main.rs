#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info,wgpu=warn,naga=warn")).init();
    // The Dock expects the rounded-square macOS grid. eframe hands these pixels to NSBitmapImageRep,
    // which reads them as premultiplied, as resvg renders them.
    #[cfg(target_os = "macos")]
    let icon = discophile::artwork::render_svg(include_bytes!("../assets/AppIcon-macOS.svg"), 512)
        .map(|image| egui::IconData { rgba: image.as_raw().to_vec(), width: 512, height: 512 });
    #[cfg(not(target_os = "macos"))]
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/AppIcon.png")).ok();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Discophile")
        .with_app_id("app.discophile.desktop")
        .with_inner_size([1280.0, 820.0])
        .with_min_inner_size([900.0, 600.0]);
    if let Some(icon) = icon {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    #[cfg(target_os = "linux")]
    let options = {
        let mut options = options;
        // Winit cannot hide Wayland windows; XWayland supports closing to the tray.
        if std::env::var_os("DISPLAY").is_some() && std::env::var("WINIT_UNIX_BACKEND").as_deref() != Ok("wayland") {
            options.event_loop_builder = Some(Box::new(|builder| {
                use winit::platform::x11::EventLoopBuilderExtX11;
                builder.with_x11();
            }));
        }
        options
    };
    eframe::run_native("Discophile", options, Box::new(|cc| Ok(Box::new(discophile::App::new(cc)))))
}

#[cfg(target_arch = "wasm32")]
fn main() {
    discophile::start_web();
}
