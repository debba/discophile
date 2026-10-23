//! A soft background tinted by the artwork: the cover is downloaded at a tiny size, averaged per
//! quadrant and drawn as a smooth gradient mesh, which reads like a heavy blur.

use discophile_core::Song;
use egui::{Color32, Id, Rect, Ui};

use crate::app::App;
use crate::tasks::update;
use crate::theme::{self, Palette};

/// Top-left, top-right, bottom-right, bottom-left.
pub type Corners = [Color32; 4];

#[derive(Clone, Default)]
struct Cache {
    key: String,
    colors: Option<Corners>,
}

fn cache_id() -> Id {
    Id::unique("now-playing-artwork-colors")
}

/// Corner colors for a song's artwork, falling back to its placeholder gradient while loading or offline.
pub fn colors(app: &mut App, song: &Song) -> Corners {
    let [top, bottom] = theme::ARTWORK_PALETTES[theme::artwork_seed(&song.id)];
    let fallback = [
        theme::hex(top),
        theme::hex(top).lerp_to_gamma(theme::hex(bottom), 0.5),
        theme::hex(bottom),
        theme::hex(top).lerp_to_gamma(theme::hex(bottom), 0.5),
    ];
    let Some(cover) = song.cover_art.clone() else { return fallback };
    let key = format!("{}:{cover}", app.session.account_key());
    let cache = app.ctx().data(|d| d.get_temp::<Cache>(cache_id())).unwrap_or_default();
    if cache.key == key {
        return cache.colors.unwrap_or(fallback);
    }
    let Some(client) = app.client() else { return fallback };
    app.ctx().data_mut(|d| d.insert_temp(cache_id(), Cache { key: key.clone(), colors: None }));
    let url = client.artwork_url(&cover, 64);
    app.tasks.spawn(async move {
        let colors = load(url).await;
        update(move |app| {
            app.ctx().data_mut(|d| {
                let cache = d.get_temp_mut_or_default::<Cache>(cache_id());
                if cache.key == key {
                    cache.colors = colors;
                }
            });
        })
    });
    fallback
}

async fn load(url: url::Url) -> Option<Corners> {
    let response = discophile_core::http::server_client().get(url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let bytes = response.bytes().await.ok()?;
    if bytes.len() > 4_000_000 {
        return None;
    }
    let image = image::load_from_memory(&bytes).ok()?.thumbnail(32, 32).to_rgb8();
    let (width, height) = image.dimensions();
    if width < 2 || height < 2 {
        return None;
    }
    let average = |x0: u32, y0: u32, x1: u32, y1: u32| {
        let (mut r, mut g, mut b, mut n) = (0u64, 0u64, 0u64, 0u64);
        for y in y0..y1 {
            for x in x0..x1 {
                let p = image.get_pixel(x, y);
                r += p[0] as u64;
                g += p[1] as u64;
                b += p[2] as u64;
                n += 1;
            }
        }
        let n = n.max(1);
        Color32::from_rgb((r / n) as u8, (g / n) as u8, (b / n) as u8)
    };
    let (mx, my) = (width / 2, height / 2);
    Some([average(0, 0, mx, my), average(mx, 0, width, my), average(mx, my, width, height), average(0, my, mx, height)])
}

/// Paints the tinted background over `rect`, blended towards the theme background for legibility.
pub fn paint_background(ui: &Ui, rect: Rect, corners: Corners) {
    let palette = Palette::of_ui(ui);
    let strength = if palette.dark { 0.42 } else { 0.24 };
    let tint = |c: Color32| palette.background.lerp_to_gamma(c, strength);
    let [tl, tr, br, bl] = corners.map(tint);
    let center = palette.background.lerp_to_gamma(average(&corners), strength * 0.6);
    let mut mesh = egui::Mesh::default();
    let c = rect.center();
    mesh.colored_vertex(rect.left_top(), tl);
    mesh.colored_vertex(rect.right_top(), tr);
    mesh.colored_vertex(rect.right_bottom(), br);
    mesh.colored_vertex(rect.left_bottom(), bl);
    mesh.colored_vertex(c, center);
    for (a, b) in [(0, 1), (1, 2), (2, 3), (3, 0)] {
        mesh.add_triangle(a, b, 4);
    }
    ui.painter().add(egui::Shape::mesh(mesh));
}

fn average(colors: &Corners) -> Color32 {
    let sum = colors.iter().fold([0u32; 3], |acc, c| [acc[0] + c.r() as u32, acc[1] + c.g() as u32, acc[2] + c.b() as u32]);
    Color32::from_rgb((sum[0] / 4) as u8, (sum[1] / 4) as u8, (sum[2] / 4) as u8)
}
