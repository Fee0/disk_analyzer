use std::time::{Duration, Instant};

use dscan_core::{ChartPoint, Segment, SegmentStyle, Tree, format_size};
use iced::widget::canvas::{self, Event, Frame, Geometry, Path, Stroke, Text, gradient};
use iced::widget::{canvas as canvas_widget, stack};
use iced::{Color, Element, Fill, Pixels, Point, Rectangle, Renderer, Size, Theme, Vector, mouse};

use crate::app::Message;
use crate::depth::{self, Shading};
use crate::fmt::{count_phrase, generous_text_width, truncate};
use crate::theme;

const DOUBLE_CLICK: Duration = Duration::from_millis(400);
const SOFT_SHADOW_LAYERS: usize = 6;
const SOFT_SHADOW_LAYER_ALPHA: f32 = 0.05;
pub const SOFT_SHADOW_DROP: f32 = 2.0;
pub const SEPARATOR_GAP: f32 = 1.0;
pub const SELECTION_OUTLINE_WIDTH: f32 = 2.0;
const TOOLTIP_RADIUS: f32 = 6.0;
const TOOLTIP_SHADOW_DROP: f32 = 4.0;
const TOOLTIP_NAME_MAX_CHARS: usize = 48;
const TOOLTIP_NAME_SIZE: f32 = 13.0;
const TOOLTIP_DETAIL_SIZE: f32 = 12.0;
const TOOLTIP_PADDING_X: f32 = 10.0;
const TOOLTIP_MIN_WIDTH: f32 = 80.0;
const TOOLTIP_HEIGHT: f32 = 44.0;
const TOOLTIP_CURSOR_OFFSET: f32 = 14.0;
const TOOLTIP_FLIPPED_GAP: f32 = 8.0;
const IRRELEVANT_SEGMENT_STRENGTH: f32 = 0.22;
pub const OPAQUE: f32 = 1.0;
const GO_UP_OUTLINE_ALPHA: f32 = 0.5;
const GO_UP_OUTLINE_WIDTH: f32 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pointed {
    GoUp,
    Segment(Segment),
    Nothing,
}

#[derive(Default)]
pub struct PointerState {
    hover: Option<Segment>,
    last_click: Option<(Instant, Segment)>,
}

fn redraw_so_the_tooltip_follows_the_cursor() -> canvas::Action<Message> {
    canvas::Action::request_redraw()
}

pub trait CursorOverlay {
    fn draw_cursor_overlay(&self, renderer: &Renderer, bounds: Rectangle, cursor: mouse::Cursor) -> Geometry;
}

struct OverTheChart<Chart>(Chart);

impl<Chart: CursorOverlay> canvas::Program<Message> for OverTheChart<Chart> {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        vec![self.0.draw_cursor_overlay(renderer, bounds, cursor)]
    }
}

pub fn chart_with_cursor_overlay<'a, Chart>(chart: Chart) -> Element<'a, Message>
where
    Chart: canvas::Program<Message> + CursorOverlay + Copy + 'a,
{
    let drawn_in_a_layer_above_the_chart_s_text = canvas_widget(OverTheChart(chart)).width(Fill).height(Fill);
    stack![canvas_widget(chart).width(Fill).height(Fill), drawn_in_a_layer_above_the_chart_s_text].into()
}

pub fn chart_point(p: Point) -> ChartPoint {
    ChartPoint { x: p.x, y: p.y }
}

pub fn iced_point(p: ChartPoint) -> Point {
    Point::new(p.x, p.y)
}

pub fn on_pointer(
    state: &mut PointerState,
    event: &Event,
    pointed: impl FnOnce() -> Option<Pointed>,
    hovered_in_the_app: Option<Segment>,
) -> Option<canvas::Action<Message>> {
    match event {
        Event::Mouse(mouse::Event::CursorMoved { .. } | mouse::Event::CursorLeft) => {
            let hit = match pointed() {
                Some(Pointed::Segment(segment)) => Some(segment),
                _ => None,
            };
            let browser_forgot_the_segment_under_the_cursor = hit.is_some() && hit != hovered_in_the_app;
            if hit != state.hover || browser_forgot_the_segment_under_the_cursor {
                state.hover = hit;
                return Some(canvas::Action::publish(Message::Hover(hit)));
            }
            hit.map(|_| redraw_so_the_tooltip_follows_the_cursor())
        }
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
            let msg = match pointed()? {
                Pointed::GoUp => Message::GoUp,
                Pointed::Segment(segment) => {
                    let now = Instant::now();
                    if matches!(state.last_click, Some((t, s)) if s == segment && now - t < DOUBLE_CLICK) {
                        state.last_click = None;
                        Message::Activate(segment)
                    } else {
                        state.last_click = Some((now, segment));
                        Message::Press(segment)
                    }
                }
                Pointed::Nothing => Message::Deselect,
            };
            Some(canvas::Action::publish(msg).and_capture())
        }
        _ => None,
    }
}

