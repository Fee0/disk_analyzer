use dscan_core::transition::at_rest;
use dscan_core::{ChartFrame, ChartGeometry, ChartHit, FrameArc, Layout, NodeId, Segment, Tree, format_size};
use iced::widget::canvas::{self, Cache, Event, Frame, Geometry, Path, Text};
use iced::{Color, Pixels, Point, Rectangle, Renderer, Size, Theme, Vector, alignment, mouse};

use crate::app::Message;
use crate::chart_canvas::{
    CursorOverlay, OPAQUE, Pointed, PointerState, SEPARATOR_GAP, SOFT_SHADOW_DROP, TooltipSubject, chart_point,
    draw_tooltip, fill_soft_shadow, go_up_outline, iced_point, interaction, lit_from_above, on_pointer, rim,
    selection_outline, style_color,
};
use crate::fmt::{fit_width, generous_text_width};
use crate::theme;

const SLIVER_MAX_ARC_LENGTH: f32 = 3.0;
const CENTER_LABEL_WIDTH_PER_HOLE_RADIUS: f32 = 1.6;
const CENTER_DISC_INSET: f32 = 2.0;

#[derive(Clone, Copy)]
pub struct Sunburst<'a> {
    pub tree: &'a Tree,
    pub layout: &'a Layout,
    pub chart_cache: &'a Cache,
    pub highlight_cache: &'a Cache,
    pub hovered: Option<Segment>,
    pub selected: Option<NodeId>,
    pub moving: Option<&'a ChartFrame>,
}

#[derive(Clone, Copy)]
struct Geo {
    chart: ChartGeometry,
    center: Point,
}

impl Geo {
    fn new(size: Size) -> Self {
        let chart = ChartGeometry::fitting(size.width, size.height);
        Geo { chart, center: iced_point(chart.center) }
    }

    fn sector(&self, inner_ring: f64, outer_ring: f64, start: f64, end: f64) -> Path {
        let half_gap = SEPARATOR_GAP / 2.0;
        let inner_radius = self.chart.radius(inner_ring) + half_gap;
        let outer_radius = (self.chart.radius(outer_ring) - half_gap).max(inner_radius);
        let (a0, a1) = (ChartGeometry::angle(start), ChartGeometry::angle(end));
        let gaps_would_hide_the_color = (a1 - a0) * inner_radius <= SEPARATOR_GAP + SLIVER_MAX_ARC_LENGTH;
        let edge = |radius: f32| {
            let angular_half_gap = if gaps_would_hide_the_color { 0.0 } else { half_gap / radius };
            ArcEdge { radius, start: a0 + angular_half_gap, end: a1 - angular_half_gap }
        };
        annular_sector_as_one_polygon(self.center, edge(inner_radius), edge(outer_radius))
    }

    fn arc_path(&self, ring: usize, arc: &dscan_core::Arc) -> Path {
        self.sector(ring as f64, ring as f64 + 1.0, arc.start, arc.end)
    }
}

const AT_REST: f64 = 1.0;

fn frame_arc_color(a: &FrameArc, style_progress: f64) -> Color {
    let ring = a.inner_ring as f32;
    if a.style_before == a.style_after {
        return style_color(a.style_after, ring);
    }
    theme::mix(style_color(a.style_before, ring), style_color(a.style_after, ring), style_progress as f32)
}

#[derive(Clone, Copy)]
struct ArcEdge {
    radius: f32,
    start: f32,
    end: f32,
}

const ARC_STEP_LENGTH: f32 = 3.0;
const MAX_ARC_STEPS: f32 = 720.0;

impl ArcEdge {
    fn points(self, center: Point) -> impl DoubleEndedIterator<Item = Point> {
        let sweep = self.end - self.start;
        let steps = ((sweep.abs() * self.radius) / ARC_STEP_LENGTH).ceil().clamp(1.0, MAX_ARC_STEPS) as usize;
        (0..=steps).map(move |i| {
            let (sin, cos) = (self.start + sweep * i as f32 / steps as f32).sin_cos();
            Point::new(center.x + cos * self.radius, center.y + sin * self.radius)
        })
    }
}

fn annular_sector_as_one_polygon(center: Point, inner: ArcEdge, outer: ArcEdge) -> Path {
    Path::new(|b| {
        let mut outline = outer.points(center).chain(inner.points(center).rev());
        if let Some(first) = outline.next() {
            b.move_to(first);
        }
        outline.for_each(|p| b.line_to(p));
        b.close();
    })
}

