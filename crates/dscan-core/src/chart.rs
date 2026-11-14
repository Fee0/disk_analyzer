use std::sync::Arc;

use crate::exclusions::Exclusions;
use crate::layout::{Layout, LayoutParams, Segment, partition};
use crate::paint::Paint;
use crate::transition::{FrameArc, Transition, TransitionKind};
use crate::tree::{NodeId, Tree};
use crate::treemap::{Treemap, treemap};
use crate::treemap_transition::{FrameTile, TreemapTransition};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChartKind {
    #[default]
    Sunburst,
    Icicle,
    Treemap,
}

impl ChartKind {
    pub const ALL: [ChartKind; 3] = [ChartKind::Sunburst, ChartKind::Icicle, ChartKind::Treemap];

    pub fn next(self) -> Self {
        match self {
            ChartKind::Sunburst => ChartKind::Icicle,
            ChartKind::Icicle => ChartKind::Treemap,
            ChartKind::Treemap => ChartKind::Sunburst,
        }
    }

    pub fn depends_on_size(self) -> bool {
        self == ChartKind::Treemap
    }
}

pub struct ChartInput<'a> {
    pub tree: &'a Tree,
    pub focus: NodeId,
    pub size: (f32, f32),
    pub paint: Paint<'a>,
    pub excluded: &'a Exclusions,
}

#[derive(Clone, Debug)]
pub enum Chart {
    Sunburst(Layout),
    Icicle(Layout),
    Treemap(Arc<Treemap>),
}

impl Default for Chart {
    fn default() -> Self {
        Chart::Sunburst(Layout::default())
    }
}

impl Chart {
    pub fn new(kind: ChartKind, input: ChartInput<'_>) -> Self {
        let ChartInput { tree, focus, size, paint, excluded } = input;
        let partitioned = || partition(tree, focus, LayoutParams::default(), paint, excluded);
        match kind {
            ChartKind::Sunburst => Chart::Sunburst(partitioned()),
            ChartKind::Icicle => Chart::Icicle(partitioned()),
            ChartKind::Treemap => Chart::Treemap(Arc::new(treemap(tree, focus, size, paint, excluded))),
        }
    }

    pub fn redrawn_as(self, kind: ChartKind) -> Result<Chart, Chart> {
        match (self, kind) {
            (Chart::Sunburst(layout) | Chart::Icicle(layout), ChartKind::Sunburst) => Ok(Chart::Sunburst(layout)),
            (Chart::Sunburst(layout) | Chart::Icicle(layout), ChartKind::Icicle) => Ok(Chart::Icicle(layout)),
            (chart, _) => Err(chart),
        }
    }

    pub fn kind(&self) -> ChartKind {
        match self {
            Chart::Sunburst(_) => ChartKind::Sunburst,
            Chart::Icicle(_) => ChartKind::Icicle,
            Chart::Treemap(_) => ChartKind::Treemap,
        }
    }

    pub fn total(&self) -> u64 {
        match self {
            Chart::Sunburst(layout) | Chart::Icicle(layout) => layout.total,
            Chart::Treemap(map) => map.total,
        }
    }

    pub fn shows(&self, segment: Segment) -> bool {
        match self {
            Chart::Sunburst(layout) | Chart::Icicle(layout) => layout.find(segment).is_some(),
            Chart::Treemap(map) => map.find(segment).is_some(),
        }
    }

    pub fn reveal(&self) -> ChartTransition {
        match self {
            Chart::Sunburst(layout) | Chart::Icicle(layout) => ChartTransition::Arcs(Transition::reveal(layout)),
            Chart::Treemap(map) => ChartTransition::Tiles(TreemapTransition::reveal(map)),
        }
    }

