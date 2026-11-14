use dscan_core::{NodeId, Segment, TileRect, Tree, format_size};
use iced::widget::canvas::{Frame, Path, Stroke, Text};
use iced::{Color, Font, Pixels, Point, Size, Vector, alignment, font};

use crate::chart_canvas::{
    SELECTION_OUTLINE_WIDTH, SEPARATOR_GAP, SOFT_SHADOW_DROP, can_go_up, draw_raised_card, segment_name,
    selection_outline,
};
use crate::fmt::{fit_width, generous_text_width};
use crate::theme;

const FOCUS_BAR_RADIUS: f32 = 8.0;
const FOCUS_BAR_PADDING_X: f32 = 12.0;
const FOCUS_NAME_SIZE: f32 = 14.0;
const FOCUS_TOTAL_SIZE: f32 = 15.0;
const FOCUS_BAR_LABEL_GAP: f32 = 16.0;
const UP_CHEVRON_HALF_WIDTH: f32 = 4.5;
const UP_CHEVRON_HALF_HEIGHT: f32 = 2.5;
const UP_CHEVRON_ROOM: f32 = 18.0;
const UP_CHEVRON_STROKE: f32 = 1.6;
const LABEL_NAME_SIZE: f32 = 12.0;
const LABEL_SIZE_SIZE: f32 = 11.0;
const LABEL_PADDING: f32 = 5.0;
const LABEL_LINE_GAP: f32 = 15.0;
const LABEL_MIN_WIDTH: f32 = 60.0;
const LABEL_MIN_HEIGHT: f32 = 34.0;
pub const LABEL_SIZE_LINE_STRENGTH: f32 = 0.75;
const LIGHT_TILE_LUMINANCE: f32 = 0.45;
const HOVER_FILL_ALPHA: f32 = 0.16;
const HOVER_OUTLINE_ALPHA: f32 = 0.6;
const SELECTION_FILL_ALPHA: f32 = 0.12;

pub const LABEL_FONT: Font = Font { weight: font::Weight::Semibold, ..Font::DEFAULT };

pub fn top_left(rect: &TileRect) -> Point {
    Point::new(rect.x, rect.y)
}

pub fn size_of(rect: &TileRect) -> Size {
    Size::new(rect.width, rect.height)
}

pub fn rectangle(rect: &TileRect) -> Path {
    Path::rectangle(top_left(rect), size_of(rect))
}

pub fn with_separator(rect: &TileRect) -> TileRect {
    let room_for_a_gap = rect.width > 2.0 * SEPARATOR_GAP && rect.height > 2.0 * SEPARATOR_GAP;
    if room_for_a_gap { rect.inset(SEPARATOR_GAP / 2.0) } else { *rect }
}

fn luminance(c: Color) -> f32 {
    0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b
}

pub fn text_color_on(background: Color) -> Color {
    if luminance(background) > LIGHT_TILE_LUMINANCE { theme::BG } else { theme::TEXT }
}

pub fn outline_selected(frame: &mut Frame, rect: &TileRect) {
    let outline = rectangle(&rect.inset(SELECTION_OUTLINE_WIDTH / 2.0));
    frame.fill(&outline, Color::WHITE.scale_alpha(SELECTION_FILL_ALPHA));
    frame.stroke(&outline, selection_outline());
}

pub fn outline_hovered(frame: &mut Frame, rect: &TileRect) {
    let outline = rectangle(&with_separator(rect));
    frame.fill(&outline, Color::WHITE.scale_alpha(HOVER_FILL_ALPHA));
    frame.stroke(&outline, Stroke::default().with_color(Color::WHITE.scale_alpha(HOVER_OUTLINE_ALPHA)));
}

pub struct Label<'a> {
    pub tree: &'a Tree,
    pub segment: Segment,
    pub size: u64,
    pub background: Color,
}