impl Sunburst<'_> {
    fn locate(&self, geo: &Geo, p: Point) -> ChartHit<'_> {
        geo.chart.hit(self.layout, chart_point(p))
    }

    fn pointed(&self, geo: &Geo, bounds: Rectangle, cursor: mouse::Cursor) -> Option<Pointed> {
        cursor.position_in(bounds).map(|p| match self.locate(geo, p) {
            ChartHit::Hole => Pointed::GoUp,
            ChartHit::Arc(a) => Pointed::Segment(a.segment),
            ChartHit::Nothing => Pointed::Nothing,
        })
    }

    fn can_go_up(&self) -> bool {
        self.layout.focus != self.tree.root()
    }

    fn draw_chart(&self, frame: &mut Frame, geo: &Geo, arcs: &[FrameArc], style_progress: f64) {
        for a in arcs {
            frame.fill(&geo.sector(a.inner_ring, a.outer_ring, a.start, a.end), frame_arc_color(a, style_progress));
        }
        self.draw_center_disc_with_focus_size(frame, geo);
    }

    fn draw_center_disc_with_focus_size(&self, frame: &mut Frame, geo: &Geo) {
        let radius = geo.chart.hole_radius - CENTER_DISC_INSET;
        let dropped = geo.center + Vector::new(0.0, SOFT_SHADOW_DROP);
        fill_soft_shadow(frame, |outset| Path::circle(dropped, radius + outset));
        let disc = Path::circle(geo.center, radius);
        frame.fill(&disc, lit_from_above(theme::PANEL, geo.center.y - radius, geo.center.y + radius, OPAQUE));
        frame.stroke(&disc, rim());
        let focus = self.layout.focus;
        let label_width = geo.chart.hole_radius * CENTER_LABEL_WIDTH_PER_HOLE_RADIUS;
        let total = match self.moving {
            Some(ChartFrame::Arcs { total, .. }) => *total,
            _ => self.layout.total,
        };
        let total = format_size(total);
        let total_size =
            (geo.chart.hole_radius * 0.28).clamp(10.0, 26.0).min(label_width / generous_text_width(&total, 1.0));
        frame.fill_text(Text {
            content: total,
            position: geo.center - Vector::new(0.0, 6.0),
            color: theme::kind_color(self.tree.node(focus).kind()),
            size: Pixels(total_size),
            align_x: alignment::Horizontal::Center.into(),
            align_y: alignment::Vertical::Center,
            ..Text::default()
        });
        let name_size = (geo.chart.hole_radius * 0.15).clamp(10.0, 14.0);
        frame.fill_text(Text {
            content: fit_width(&self.tree.name(focus), label_width, name_size),
            position: geo.center + Vector::new(0.0, geo.chart.hole_radius * 0.30),
            color: theme::TEXT_DIM,
            size: Pixels(name_size),
            align_x: alignment::Horizontal::Center.into(),
            align_y: alignment::Vertical::Center,
            ..Text::default()
        });
    }

    fn draw_highlight(&self, frame: &mut Frame, geo: &Geo) {
        self.draw_selection_outline(frame, geo);
        self.draw_hovered_arc_and_visible_descendants(frame, geo);
    }

    fn draw_selection_outline(&self, frame: &mut Frame, geo: &Geo) {
        if let Some((ring, a)) = self.selected.and_then(|s| self.layout.find(Segment::Node(s))) {
            let path = geo.arc_path(ring, a);
            frame.fill(&path, Color::WHITE.scale_alpha(0.18));
            frame.stroke(&path, selection_outline());
        }
    }

    fn draw_hovered_arc_and_visible_descendants(&self, frame: &mut Frame, geo: &Geo) {
        if let Some((ring, a)) = self.hovered.and_then(|h| self.layout.find(h)) {
            frame.fill(&geo.arc_path(ring, a), Color::WHITE.scale_alpha(0.28));
            for (r, d) in self.layout.descendants(ring, a) {
                frame.fill(&geo.arc_path(r, d), Color::WHITE.scale_alpha(0.14));
            }
        }
    }
}

impl CursorOverlay for Sunburst<'_> {
    fn draw_cursor_overlay(&self, renderer: &Renderer, bounds: Rectangle, cursor: mouse::Cursor) -> Geometry {
        let geo = &Geo::new(bounds.size());
        let mut overlay = Frame::new(renderer, bounds.size());
        if let Some(p) = cursor.position_in(bounds) {
            match self.locate(geo, p) {
                ChartHit::Hole if self.can_go_up() => overlay
                    .stroke(&Path::circle(geo.center, geo.chart.hole_radius - CENTER_DISC_INSET), go_up_outline()),
                ChartHit::Arc(a) => {
                    let subject =
                        TooltipSubject { tree: self.tree, segment: a.segment, size: a.size, total: self.layout.total };
                    draw_tooltip(&mut overlay, bounds, p, subject)
                }
                _ => {}
            }
        }
        overlay.into_geometry()
    }
}

