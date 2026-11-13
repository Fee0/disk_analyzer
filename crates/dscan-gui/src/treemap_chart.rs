use dscan_core::{
    ChartFrame, FrameTile, NodeId, Segment, Tile, TileLook, TileRect, Tree, Treemap, TreemapHit, format_size,
};
use iced::widget::canvas::{self, Cache, Event, Fill, Frame, Geometry, Path, Stroke, Text};
use iced::{Color, Font, Pixels, Point, Rectangle, Renderer, Size, Theme, Vector, alignment, font, mouse};

use crate::app::Message;
use crate::chart_canvas::{
    CursorOverlay, Pointed, PointerState, SELECTION_OUTLINE_WIDTH, SEPARATOR_GAP, SOFT_SHADOW_DROP, TooltipSubject,
    chart_point, draw_raised_card, draw_tooltip, go_up_outline, interaction, lit_from_above, on_pointer, segment_name,
    selection_outline, style_color,
};
use crate::fmt::{fit_width, generous_text_width};
use crate::theme;

const FOLDER_TINT: f32 = 0.10;
const FOLDER_TINT_PER_DEPTH: f32 = 0.07;
const DEEPEST_FOLDER_TINT: f32 = 0.45;
const FOLDER_OUTLINE_STRENGTH: f32 = 0.45;
const HEADER_TEXT_SIZE: f32 = 11.5;
const HEADER_PADDING_X: f32 = 5.0;
const HEADER_LABEL_GAP: f32 = 8.0;
const HEADER_NAME_ROOM_WORTH_A_SIZE: f32 = 36.0;
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
const LABEL_SIZE_LINE_STRENGTH: f32 = 0.75;
const LIGHT_TILE_LUMINANCE: f32 = 0.45;
const HOVER_FILL_ALPHA: f32 = 0.16;
const HOVER_OUTLINE_ALPHA: f32 = 0.6;
const SELECTION_FILL_ALPHA: f32 = 0.12;

const LABEL_FONT: Font = Font { weight: font::Weight::Semibold, ..Font::DEFAULT };

#[derive(Clone, Copy)]
pub struct TreemapChart<'a> {
    pub tree: &'a Tree,
    pub map: &'a Treemap,
    pub chart_cache: &'a Cache,
    pub highlight_cache: &'a Cache,
    pub hovered: Option<Segment>,
    pub selected: Option<NodeId>,
    pub moving: Option<&'a ChartFrame>,
}

fn top_left(rect: &TileRect) -> Point {
    Point::new(rect.x, rect.y)
}

fn size_of(rect: &TileRect) -> Size {
    Size::new(rect.width, rect.height)
}

fn rectangle(rect: &TileRect) -> Path {
    Path::rectangle(top_left(rect), size_of(rect))
}

fn with_separator(rect: &TileRect) -> TileRect {
    let room_for_a_gap = rect.width > 2.0 * SEPARATOR_GAP && rect.height > 2.0 * SEPARATOR_GAP;
    if room_for_a_gap { rect.inset(SEPARATOR_GAP / 2.0) } else { *rect }
}

fn folder_tint(depth: usize) -> f32 {
    (FOLDER_TINT + FOLDER_TINT_PER_DEPTH * depth as f32).min(DEEPEST_FOLDER_TINT)
}

fn look_color(look: &TileLook) -> Color {
    let solid = style_color(look.style, look.depth as f32);
    if look.holds_tiles { theme::mix(theme::BG, solid, folder_tint(look.depth)) } else { solid }
}

fn luminance(c: Color) -> f32 {
    0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b
}

fn text_color_on(background: Color) -> Color {
    if luminance(background) > LIGHT_TILE_LUMINANCE { theme::BG } else { theme::TEXT }
}

fn fill_tile(frame: &mut Frame, tile: &FrameTile) {
    let rect = with_separator(&tile.rect);
    let color = look_color(&tile.look);
    let fill: Fill = if tile.look.holds_tiles {
        color.scale_alpha(tile.opacity).into()
    } else {
        lit_from_above(color, rect.y, rect.bottom(), tile.opacity).into()
    };
    frame.fill_rectangle(top_left(&rect), size_of(&rect), fill);
}

