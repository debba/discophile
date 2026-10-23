//! Discophile's visual language, matching the iOS app's palette and hierarchy at desktop density.

use egui::{Color32, FontData, FontDefinitions, FontFamily, FontId, RichText, TextStyle, Visuals};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

pub const fn hex(value: u32) -> Color32 {
    Color32::from_rgb((value >> 16) as u8, (value >> 8) as u8, value as u8)
}

/// Colors resolved for the current light or dark appearance.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub dark: bool,
    pub accent: Color32,
    pub secondary: Color32,
    pub positive: Color32,
    /// Filled call-to-action buttons and badges.
    pub action: Color32,
    pub background: Color32,
    pub surface: Color32,
    pub surface_raised: Color32,
    pub sidebar: Color32,
    pub separator: Color32,
    pub text: Color32,
    pub hover: Color32,
    pub danger: Color32,
}

impl Palette {
    pub fn dark() -> Self {
        Self {
            dark: true,
            accent: hex(0xFFA3BE),
            secondary: hex(0xA8A3AF),
            positive: hex(0x96DAB1),
            action: hex(0x9F1640),
            background: hex(0x141418),
            surface: hex(0x1D1D23),
            surface_raised: hex(0x292930),
            sidebar: hex(0x101014),
            separator: Color32::from_white_alpha(22),
            text: hex(0xF6F2F7),
            hover: Color32::from_white_alpha(14),
            danger: hex(0xFF8A80),
        }
    }
    pub fn light() -> Self {
        Self {
            dark: false,
            accent: hex(0xAA1745),
            secondary: hex(0x5D5461),
            positive: hex(0x23613F),
            action: hex(0x9F1640),
            background: hex(0xF7F7FA),
            surface: Color32::WHITE,
            surface_raised: hex(0xEDEDF2),
            sidebar: hex(0xEFEFF4),
            separator: Color32::from_black_alpha(22),
            text: hex(0x1C1A1E),
            hover: Color32::from_black_alpha(12),
            danger: hex(0xB3261E),
        }
    }
    pub fn of(ctx: &egui::Context) -> Self {
        if ctx.global_style().visuals.dark_mode { Self::dark() } else { Self::light() }
    }
    pub fn of_ui(ui: &egui::Ui) -> Self {
        if ui.visuals().dark_mode { Self::dark() } else { Self::light() }
    }
}

/// Artwork placeholder gradients, chosen by a stable hash of the item id (same as iOS).
pub const ARTWORK_PALETTES: [[u32; 2]; 6] =
    [[0x596E73, 0x152C38], [0xE4A398, 0xA34559], [0x504B91, 0x171A40], [0x91A89B, 0x294438], [0x779CC3, 0x24315A], [0xD6AF7B, 0x78462E]];

pub fn artwork_seed(id: &str) -> usize {
    (id.bytes().fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32)) % 6) as usize
}

pub fn semibold() -> FontFamily {
    FontFamily::Name("semibold".into())
}
pub fn bold() -> FontFamily {
    FontFamily::Name("bold".into())
}
/// Filled icon glyphs, like SF Symbols' `.fill` variants.
pub fn icon_fill() -> FontFamily {
    FontFamily::Name("icon-fill".into())
}

/// An outlined icon at `size` points.
pub fn icon(glyph: &str, size: f32) -> RichText {
    RichText::new(glyph).font(FontId::new(size, FontFamily::Proportional))
}

/// A filled icon at `size` points.
pub fn icon_filled(glyph: &str, size: f32) -> RichText {
    RichText::new(glyph).font(FontId::new(size, icon_fill()))
}

pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let faces: [(&str, &'static [u8]); 3] = [
        ("inter", include_bytes!("../assets/fonts/Inter-Regular.ttf")),
        ("inter-semibold", include_bytes!("../assets/fonts/Inter-SemiBold.ttf")),
        ("inter-bold", include_bytes!("../assets/fonts/Inter-Bold.ttf")),
    ];
    for (name, bytes) in faces {
        fonts.font_data.insert(name.into(), FontData::from_static(bytes).into());
    }
    fonts.font_data.insert("phosphor".into(), FontData::from_static(include_bytes!("../assets/fonts/Phosphor.ttf")).into());
    fonts.font_data.insert("phosphor-fill".into(), FontData::from_static(include_bytes!("../assets/fonts/Phosphor-Fill.ttf")).into());
    // Phosphor must precede egui's emoji font, which also covers part of the private-use area.
    let defaults: Vec<String> = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let family = |primary: &str| {
        let mut list = vec![primary.to_string(), "phosphor".to_string()];
        list.extend(defaults.iter().cloned());
        list
    };
    fonts.families.insert(FontFamily::Proportional, family("inter"));
    fonts.families.insert(semibold(), family("inter-semibold"));
    fonts.families.insert(bold(), family("inter-bold"));
    fonts.families.insert(icon_fill(), vec!["phosphor-fill".into(), "inter".into()]);
    ctx.set_fonts(fonts);
}

