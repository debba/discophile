//! Shared components that carry the iOS app's look to the desktop: rounded surfaces, pink accents,
//! large artwork and bold headings.

use discophile_core::{Album, Song, l10n};
use egui::{Align, Color32, CornerRadius, FontId, Layout, Margin, Pos2, Rect, Response, RichText, Sense, Stroke, Ui, Vec2, vec2};

use crate::app::App;
use crate::theme::{self, Font, Palette, icons};

pub const CARD_RADIUS: u8 = 12;
pub const PAGE_MARGIN: f32 = 32.0;

/// The spacing scale. Use these instead of ad-hoc numbers so rhythm stays consistent.
pub mod space {
    pub const XS: f32 = 4.0;
    pub const S: f32 = 8.0;
    pub const M: f32 = 12.0;
    pub const L: f32 = 16.0;
    pub const XL: f32 = 24.0;
    pub const XXL: f32 = 32.0;
}

/// Standard control heights: compact chips, regular fields and buttons, prominent actions.
pub const CONTROL_SMALL: f32 = 30.0;
pub const CONTROL: f32 = 36.0;
pub const CONTROL_LARGE: f32 = 44.0;

/// A modal dialog with sheet-like padding and corners. Use instead of `egui::Modal::new`, whose
/// default popup frame has menu-sized margins that crowd dialog content against the edges.
pub fn modal(ctx: &egui::Context, id: egui::Id) -> egui::Modal {
    let frame = egui::Frame::popup(&ctx.global_style()).inner_margin(Margin::same(24)).corner_radius(CornerRadius::same(18));
    egui::Modal::new(id).frame(frame)
}

/// Paints a glyph so its drawn shape, not its font box, is centered on `center`.
/// Icon glyphs are not centered within their advance width or line box, so text alignment alone
/// leaves chevrons and arrows visibly off-center inside round buttons.
pub fn paint_icon(painter: &egui::Painter, center: Pos2, glyph: &str, font: FontId, color: Color32) -> Rect {
    let galley = painter.layout_no_wrap(glyph.to_string(), font, color);
    let bounds = galley.mesh_bounds;
    let origin = if bounds.is_positive() { center - bounds.center().to_vec2() } else { center - galley.size() / 2.0 };
    let painted = bounds.translate(origin.to_vec2());
    painter.galley(origin, galley, color);
    painted
}

/// True while the user navigates with the keyboard (after Tab, until the next pointer press),
/// so focus rings appear for keyboard users without flashing on every mouse click.
pub fn keyboard_navigation(ctx: &egui::Context) -> bool {
    let id = egui::Id::unique("keyboard-navigation");
    let (tab, pointer) = ctx.input(|i| (i.key_pressed(egui::Key::Tab), i.pointer.any_pressed()));
    let mut active = ctx.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    if tab {
        active = true;
    } else if pointer {
        active = false;
    }
    ctx.data_mut(|d| d.insert_temp(id, active));
    active
}

/// Draws the accent focus ring around a focused custom widget during keyboard navigation.
pub fn focus_ring(ui: &Ui, response: &Response, radius: u8) {
    if response.has_focus() && keyboard_navigation(ui.ctx()) {
        let palette = Palette::of_ui(ui);
        ui.painter().rect_stroke(response.rect.expand(2.0), CornerRadius::same(radius.saturating_add(2)), Stroke::new(2.0, palette.accent), egui::StrokeKind::Outside);
    }
}

pub fn card_frame(ui: &Ui) -> egui::Frame {
    let palette = Palette::of_ui(ui);
    egui::Frame::new().fill(palette.surface).stroke(Stroke::new(1.0, palette.separator)).corner_radius(CornerRadius::same(CARD_RADIUS)).inner_margin(Margin::same(18))
}

/// A solid reading surface, like the iOS list cards.
pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<R> {
    card_frame(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        add(ui)
    })
}

