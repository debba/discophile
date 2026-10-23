#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info,wgpu=warn,naga=warn")).init();
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
    eframe::run_native("Discophile", options, Box::new(|cc| Ok(Box::new(discophile::App::new(cc)))))
}

#[cfg(target_arch = "wasm32")]
fn main() {
    discophile::start_web();
}