fn outline_folder(frame: &mut Frame, folder: &FrameTile) {
    let edge = with_separator(&folder.rect).inset(0.5);
    let color = style_color(folder.look.style, folder.look.depth as f32);
    let alpha = FOLDER_OUTLINE_STRENGTH * folder.opacity;
    frame.stroke(&rectangle(&edge), Stroke::default().with_color(color.scale_alpha(alpha)).with_width(1.0));
}

impl TreemapChart<'_> {
    fn can_go_up(&self) -> bool {
        self.map.focus != self.tree.root()
    }

    fn pointed(&self, bounds: Rectangle, cursor: mouse::Cursor) -> Option<Pointed> {
        cursor.position_in(bounds).map(|p| match self.map.hit(chart_point(p)) {
            TreemapHit::FocusBar => Pointed::GoUp,
            TreemapHit::Tile(tile) => Pointed::Segment(tile.segment),
            TreemapHit::Nothing => Pointed::Nothing,
        })
    }

    fn draw_tiles(&self, frame: &mut Frame, tiles: &[FrameTile], total: u64) {
        self.draw_focus_bar(frame, total);
        for tile in tiles {
            fill_tile(frame, tile);
        }
        for folder in tiles.iter().filter(|t| t.look.holds_tiles) {
            outline_folder(frame, folder);
        }
    }

    fn draw_resting(&self, frame: &mut Frame) {
        self.draw_tiles(frame, &dscan_core::tiles_at_rest(self.map), self.map.total);
        for tile in &self.map.tiles {
            match tile.header() {
                Some(header) => self.draw_header(frame, tile, header),
                None => self.draw_label(frame, tile),
            }
        }
    }

    fn draw_header(&self, frame: &mut Frame, tile: &Tile, header: TileRect) {
        let color = text_color_on(look_color(&TileLook::of(tile)));
        let middle_y = header.center().y;
        let (left, right) = (header.x + HEADER_PADDING_X, header.right() - HEADER_PADDING_X);
        let size = format_size(tile.size);
        let name_room_beside_the_size = right - generous_text_width(&size, HEADER_TEXT_SIZE) - HEADER_LABEL_GAP - left;
        let name_right = if name_room_beside_the_size >= HEADER_NAME_ROOM_WORTH_A_SIZE {
            frame.fill_text(Text {
                content: size,
                position: Point::new(right, middle_y),
                color: color.scale_alpha(LABEL_SIZE_LINE_STRENGTH),
                size: Pixels(HEADER_TEXT_SIZE),
                align_x: alignment::Horizontal::Right.into(),
                align_y: alignment::Vertical::Center,
                ..Text::default()
            });
            left + name_room_beside_the_size
        } else {
            right
        };
        frame.fill_text(Text {
            content: fit_width(&segment_name(self.tree, tile.segment), name_right - left, HEADER_TEXT_SIZE),
            position: Point::new(left, middle_y),
            color,
            size: Pixels(HEADER_TEXT_SIZE),
            font: LABEL_FONT,
            align_y: alignment::Vertical::Center,
            ..Text::default()
        });
    }

    fn draw_label(&self, frame: &mut Frame, tile: &Tile) {
        let rect = with_separator(&tile.rect);
        if rect.width < LABEL_MIN_WIDTH || rect.height < LABEL_MIN_HEIGHT {
            return;
        }
        let (name, size) = (segment_name(self.tree, tile.segment), format_size(tile.size));
        let room = rect.width - 2.0 * LABEL_PADDING;
        let name = fit_width(&name, room, LABEL_NAME_SIZE);
        let color = text_color_on(look_color(&TileLook::of(tile)));
        let origin = top_left(&rect) + Vector::new(LABEL_PADDING, LABEL_PADDING);
        frame.fill_text(Text {
            content: name,
            position: origin,
            color,
            size: Pixels(LABEL_NAME_SIZE),
            font: LABEL_FONT,
            ..Text::default()
        });
        frame.fill_text(Text {
            content: fit_width(&size, room, LABEL_SIZE_SIZE),
            position: origin + Vector::new(0.0, LABEL_LINE_GAP),
            color: color.scale_alpha(LABEL_SIZE_LINE_STRENGTH),
            size: Pixels(LABEL_SIZE_SIZE),
            ..Text::default()
        });
    }

    fn focus_bar_path(&self) -> Path {
        let bar = &self.map.focus_bar;
        Path::rounded_rectangle(top_left(bar), size_of(bar), FOCUS_BAR_RADIUS.into())
    }

    fn draw_focus_bar(&self, frame: &mut Frame, total: u64) {
        let bar = self.map.focus_bar;
        if bar.width <= 0.0 {
            return;
        }
        draw_raised_card(frame, top_left(&bar), size_of(&bar), FOCUS_BAR_RADIUS, SOFT_SHADOW_DROP, theme::PANEL);

        let middle_y = bar.center().y;
        let mut name_x = bar.x + FOCUS_BAR_PADDING_X;
        if self.can_go_up() {
            self.draw_up_chevron(frame, Point::new(name_x + UP_CHEVRON_HALF_WIDTH, middle_y));
            name_x += UP_CHEVRON_ROOM;
        }
        let total = format_size(total);
        let total_width = generous_text_width(&total, FOCUS_TOTAL_SIZE);
        let total_x = bar.right() - FOCUS_BAR_PADDING_X;
        frame.fill_text(Text {
            content: total,
            position: Point::new(total_x, middle_y),
            color: theme::kind_color(self.tree.node(self.map.focus).kind()),
            size: Pixels(FOCUS_TOTAL_SIZE),
            font: LABEL_FONT,
            align_x: alignment::Horizontal::Right.into(),
            align_y: alignment::Vertical::Center,
            ..Text::default()
        });
        let name_room = total_x - total_width - FOCUS_BAR_LABEL_GAP - name_x;
        frame.fill_text(Text {
            content: fit_width(&self.tree.name(self.map.focus), name_room, FOCUS_NAME_SIZE),
            position: Point::new(name_x, middle_y),
            color: theme::TEXT,
            size: Pixels(FOCUS_NAME_SIZE),
            align_y: alignment::Vertical::Center,
            ..Text::default()
        });
    }

    fn draw_up_chevron(&self, frame: &mut Frame, center: Point) {
        let chevron = Path::new(|b| {
            b.move_to(center + Vector::new(-UP_CHEVRON_HALF_WIDTH, UP_CHEVRON_HALF_HEIGHT));
            b.line_to(center + Vector::new(0.0, -UP_CHEVRON_HALF_HEIGHT));
            b.line_to(center + Vector::new(UP_CHEVRON_HALF_WIDTH, UP_CHEVRON_HALF_HEIGHT));
        });
        frame.stroke(&chevron, Stroke::default().with_color(theme::TEXT_DIM).with_width(UP_CHEVRON_STROKE));
    }

    fn draw_highlight(&self, frame: &mut Frame) {
        if let Some(tile) = self.selected.and_then(|s| self.map.find(Segment::Node(s))) {
            let outline = rectangle(&tile.rect.inset(SELECTION_OUTLINE_WIDTH / 2.0));
            frame.fill(&outline, Color::WHITE.scale_alpha(SELECTION_FILL_ALPHA));
            frame.stroke(&outline, selection_outline());
        }
        if let Some(tile) = self.hovered.and_then(|h| self.map.find(h)) {
            let outline = rectangle(&with_separator(&tile.rect));
            frame.fill(&outline, Color::WHITE.scale_alpha(HOVER_FILL_ALPHA));
            frame.stroke(&outline, Stroke::default().with_color(Color::WHITE.scale_alpha(HOVER_OUTLINE_ALPHA)));
        }
    }
}