/// A card that highlights on hover and reports clicks anywhere on it.
pub fn clickable_card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> (Response, R) {
    let palette = Palette::of_ui(ui);
    let id = ui.next_auto_id();
    let hovered = ui.ctx().read_response(id).is_some_and(|r| r.hovered());
    let fill = if hovered { palette.surface_raised } else { palette.surface };
    let inner = egui::Frame::new().fill(fill).stroke(Stroke::new(1.0, palette.separator)).corner_radius(CornerRadius::same(CARD_RADIUS)).inner_margin(Margin::same(18)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        add(ui)
    });
    let response = ui.interact(inner.response.rect, id, Sense::click());
    focus_ring(ui, &response, CARD_RADIUS);
    (response.on_hover_cursor(egui::CursorIcon::PointingHand), inner.inner)
}

/// The filled pink call to action (`ActionStyle`), or its outlined secondary form.
pub fn action_button(ui: &mut Ui, label: impl Into<String>, secondary: bool) -> Response {
    let palette = Palette::of_ui(ui);
    let text = theme::text(label, Font::Headline).color(if secondary { palette.accent } else { Color32::WHITE });
    let button = egui::Button::new(text)
        .fill(if secondary { palette.surface } else { palette.action })
        .stroke(if secondary { Stroke::new(1.0, palette.accent.gamma_multiply(0.45)) } else { Stroke::NONE })
        .corner_radius(CornerRadius::same(14))
        .min_size(vec2(0.0, CONTROL_LARGE));
    ui.add(button)
}

/// A compact rounded button with an icon and a label (Play, Shuffle, Download all…).
pub fn pill_button(ui: &mut Ui, icon: &str, label: impl Into<String>, filled: bool) -> Response {
    let palette = Palette::of_ui(ui);
    let color = if filled { Color32::WHITE } else { palette.accent };
    let text = RichText::new(format!("{icon}  {}", label.into())).font(Font::Headline.id()).color(color);
    ui.add(
        egui::Button::new(text)
            .fill(if filled { palette.action } else { palette.surface_raised })
            .corner_radius(CornerRadius::same(12))
            .min_size(vec2(110.0, CONTROL)),
    )
}

/// A borderless icon button with a 32-point hit area and the label as tooltip and accessible name.
pub fn icon_button(ui: &mut Ui, icon: &str, label: &str, size: f32) -> Response {
    icon_button_tinted(ui, icon, label, size, None)
}

pub fn icon_button_tinted(ui: &mut Ui, icon: &str, label: &str, size: f32, tint: Option<Color32>) -> Response {
    let side = (size + 16.0).max(32.0);
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(side), Sense::click());
    let palette = Palette::of_ui(ui);
    if response.is_pointer_button_down_on() {
        ui.painter().circle_filled(rect.center(), side / 2.0, palette.hover.gamma_multiply(1.8));
    } else if response.hovered() {
        ui.painter().circle_filled(rect.center(), side / 2.0, palette.hover);
    }
    let color = if !ui.is_enabled() { palette.secondary.gamma_multiply(0.45) } else { tint.unwrap_or(palette.text) };
    paint_icon(ui.painter(), rect.center(), icon, FontId::proportional(size), color);
    focus_ring(ui, &response, (side / 2.0) as u8);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label));
    let response = response.on_hover_text(label);
    if ui.is_enabled() { response.on_hover_cursor(egui::CursorIcon::PointingHand) } else { response }
}

/// A filled circular play button, as used on cards and headers.
pub fn round_play_button(ui: &mut Ui, playing: bool, label: &str, diameter: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(diameter), Sense::click());
    let palette = Palette::of_ui(ui);
    let fill = if response.hovered() { palette.action.gamma_multiply(1.15) } else { palette.action };
    ui.painter().circle_filled(rect.center(), diameter / 2.0, fill);
    let glyph = if playing { icons::PAUSE } else { icons::PLAY };
    // The play triangle's visual weight sits left of its geometric center; nudge it like iOS does.
    let nudge = if playing { 0.0 } else { diameter * 0.03 };
    paint_icon(ui.painter(), rect.center() + vec2(nudge, 0.0), glyph, FontId::new(diameter * 0.42, theme::icon_fill()), Color32::WHITE);
    focus_ring(ui, &response, (diameter / 2.0) as u8);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    response.on_hover_text(label).on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn page_title(ui: &mut Ui, title: &str) {
    ui.add_space(12.0);
    ui.label(theme::text(title, Font::LargeTitle));
    ui.add_space(14.0);
}

