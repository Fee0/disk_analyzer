//! Colors: a dark palette and the per-kind segment colors.

use dscan_core::Kind;
use iced::Color;

pub const BG: Color = Color::from_rgb8(0x24, 0x26, 0x2b);
pub const PANEL: Color = Color::from_rgb8(0x2c, 0x2f, 0x35);
pub const PANEL_HI: Color = Color::from_rgb8(0x36, 0x3a, 0x42);
pub const TEXT: Color = Color::from_rgb8(0xe8, 0xea, 0xee);
pub const TEXT_DIM: Color = Color::from_rgb8(0x9a, 0x9f, 0xab);
pub const ACCENT: Color = Color::from_rgb8(0x4f, 0x8c, 0xf7);
pub const DANGER: Color = Color::from_rgb8(0xe5, 0x5b, 0x5b);

/// (hue degrees, saturation, lightness) per kind.
fn hsl_of(kind: Kind) -> (f32, f32, f32) {
    match kind {
        Kind::Video => (142.0, 0.62, 0.62),
        Kind::Image => (188.0, 0.72, 0.62),
        Kind::Audio => (328.0, 0.68, 0.70),
        Kind::Archive => (44.0, 0.88, 0.66),
        Kind::DiskImage => (24.0, 0.82, 0.64),
        Kind::Document => (212.0, 0.82, 0.70),
        Kind::Code => (264.0, 0.70, 0.74),
        Kind::App => (2.0, 0.70, 0.68),
        Kind::Library => (292.0, 0.50, 0.68),
        Kind::Cache => (168.0, 0.45, 0.54),
        Kind::System => (222.0, 0.18, 0.62),
        Kind::Other => (220.0, 0.10, 0.56),
        Kind::Hidden => (220.0, 0.05, 0.40),
        Kind::Purgeable => (220.0, 0.06, 0.50),
        Kind::SmallItems => (220.0, 0.05, 0.33),
    }
}

fn hsl(h: f32, s: f32, l: f32) -> Color {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = (h.rem_euclid(360.0)) / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    Color::from_rgb(r + m, g + m, b + m)
}

/// Base color of a kind (list dots, legend).
pub fn kind_color(kind: Kind) -> Color {
    let (h, s, l) = hsl_of(kind);
    hsl(h, s, l)
}

/// Segment color: kind hue, darker on outer rings, with a slight
/// per-branch hue shift so neighbouring branches stay distinguishable.
pub fn segment_color(kind: Kind, ring: usize, branch: u32) -> Color {
    let (h, s, l) = hsl_of(kind);
    let shift = if kind.is_synthetic() { 0.0 } else { ((branch % 5) as f32 - 2.0) * 4.0 };
    hsl(h + shift, s, (l - ring as f32 * 0.035).max(0.25))
}

pub fn mix(a: Color, b: Color, t: f32) -> Color {
    Color {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
}

pub fn app_theme() -> iced::Theme {
    iced::Theme::custom(
        "Disk Analyzer".to_string(),
        iced::theme::Palette {
            background: BG,
            text: TEXT,
            primary: ACCENT,
            success: kind_color(Kind::Video),
            warning: kind_color(Kind::Archive),
            danger: DANGER,
        },
    )
}
