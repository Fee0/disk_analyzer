use dscan_core::{
    ChartFrame, FrameTile, NodeId, Segment, Tile, TileLook, TileRect, Tree, Treemap, TreemapHit, format_size,
};
use iced::widget::canvas::{self, Cache, Event, Fill, Frame, Geometry, Stroke, Text};
use iced::{Color, Pixels, Point, Rectangle, Renderer, Theme, alignment, mouse};

use crate::app::Message;
use crate::chart_canvas::{
    CursorOverlay, Pointed, PointerState, TooltipSubject, can_go_up, chart_point, draw_tooltip, go_up_outline,
    interaction, lit_from_above, on_pointer, segment_name, style_color,
};
use crate::fmt::{fit_width, generous_text_width};
use crate::rect_chart::{
    FocusBar, LABEL_FONT, LABEL_SIZE_LINE_STRENGTH, Label, draw_focus_bar, draw_label, focus_bar_path, outline_hovered,
    outline_selected, rectangle, size_of, text_color_on, top_left, with_separator,
};
use crate::theme;

const FOLDER_TINT: f32 = 0.10;
const FOLDER_TINT_PER_DEPTH: f32 = 0.07;
const DEEPEST_FOLDER_TINT: f32 = 0.45;
const FOLDER_OUTLINE_STRENGTH: f32 = 0.45;
const HEADER_TEXT_SIZE: f32 = 11.5;
const HEADER_PADDING_X: f32 = 5.0;
const HEADER_LABEL_GAP: f32 = 8.0;
const HEADER_NAME_ROOM_WORTH_A_SIZE: f32 = 36.0;

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

fn folder_tint(depth: usize) -> f32 {
    (FOLDER_TINT + FOLDER_TINT_PER_DEPTH * depth as f32).min(DEEPEST_FOLDER_TINT)
}

fn look_color(look: &TileLook) -> Color {
    let solid = style_color(look.style, look.depth as f32);
    if look.holds_tiles { theme::mix(theme::BG, solid, folder_tint(look.depth)) } else { solid }
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
        can_go_up(self.tree, self.map.focus)
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
        let label = Label {
            tree: self.tree,
            segment: tile.segment,
            size: tile.size,
            background: look_color(&TileLook::of(tile)),
        };
        draw_label(frame, &with_separator(&tile.rect), label);
    }

    fn draw_focus_bar(&self, frame: &mut Frame, total: u64) {
        draw_focus_bar(frame, FocusBar { rect: self.map.focus_bar, tree: self.tree, focus: self.map.focus, total });
    }

    fn draw_highlight(&self, frame: &mut Frame) {
        if let Some(tile) = self.selected.and_then(|s| self.map.find(Segment::Node(s))) {
            outline_selected(frame, &tile.rect);
        }
        if let Some(tile) = self.hovered.and_then(|h| self.map.find(h)) {
            outline_hovered(frame, &tile.rect);
        }
    }
}

impl CursorOverlay for TreemapChart<'_> {
    fn draw_cursor_overlay(&self, renderer: &Renderer, bounds: Rectangle, cursor: mouse::Cursor) -> Geometry {
        let mut overlay = Frame::new(renderer, bounds.size());
        if let Some(p) = cursor.position_in(bounds) {
            match self.map.hit(chart_point(p)) {
                TreemapHit::FocusBar if self.can_go_up() => {
                    overlay.stroke(&focus_bar_path(&self.map.focus_bar), go_up_outline())
                }
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
    use iced::Size;

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