/// Page title with trailing controls on the same line.
pub fn page_header(ui: &mut Ui, title: &str, trailing: impl FnOnce(&mut Ui)) {
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.label(theme::text(title, Font::LargeTitle));
        ui.with_layout(Layout::right_to_left(Align::Center), trailing);
    });
    ui.add_space(14.0);
}

pub fn section_heading(ui: &mut Ui, title: &str, subtitle: Option<&str>) {
    ui.add_space(space::XL);
    ui.label(theme::text(title, Font::Title2));
    if let Some(subtitle) = subtitle {
        ui.add_space(2.0);
        ui.label(theme::secondary(ui, subtitle, Font::Subheadline));
    }
    ui.add_space(space::M);
}

/// An unread-count badge drawn at the top-right of `rect`; hidden at zero.
pub fn count_badge(ui: &Ui, rect: Rect, count: usize) {
    if count == 0 {
        return;
    }
    let text = if count > 99 { "99+".to_string() } else { count.to_string() };
    let palette = Palette::of_ui(ui);
    let galley = ui.painter().layout_no_wrap(text, egui::FontId::new(10.0, theme::bold()), Color32::WHITE);
    let size = vec2((galley.size().x + 10.0).max(17.0), 17.0);
    let badge = Rect::from_center_size(rect.right_top() + vec2(-4.0, 4.0), size);
    ui.painter().rect_filled(badge, CornerRadius::same(9), palette.action);
    ui.painter().galley(badge.center() - galley.size() / 2.0, galley, Color32::WHITE);
}

pub fn empty_state(ui: &mut Ui, icon: &str, title: &str, message: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(48.0);
        ui.label(theme::icon(icon, 44.0).color(Palette::of_ui(ui).accent));
        ui.add_space(8.0);
        ui.label(theme::text(title, Font::Title3));
        ui.label(theme::secondary(ui, message, Font::Subheadline));
        ui.add_space(24.0);
    });
}

pub fn loading(ui: &mut Ui) {
    ui.vertical_centered(|ui| {
        ui.add_space(32.0);
        ui.spinner();
        ui.add_space(32.0);
    });
}

/// An inline error with an optional Try again button. Returns true when retry was pressed.
pub fn error_banner(ui: &mut Ui, message: &str, retry: bool) -> bool {
    let palette = Palette::of_ui(ui);
    let mut pressed = false;
    egui::Frame::new().fill(palette.danger.gamma_multiply(0.15)).corner_radius(CornerRadius::same(12)).inner_margin(Margin::same(12)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            ui.label(theme::icon(icons::WARNING_CIRCLE, 18.0).color(palette.danger));
            ui.label(theme::text(message, Font::Callout));
            if retry && ui.button(l10n::t("Try again")).clicked() {
                pressed = true;
            }
        });
    });
    pressed
}

/// A rounded search field with a magnifier glyph, a fixed control height and vertically centered text.
pub fn search_field(ui: &mut Ui, text: &mut String, hint: &str, width: f32) -> Response {
    search_field_with_id(ui, None, text, hint, width)
}

pub fn search_field_with_id(ui: &mut Ui, id: Option<egui::Id>, text: &mut String, hint: &str, width: f32) -> Response {
    let palette = Palette::of_ui(ui);
    let width = width.min(ui.available_width());
    let (rect, _) = ui.allocate_exact_size(vec2(width, CONTROL), Sense::hover());
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(Layout::left_to_right(Align::Center)));
    let ui = &mut child;
    let field_id = id.unwrap_or_else(|| ui.make_persistent_id("search-field"));
    let focused = ui.memory(|m| m.has_focus(field_id));
    ui.painter().rect(
        rect,
        CornerRadius::same(10),
        palette.surface_raised,
        Stroke::new(1.0, if focused { palette.accent } else { palette.separator }),
        egui::StrokeKind::Inside,
    );
    // Explicit gaps only: the default item spacing would push the text far from the magnifier.
    ui.spacing_mut().item_spacing.x = 0.0;
    ui.add_space(space::M);
    let icon_rect = ui.allocate_exact_size(vec2(16.0, CONTROL), Sense::hover()).0;
    paint_icon(ui.painter(), icon_rect.center(), icons::MAGNIFYING_GLASS, FontId::proportional(15.0), palette.secondary);
    ui.add_space(space::S);
    // The clear button's hover circle supplies its own padding.
    let trailing = if text.is_empty() { space::M } else { 32.0 + space::XS };
    let field_width = (ui.available_width() - trailing).max(40.0);
    let response = ui.add_sized(
        vec2(field_width, CONTROL),
        egui::TextEdit::singleline(text)
            .id(field_id)
            .hint_text(hint)
            .frame(egui::Frame::NONE)
            .margin(Margin::ZERO)
            .align(egui::Align2::LEFT_CENTER),
    );
    if !text.is_empty() && icon_button(ui, icons::X_CIRCLE, &l10n::t("Clear"), 14.0).clicked() {
        text.clear();
        ui.memory_mut(|m| m.request_focus(field_id));
    }
    response
}

