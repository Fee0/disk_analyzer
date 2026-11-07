use std::f32::consts::{FRAC_PI_2, TAU};
use std::time::{Duration, Instant};

use dscan_core::transition::at_rest;
use dscan_core::{Arc, ArcStyle, FrameArc, Layout, MAX_DEPTH, NodeId, Segment, Tree, format_size};
use iced::widget::canvas::{self, Cache, Event, Frame, Geometry, Path, Stroke, Text};
use iced::{Color, Pixels, Point, Rectangle, Renderer, Size, Theme, Vector, alignment, mouse};

use crate::animation::MovingChart;
use crate::app::Message;
use crate::fmt::{fit_width, generous_text_width, truncate};
use crate::theme;

const DOUBLE_CLICK: Duration = Duration::from_millis(400);
const SLIVER_MAX_ARC_LENGTH: f32 = 3.0;
const SEPARATOR_GAP: f32 = 1.0;
const CENTER_LABEL_WIDTH_PER_HOLE_RADIUS: f32 = 1.6;

pub struct Sunburst<'a> {
    pub tree: &'a Tree,
    pub layout: &'a Layout,
    pub chart_cache: &'a Cache,
    pub highlight_cache: &'a Cache,
    pub hovered: Option<Segment>,
    pub selected: Option<NodeId>,
    pub moving: Option<&'a MovingChart>,
}

#[derive(Default)]
pub struct State {
    hover: Option<Segment>,
    last_click: Option<(Instant, Segment)>,
}

#[derive(Clone, Copy)]
struct Geo {
    center: Point,
    r0: f32,
    ring_w: f32,
}

impl Geo {
    fn new(size: Size) -> Self {
        let r = (size.width.min(size.height) / 2.0 - 16.0).max(60.0);
        let r0 = r * 0.22;
        Geo { center: Point::new(size.width / 2.0, size.height / 2.0), r0, ring_w: (r - r0) / MAX_DEPTH as f32 }
    }

    fn angle(frac: f64) -> f32 {
        frac as f32 * TAU - FRAC_PI_2
    }

    fn radius(&self, ring: f64) -> f32 {
        self.r0 + ring as f32 * self.ring_w
    }

    fn sector(&self, inner_ring: f64, outer_ring: f64, start: f64, end: f64) -> Path {
        let half_gap = SEPARATOR_GAP / 2.0;
        let inner_radius = self.radius(inner_ring) + half_gap;
        let outer_radius = (self.radius(outer_ring) - half_gap).max(inner_radius);
        let (a0, a1) = (Self::angle(start), Self::angle(end));
        let gaps_would_hide_the_color = (a1 - a0) * inner_radius <= SEPARATOR_GAP + SLIVER_MAX_ARC_LENGTH;
        let edge = |radius: f32| {
            let angular_half_gap = if gaps_would_hide_the_color { 0.0 } else { half_gap / radius };
            ArcEdge { radius, start: a0 + angular_half_gap, end: a1 - angular_half_gap }
        };
        annular_sector_as_one_polygon(self.center, edge(inner_radius), edge(outer_radius))
    }

    fn arc_path(&self, ring: usize, arc: &Arc) -> Path {
        self.sector(ring as f64, ring as f64 + 1.0, arc.start, arc.end)
    }
}

const IRRELEVANT_ARC_STRENGTH: f32 = 0.22;

fn style_color(style: ArcStyle, ring: f32) -> Color {
    let color = theme::segment_color(style.kind, ring, style.branch);
    if style.relevant { color } else { theme::mix(theme::BG, color, IRRELEVANT_ARC_STRENGTH) }
}

const AT_REST: f64 = 1.0;

fn frame_arc_color(a: &FrameArc, style_progress: f64) -> Color {
    let ring = a.inner_ring as f32;
    if a.style_before == a.style_after {
        return style_color(a.style_after, ring);
    }
    theme::mix(style_color(a.style_before, ring), style_color(a.style_after, ring), style_progress as f32)
}