fn has_room_for_a_label(rect: &TileRect) -> bool {
    rect.width >= LABEL_MIN_WIDTH && rect.height >= LABEL_MIN_HEIGHT
}

pub fn draw_label(frame: &mut Frame, rect: &TileRect, label: Label<'_>) {
    if !has_room_for_a_label(rect) {
        return;
    }
    let room = rect.width - 2.0 * LABEL_PADDING;
    let color = text_color_on(label.background);
    let origin = top_left(rect) + Vector::new(LABEL_PADDING, LABEL_PADDING);
    frame.fill_text(Text {
        content: fit_width(&segment_name(label.tree, label.segment), room, LABEL_NAME_SIZE),
        position: origin,
        color,
        size: Pixels(LABEL_NAME_SIZE),
        font: LABEL_FONT,
        ..Text::default()
    });
    frame.fill_text(Text {
        content: fit_width(&format_size(label.size), room, LABEL_SIZE_SIZE),
        position: origin + Vector::new(0.0, LABEL_LINE_GAP),
        color: color.scale_alpha(LABEL_SIZE_LINE_STRENGTH),
        size: Pixels(LABEL_SIZE_SIZE),
        ..Text::default()
    });
}

pub struct FocusBar<'a> {
    pub rect: TileRect,
    pub tree: &'a Tree,
    pub focus: NodeId,
    pub total: u64,
}

pub fn focus_bar_path(bar: &TileRect) -> Path {
    Path::rounded_rectangle(top_left(bar), size_of(bar), FOCUS_BAR_RADIUS.into())
}

pub fn draw_focus_bar(frame: &mut Frame, focus: FocusBar<'_>) {
    let bar = focus.rect;
    if bar.width <= 0.0 {
        return;
    }
    draw_raised_card(frame, top_left(&bar), size_of(&bar), FOCUS_BAR_RADIUS, SOFT_SHADOW_DROP, theme::PANEL);

    let middle_y = bar.center().y;
    let mut name_x = bar.x + FOCUS_BAR_PADDING_X;
    if can_go_up(focus.tree, focus.focus) {
        draw_up_chevron(frame, Point::new(name_x + UP_CHEVRON_HALF_WIDTH, middle_y));
        name_x += UP_CHEVRON_ROOM;
    }
    let total = format_size(focus.total);
    let total_width = generous_text_width(&total, FOCUS_TOTAL_SIZE);
    let total_x = bar.right() - FOCUS_BAR_PADDING_X;
    frame.fill_text(Text {
        content: total,
        position: Point::new(total_x, middle_y),
        color: theme::kind_color(focus.tree.node(focus.focus).kind()),
        size: Pixels(FOCUS_TOTAL_SIZE),
        font: LABEL_FONT,
        align_x: alignment::Horizontal::Right.into(),
        align_y: alignment::Vertical::Center,
        ..Text::default()
    });
    let name_room = total_x - total_width - FOCUS_BAR_LABEL_GAP - name_x;
    frame.fill_text(Text {
        content: fit_width(&focus.tree.name(focus.focus), name_room, FOCUS_NAME_SIZE),
        position: Point::new(name_x, middle_y),
        color: theme::TEXT,
        size: Pixels(FOCUS_NAME_SIZE),
        align_y: alignment::Vertical::Center,
        ..Text::default()
    });
}

fn draw_up_chevron(frame: &mut Frame, center: Point) {
    let chevron = Path::new(|b| {
        b.move_to(center + Vector::new(-UP_CHEVRON_HALF_WIDTH, UP_CHEVRON_HALF_HEIGHT));
        b.line_to(center + Vector::new(0.0, -UP_CHEVRON_HALF_HEIGHT));
        b.line_to(center + Vector::new(UP_CHEVRON_HALF_WIDTH, UP_CHEVRON_HALF_HEIGHT));
    });
    frame.stroke(&chevron, Stroke::default().with_color(theme::TEXT_DIM).with_width(UP_CHEVRON_STROKE));
}