/// Lays out equally sized tiles in as many columns as fit.
pub fn tile_grid<T>(ui: &mut Ui, items: &[T], min_tile: f32, mut tile: impl FnMut(&mut Ui, &T, f32)) {
    let gap = 20.0;
    let available = ui.available_width();
    let columns = ((available + gap) / (min_tile + gap)).floor().max(1.0) as usize;
    let width = ((available - gap * (columns as f32 - 1.0)) / columns as f32).floor();
    for row in items.chunks(columns) {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for item in row {
                ui.allocate_ui_with_layout(vec2(width, 0.0), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(width);
                    tile(ui, item, width);
                });
            }
        });
        ui.add_space(14.0);
    }
}

/// Artwork from the active music library, with the iOS gradient placeholder.
pub fn artwork(ui: &mut Ui, app: &mut App, seed: &str, cover: Option<&str>, size: f32, radius: f32) -> Response {
    let texture = cover.and_then(|cover| app.artwork_texture(cover));
    artwork_with(ui, seed, texture, size, radius)
}

/// Artwork from an arbitrary URL (audiobook covers, podcast directory results).
pub fn artwork_url(ui: &mut Ui, app: &mut App, seed: &str, key: &str, url: Option<url::Url>, size: f32, radius: f32) -> Response {
    let texture = app.artwork.texture(&app.tasks, key, url, None);
    artwork_with(ui, seed, texture, size, radius)
}

pub fn artwork_with(ui: &mut Ui, seed: &str, texture: Option<egui::TextureHandle>, size: f32, radius: f32) -> Response {
    // Clickable with the pointer but not a keyboard stop: the surrounding tile or row is the control.
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::CLICK);
    paint_artwork(ui, rect, seed, texture.as_ref(), radius);
    response
}

pub fn paint_artwork(ui: &Ui, rect: Rect, seed: &str, texture: Option<&egui::TextureHandle>, radius: f32) {
    paint_artwork_or(ui, rect, seed, texture, radius, icons::MUSIC_NOTE);
}

/// Like [`paint_artwork`], with `placeholder` drawn on the gradient when there is no image.
pub fn paint_artwork_or(ui: &Ui, rect: Rect, seed: &str, texture: Option<&egui::TextureHandle>, radius: f32, placeholder: &str) {
    let corner = CornerRadius::same(radius.round().clamp(0.0, 255.0) as u8);
    if let Some(texture) = texture {
        egui::Image::new(texture).corner_radius(corner).paint_at(ui, rect);
        return;
    }
    let [top, bottom] = theme::ARTWORK_PALETTES[theme::artwork_seed(seed)];
    paint_gradient(ui, rect, theme::hex(top), theme::hex(bottom), corner);
    paint_icon(ui.painter(), rect.center(), placeholder, FontId::proportional(rect.width() * 0.34), Color32::from_white_alpha(217));
}

/// Round artist artwork; without a photo, a person on the gradient.
pub fn artist_artwork(ui: &mut Ui, app: &mut App, seed: &str, cover: Option<&str>, size: f32) -> Response {
    let texture = cover.and_then(|cover| app.artwork_texture(cover));
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::CLICK);
    paint_artwork_or(ui, rect, seed, texture.as_ref(), size / 2.0, icons::USER);
    response
}