enum Hit<'a> {
    Center,
    Arc(&'a Arc),
    Nothing,
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
    fn locate(&self, geo: &Geo, p: Point) -> Hit<'_> {
        let dist = p.distance(geo.center);
        if dist < geo.r0 {
            return Hit::Center;
        }
        let d = p - geo.center;
        let ring = ((dist - geo.r0) / geo.ring_w) as usize;
        let frac = ((d.y.atan2(d.x) + FRAC_PI_2) / TAU).rem_euclid(1.0) as f64;
        self.layout.hit(ring, frac).map_or(Hit::Nothing, Hit::Arc)
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
        frame.fill(&Path::circle(geo.center, geo.r0 - 2.0), theme::PANEL);
        let focus = self.layout.focus;
        let label_width = geo.r0 * CENTER_LABEL_WIDTH_PER_HOLE_RADIUS;
        let total = format_size(self.moving.map_or(self.layout.total, |m| m.total));
        let total_size = (geo.r0 * 0.28).clamp(10.0, 26.0).min(label_width / generous_text_width(&total, 1.0));
        frame.fill_text(Text {
            content: total,
            position: geo.center - Vector::new(0.0, 6.0),
            color: theme::kind_color(self.tree.node(focus).kind()),
            size: Pixels(total_size),
            align_x: alignment::Horizontal::Center.into(),
            align_y: alignment::Vertical::Center,
            ..Text::default()
        });
        let name_size = (geo.r0 * 0.15).clamp(10.0, 14.0);
        frame.fill_text(Text {
            content: fit_width(&self.tree.name(focus), label_width, name_size),
            position: geo.center + Vector::new(0.0, geo.r0 * 0.30),
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
            frame.stroke(&path, Stroke::default().with_color(Color::WHITE).with_width(2.0));
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

    fn draw_tooltip(&self, frame: &mut Frame, bounds: Rectangle, p: Point, arc: &Arc) {
        let name = match arc.segment {
            Segment::Node(n) => self.tree.name(n).into_owned(),
            Segment::Small { count, .. } => format!("{count} smaller items"),
        };
        let pct = if self.layout.total > 0 { arc.size as f64 * 100.0 / self.layout.total as f64 } else { 0.0 };
        let line1 = truncate(&name, 48);
        let line2 = format!("{} · {:.1}%", format_size(arc.size), pct);
        let w = (line1.chars().count().max(line2.len()) as f32 * 7.4 + 20.0).max(80.0);
        let h = 44.0;
        let mut pos = p + Vector::new(14.0, 14.0);
        if pos.x + w > bounds.width {
            pos.x = p.x - w - 8.0;
        }
        if pos.y + h > bounds.height {
            pos.y = p.y - h - 8.0;
        }
        frame.fill(&Path::rounded_rectangle(pos, Size::new(w, h), 6.0.into()), theme::PANEL_HI.scale_alpha(0.96));
        for (content, dy, color, size) in [(line1, 7.0, theme::TEXT, 13.0), (line2, 24.0, theme::TEXT_DIM, 12.0)] {
            frame.fill_text(Text {
                content,
                position: pos + Vector::new(10.0, dy),
                color,
                size: Pixels(size),
                ..Text::default()
            });
        }
    }

    fn draw_cursor_overlay(
        &self,
        renderer: &Renderer,
        bounds: Rectangle,
        cursor: mouse::Cursor,
        geo: &Geo,
    ) -> Geometry {
        let mut overlay = Frame::new(renderer, bounds.size());
        if let Some(p) = cursor.position_in(bounds) {
            match self.locate(geo, p) {
                Hit::Center if self.can_go_up() => overlay.stroke(
                    &Path::circle(geo.center, geo.r0 - 2.0),
                    Stroke::default().with_color(Color::WHITE.scale_alpha(0.5)).with_width(2.0),
                ),
                Hit::Arc(a) => self.draw_tooltip(&mut overlay, bounds, p, a),
                _ => {}
            }
        }
        overlay.into_geometry()
    }
}

fn redraw_so_the_tooltip_follows_the_cursor() -> canvas::Action<Message> {
    canvas::Action::request_redraw()
}

impl canvas::Program<Message> for Sunburst<'_> {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let geo = Geo::new(bounds.size());
        match event {
            Event::Mouse(mouse::Event::CursorMoved { .. } | mouse::Event::CursorLeft) => {
                let hit = match cursor.position_in(bounds).map(|p| self.locate(&geo, p)) {
                    Some(Hit::Arc(a)) => Some(a.segment),
                    _ => None,
                };
                if hit != state.hover {
                    state.hover = hit;
                    return Some(canvas::Action::publish(Message::Hover(hit)));
                }
                hit.map(|_| redraw_so_the_tooltip_follows_the_cursor())
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let p = cursor.position_in(bounds)?;
                let msg = match self.locate(&geo, p) {
                    Hit::Center => Message::GoUp,
                    Hit::Arc(a) => {
                        let (seg, now) = (a.segment, Instant::now());
                        if matches!(state.last_click, Some((t, s)) if s == seg && now - t < DOUBLE_CLICK) {
                            state.last_click = None;
                            Message::Activate(seg)
                        } else {
                            state.last_click = Some((now, seg));
                            Message::Press(seg)
                        }
                    }
                    Hit::Nothing => Message::Deselect,
                };
                Some(canvas::Action::publish(msg).and_capture())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let geo = Geo::new(bounds.size());
        let uncached_cursor_overlay = self.draw_cursor_overlay(renderer, bounds, cursor, &geo);
        if let Some(moving) = self.moving {
            let mut moving_chart = Frame::new(renderer, bounds.size());
            self.draw_chart(&mut moving_chart, &geo, &moving.arcs, moving.style_progress);
            return vec![moving_chart.into_geometry(), uncached_cursor_overlay];
        }
        let chart = self
            .chart_cache
            .draw(renderer, bounds.size(), |f| self.draw_chart(f, &geo, &at_rest(self.layout), AT_REST));
        let highlight = self.highlight_cache.draw(renderer, bounds.size(), |f| self.draw_highlight(f, &geo));
        vec![chart, highlight, uncached_cursor_overlay]
    }

    fn mouse_interaction(&self, _state: &State, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        let geo = Geo::new(bounds.size());
        match cursor.position_in(bounds).map(|p| self.locate(&geo, p)) {
            Some(Hit::Arc(_)) => mouse::Interaction::Pointer,
            Some(Hit::Center) if self.can_go_up() => mouse::Interaction::Pointer,
            _ => mouse::Interaction::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use iced::widget::canvas::path::{self, lyon_path::Event};

    use super::*;

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