    pub fn transition_to(&self, next: &Chart) -> ChartTransition {
        match (self, next) {
            (Chart::Sunburst(from), Chart::Sunburst(to)) | (Chart::Icicle(from), Chart::Icicle(to)) => {
                ChartTransition::Arcs(Transition::between(from, to))
            }
            (Chart::Treemap(from), Chart::Treemap(to)) => ChartTransition::Tiles(TreemapTransition::between(from, to)),
            _ => next.reveal(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum ChartTransition {
    Arcs(Transition),
    Tiles(TreemapTransition),
}

#[derive(Clone, Debug, PartialEq)]
pub enum ChartFrame {
    Arcs { arcs: Vec<FrameArc>, total: u64, style_progress: f64 },
    Tiles { tiles: Vec<FrameTile>, total: u64 },
}

impl ChartTransition {
    pub fn kind(&self) -> TransitionKind {
        match self {
            ChartTransition::Arcs(t) => t.kind(),
            ChartTransition::Tiles(t) => t.kind(),
        }
    }

    pub fn frame(&self, progress: f64) -> ChartFrame {
        match self {
            ChartTransition::Arcs(t) => {
                ChartFrame::Arcs { arcs: t.frame(progress), total: t.total(progress), style_progress: progress }
            }
            ChartTransition::Tiles(t) => ChartFrame::Tiles { tiles: t.frame(progress), total: t.total(progress) },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paint::ColorBy;
    use crate::tree::test_util::{T::*, tree};

    fn chart(t: &Tree, kind: ChartKind) -> Chart {
        let excluded = Exclusions::default();
        Chart::new(
            kind,
            ChartInput { tree: t, focus: t.root(), size: (800.0, 600.0), paint: Paint::default(), excluded: &excluded },
        )
    }

    fn sample() -> Tree {
        tree(vec![D("a", vec![F("x", 600_000), F("y", 300_000)]), F("b", 100_000)])
    }

    #[test]
    fn switching_between_chart_kinds_reveals_the_new_chart() {
        let t = sample();
        for from in ChartKind::ALL {
            for to in ChartKind::ALL.into_iter().filter(|&to| to != from) {
                let switch = chart(&t, from).transition_to(&chart(&t, to));
                assert_eq!(switch.kind(), TransitionKind::Reveal, "{from:?} to {to:?} has no shapes to morph");
            }
        }
        let to_the_treemap = chart(&t, ChartKind::Sunburst).transition_to(&chart(&t, ChartKind::Treemap));
        assert!(
            matches!(to_the_treemap.frame(1.0), ChartFrame::Tiles { .. }),
            "the reveal draws the chart switched to"
        );
    }

    #[test]
    fn the_sunburst_and_the_icicle_redraw_one_partition_without_laying_it_out_again() {
        let t = sample();
        let Chart::Sunburst(rings) = chart(&t, ChartKind::Sunburst) else { panic!("a sunburst") };
        let unrolled = Chart::Sunburst(rings.clone()).redrawn_as(ChartKind::Icicle);
        assert!(matches!(&unrolled, Ok(Chart::Icicle(rows)) if rows.rings == rings.rings), "{unrolled:?}");
        let rolled_back = unrolled.unwrap().redrawn_as(ChartKind::Sunburst);
        assert!(matches!(rolled_back, Ok(Chart::Sunburst(_))));

        let treemap = chart(&t, ChartKind::Treemap).redrawn_as(ChartKind::Icicle);
        assert!(matches!(treemap, Err(Chart::Treemap(_))), "a treemap holds no partition to redraw");
        let sunburst = chart(&t, ChartKind::Sunburst).redrawn_as(ChartKind::Treemap);
        assert!(matches!(sunburst, Err(Chart::Sunburst(_))), "a treemap needs its own layout");
    }

    #[test]
    fn every_kind_shows_the_same_items_and_total() {
        let t = sample();
        let a = Segment::Node(crate::tree::test_util::descend(&t, &["a"]));
        let charts = ChartKind::ALL.map(|kind| chart(&t, kind));
        for c in &charts {
            assert_eq!(c.total(), t.size(t.root()), "{:?}", c.kind());
            assert!(c.shows(a), "{:?}", c.kind());
        }
        assert_eq!(charts.map(|c| c.kind()), ChartKind::ALL, "each chart reports the kind it was built as");
    }

    #[test]
    fn the_next_kind_cycles_through_every_kind_and_back() {
        let mut kind = ChartKind::default();
        let mut seen = Vec::new();
        for _ in ChartKind::ALL {
            seen.push(kind);
            kind = kind.next();
        }
        assert_eq!(seen, ChartKind::ALL, "the toolbar order");
        assert_eq!(kind, ChartKind::default(), "the cycle wraps around");
    }

    #[test]
    fn only_the_treemap_is_laid_out_for_the_chart_s_size() {
        let depends: Vec<bool> = ChartKind::ALL.iter().map(|k| k.depends_on_size()).collect();
        assert_eq!(depends, [false, false, true], "the sunburst and the icicle scale to any size when drawn");
    }

    #[test]
    fn coloring_by_age_instead_of_type_morphs_every_kind_of_chart_in_place() {
        let t = sample();
        let excluded = Exclusions::default();
        let colored = |kind: ChartKind, color_by: ColorBy| {
            let paint = Paint { color_by, ..Paint::default() };
            Chart::new(kind, ChartInput { tree: &t, focus: t.root(), size: (800.0, 600.0), paint, excluded: &excluded })
        };
        for kind in ChartKind::ALL {
            let recolor = colored(kind, ColorBy::Kind).transition_to(&colored(kind, ColorBy::Age));
            assert_eq!(recolor.kind(), TransitionKind::Morph, "{kind:?} keeps its shapes");
        }
        let recolor =
            colored(ChartKind::Sunburst, ColorBy::Kind).transition_to(&colored(ChartKind::Sunburst, ColorBy::Age));
        let ChartFrame::Arcs { arcs, .. } = recolor.frame(0.5) else { panic!("a sunburst frame") };
        assert!(
            arcs.iter().all(|a| a.style_before.shown_age.is_none() && a.style_after.shown_age.is_some()),
            "every arc fades from its type color to its age color"
        );
    }

    #[test]
    fn a_frame_carries_the_total_counted_so_far() {
        let t = sample();
        for kind in ChartKind::ALL {
            let total = match chart(&t, kind).reveal().frame(0.5) {
                ChartFrame::Arcs { total, .. } | ChartFrame::Tiles { total, .. } => total,
            };
            assert_eq!(total, t.size(t.root()) / 2, "{kind:?} counts up halfway");
        }
    }
}
