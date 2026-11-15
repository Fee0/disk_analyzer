use dscan_core::transition::at_rest;
use dscan_core::{ChartFrame, FrameArc, IcicleGeometry, IcicleHit, Layout, NodeId, Segment, SegmentStyle, Tree};
use iced::widget::canvas::{self, Cache, Event, Frame, Geometry};
use iced::{Color, Rectangle, Renderer, Size, Theme, mouse};

use crate::app::Message;
use crate::chart_canvas::{
    AT_REST, CursorOverlay, OPAQUE, Pointed, PointerState, TooltipSubject, can_go_up, chart_point, draw_tooltip,
    frame_arc_color, go_up_outline, interaction, lit_from_above, on_pointer, style_color,
};
use crate::rect_chart::{
    FocusBar, Label, draw_focus_bar, draw_label, focus_bar_path, outline_hovered, outline_selected, size_of, top_left,
    with_separator,
};

const DESCENDANT_HOVER_FILL_ALPHA: f32 = 0.08;

#[derive(Clone, Copy)]
pub struct Icicle<'a> {
    pub tree: &'a Tree,
    pub layout: &'a Layout,
    pub chart_cache: &'a Cache,
    pub highlight_cache: &'a Cache,
    pub hovered: Option<Segment>,
    pub selected: Option<NodeId>,
    pub moving: Option<&'a ChartFrame>,
}

impl Icicle<'_> {
    fn geometry(&self, size: Size) -> IcicleGeometry {
        IcicleGeometry::fitting(size.width, size.height, self.layout.depth)
    }

    fn can_go_up(&self) -> bool {
        can_go_up(self.tree, self.layout.focus)
    }

    fn pointed(&self, geo: &IcicleGeometry, bounds: Rectangle, cursor: mouse::Cursor) -> Option<Pointed> {
        cursor.position_in(bounds).map(|p| match geo.hit(self.layout, chart_point(p)) {
            IcicleHit::FocusBar => Pointed::GoUp,
            IcicleHit::Bar(arc) => Pointed::Segment(arc.segment),
            IcicleHit::Nothing => Pointed::Nothing,
        })
    }

    fn draw_bars(&self, frame: &mut Frame, geo: &IcicleGeometry, arcs: &[FrameArc], style_progress: f64, total: u64) {
        self.draw_focus_bar(frame, geo, total);
        for a in arcs {
            let rect = with_separator(&geo.rect(a.inner_ring, a.outer_ring, a.start, a.end));
            let color = frame_arc_color(a, style_progress);
            frame.fill_rectangle(top_left(&rect), size_of(&rect), lit_from_above(color, rect.y, rect.bottom(), OPAQUE));
        }
    }

    fn draw_resting(&self, frame: &mut Frame, geo: &IcicleGeometry) {
        self.draw_bars(frame, geo, &at_rest(self.layout), AT_REST, self.layout.total);
        for (ring, arcs) in self.layout.rings.iter().enumerate() {
            for arc in arcs {
                let label = Label {
                    tree: self.tree,
                    segment: arc.segment,
                    size: arc.size,
                    background: style_color(SegmentStyle::of(arc), ring as f32),
                };
                draw_label(frame, &with_separator(&geo.bar(ring, arc)), label);
            }
        }
    }

    fn draw_focus_bar(&self, frame: &mut Frame, geo: &IcicleGeometry, total: u64) {
        draw_focus_bar(frame, FocusBar { rect: geo.focus_bar, tree: self.tree, focus: self.layout.focus, total });
    }

    fn draw_highlight(&self, frame: &mut Frame, geo: &IcicleGeometry) {
        if let Some((ring, arc)) = self.selected.and_then(|s| self.layout.find(Segment::Node(s))) {
            outline_selected(frame, &geo.bar(ring, arc));
        }
        if let Some((ring, arc)) = self.hovered.and_then(|h| self.layout.find(h)) {
            for (r, inside) in self.layout.descendants(ring, arc) {
                let bar = with_separator(&geo.bar(r, inside));
                frame.fill_rectangle(
                    top_left(&bar),
                    size_of(&bar),
                    Color::WHITE.scale_alpha(DESCENDANT_HOVER_FILL_ALPHA),
                );
            }
            outline_hovered(frame, &geo.bar(ring, arc));
        }
    }
}