pub fn interaction(pointed: Option<Pointed>, can_go_up: bool) -> mouse::Interaction {
    match pointed {
        Some(Pointed::Segment(_)) => mouse::Interaction::Pointer,
        Some(Pointed::GoUp) if can_go_up => mouse::Interaction::Pointer,
        _ => mouse::Interaction::default(),
    }
}

pub fn style_color(style: SegmentStyle, layer: f32) -> Color {
    let color = theme::segment_color(style.kind, layer, style.branch);
    if style.relevant { color } else { theme::mix(theme::BG, color, IRRELEVANT_SEGMENT_STRENGTH) }
}

pub fn fill_soft_shadow(frame: &mut Frame, outset_shape: impl Fn(f32) -> Path) {
    for outset in 0..SOFT_SHADOW_LAYERS {
        frame.fill(&outset_shape(outset as f32), Color::BLACK.scale_alpha(SOFT_SHADOW_LAYER_ALPHA));
    }
}

fn fill_rounded_soft_shadow(frame: &mut Frame, top_left: Point, size: Size, radius: f32, drop: f32) {
    let dropped = top_left + Vector::new(0.0, drop);
    fill_soft_shadow(frame, |outset| {
        let grown = Size::new(size.width + 2.0 * outset, size.height + 2.0 * outset);
        Path::rounded_rectangle(dropped - Vector::new(outset, outset), grown, (radius + outset).into())
    });
}

pub fn lit_from_above(base: Color, top: f32, bottom: f32, opacity: f32) -> gradient::Linear {
    let Shading { top: lit, bottom: shaded } = depth::lit_shading(base);
    gradient::Linear::new(Point::new(0.0, top), Point::new(0.0, bottom))
        .add_stop(0.0, lit.scale_alpha(opacity))
        .add_stop(1.0, shaded.scale_alpha(opacity))
}

pub fn rim() -> Stroke<'static> {
    Stroke::default().with_color(Color::WHITE.scale_alpha(depth::RIM_ALPHA)).with_width(1.0)
}

pub fn draw_raised_card(frame: &mut Frame, top_left: Point, size: Size, radius: f32, drop: f32, base: Color) {
    fill_rounded_soft_shadow(frame, top_left, size, radius, drop);
    let card = Path::rounded_rectangle(top_left, size, radius.into());
    frame.fill(&card, lit_from_above(base, top_left.y, top_left.y + size.height, OPAQUE));
    frame.stroke(&card, rim());
}

pub fn selection_outline() -> Stroke<'static> {
    Stroke::default().with_color(Color::WHITE).with_width(SELECTION_OUTLINE_WIDTH)
}

pub fn go_up_outline() -> Stroke<'static> {
    Stroke::default().with_color(Color::WHITE.scale_alpha(GO_UP_OUTLINE_ALPHA)).with_width(GO_UP_OUTLINE_WIDTH)
}

pub struct TooltipSubject<'a> {
    pub tree: &'a Tree,
    pub segment: Segment,
    pub size: u64,
    pub total: u64,
}

pub fn segment_name(tree: &Tree, segment: Segment) -> String {
    match segment {
        Segment::Node(n) => tree.name(n).into_owned(),
        Segment::Small { count, .. } => count_phrase(count as u64, "smaller item", "smaller items"),
    }
}

pub fn draw_tooltip(frame: &mut Frame, bounds: Rectangle, cursor: Point, subject: TooltipSubject<'_>) {
    let name = segment_name(subject.tree, subject.segment);
    let pct = if subject.total > 0 { subject.size as f64 * 100.0 / subject.total as f64 } else { 0.0 };
    let line1 = truncate(&name, TOOLTIP_NAME_MAX_CHARS);
    let line2 = format!("{} · {:.1}%", format_size(subject.size), pct);
    let text_width =
        generous_text_width(&line1, TOOLTIP_NAME_SIZE).max(generous_text_width(&line2, TOOLTIP_DETAIL_SIZE));
    let w = (text_width + 2.0 * TOOLTIP_PADDING_X).max(TOOLTIP_MIN_WIDTH);
    let h = TOOLTIP_HEIGHT;
    let mut pos = cursor + Vector::new(TOOLTIP_CURSOR_OFFSET, TOOLTIP_CURSOR_OFFSET);
    if pos.x + w > bounds.width {
        pos.x = cursor.x - w - TOOLTIP_FLIPPED_GAP;
    }
    if pos.y + h > bounds.height {
        pos.y = cursor.y - h - TOOLTIP_FLIPPED_GAP;
    }
    let size = Size::new(w, h);
    draw_raised_card(frame, pos, size, TOOLTIP_RADIUS, TOOLTIP_SHADOW_DROP, theme::PANEL_HI);
    let lines = [(line1, 7.0, theme::TEXT, TOOLTIP_NAME_SIZE), (line2, 24.0, theme::TEXT_DIM, TOOLTIP_DETAIL_SIZE)];
    for (content, dy, color, size) in lines {
        frame.fill_text(Text {
            content,
            position: pos + Vector::new(TOOLTIP_PADDING_X, dy),
            color,
            size: Pixels(size),
            ..Text::default()
        });
    }
}
