use iced::widget::{button, container, progress_bar, scrollable, text_input};
use iced::{Background, Border, Color, Degrees, Element, Gradient, Shadow, Theme, Vector, border, gradient};

use crate::theme;

const TOP_TO_BOTTOM: Degrees = Degrees(180.0);
const LIGHT_FROM_ABOVE: f32 = 0.05;
const SHADE_BELOW: f32 = 0.10;
pub const RIM_ALPHA: f32 = 0.02;
const HOVER_LIFT: f32 = 0.08;
const PRESS_SINK: f32 = 0.12;
const DISABLED_STRENGTH: f32 = 0.5;
const WELL_TOP_SHADE: f32 = 0.15;
const WELL_EDGE_ALPHA: f32 = 0.35;
const SCROLLER_ALPHA: f32 = 0.05;
const GRABBED_SCROLLER_ALPHA: f32 = 0.12;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Elevation {
    Control,
    Panel,
    Floating,
}

impl Elevation {
    fn shadow(self) -> Shadow {
        let (alpha, offset_y, blur_radius) = match self {
            Self::Control => (0.5, 1.0, 3.0),
            Self::Panel => (0.45, 4.0, 18.0),
            Self::Floating => (0.55, 8.0, 28.0),
        };
        Shadow { color: Color::BLACK.scale_alpha(alpha), offset: Vector::new(0.0, offset_y), blur_radius }
    }
}

fn top_to_bottom(top: Color, bottom: Color) -> Background {
    Background::Gradient(Gradient::Linear(
        gradient::Linear::new(TOP_TO_BOTTOM).add_stop(0.0, top).add_stop(1.0, bottom),
    ))
}

pub struct Shading {
    pub top: Color,
    pub bottom: Color,
}

pub fn lit_shading(base: Color) -> Shading {
    Shading {
        top: theme::mix(base, Color::WHITE, LIGHT_FROM_ABOVE),
        bottom: theme::mix(base, Color::BLACK, SHADE_BELOW),
    }
}

pub fn lit_from_above(base: Color) -> Background {
    let Shading { top, bottom } = lit_shading(base);
    top_to_bottom(top, bottom)
}

fn pressed_in(base: Color) -> Background {
    let Shading { top, bottom } = lit_shading(base);
    top_to_bottom(bottom, top)
}

pub fn outline(radius: f32, color: Color) -> Border {
    border::rounded(radius).width(1).color(color)
}

pub fn rim(radius: f32) -> Border {
    outline(radius, Color::WHITE.scale_alpha(RIM_ALPHA))
}

pub fn surface(base: Color, radius: f32) -> container::Style {
    container::Style { background: Some(lit_from_above(base)), border: rim(radius), ..container::Style::default() }
        .color(theme::TEXT)
}

pub fn well(base: Color, radius: f32) -> container::Style {
    container::Style { background: Some(sunken(base)), border: well_edge(radius), ..container::Style::default() }
}

fn sunken(base: Color) -> Background {
    top_to_bottom(theme::mix(base, Color::BLACK, WELL_TOP_SHADE), base)
}

fn well_edge(radius: f32) -> Border {
    outline(radius, Color::BLACK.scale_alpha(WELL_EDGE_ALPHA))
}

pub fn casting_shadow<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    radius: f32,
    elevation: Elevation,
) -> container::Container<'a, Message> {
    container(content).style(move |_| container::Style {
        border: border::rounded(radius),
        shadow: elevation.shadow(),
        ..container::Style::default()
    })
}

pub fn raised_button(base: Color, text_color: Color, radius: f32) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let background = match status {
            button::Status::Active => lit_from_above(base),
            button::Status::Hovered => lit_from_above(theme::mix(base, Color::WHITE, HOVER_LIFT)),
            button::Status::Pressed => pressed_in(theme::mix(base, Color::BLACK, PRESS_SINK)),
            button::Status::Disabled => Background::Color(base.scale_alpha(DISABLED_STRENGTH)),
        };
        let text_color =
            if status == button::Status::Disabled { text_color.scale_alpha(DISABLED_STRENGTH) } else { text_color };
        button::Style { background: Some(background), text_color, border: rim(radius), ..button::Style::default() }
    }
}

pub fn well_input(radius: f32) -> impl Fn(&Theme, text_input::Status) -> text_input::Style {
    move |t, status| {
        let focused = matches!(status, text_input::Status::Focused { .. });
        let border = if focused { outline(radius, theme::ACCENT) } else { well_edge(radius) };
        text_input::Style { background: sunken(theme::WELL), border, ..text_input::default(t, status) }
    }
}

pub fn meter(fill: Color, radius: f32) -> progress_bar::Style {
    progress_bar::Style { background: sunken(theme::WELL), bar: lit_from_above(fill), border: well_edge(radius) }
}

pub fn slim_scrollbar(t: &Theme, status: scrollable::Status) -> scrollable::Style {
    let grabbed = matches!(
        status,
        scrollable::Status::Hovered { is_vertical_scrollbar_hovered: true, .. }
            | scrollable::Status::Dragged { is_vertical_scrollbar_dragged: true, .. }
    );
    let alpha = if grabbed { GRABBED_SCROLLER_ALPHA } else { SCROLLER_ALPHA };
    let rail = scrollable::Rail {
        background: None,
        border: border::rounded(u32::MAX),
        scroller: scrollable::Scroller {
            background: Color::WHITE.scale_alpha(alpha).into(),
            border: border::rounded(u32::MAX),
        },
    };
    scrollable::Style { vertical_rail: rail, horizontal_rail: rail, ..scrollable::default(t, status) }
}
