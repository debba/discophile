//! First-run screens: the three-page introduction, the welcome screen shown while nothing is
//! configured, and the "Explore the design" preview of fictional albums.

use discophile_core::l10n::{self, t, tf};
use egui::emath::Rot2;
use egui::{Align, Align2, Color32, CornerRadius, FontId, Layout, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2, vec2};

use crate::app::App;
use crate::theme::{self, Font, Palette, hex, icons};
use crate::ui::widgets;

const TITLES: [&str; 3] = ["Your music.\nBeautifully yours.", "Take your favorites\nwith you.", "A word,\nand you're listening."];
const SUBTITLES: [&str; 3] = [
    "A quieter home for music, audiobooks and radio. Connect your libraries and make yourself at home.",
    "Save a song, an album, or a podcast. Find everything in your library, even when the world goes offline.",
    "Ask the music assistant for a selection, or press Space to play and pause. Keep listening with your keyboard's media keys or your headphones.",
];
/// Icon, title and description of the two highlights on each page.
const FEATURES: [[(&str, &str, &str); 2]; 3] = [
    [
        (icons::STACK, "Your library, your server", "Connect Subsonic, Navidrome, Jellyfin or Audiobookshelf."),
        (icons::HEART, "Room for your favorites", "Albums, artists, and playlists, all together."),
    ],
    [
        (icons::ARROW_CIRCLE_DOWN, "One click to download", "Keep the music and episodes you choose."),
        (icons::FUNNEL_SIMPLE, "One library, online or off", "Use Downloaded only in your normal library views."),
    ],
    [
        (icons::SPARKLE, "Just ask your assistant", "Try: Something calm for a rainy evening."),
        (icons::PERSON_SIMPLE, "Made for the way you listen", "Screen readers, keyboard navigation, and reduced motion."),
    ],
];

fn page_id() -> egui::Id {
    egui::Id::unique("introduction-page")
}

/// The three-page introduction. Arrow keys change pages; Skip and Get started finish it.
pub fn introduction(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let mut page = ctx.data(|d| d.get_temp::<usize>(page_id())).unwrap_or(0).min(2);
    let mut finish = false;
    if !ctx.egui_wants_keyboard_input() {
        let (left, right) = ui.input(|i| {
            (i.modifiers.is_none() && i.key_pressed(egui::Key::ArrowLeft), i.modifiers.is_none() && i.key_pressed(egui::Key::ArrowRight))
        });
        if left {
            page = page.saturating_sub(1);
        }
        if right && page < 2 {
            page += 1;
        }
    }
    let palette = Palette::of_ui(ui);

    ui.add_space(14.0);
    centered_column(ui, 620.0, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("DISCOPHILE").font(FontId::new(11.0, theme::bold())).extra_letter_spacing(3.0).color(palette.secondary),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let skip =
                    egui::Button::new(theme::text(t("Skip"), Font::Headline).color(palette.accent)).frame(false).min_size(vec2(44.0, 32.0));
                if ui.add(skip).clicked() {
                    finish = true;
                }
            });
        });
    });

    // The controls keep their place on every page so repeated clicks land on the same button.
    let controls_height = 150.0;
    egui::ScrollArea::vertical()
        .id_salt("introduction")
        .auto_shrink([false, false])
        .max_height((ui.available_height() - controls_height).max(160.0))
        .show(ui, |ui| {
            centered_column(ui, 620.0, |ui| {
                ui.add_space(12.0);
                // Like iOS at accessibility text sizes, the illustration yields to text on short windows.
                if ui.ctx().content_rect().height() > 620.0 {
                    let height = if page == 0 { 245.0 } else { 195.0 };
                    record_illustration(ui, page, height);
                    ui.add_space(18.0);
                }
                ui.label(theme::text(t(TITLES[page]), Font::LargeTitle));
                ui.add_space(4.0);
                ui.label(theme::secondary(ui, t(SUBTITLES[page]), Font::Body));
                ui.add_space(16.0);
                for (icon, title, detail) in FEATURES[page] {
                    feature_row(ui, icon, &t(title), &t(detail));
                    ui.add_space(10.0);
                }
            });
        });

    centered_column(ui, 620.0, |ui| {
        ui.add_space(6.0);
        let dots_width = 3.0 * 44.0;
        ui.horizontal(|ui| {
            ui.add_space(((ui.available_width() - dots_width) / 2.0).max(0.0));
            ui.spacing_mut().item_spacing.x = 0.0;
            for index in 0..3 {
                if page_dot(ui, index, index == page).clicked() {
                    page = index;
                }
            }
        });
        ui.add_space(4.0);
        let label = if page == 2 { t("Get started") } else { t("Continue") };
        if wide_action(ui, None, &label, Some(icons::ARROW_RIGHT), false).clicked() {
            if page < 2 {
                page += 1;
            } else {
                finish = true;
            }
        }
        if page == 0 {
            ui.add_space(32.0);
        } else {
            ui.vertical_centered(|ui| {
                let back =
                    egui::Button::new(theme::text(t("Back"), Font::Headline).color(palette.accent)).frame(false).min_size(vec2(44.0, 32.0));
                if ui.add(back).clicked() {
                    page -= 1;
                }
            });
        }
    });

    if finish {
        app.settings.introduction_completed = true;
        app.settings.save();
        page = 0;
    }
    ctx.data_mut(|d| d.insert_temp(page_id(), page));
}

