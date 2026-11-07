use dscan_core::Kind;
use iced::Color;

pub const BG: Color = Color::from_rgb8(0x24, 0x26, 0x2b);
pub const PANEL: Color = Color::from_rgb8(0x2c, 0x2f, 0x35);
pub const PANEL_HI: Color = Color::from_rgb8(0x36, 0x3a, 0x42);
pub const TEXT: Color = Color::from_rgb8(0xe8, 0xea, 0xee);
pub const TEXT_DIM: Color = Color::from_rgb8(0x9a, 0x9f, 0xab);
pub const ACCENT: Color = Color::from_rgb8(0x4f, 0x8c, 0xf7);
pub const DANGER: Color = Color::from_rgb8(0xe5, 0x5b, 0x5b);
pub const NEUTRAL_BUTTON: Color = Color::from_rgb8(0x4a, 0x4f, 0x5a);

const LIGHTNESS_LOSS_PER_RING: f32 = 0.035;
const MIN_SEGMENT_LIGHTNESS: f32 = 0.25;
const BRANCH_HUE_VARIANTS: u32 = 5;
const BRANCH_HUE_STEP_DEGREES: f32 = 4.0;

#[derive(Clone, Copy)]
struct Hsl {
    hue_degrees: f32,
    saturation: f32,
    lightness: f32,
}

const fn hsl(hue_degrees: f32, saturation: f32, lightness: f32) -> Hsl {
    Hsl { hue_degrees, saturation, lightness }
}

impl Hsl {
    fn to_color(self) -> Color {
        let Hsl { hue_degrees: h, saturation: s, lightness: l } = self;
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
}

fn base_hsl(kind: Kind) -> Hsl {
    match kind {
        Kind::Video => hsl(142.0, 0.62, 0.62),
        Kind::Image => hsl(188.0, 0.72, 0.62),
        Kind::Audio => hsl(328.0, 0.68, 0.70),
        Kind::Archive => hsl(44.0, 0.88, 0.66),
        Kind::DiskImage => hsl(24.0, 0.82, 0.64),
        Kind::Document => hsl(212.0, 0.82, 0.70),
        Kind::Code => hsl(264.0, 0.70, 0.74),
        Kind::App => hsl(2.0, 0.70, 0.68),
        Kind::Library => hsl(292.0, 0.50, 0.68),
        Kind::Cache => hsl(168.0, 0.45, 0.54),
        Kind::System => hsl(222.0, 0.18, 0.62),
        Kind::Other => hsl(220.0, 0.10, 0.56),
        Kind::Hidden => hsl(220.0, 0.05, 0.40),
        Kind::Purgeable => hsl(220.0, 0.06, 0.50),
        Kind::SmallItems => hsl(220.0, 0.05, 0.33),
    }
}

pub fn kind_color(kind: Kind) -> Color {
    base_hsl(kind).to_color()
}

fn hue_shift_distinguishing_neighbour_branches(kind: Kind, branch: u32) -> f32 {
    if kind.is_synthetic() {
        return 0.0;
    }
    let centered_variant = (branch % BRANCH_HUE_VARIANTS) as f32 - (BRANCH_HUE_VARIANTS / 2) as f32;
    centered_variant * BRANCH_HUE_STEP_DEGREES
}

fn darkened_for_ring(lightness: f32, ring: f32) -> f32 {
    (lightness - ring * LIGHTNESS_LOSS_PER_RING).max(MIN_SEGMENT_LIGHTNESS)
}

pub fn segment_color(kind: Kind, ring: f32, branch: u32) -> Color {
    let base = base_hsl(kind);
    Hsl {
        hue_degrees: base.hue_degrees + hue_shift_distinguishing_neighbour_branches(kind, branch),
        saturation: base.saturation,
        lightness: darkened_for_ring(base.lightness, ring),
    }
    .to_color()
}

pub fn mix(a: Color, b: Color, t: f32) -> Color {
    Color { r: a.r + (b.r - a.r) * t, g: a.g + (b.g - a.g) * t, b: a.b + (b.b - a.b) * t, a: a.a + (b.a - a.a) * t }
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