pub fn apply(ctx: &egui::Context, dark: bool) {
    // Pin egui's theme to the resolved appearance so styles and `Palette::of` agree with the setting.
    ctx.set_theme(if dark { egui::ThemePreference::Dark } else { egui::ThemePreference::Light });
    let palette = if dark { Palette::dark() } else { Palette::light() };
    let mut visuals = if dark { Visuals::dark() } else { Visuals::light() };
    visuals.panel_fill = palette.background;
    visuals.window_fill = palette.surface;
    visuals.extreme_bg_color = if dark { hex(0x0B0A0D) } else { hex(0xF4F1F5) };
    visuals.faint_bg_color = palette.surface;
    visuals.override_text_color = Some(palette.text);
    visuals.hyperlink_color = palette.accent;
    visuals.selection.bg_fill = palette.accent.gamma_multiply(if dark { 0.35 } else { 0.25 });
    visuals.selection.stroke.color = palette.accent;
    visuals.widgets.noninteractive.bg_stroke.color = palette.separator;
    visuals.widgets.inactive.weak_bg_fill = palette.surface_raised;
    visuals.widgets.inactive.bg_fill = palette.surface_raised;
    visuals.widgets.hovered.weak_bg_fill = palette.hover;
    visuals.widgets.active.weak_bg_fill = palette.accent.gamma_multiply(0.3);
    for w in [&mut visuals.widgets.inactive, &mut visuals.widgets.hovered, &mut visuals.widgets.active, &mut visuals.widgets.open] {
        w.corner_radius = egui::CornerRadius::same(7);
    }
    visuals.window_corner_radius = egui::CornerRadius::same(18);
    visuals.menu_corner_radius = egui::CornerRadius::same(12);
    visuals.window_stroke.color = palette.separator;
    visuals.slider_trailing_fill = true;
    ctx.set_visuals(visuals);

    ctx.all_styles_mut(|style| {
        use FontFamily::Proportional;
        style.text_styles = [
            (TextStyle::Heading, FontId::new(28.0, bold())),
            (TextStyle::Body, FontId::new(14.0, Proportional)),
            (TextStyle::Monospace, FontId::new(13.0, FontFamily::Monospace)),
            (TextStyle::Button, FontId::new(14.0, Proportional)),
            (TextStyle::Small, FontId::new(11.5, Proportional)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(10.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.spacing.interact_size.y = 30.0;
        style.spacing.slider_rail_height = 4.0;
        style.spacing.scroll.floating = true;
        style.spacing.scroll.bar_width = 8.0;
    });
}

/// Type scale mirroring SwiftUI's text styles, scaled for desktop reading distance.
#[derive(Clone, Copy, Debug)]
pub enum Font {
    LargeTitle,
    Title,
    Title2,
    Title3,
    Headline,
    Body,
    Callout,
    Subheadline,
    Footnote,
    Caption,
}

impl Font {
    pub fn id(self) -> FontId {
        match self {
            Font::LargeTitle => FontId::new(30.0, bold()),
            Font::Title => FontId::new(25.0, bold()),
            Font::Title2 => FontId::new(20.0, bold()),
            Font::Title3 => FontId::new(17.0, semibold()),
            Font::Headline => FontId::new(14.5, semibold()),
            Font::Body => FontId::new(14.0, FontFamily::Proportional),
            Font::Callout => FontId::new(13.5, FontFamily::Proportional),
            Font::Subheadline => FontId::new(13.0, FontFamily::Proportional),
            Font::Footnote => FontId::new(12.0, FontFamily::Proportional),
            Font::Caption => FontId::new(11.0, FontFamily::Proportional),
        }
    }
}

pub fn text(value: impl Into<String>, font: Font) -> RichText {
    RichText::new(value).font(font.id())
}

/// Secondary-colored text, as iOS uses for subtitles.
pub fn secondary(ui: &egui::Ui, value: impl Into<String>, font: Font) -> RichText {
    text(value, font).color(Palette::of_ui(ui).secondary)
}

pub fn accent(ui: &egui::Ui, value: impl Into<String>, font: Font) -> RichText {
    text(value, font).color(Palette::of_ui(ui).accent)
}

/// Phosphor glyphs standing in for the SF Symbols used on iOS.
pub use crate::icons;