impl CursorOverlay for TreemapChart<'_> {
    fn draw_cursor_overlay(&self, renderer: &Renderer, bounds: Rectangle, cursor: mouse::Cursor) -> Geometry {
        let mut overlay = Frame::new(renderer, bounds.size());
        if let Some(p) = cursor.position_in(bounds) {
            match self.map.hit(chart_point(p)) {
                TreemapHit::FocusBar if self.can_go_up() => overlay.stroke(&self.focus_bar_path(), go_up_outline()),
                TreemapHit::Tile(tile) => {
                    let subject = TooltipSubject {
                        tree: self.tree,
                        segment: tile.segment,
                        size: tile.size,
                        total: self.map.total,
                    };
                    draw_tooltip(&mut overlay, bounds, p, subject);
                }
                _ => {}
            }
        }
        overlay.into_geometry()
    }
}

impl canvas::Program<Message> for TreemapChart<'_> {
    type State = PointerState;

    fn update(
        &self,
        state: &mut PointerState,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        on_pointer(state, event, || self.pointed(bounds, cursor), self.hovered)
    }

    fn draw(
        &self,
        _state: &PointerState,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        if let Some(ChartFrame::Tiles { tiles, total }) = self.moving {
            let mut moving_chart = Frame::new(renderer, bounds.size());
            self.draw_tiles(&mut moving_chart, tiles, *total);
            return vec![moving_chart.into_geometry()];
        }
        let chart = self.chart_cache.draw(renderer, bounds.size(), |f| self.draw_resting(f));
        let highlight = self.highlight_cache.draw(renderer, bounds.size(), |f| self.draw_highlight(f));
        vec![chart, highlight]
    }

    fn mouse_interaction(&self, _state: &PointerState, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        interaction(self.pointed(bounds, cursor), self.can_go_up())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chart_canvas::iced_point;
    use dscan_core::Chart as ChartLayout;

    const CHART: Size = Size::new(700.0, 500.0);

    fn chart<'a>(b: &'a crate::app::Browser, hovered: Option<Segment>) -> TreemapChart<'a> {
        let ChartLayout::Treemap(map) = &b.chart else { panic!("the browser shows a treemap") };
        TreemapChart {
            tree: &b.tree,
            map,
            chart_cache: &b.chart_cache,
            highlight_cache: &b.highlight_cache,
            hovered,
            selected: None,
            moving: None,
        }
    }

    fn published(chart: &TreemapChart<'_>, state: &mut PointerState, event: Event, p: Point) -> Option<Message> {
        let bounds = Rectangle::new(Point::ORIGIN, CHART);
        canvas::Program::update(chart, state, &event, bounds, mouse::Cursor::Available(p))
            .and_then(|action| action.into_inner().0)
    }

    fn measured_treemap_browser(dir: &std::path::Path) -> crate::app::Browser {
        std::fs::write(dir.join("a.bin"), vec![1u8; 40_000]).unwrap();
        std::fs::write(dir.join("b.bin"), vec![1u8; 20_000]).unwrap();
        let mut b = crate::app::test_util::scanned(dscan_core::ScanOptions::folder(dir));
        let _ = b.update(Message::ShowChart(dscan_core::ChartKind::Treemap));
        let _ = b.update(Message::ChartMeasured(CHART));
        b
    }

    #[test]
    fn the_treemap_is_laid_out_for_the_measured_chart_size() {
        let dir = tempfile::tempdir().unwrap();
        let b = measured_treemap_browser(dir.path());
        let ChartLayout::Treemap(map) = &b.chart else { unreachable!() };
        assert_eq!(map.area.width, CHART.width, "the tiles fill the canvas width");
        assert!(map.tiles.len() >= 2, "both files have tiles: {:?}", map.tiles);
    }

    #[test]
    fn hovering_a_tile_hovers_it_and_clicking_the_focus_bar_goes_up() {
        let dir = tempfile::tempdir().unwrap();
        let b = measured_treemap_browser(dir.path());
        let ChartLayout::Treemap(map) = &b.chart else { unreachable!() };
        let tile = map.tiles[0];
        let over_the_tile = iced_point(tile.rect.center());
        let mut state = PointerState::default();
        let moved = Event::Mouse(mouse::Event::CursorMoved { position: over_the_tile });
        let hovered = published(&chart(&b, None), &mut state, moved, over_the_tile);
        assert!(matches!(hovered, Some(Message::Hover(Some(s))) if s == tile.segment), "{hovered:?}");

        let pressed = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        let press = published(&chart(&b, Some(tile.segment)), &mut state, pressed.clone(), over_the_tile);
        assert!(matches!(press, Some(Message::Press(s)) if s == tile.segment), "{press:?}");
        let on_the_bar = iced_point(map.focus_bar.center());
        let up = published(&chart(&b, None), &mut state, pressed, on_the_bar);
        assert!(matches!(up, Some(Message::GoUp)), "{up:?}");
    }
}