fn feature_row(ui: &mut Ui, icon: &str, title: &str, detail: &str) {
    let palette = Palette::of_ui(ui);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 14.0;
        ui.add_sized([28.0, 30.0], egui::Label::new(theme::icon_filled(icon, 22.0).color(palette.accent)).selectable(false));
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(theme::text(title, Font::Headline));
            ui.label(theme::secondary(ui, detail, Font::Subheadline));
        });
    });
}

fn page_dot(ui: &mut Ui, index: usize, selected: bool) -> egui::Response {
    let palette = Palette::of_ui(ui);
    let (rect, response) = ui.allocate_exact_size(vec2(44.0, 32.0), Sense::click());
    let size = vec2(if selected { 24.0 } else { 7.0 }, 7.0);
    let color = if selected { palette.accent } else { palette.secondary.gamma_multiply(if response.hovered() { 0.9 } else { 0.55 }) };
    ui.painter().rect_filled(Rect::from_center_size(rect.center(), size), CornerRadius::same(4), color);
    let label = tf("Introduction page %lld", &[&(index + 1)]);
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Tab, true, selected, &label));
    response.on_hover_text(label).on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Shown when no library, audiobook server or radio station is configured.
pub fn welcome(app: &mut App, ui: &mut Ui) {
    let palette = Palette::of_ui(ui);
    egui::ScrollArea::vertical().id_salt("welcome").auto_shrink([false, false]).show(ui, |ui| {
        centered_column(ui, 620.0, |ui| {
            ui.add_space(28.0);
            ui.horizontal(|ui| {
                let icon = crate::artwork::app_icon(ui.ctx(), 192);
                ui.add(egui::Image::new(&icon).fit_to_exact_size(vec2(92.0, 92.0)).corner_radius(CornerRadius::same(26)));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new("DISCOPHILE")
                            .font(FontId::new(11.0, theme::bold()))
                            .extra_letter_spacing(3.0)
                            .color(palette.secondary),
                    );
                });
            });
            ui.add_space(20.0);
            if ui.ctx().content_rect().height() > 700.0 {
                record_illustration(ui, 0, 260.0);
                ui.add_space(24.0);
            }
            ui.label(theme::text(t("Good music.\nA place to call home."), Font::LargeTitle));
            ui.add_space(4.0);
            ui.label(theme::secondary(ui, t("Connect your music, audiobooks and internet radio in one place."), Font::Body));
            ui.add_space(24.0);

            if wide_action(ui, Some(icons::PLUS_CIRCLE), &t("Connect a music server"), None, false).clicked() {
                app.sheets.add_account_provider = None;
                app.sheets.add_account = true;
            }
            ui.add_space(4.0);
            if wide_action(ui, Some(icons::BOOKS), &t("Connect Audiobookshelf"), None, true).clicked() {
                app.sheets.add_account_provider = Some("audiobookshelf".into());
                app.sheets.add_account = true;
            }
            ui.add_space(4.0);
            if wide_action(ui, Some(icons::RADIO), &t("Add a radio station"), None, true).clicked() {
                app.sheets.add_account_provider = Some("radio".into());
                app.sheets.add_account = true;
            }
            ui.add_space(10.0);
            ui.vertical_centered(|ui| {
                let explore = egui::Button::new(theme::text(t("Explore the design"), Font::Headline).color(palette.accent))
                    .frame(false)
                    .min_size(vec2(44.0, 32.0));
                if ui.add(explore).on_hover_text(t("Fictional albums that show how Discophile looks. They cannot be played.")).clicked() {
                    ui.ctx().data_mut(|d| d.insert_temp(explore_id(), ExploreState { open: true, album: None }));
                }
            });
            ui.add_space(8.0);
            ui.label(theme::secondary(ui, t("Subsonic, Navidrome, Jellyfin, Audiobookshelf and Internet Radio."), Font::Footnote));
            ui.add_space(28.0);
        });
    });
    explore_window(ui.ctx());
}

