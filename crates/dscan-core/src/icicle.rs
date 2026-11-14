use crate::chart_geometry::ChartPoint;
use crate::layout::{Arc, Layout, MAX_DEPTH};
use crate::treemap::{FocusBarAbove, TileRect};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IcicleGeometry {
    pub focus_bar: TileRect,
    pub rows: TileRect,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum IcicleHit<'a> {
    FocusBar,
    Bar(&'a Arc),
    Nothing,
}

impl IcicleGeometry {
    pub fn fitting(width: f32, height: f32) -> Self {
        let FocusBarAbove { focus_bar, area } = FocusBarAbove::fitting(width, height);
        IcicleGeometry { focus_bar, rows: area }
    }

    pub fn row_height(&self) -> f32 {
        self.rows.height / MAX_DEPTH as f32
    }

    fn x(&self, share_of_the_width: f64) -> f32 {
        self.rows.x + share_of_the_width as f32 * self.rows.width
    }

    fn y(&self, ring: f64) -> f32 {
        self.rows.y + ring as f32 * self.row_height()
    }

    pub fn rect(&self, inner_ring: f64, outer_ring: f64, start: f64, end: f64) -> TileRect {
        let (left, top) = (self.x(start), self.y(inner_ring));
        TileRect::new(left, top, self.x(end) - left, self.y(outer_ring) - top)
    }

    pub fn bar(&self, ring: usize, arc: &Arc) -> TileRect {
        self.rect(ring as f64, ring as f64 + 1.0, arc.start, arc.end)
    }

    pub fn hit<'a>(&self, layout: &'a Layout, p: ChartPoint) -> IcicleHit<'a> {
        if self.focus_bar.contains(p) {
            return IcicleHit::FocusBar;
        }
        if !self.rows.contains(p) {
            return IcicleHit::Nothing;
        }
        let ring = ((p.y - self.rows.y) / self.row_height()) as usize;
        let share_of_the_width = f64::from((p.x - self.rows.x) / self.rows.width);
        layout.hit(ring, share_of_the_width).map_or(IcicleHit::Nothing, IcicleHit::Bar)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exclusions::Exclusions;
    use crate::layout::{LayoutParams, Segment, partition};
    use crate::tree::Tree;
    use crate::tree::test_util::{T::*, descend, tree};

    const SIZE: (f32, f32) = (800.0, 600.0);
    const PIXEL: f32 = 1e-3;

    fn sample() -> Tree {
        tree(vec![D("a", vec![F("x.mov", 600), F("y.mov", 200)]), F("b", 200)])
    }

    fn layout_of(t: &Tree) -> Layout {
        partition(t, t.root(), LayoutParams::default(), None, &Exclusions::default())
    }

    #[test]
    fn the_focus_bar_sits_on_top_and_the_deepest_ring_ends_at_the_bottom() {
        let g = IcicleGeometry::fitting(SIZE.0, SIZE.1);
        assert_eq!(g.focus_bar.y, 0.0);
        assert!(g.rows.y > g.focus_bar.bottom(), "a gap separates the bar from the rows");
        let deepest = g.rect(MAX_DEPTH as f64 - 1.0, MAX_DEPTH as f64, 0.0, 1.0);
        assert!((deepest.bottom() - SIZE.1).abs() < PIXEL, "{deepest:?} reaches the bottom");
        assert_eq!((deepest.x, deepest.width), (0.0, SIZE.0), "a whole ring spans the full width");
    }

    #[test]
    fn a_bar_is_as_wide_as_its_share_of_the_focus_and_one_row_tall() {
        let t = sample();
        let layout = layout_of(&t);
        let g = IcicleGeometry::fitting(SIZE.0, SIZE.1);
        let (ring, x) = layout.find(Segment::Node(descend(&t, &["a", "x.mov"]))).unwrap();
        let bar = g.bar(ring, x);
        assert!((bar.width - SIZE.0 * 0.6).abs() < PIXEL, "x.mov holds 60% of the focus: {bar:?}");
        assert!((bar.height - g.row_height()).abs() < PIXEL);
        assert!((bar.y - (g.rows.y + g.row_height())).abs() < PIXEL, "a child sits one row below its folder");
    }

    #[test]
    fn a_point_hits_the_focus_bar_the_bar_drawn_under_it_or_nothing() {
        let t = sample();
        let layout = layout_of(&t);
        let g = IcicleGeometry::fitting(SIZE.0, SIZE.1);
        assert_eq!(g.hit(&layout, g.focus_bar.center()), IcicleHit::FocusBar);
        for path in [&["a"][..], &["b"], &["a", "x.mov"], &["a", "y.mov"]] {
            let (ring, arc) = layout.find(Segment::Node(descend(&t, path))).unwrap();
            assert_eq!(g.hit(&layout, g.bar(ring, arc).center()), IcicleHit::Bar(arc), "{path:?}");
        }
        let below_the_file_b = ChartPoint { x: SIZE.0 - 1.0, y: g.rows.y + 1.5 * g.row_height() };
        assert_eq!(g.hit(&layout, below_the_file_b), IcicleHit::Nothing, "a file has nothing below it");
        let between_bar_and_rows = ChartPoint { x: 10.0, y: g.focus_bar.bottom() + 1.0 };
        assert_eq!(g.hit(&layout, between_bar_and_rows), IcicleHit::Nothing);
        assert_eq!(g.hit(&layout, ChartPoint { x: SIZE.0 + 5.0, y: 300.0 }), IcicleHit::Nothing);
    }

    #[test]
    fn a_chart_with_no_room_hits_nothing_instead_of_dividing_by_zero() {
        let t = sample();
        let g = IcicleGeometry::fitting(0.0, 0.0);
        assert_eq!(g.hit(&layout_of(&t), ChartPoint { x: 0.0, y: 0.0 }), IcicleHit::Nothing);
    }
}
