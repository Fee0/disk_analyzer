use std::sync::Arc;

use crate::exclusions::Exclusions;
use crate::layout::{Layout, LayoutParams, Segment, sunburst};
use crate::search::SearchResult;
use crate::transition::{FrameArc, Transition, TransitionKind};
use crate::tree::{NodeId, Tree};
use crate::treemap::{Treemap, treemap};
use crate::treemap_transition::{FrameTile, TreemapTransition};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChartKind {
    #[default]
    Sunburst,
    Treemap,
}

impl ChartKind {
    pub fn other(self) -> Self {
        match self {
            ChartKind::Sunburst => ChartKind::Treemap,
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
    pub search: Option<&'a SearchResult>,
    pub excluded: &'a Exclusions,
}

#[derive(Clone, Debug)]
pub enum Chart {
    Sunburst(Layout),
    Treemap(Arc<Treemap>),
}

impl Default for Chart {
    fn default() -> Self {
        Chart::Sunburst(Layout::default())
    }
}

impl Chart {
    pub fn new(kind: ChartKind, input: ChartInput<'_>) -> Self {
        let ChartInput { tree, focus, size, search, excluded } = input;
        match kind {
            ChartKind::Sunburst => Chart::Sunburst(sunburst(tree, focus, LayoutParams::default(), search, excluded)),
            ChartKind::Treemap => Chart::Treemap(Arc::new(treemap(tree, focus, size, search, excluded))),
        }
    }

    pub fn kind(&self) -> ChartKind {
        match self {
            Chart::Sunburst(_) => ChartKind::Sunburst,
            Chart::Treemap(_) => ChartKind::Treemap,
        }
    }

    pub fn total(&self) -> u64 {
        match self {
            Chart::Sunburst(layout) => layout.total,
            Chart::Treemap(map) => map.total,
        }
    }

    pub fn shows(&self, segment: Segment) -> bool {
        match self {
            Chart::Sunburst(layout) => layout.find(segment).is_some(),
            Chart::Treemap(map) => map.find(segment).is_some(),
        }
    }

    pub fn reveal(&self) -> ChartTransition {
        match self {
            Chart::Sunburst(layout) => ChartTransition::Sunburst(Transition::reveal(layout)),
            Chart::Treemap(map) => ChartTransition::Treemap(TreemapTransition::reveal(map)),
        }
    }

    pub fn transition_to(&self, next: &Chart) -> ChartTransition {
        match (self, next) {
            (Chart::Sunburst(from), Chart::Sunburst(to)) => ChartTransition::Sunburst(Transition::between(from, to)),
            (Chart::Treemap(from), Chart::Treemap(to)) => {
                ChartTransition::Treemap(TreemapTransition::between(from, to))
            }
            _ => next.reveal(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum ChartTransition {
    Sunburst(Transition),
    Treemap(TreemapTransition),
}

#[derive(Clone, Debug, PartialEq)]
pub enum ChartFrame {
    Arcs { arcs: Vec<FrameArc>, total: u64, style_progress: f64 },
    Tiles { tiles: Vec<FrameTile>, total: u64 },
}

impl ChartTransition {
    pub fn kind(&self) -> TransitionKind {
        match self {
            ChartTransition::Sunburst(t) => t.kind(),
            ChartTransition::Treemap(t) => t.kind(),
        }
    }

    pub fn frame(&self, progress: f64) -> ChartFrame {
        match self {
            ChartTransition::Sunburst(t) => {
                ChartFrame::Arcs { arcs: t.frame(progress), total: t.total(progress), style_progress: progress }
            }
            ChartTransition::Treemap(t) => ChartFrame::Tiles { tiles: t.frame(progress), total: t.total(progress) },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::test_util::{T::*, tree};

    fn chart(t: &Tree, kind: ChartKind) -> Chart {
        let excluded = Exclusions::default();
        Chart::new(
            kind,
            ChartInput { tree: t, focus: t.root(), size: (800.0, 600.0), search: None, excluded: &excluded },
        )
    }

    fn sample() -> Tree {
        tree(vec![D("a", vec![F("x", 600_000), F("y", 300_000)]), F("b", 100_000)])
    }

    #[test]
    fn switching_between_chart_kinds_reveals_the_new_chart() {
        let t = sample();
        let (sunburst, treemap) = (chart(&t, ChartKind::Sunburst), chart(&t, ChartKind::Treemap));
        let switch = sunburst.transition_to(&treemap);
        assert_eq!(switch.kind(), TransitionKind::Reveal, "two kinds have no shapes in common to morph");
        assert!(matches!(switch.frame(1.0), ChartFrame::Tiles { .. }), "the reveal draws the chart switched to");
    }

    #[test]
    fn both_kinds_show_the_same_items_and_total() {
        let t = sample();
        let (sunburst, treemap) = (chart(&t, ChartKind::Sunburst), chart(&t, ChartKind::Treemap));
        assert_eq!(sunburst.total(), treemap.total());
        let a = Segment::Node(crate::tree::test_util::descend(&t, &["a"]));
        assert!(sunburst.shows(a) && treemap.shows(a));
        assert_eq!(sunburst.kind().other(), treemap.kind());
        assert!(treemap.kind().depends_on_size() && !sunburst.kind().depends_on_size());
    }

    #[test]
    fn a_frame_carries_the_total_counted_so_far() {
        let t = sample();
        for kind in [ChartKind::Sunburst, ChartKind::Treemap] {
            let total = match chart(&t, kind).reveal().frame(0.5) {
                ChartFrame::Arcs { total, .. } | ChartFrame::Tiles { total, .. } => total,
            };
            assert_eq!(total, t.size(t.root()) / 2, "{kind:?} counts up halfway");
        }
    }
}
