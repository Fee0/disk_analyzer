use std::f32::consts::{FRAC_PI_2, TAU};

use crate::layout::{Arc, Layout, MAX_DEPTH};

const MARGIN: f32 = 16.0;
const SMALLEST_RADIUS: f32 = 60.0;
const HOLE_SHARE_OF_RADIUS: f32 = 0.22;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChartPoint {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChartGeometry {
    pub center: ChartPoint,
    pub hole_radius: f32,
    pub ring_width: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ChartHit<'a> {
    Hole,
    Arc(&'a Arc),
    Nothing,
}

impl ChartGeometry {
    pub fn fitting(width: f32, height: f32) -> Self {
        let radius = (width.min(height) / 2.0 - MARGIN).max(SMALLEST_RADIUS);
        let hole_radius = radius * HOLE_SHARE_OF_RADIUS;
        ChartGeometry {
            center: ChartPoint { x: width / 2.0, y: height / 2.0 },
            hole_radius,
            ring_width: (radius - hole_radius) / MAX_DEPTH as f32,
        }
    }

    pub fn radius(&self, ring: f64) -> f32 {
        self.hole_radius + ring as f32 * self.ring_width
    }

    pub fn angle(turn_fraction: f64) -> f32 {
        turn_fraction as f32 * TAU - FRAC_PI_2
    }

    pub fn hit<'a>(&self, layout: &'a Layout, p: ChartPoint) -> ChartHit<'a> {
        let (dx, dy) = (p.x - self.center.x, p.y - self.center.y);
        let distance = dx.hypot(dy);
        if distance < self.hole_radius {
            return ChartHit::Hole;
        }
        let ring = ((distance - self.hole_radius) / self.ring_width) as usize;
        let turn_fraction = ((dy.atan2(dx) + FRAC_PI_2) / TAU).rem_euclid(1.0) as f64;
        layout.hit(ring, turn_fraction).map_or(ChartHit::Nothing, ChartHit::Arc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exclusions::Exclusions;
    use crate::layout::{LayoutParams, sunburst};
    use crate::tree::test_util::{T::*, descend, tree};

    const SIDE: f32 = 600.0;

    fn point_at(g: &ChartGeometry, ring: f64, turn_fraction: f64) -> ChartPoint {
        let (radius, angle) = (g.radius(ring), ChartGeometry::angle(turn_fraction));
        ChartPoint { x: g.center.x + radius * angle.cos(), y: g.center.y + radius * angle.sin() }
    }

    #[test]
    fn the_chart_fills_the_shorter_side_and_splits_the_rest_of_the_radius_into_rings() {
        let g = ChartGeometry::fitting(SIDE + 200.0, SIDE);
        assert_eq!(g.center, ChartPoint { x: (SIDE + 200.0) / 2.0, y: SIDE / 2.0 });
        assert_eq!(g.radius(MAX_DEPTH as f64), SIDE / 2.0 - MARGIN, "the outer ring keeps a margin from the edge");
        assert_eq!(g.hole_radius, g.radius(MAX_DEPTH as f64) * HOLE_SHARE_OF_RADIUS);
    }

    #[test]
    fn a_tiny_chart_keeps_its_smallest_radius() {
        let g = ChartGeometry::fitting(10.0, 10.0);
        assert_eq!(g.radius(MAX_DEPTH as f64), SMALLEST_RADIUS);
    }

    #[test]
    fn turns_start_at_twelve_o_clock_and_run_clockwise_on_screen() {
        let g = ChartGeometry::fitting(SIDE, SIDE);
        let at = |turn_fraction| point_at(&g, 0.5, turn_fraction);
        let (top, right, bottom) = (at(0.0), at(0.25), at(0.5));
        assert!(top.y < g.center.y && (top.x - g.center.x).abs() < 1e-3, "{top:?}");
        assert!(right.x > g.center.x && (right.y - g.center.y).abs() < 1e-3, "{right:?}");
        assert!(bottom.y > g.center.y, "screen y grows downward: {bottom:?}");
    }

    #[test]
    fn a_point_hits_the_hole_the_arc_drawn_under_it_or_nothing() {
        let t = tree(vec![F("big", 600), F("small", 200)]);
        let layout = sunburst(&t, t.root(), LayoutParams::default(), None, &Exclusions::default());
        let g = ChartGeometry::fitting(SIDE, SIDE);
        assert_eq!(g.hit(&layout, g.center), ChartHit::Hole);
        for name in ["big", "small"] {
            let (ring, arc) = layout.find(crate::layout::Segment::Node(descend(&t, &[name]))).unwrap();
            let middle = point_at(&g, ring as f64 + 0.5, arc.mid());
            assert_eq!(g.hit(&layout, middle), ChartHit::Arc(arc), "{name}");
        }
        assert_eq!(g.hit(&layout, point_at(&g, 1.5, 0.1)), ChartHit::Nothing, "no ring is drawn outside the files");
        assert_eq!(g.hit(&layout, ChartPoint { x: 0.0, y: 0.0 }), ChartHit::Nothing, "a corner is off the chart");
    }
}
