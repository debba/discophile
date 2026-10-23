//! AutoEq headphone corrections: the index (downloaded only when the browser opens, cached for 30 days),
//! profile download, ParametricEQ import and the Choose headphones window.

use discophile_core::audio_effects::{AutoEqEntry, AutoEqProfile, EqualizerMode, auto_eq};
use discophile_core::l10n::t;
use egui::{CornerRadius, Margin, Sense};

use crate::app::App;
use crate::tasks::update;
use crate::theme::{self, Font, Palette, icons};
use crate::ui::widgets;

/// Downloads (or reads the cached) AutoEq index unless it is already loaded.
pub fn load_index(app: &mut App, force: bool) {
    let state = &mut app.effects;
    if state.loading_index || (!state.entries.is_empty() && !force) {
        return;
    }
    state.loading_index = true;
    state.error = None;
    app.tasks.spawn(async move {
        let cached = cache::read();
        let fresh = cached.as_ref().filter(|(_, saved)| auto_eq::index_is_fresh(*saved, discophile_core::now_millis()));
        let mut entries = Vec::new();
        if !force && let Some((text, _)) = fresh {
            entries = auto_eq::entries(text);
        }
        let mut failed = false;
        if entries.is_empty() {
            match auto_eq::fetch_index().await {
                Ok((text, parsed)) => {
                    cache::write(&text);
                    entries = parsed;
                }
                Err(error) => {
                    log::warn!("AutoEq index: {error}");
                    entries = cached.map(|(text, _)| auto_eq::entries(&text)).unwrap_or_default();
                    failed = entries.is_empty();
                }
            }
        }
        update(move |app| {
            let state = &mut app.effects;
            state.loading_index = false;
            state.entries = entries;
            state.results_query = None;
            if failed {
                state.error = Some(t("The AutoEq headphone list could not be downloaded. Check your connection and try again."));
            }
        })
    });
}

/// Results for the browser's query, recomputed only when the query or the index changes.
pub fn search(app: &mut App) -> &[AutoEqEntry] {
    let state = &mut app.effects;
    if state.results_query.as_deref() != Some(state.query.as_str()) {
        state.results = auto_eq::search(&state.entries, &state.query, auto_eq::SEARCH_LIMIT);
        state.results_query = Some(state.query.clone());
    }
    &app.effects.results
}

/// Downloads a headphone profile and makes it the active equalizer.
pub fn choose_profile(app: &mut App, entry: AutoEqEntry) {
    if app.effects.loading_profile.is_some() {
        return;
    }
    app.effects.loading_profile = Some(entry.path.clone());
    app.tasks.spawn(async move {
        let result = auto_eq::fetch_profile(&entry).await;
        update(move |app| {
            app.effects.loading_profile = None;
            match result {
                Ok(profile) => {
                    app.effects.settings.auto_eq = Some(profile);
                    app.effects.settings.equalizer = EqualizerMode::AutoEq;
                    app.effects.error = None;
                    app.effects.browser_open = false;
                    super::changed(app);
                }
                Err(_) => app.effects.error = Some(t("This AutoEq profile could not be loaded. Try again or choose another measurement.")),
            }
        })
    });
}

fn finish_import(app: &mut App, name: &str, text: Option<String>) {
    app.effects.importing = false;
    let profile = text.ok_or(()).and_then(|text| AutoEqProfile::import(name, &text, &t("Imported file")).map_err(|_| ()));
    match profile {
        Ok(profile) => {
            app.effects.settings.auto_eq = Some(profile);
            app.effects.settings.equalizer = EqualizerMode::AutoEq;
            app.effects.error = None;
            super::changed(app);
        }
        Err(()) => {
            app.effects.error = Some(t(
                "This file is not a valid ParametricEQ profile. Export Equalizer APO parametric settings from AutoEq and try again.",
            ))
        }
    }
}

/// Opens a file picker for an Equalizer APO `ParametricEQ.txt`.
#[cfg(not(target_arch = "wasm32"))]
pub fn import_profile(app: &mut App) {
    if app.effects.importing {
        return;
    }
    app.effects.importing = true;
    let tasks = app.tasks.clone();
    // The dialog runs its own event handling; a dedicated thread keeps it independent of the IO runtime.
    std::thread::spawn(move || {
        let file = futures::executor::block_on(rfd::AsyncFileDialog::new().add_filter("ParametricEQ", &["txt"]).pick_file());
        let Some(file) = file else { return tasks.send(update(|app| app.effects.importing = false)) };
        let name = file.file_name();
        let path = file.path().to_path_buf();
        let too_large = std::fs::metadata(&path).map(|m| m.len() as usize >= AutoEqProfile::MAX_IMPORT_BYTES).unwrap_or(true);
        let text = if too_large { None } else { std::fs::read(&path).ok().and_then(|bytes| String::from_utf8(bytes).ok()) };
        tasks.send(update(move |app| finish_import(app, &name, text)));
    });
}