impl canvas::Program<Message> for Sunburst<'_> {
    type State = PointerState;

    fn update(
        &self,
        state: &mut PointerState,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        on_pointer(state, event, || self.pointed(&Geo::new(bounds.size()), bounds, cursor), self.hovered)
    }

    fn draw(
        &self,
        _state: &PointerState,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let geo = Geo::new(bounds.size());
        if let Some(ChartFrame::Arcs { arcs, style_progress, .. }) = self.moving {
            let mut moving_chart = Frame::new(renderer, bounds.size());
            self.draw_chart(&mut moving_chart, &geo, arcs, *style_progress);
            return vec![moving_chart.into_geometry()];
        }
        let chart = self
            .chart_cache
            .draw(renderer, bounds.size(), |f| self.draw_chart(f, &geo, &at_rest(self.layout), AT_REST));
        let highlight = self.highlight_cache.draw(renderer, bounds.size(), |f| self.draw_highlight(f, &geo));
        vec![chart, highlight]
    }

    fn mouse_interaction(&self, _state: &PointerState, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        interaction(self.pointed(&Geo::new(bounds.size()), bounds, cursor), self.can_go_up())
    }
}

#[cfg(test)]
mod tests {
    use iced::widget::canvas::path::{self, lyon_path::Event};

    use super::*;

    const CHART_SIDE: f32 = 600.0;
    const PROBE_STEP: usize = 5;

    fn some_point_over_an_arc(chart: &Sunburst<'_>, geo: &Geo) -> (Point, Segment) {
        let grid = (0..CHART_SIDE as usize).step_by(PROBE_STEP);
        grid.clone()
            .flat_map(|x| grid.clone().map(move |y| Point::new(x as f32, y as f32)))
            .find_map(|p| match chart.locate(geo, p) {
                ChartHit::Arc(a) => Some((p, a.segment)),
                _ => None,
            })
            .expect("the chart draws at least one arc")
    }

    fn published_by_moving_to(chart: &Sunburst<'_>, state: &mut PointerState, p: Point) -> Option<Message> {
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(CHART_SIDE, CHART_SIDE));
        let moved = canvas::Event::Mouse(mouse::Event::CursorMoved { position: p });
        canvas::Program::update(chart, state, &moved, bounds, mouse::Cursor::Available(p))
            .and_then(|action| action.into_inner().0)
    }

    #[test]
    fn the_arc_under_the_cursor_is_hovered_again_after_the_browser_forgot_it() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.bin"), vec![1u8; 4000]).unwrap();
        std::fs::write(dir.path().join("b.bin"), vec![1u8; 2000]).unwrap();
        let b = crate::app::test_util::scanned(dscan_core::ScanOptions::folder(dir.path()));
        let chart = |hovered| Sunburst {
            tree: &b.tree,
            layout: match &b.chart {
                dscan_core::Chart::Sunburst(layout) => layout,
                dscan_core::Chart::Treemap(_) => panic!("a new browser shows the sunburst"),
            },
            chart_cache: &b.chart_cache,
            highlight_cache: &b.highlight_cache,
            hovered,
            selected: None,
            moving: None,
        };
        let (p, segment) = some_point_over_an_arc(&chart(None), &Geo::new(Size::new(CHART_SIDE, CHART_SIDE)));
        let mut state = PointerState::default();

        let entering = published_by_moving_to(&chart(None), &mut state, p);
        assert!(matches!(entering, Some(Message::Hover(Some(s))) if s == segment), "{entering:?}");
        let still_hovered = published_by_moving_to(&chart(Some(segment)), &mut state, p);
        assert!(still_hovered.is_none(), "moving within the hovered arc says nothing new: {still_hovered:?}");
        let after_the_browser_cleared_it = published_by_moving_to(&chart(None), &mut state, p);
        assert!(
            matches!(after_the_browser_cleared_it, Some(Message::Hover(Some(s))) if s == segment),
            "the next move hovers the arc again: {after_the_browser_cleared_it:?}"
        );
    }

    fn subpath_count(path: &Path) -> usize {
        path.raw().iter().filter(|event| matches!(event, Event::Begin { .. })).count()
    }

    #[test]
    fn iced_arc_builder_starts_a_subpath_per_arc_so_sectors_are_tessellated_as_one_polygon() {
        let (center, r_in, r_out, a0, a1) = (Point::new(100.0, 100.0), 40.0, 60.0, 0.0, 1.0);
        let with_iced_arcs = Path::new(|b| {
            b.arc(path::Arc { center, radius: r_out, start_angle: a0.into(), end_angle: a1.into() });
            b.arc(path::Arc { center, radius: r_in, start_angle: a1.into(), end_angle: a0.into() });
            b.close();
        });
        assert!(subpath_count(&with_iced_arcs) > 1, "iced's arc builder splits a sector into several sub-paths");
        let edge = |radius| ArcEdge { radius, start: a0, end: a1 };
        assert_eq!(
            subpath_count(&annular_sector_as_one_polygon(center, edge(r_in), edge(r_out))),
            1,
            "a hand-tessellated sector is one closed polygon"
        );
    }
}