impl CursorOverlay for Icicle<'_> {
    fn draw_cursor_overlay(&self, renderer: &Renderer, bounds: Rectangle, cursor: mouse::Cursor) -> Geometry {
        let geo = self.geometry(bounds.size());
        let mut overlay = Frame::new(renderer, bounds.size());
        if let Some(p) = cursor.position_in(bounds) {
            match geo.hit(self.layout, chart_point(p)) {
                IcicleHit::FocusBar if self.can_go_up() => {
                    overlay.stroke(&focus_bar_path(&geo.focus_bar), go_up_outline())
                }
                IcicleHit::Bar(arc) => {
                    let subject = TooltipSubject {
                        tree: self.tree,
                        segment: arc.segment,
                        size: arc.size,
                        total: self.layout.total,
                    };
                    draw_tooltip(&mut overlay, bounds, p, subject);
                }
                _ => {}
            }
        }
        overlay.into_geometry()
    }
}

impl canvas::Program<Message> for Icicle<'_> {
    type State = PointerState;

    fn update(
        &self,
        state: &mut PointerState,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        on_pointer(state, event, || self.pointed(&self.geometry(bounds.size()), bounds, cursor), self.hovered)
    }

    fn draw(
        &self,
        _state: &PointerState,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let geo = self.geometry(bounds.size());
        if let Some(ChartFrame::Arcs { arcs, total, style_progress }) = self.moving {
            let mut moving_chart = Frame::new(renderer, bounds.size());
            self.draw_bars(&mut moving_chart, &geo, arcs, *style_progress, *total);
            return vec![moving_chart.into_geometry()];
        }
        let chart = self.chart_cache.draw(renderer, bounds.size(), |f| self.draw_resting(f, &geo));
        let highlight = self.highlight_cache.draw(renderer, bounds.size(), |f| self.draw_highlight(f, &geo));
        vec![chart, highlight]
    }

    fn mouse_interaction(&self, _state: &PointerState, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        interaction(self.pointed(&self.geometry(bounds.size()), bounds, cursor), self.can_go_up())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chart_canvas::iced_point;
    use dscan_core::Chart as ChartLayout;
    use iced::Point;

    const CHART: Size = Size::new(700.0, 500.0);

    fn chart<'a>(b: &'a crate::app::Browser, hovered: Option<Segment>) -> Icicle<'a> {
        let ChartLayout::Icicle(layout) = &b.chart else { panic!("the browser shows an icicle") };
        Icicle {
            tree: &b.tree,
            layout,
            chart_cache: &b.chart_cache,
            highlight_cache: &b.highlight_cache,
            hovered,
            selected: None,
            moving: None,
        }
    }

    fn published(chart: &Icicle<'_>, state: &mut PointerState, event: Event, p: Point) -> Option<Message> {
        let bounds = Rectangle::new(Point::ORIGIN, CHART);
        canvas::Program::update(chart, state, &event, bounds, mouse::Cursor::Available(p))
            .and_then(|action| action.into_inner().0)
    }

    #[test]
    fn hovering_a_bar_hovers_it_and_clicking_the_focus_bar_goes_up() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.bin"), vec![1u8; 40_000]).unwrap();
        std::fs::write(dir.path().join("b.bin"), vec![1u8; 20_000]).unwrap();
        let mut b = crate::app::test_util::scanned(dscan_core::ScanOptions::folder(dir.path()));
        let _ = b.update(Message::ShowChart(dscan_core::ChartKind::Icicle));
        let ChartLayout::Icicle(layout) = &b.chart else { panic!("the browser switched to the icicle") };
        let geo = IcicleGeometry::fitting(CHART.width, CHART.height, layout.depth);
        let first = layout.rings[0][0];
        let over_the_bar = iced_point(geo.bar(0, &first).center());

        let mut state = PointerState::default();
        let moved = Event::Mouse(mouse::Event::CursorMoved { position: over_the_bar });
        let hovered = published(&chart(&b, None), &mut state, moved, over_the_bar);
        assert!(matches!(hovered, Some(Message::Hover(Some(s))) if s == first.segment), "{hovered:?}");

        let pressed = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        let press = published(&chart(&b, Some(first.segment)), &mut state, pressed.clone(), over_the_bar);
        assert!(matches!(press, Some(Message::Press(s)) if s == first.segment), "{press:?}");
        let on_the_focus_bar = iced_point(geo.focus_bar.center());
        let up = published(&chart(&b, None), &mut state, pressed, on_the_focus_bar);
        assert!(matches!(up, Some(Message::GoUp)), "{up:?}");
    }
}