/// A diagonal two-color gradient filling a rounded rectangle, circles included.
pub fn paint_gradient(ui: &Ui, rect: Rect, start: Color32, end: Color32, corner: CornerRadius) {
    // Meshes are not antialiased: the rounded base draws the smooth edge, then a gradient fan over the
    // same outline, one physical pixel inside, fills it.
    ui.painter().rect_filled(rect, corner, start.lerp_to_gamma(end, 0.5));
    let inset = 1.0 / ui.ctx().pixels_per_point();
    let inner = rect.shrink(inset);
    let max_radius = inner.width().min(inner.height()) / 2.0;
    let radius = |r: u8| (f32::from(r) - inset).clamp(0.0, max_radius);
    let corners = [
        (Pos2::new(inner.right(), inner.top()), radius(corner.ne), vec2(-1.0, 1.0), -90.0f32),
        (Pos2::new(inner.right(), inner.bottom()), radius(corner.se), vec2(-1.0, -1.0), 0.0),
        (Pos2::new(inner.left(), inner.bottom()), radius(corner.sw), vec2(1.0, -1.0), 90.0),
        (Pos2::new(inner.left(), inner.top()), radius(corner.nw), vec2(1.0, 1.0), 180.0),
    ];
    let mix = |p: Pos2| {
        let t = ((p.x - inner.left()) + (p.y - inner.top())) / (inner.width() + inner.height()).max(1.0);
        start.lerp_to_gamma(end, t.clamp(0.0, 1.0))
    };
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(inner.center(), mix(inner.center()));
    const STEPS: usize = 12;
    for (point, r, toward_center, start_angle) in corners {
        let center = point + toward_center * r;
        for step in 0..=STEPS {
            let angle = (start_angle + 90.0 * step as f32 / STEPS as f32).to_radians();
            let p = center + r * vec2(angle.cos(), angle.sin());
            mesh.colored_vertex(p, mix(p));
        }
    }
    let outline = (mesh.vertices.len() - 1) as u32;
    for i in 0..outline {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % outline);
    }
    ui.painter().add(egui::Shape::mesh(mesh));
}

/// An album tile for grids: artwork, title and artist. Clicking opens the album.
pub fn album_tile(ui: &mut Ui, app: &mut App, album: &Album, width: f32) -> Response {
    let art = artwork(ui, app, &album.id, album.cover_art.as_deref(), width, 12.0);
    ui.add(egui::Label::new(theme::text(&album.name, Font::Headline)).truncate().selectable(false));
    let artist = album.artist.clone().unwrap_or_default();
    let subtitle = match album.year {
        Some(year) if year > 0 => format!("{artist} · {year}"),
        _ => artist,
    };
    let rect = art.rect.union(ui.min_rect());
    ui.add(egui::Label::new(theme::secondary(ui, subtitle.clone(), Font::Subheadline)).truncate().selectable(false));
    let rect = rect.union(ui.min_rect());
    tile_target(ui, rect, ("album-tile", &album.id), &format!("{}, {subtitle}", album.name))
}