/// A full-width call to action in the iOS `ActionStyle`, with centered text and optional symbols.
/// Symbols use the filled Phosphor face, which no default fallback font shadows.
pub(crate) fn wide_action(ui: &mut Ui, leading: Option<&str>, label: &str, trailing: Option<&str>, secondary: bool) -> egui::Response {
    let palette = Palette::of_ui(ui);
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 46.0), Sense::click());
    let enabled = ui.is_enabled();
    let (fill, color) = if secondary { (palette.surface, palette.accent) } else { (palette.action, Color32::WHITE) };
    let fill = if response.is_pointer_button_down_on() {
        fill.gamma_multiply(0.8)
    } else if response.hovered() && secondary {
        palette.surface_raised
    } else {
        fill
    };
    let opacity = if enabled { 1.0 } else { 0.5 };
    ui.painter().rect_filled(rect, CornerRadius::same(16), fill.gamma_multiply(opacity));
    if secondary {
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(16),
            Stroke::new(1.0, palette.accent.gamma_multiply(0.45)),
            egui::StrokeKind::Inside,
        );
    }
    if response.has_focus() {
        ui.painter().rect_stroke(rect.expand(2.0), CornerRadius::same(18), Stroke::new(2.0, palette.accent), egui::StrokeKind::Outside);
    }
    let color = color.gamma_multiply(opacity);
    let text = ui.painter().layout_no_wrap(label.to_string(), Font::Headline.id(), color);
    let symbol = |glyph: &str| ui.painter().layout_no_wrap(glyph.to_string(), FontId::new(18.0, theme::icon_fill()), color);
    let leading = leading.map(symbol);
    let trailing = trailing.map(symbol);
    let gap = 8.0;
    let total = text.size().x + leading.as_ref().map_or(0.0, |g| g.size().x + gap) + trailing.as_ref().map_or(0.0, |g| g.size().x + gap);
    let mut x = rect.center().x - total / 2.0;
    for galley in [leading, Some(text), trailing].into_iter().flatten() {
        let width = galley.size().x;
        ui.painter().galley(egui::pos2(x, rect.center().y - galley.size().y / 2.0), galley, color);
        x += width + gap;
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Lays out `add` in a column of at most `max_width`, centered horizontally.
pub(crate) fn centered_column<R>(ui: &mut Ui, max_width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let available = ui.available_width();
    let width = available.min(max_width) - 32.0;
    let margin = ((available - width) / 2.0).max(0.0);
    ui.horizontal_top(|ui| {
        ui.add_space(margin);
        ui.vertical(|ui| {
            ui.set_width(width.max(100.0));
            add(ui)
        })
        .inner
    })
    .inner
}

// MARK: - Illustration

/// A port of the iOS `RecordIllustration`: a tilted gradient card, a grooved vinyl with its label, and
/// a rotated badge whose symbol depends on `variant` (0 sparkle, 1 download, 2 waveform). Decorative only.
pub(crate) fn record_illustration(ui: &mut Ui, variant: usize, height: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let w = (height * 1.39).min(rect.width());
    let center = rect.center();
    let painter = ui.painter();

    let card_size = w * 0.66;
    let card_center = center + vec2(-w * 0.12, 0.0);
    let card_angle = (-9.0f32).to_radians();
    let rotation = Rot2::from_angle(card_angle);
    let stops = [hex(0xF48D9D), hex(0xC67CB5), hex(0x4D376D)];
    let card = rounded_rect_points(card_center, Vec2::splat(card_size), 28.0, card_angle);
    fill_gradient(painter, &card, |p| {
        let local = rotation.inverse() * (p - card_center);
        let progress = ((local.x + local.y) / card_size + 1.0) / 2.0;
        three_stops(stops, progress)
    });

    let vinyl_radius = w * 0.69 / 2.0;
    let vinyl_center = center + vec2(w * 0.12, 7.0);
    painter.circle_filled(vinyl_center, vinyl_radius, hex(0x17121E));
    for i in 0..12 {
        let radius = vinyl_radius - (8 + i * 6) as f32 - 0.5;
        if radius > 0.0 {
            painter.circle_stroke(vinyl_center, radius, Stroke::new(1.0, Color32::from_white_alpha(20)));
        }
    }
    let label_radius = w * 0.10;
    let label: Vec<Pos2> = (0..72)
        .map(|i| {
            let angle = i as f32 / 72.0 * std::f32::consts::TAU;
            vinyl_center + vec2(angle.cos(), angle.sin()) * label_radius
        })
        .collect();
    fill_gradient(painter, &label, |p| {
        let local = p - vinyl_center;
        let progress = ((local.x + local.y) / (2.0 * label_radius) + 1.0) / 2.0;
        hex(0xF6B2A9).lerp_to_gamma(hex(0xAD3D6A), progress.clamp(0.0, 1.0))
    });
    painter.circle_filled(vinyl_center, 5.0, hex(0x17121E));

    let badge_center = center + vec2(w * 0.30, w * 0.22);
    let badge_angle = 10.0f32.to_radians();
    let badge = rounded_rect_points(badge_center, Vec2::splat(76.0), 22.0, badge_angle);
    fill_gradient(painter, &badge, |_| hex(0x6B234B));
    let glyph = match variant {
        1 => icons::ARROW_CIRCLE_DOWN,
        2 => icons::WAVEFORM,
        _ => icons::SPARKLE,
    };
    let galley = painter.layout_no_wrap(glyph.to_string(), FontId::new(36.0, theme::icon_fill()), Color32::WHITE);
    let position = badge_center - galley.size() / 2.0;
    painter.add(egui::epaint::TextShape::new(position, galley, Color32::WHITE).with_angle_and_anchor(badge_angle, Align2::CENTER_CENTER));
}

fn three_stops(stops: [Color32; 3], progress: f32) -> Color32 {
    let progress = progress.clamp(0.0, 1.0);
    if progress < 0.5 { stops[0].lerp_to_gamma(stops[1], progress * 2.0) } else { stops[1].lerp_to_gamma(stops[2], (progress - 0.5) * 2.0) }
}

/// The outline of a rounded rectangle rotated by `angle` radians (clockwise on screen) around its center.
fn rounded_rect_points(center: Pos2, size: Vec2, radius: f32, angle: f32) -> Vec<Pos2> {
    let rotation = Rot2::from_angle(angle);
    let half = size / 2.0;
    let radius = radius.min(half.x).min(half.y);
    let (x, y) = (half.x - radius, half.y - radius);
    let corners = [(vec2(x, y), 0.0f32), (vec2(-x, y), 90.0), (vec2(-x, -y), 180.0), (vec2(x, -y), 270.0)];
    let mut points = Vec::with_capacity(4 * 9);
    for (corner, start) in corners {
        for step in 0..=8 {
            let a = (start + step as f32 * 90.0 / 8.0).to_radians();
            points.push(center + rotation * (corner + vec2(a.cos(), a.sin()) * radius));
        }
    }
    points
}

/// Fills a convex polygon with per-point colors and a one-pixel anti-aliased edge.
fn fill_gradient(painter: &egui::Painter, points: &[Pos2], color: impl Fn(Pos2) -> Color32) {
    let count = points.len();
    if count < 3 {
        return;
    }
    let centroid = (points.iter().fold(Vec2::ZERO, |sum, p| sum + p.to_vec2()) / count as f32).to_pos2();
    let feather = 0.5 / painter.ctx().pixels_per_point();
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(centroid, color(centroid));
    for &point in points {
        let outward = (point - centroid).normalized();
        mesh.colored_vertex(point - outward * feather, color(point));
        mesh.colored_vertex(point + outward * feather, Color32::TRANSPARENT);
    }
    for i in 0..count {
        let j = (i + 1) % count;
        let (a, b) = (1 + 2 * i as u32, 1 + 2 * j as u32);
        mesh.add_triangle(0, a, b);
        mesh.add_triangle(a, a + 1, b + 1);
        mesh.add_triangle(a, b + 1, b);
    }
    painter.add(Shape::mesh(mesh));
}

// MARK: - Explore the design

const PREVIEW_ALBUMS: [(&str, &str, i32); 6] = [
    ("After the Rain", "Sora Fields", 2026),
    ("Soft Focus", "The Sunday Hours", 2025),
    ("Night Swimming", "Luca Vale", 2026),
    ("Somewhere, Slowly", "June & the Pines", 2024),
    ("Blue Hour", "Anya Coast", 2026),
    ("Open Windows", "Milo Park", 2025),
];
const PREVIEW_SONGS: [&str; 5] = ["First Light", "A Place to Begin", "Everything in Between", "Stay a Little Longer", "Coming Home"];

#[derive(Clone, Default)]
struct ExploreState {
    open: bool,
    album: Option<usize>,
}

fn explore_id() -> egui::Id {
    egui::Id::unique("explore-design")
}

/// The design preview: clearly labeled fictional albums with generated artwork. Nothing can be played.
fn explore_window(ctx: &egui::Context) {
    let mut state: ExploreState = ctx.data(|d| d.get_temp(explore_id())).unwrap_or_default();
    if !state.open {
        return;
    }
    let max_height = ctx.content_rect().height() * 0.8;
    let modal = egui::Modal::new(egui::Id::unique("explore-design-modal")).show(ctx, |ui| {
        ui.set_width(660.0f32.min(ctx.content_rect().width() - 60.0));
        let palette = Palette::of_ui(ui);
        let mut close = false;
        ui.horizontal(|ui| {
            ui.label(theme::text(t("Design preview"), Font::Title2));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if widgets::action_button(ui, t("Done"), true).clicked() {
                    close = true;
                }
            });
        });
        ui.add_space(4.0);
        egui::Frame::new()
            .fill(palette.accent.gamma_multiply(0.12))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.label(theme::icon_filled(icons::INFO, 17.0).color(palette.accent));
                    ui.label(theme::text(
                        t("These albums are fictional and cannot be played or downloaded. Connect a server for real listening."),
                        Font::Callout,
                    ));
                });
            });
        ui.add_space(10.0);
        egui::ScrollArea::vertical()
            .id_salt("explore-scroll")
            .max_height(max_height - 140.0)
            .min_scrolled_height(max_height - 140.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                let indices: Vec<usize> = (0..PREVIEW_ALBUMS.len()).collect();
                widgets::tile_grid(ui, &indices, 150.0, |ui, &index, width| {
                    let (title, artist, year) = PREVIEW_ALBUMS[index];
                    let (rect, art) = ui.allocate_exact_size(Vec2::splat(width), Sense::click());
                    paint_preview_artwork(ui, rect, index);
                    if state.album == Some(index) {
                        ui.painter().rect_stroke(rect, CornerRadius::same(12), Stroke::new(2.5, palette.accent), egui::StrokeKind::Outside);
                    }
                    ui.add(egui::Label::new(theme::text(title, Font::Headline)).truncate().selectable(false));
                    ui.add(
                        egui::Label::new(theme::secondary(ui, format!("{artist} · {year}"), Font::Subheadline))
                            .truncate()
                            .selectable(false),
                    );
                    let label = format!("{title}, {artist}");
                    art.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, state.album == Some(index), &label));
                    if art.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        state.album = if state.album == Some(index) { None } else { Some(index) };
                    }
                });
                if let Some(index) = state.album {
                    let (title, artist, _) = PREVIEW_ALBUMS[index];
                    widgets::section_heading(ui, title, Some(artist));
                    for (number, song) in PREVIEW_SONGS.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.add_sized([26.0, 30.0], egui::Label::new(theme::secondary(ui, (number + 1).to_string(), Font::Callout)));
                            ui.label(theme::text(*song, Font::Body));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(theme::secondary(ui, l10n::time(185.0 + number as f64 * 23.0), Font::Callout));
                            });
                        });
                    }
                    ui.add_space(6.0);
                    ui.add_enabled_ui(false, |ui| {
                        crate::ui::accounts::labeled_button(ui, icons::PLAY, &t("Preview only"), crate::ui::accounts::Look::Filled)
                            .on_disabled_hover_text(t("Connect a server for real listening."));
                    });
                }
            });
        close
    });
    if modal.inner || modal.should_close() {
        state = ExploreState::default();
    }
    ctx.data_mut(|d| d.insert_temp(explore_id(), state));
}