/// Opens the browser's file picker through a detached file input.
#[cfg(target_arch = "wasm32")]
pub fn import_profile(app: &mut App) {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;
    let Some(document) = web_sys::window().and_then(|w| w.document()) else { return };
    let Some(input) = document.create_element("input").ok().and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok()) else { return };
    input.set_type("file");
    input.set_accept(".txt,text/plain");
    let tasks = app.tasks.clone();
    let target = input.clone();
    let on_change: Closure<dyn FnMut()> = Closure::once(move || {
        let Some(file) = target.files().and_then(|files| files.get(0)) else { return };
        let name = file.name();
        if file.size() >= AutoEqProfile::MAX_IMPORT_BYTES as f64 {
            tasks.send(update(move |app| finish_import(app, &name, None)));
            return;
        }
        let promise = file.text();
        wasm_bindgen_futures::spawn_local(async move {
            let text = wasm_bindgen_futures::JsFuture::from(promise).await.ok().and_then(|v| v.as_string());
            tasks.send(update(move |app| finish_import(app, &name, text)));
        });
    });
    input.set_onchange(Some(on_change.as_ref().unchecked_ref()));
    on_change.forget();
    input.click();
}

/// The raw index text and when it was saved.
mod cache {
    #[cfg(not(target_arch = "wasm32"))]
    fn path() -> std::path::PathBuf {
        crate::platform::storage::cache_dir().join("AutoEq-INDEX.md")
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn read() -> Option<(String, i64)> {
        let path = path();
        let text = std::fs::read_to_string(&path).ok()?;
        let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok()?;
        let age = modified.elapsed().map(|d| d.as_millis() as i64).unwrap_or(0);
        Some((text, discophile_core::now_millis() - age))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn write(text: &str) {
        let path = path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(error) = std::fs::write(&path, text) {
            log::warn!("Unable to cache the AutoEq index: {error}");
        }
    }

    #[cfg(target_arch = "wasm32")]
    #[derive(serde::Serialize, serde::Deserialize)]
    struct Cached {
        saved: i64,
        text: String,
    }

    #[cfg(target_arch = "wasm32")]
    pub fn read() -> Option<(String, i64)> {
        crate::platform::storage::load::<Cached>("autoeq-index").map(|c| (c.text, c.saved))
    }

    #[cfg(target_arch = "wasm32")]
    pub fn write(text: &str) {
        crate::platform::storage::save("autoeq-index", &Cached { saved: discophile_core::now_millis(), text: text.to_string() });
    }
}

/// Choose headphones: search the AutoEq index and pick a measurement.
pub fn browser_window(app: &mut App, ctx: &egui::Context) {
    if !app.effects.browser_open {
        return;
    }
    load_index(app, false);
    let mut open = true;
    egui::Window::new(t("Choose headphones"))
        .id(egui::Id::unique("autoeq-browser"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_size([460.0, 520.0])
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| browser(app, ui));
    if !open || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.effects.browser_open = false;
    }
}

fn browser(app: &mut App, ui: &mut egui::Ui) {
    let palette = Palette::of_ui(ui);
    let response = widgets::search_field(ui, &mut app.effects.query, &t("Headphone model"), ui.available_width());
    if app.effects.query.is_empty() && ui.ctx().memory(|m| m.focused().is_none()) {
        response.request_focus();
    }
    ui.add_space(6.0);
    if app.effects.loading_index {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(theme::secondary(ui, t("Downloading the AutoEq list…"), Font::Callout));
        });
    }
    if let Some(error) = app.effects.error.clone()
        && widgets::error_banner(ui, &error, true)
    {
        app.effects.error = None;
        load_index(app, true);
    }
    let query_empty = app.effects.query.trim().is_empty();
    let results = search(app).to_vec();
    let selected = app.effects.settings.auto_eq.as_ref().map(|p| (p.name.clone(), p.source.clone()));
    let busy = app.effects.loading_profile.clone();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for entry in &results {
            let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 46.0), Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(rect, CornerRadius::same(8), palette.hover);
            }
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(rect.shrink2(egui::vec2(10.0, 4.0)))
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if busy.as_deref() == Some(entry.path.as_str()) {
                    ui.spinner();
                } else if selected.as_ref().is_some_and(|(n, s)| *n == entry.name && *s == entry.source) {
                    ui.label(theme::icon(icons::CHECK, 16.0).color(palette.accent)).on_hover_text(t("Selected"));
                }
                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.add(egui::Label::new(theme::text(&entry.name, Font::Body)).truncate().selectable(false));
                    ui.add(egui::Label::new(theme::secondary(ui, &entry.source, Font::Footnote)).truncate().selectable(false));
                });
            });
            let label = format!("{}, {}", entry.name, entry.source);
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
            if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && busy.is_none() {
                choose_profile(app, entry.clone());
            }
        }
        if !app.effects.loading_index && !app.effects.entries.is_empty() {
            let message = if query_empty {
                Some(t("Search for your headphone or earphone model."))
            } else if results.is_empty() {
                Some(t("No matching headphones. Try the brand or a shorter model name."))
            } else {
                None
            };
            if let Some(message) = message {
                egui::Frame::new().inner_margin(Margin::same(8)).show(ui, |ui| {
                    ui.label(theme::secondary(ui, message, Font::Callout));
                });
            }
        }
    });
}