/// One keyboard stop and accessible name for a tile made of artwork and text, with hover cursor and
/// focus ring. Add any overlay buttons after calling this so they stay clickable on top.
pub fn tile_target(ui: &mut Ui, rect: Rect, id_salt: impl std::hash::Hash + std::fmt::Debug, label: &str) -> Response {
    let response = ui.interact(rect, ui.scope_id().with(id_salt), Sense::click());
    focus_ring(ui, &response, CARD_RADIUS);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

#[derive(Default, Clone, Copy)]
pub struct TrackRowOptions {
    /// Shows the track number instead of artwork (album pages).
    pub number: Option<i64>,
    pub show_artwork: bool,
    pub show_album: bool,
    pub is_current: bool,
    pub is_playing: bool,
}

pub struct TrackRow {
    pub response: Response,
    /// Single click selects; double click (or Enter) plays, as on desktop players.
    pub play: bool,
}

/// A song row: number or artwork, title and artist, duration, and space for trailing controls.
/// Attach a context menu with `row.response.context_menu(...)`.
pub fn track_row(ui: &mut Ui, app: &mut App, song: &Song, options: TrackRowOptions, trailing: impl FnOnce(&mut Ui, &mut App)) -> TrackRow {
    let palette = Palette::of_ui(ui);
    let height = if options.show_artwork { 52.0 } else { 44.0 };
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    if response.hovered() || options.is_current {
        ui.painter().rect_filled(rect, CornerRadius::same(8), if options.is_current { palette.accent.gamma_multiply(0.12) } else { palette.hover });
    }
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(10.0, 4.0))).layout(Layout::left_to_right(Align::Center)));
    let ui = &mut child;
    if let Some(number) = options.number {
        let label = if options.is_current {
            theme::icon(if options.is_playing { icons::SPEAKER_HIGH } else { icons::SPEAKER_NONE }, 15.0).color(palette.accent)
        } else {
            theme::secondary(ui, number.to_string(), Font::Callout)
        };
        ui.allocate_ui_with_layout(vec2(28.0, height - 8.0), Layout::centered_and_justified(egui::Direction::LeftToRight), |ui| ui.label(label));
    }
    if options.show_artwork {
        artwork(ui, app, &song.id, song.cover_art.as_deref(), 40.0, 6.0);
    }
    let duration = song.duration.map(l10n::time).unwrap_or_default();
    let album_column = options.show_album && width >= 760.0;
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        trailing(ui, app);
        ui.allocate_ui_with_layout(vec2(48.0, height - 8.0), Layout::right_to_left(Align::Center), |ui| {
            ui.label(theme::secondary(ui, duration, Font::Callout).monospace());
        });
        ui.add_space(8.0);
        if album_column {
            ui.allocate_ui_with_layout(vec2(width * 0.26, height - 8.0), Layout::left_to_right(Align::Center), |ui| {
                ui.add(egui::Label::new(theme::secondary(ui, song.album.clone().unwrap_or_default(), Font::Callout)).truncate().selectable(false));
            });
        }
        ui.with_layout(Layout::top_down(Align::Min), |ui| {
            ui.add_space(if options.show_artwork { 6.0 } else { 3.0 });
            let title_color = if options.is_current { palette.accent } else { palette.text };
            ui.add(egui::Label::new(theme::text(&song.title, Font::Body).color(title_color)).truncate().selectable(false));
            let subtitle = if options.show_album && !album_column { song.subtitle() } else { song.artist.clone().unwrap_or_default() };
            if !subtitle.is_empty() {
                ui.add(egui::Label::new(theme::secondary(ui, subtitle, Font::Footnote)).truncate().selectable(false));
            }
        });
    });
    let play = response.double_clicked() || (response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
    focus_ring(ui, &response, 8);
    let label = format!("{}, {}", song.title, song.artist.clone().unwrap_or_default());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
    TrackRow { response, play }
}

/// A thin labeled toggle chip, as used for filters (All, Library, Artists…).
pub fn chip(ui: &mut Ui, selected: bool, label: &str) -> Response {
    let palette = Palette::of_ui(ui);
    let text = theme::text(label, Font::Callout).color(if selected { Color32::WHITE } else { palette.text });
    ui.add(
        egui::Button::new(text)
            .fill(if selected { palette.action } else { palette.surface_raised })
            .corner_radius(CornerRadius::same(15))
            .min_size(vec2(0.0, CONTROL_SMALL)),
    )
}

/// The page body: margins, max reading width and vertical scrolling.
pub fn page<R>(ui: &mut Ui, id: impl std::hash::Hash + std::fmt::Debug, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::ScrollArea::vertical().id_salt(id).auto_shrink([false, false]).show(ui, |ui| {
        egui::Frame::new().inner_margin(Margin { left: PAGE_MARGIN as i8, right: PAGE_MARGIN as i8, top: 12, bottom: 32 }).show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
    })
    .inner
}

/// "1 h 5 min"-style totals for headers, with a song count.
pub fn songs_summary(songs: &[Song]) -> String {
    let total: f64 = songs.iter().filter_map(|s| s.duration).sum();
    let count = l10n::count("%lld songs", songs.len() as i64);
    if total > 0.0 { format!("{count} · {}", l10n::short_duration(total)) } else { count }
}