/// The iOS preview artwork: the placeholder gradient with drawn contour lines and a soft highlight.
fn paint_preview_artwork(ui: &Ui, rect: Rect, index: usize) {
    let seed = theme::artwork_seed(&format!("preview-{index}"));
    let [top, bottom] = theme::ARTWORK_PALETTES[seed];
    widgets::paint_gradient(ui, rect, hex(top), hex(bottom), CornerRadius::same(12));
    let painter = ui.painter();
    let rotation = Rot2::from_angle(((seed * 13 + 20) as f32).to_radians());
    let stroke = Stroke::new((rect.width() / 160.0).max(1.0), Color32::from_white_alpha(89));
    // Lines are kept inside the rounded artwork by splitting each ellipse where it leaves the shape.
    let inner = rect.shrink(1.5);
    let inside = |p: Pos2| {
        let radius = 11.0;
        let core = inner.shrink(radius);
        let nearest = p.clamp(core.min, core.max);
        inner.contains(p) && (p - nearest).length() <= radius
    };
    for ring in 0..7 {
        let radii = vec2(rect.width() * 1.1, rect.height() * (0.3 + ring as f32 * 0.13)) / 2.0;
        let mut segment: Vec<Pos2> = Vec::new();
        for i in 0..=192 {
            let a = i as f32 / 192.0 * std::f32::consts::TAU;
            let point = rect.center() + rotation * vec2(a.cos() * radii.x, a.sin() * radii.y);
            if inside(point) {
                segment.push(point);
            } else if segment.len() > 1 {
                painter.add(Shape::line(std::mem::take(&mut segment), stroke));
            } else {
                segment.clear();
            }
        }
        if segment.len() > 1 {
            painter.add(Shape::line(segment, stroke));
        }
    }
    let glow = rect.center() + vec2(rect.width() * 0.1, -rect.height() * 0.14);
    painter.circle_filled(glow, rect.width() * 0.125 + 2.0, Color32::from_white_alpha(40));
    painter.circle_filled(glow, rect.width() * 0.125, Color32::from_white_alpha(102));
}
