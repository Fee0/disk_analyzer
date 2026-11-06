//! Sunburst geometry (pure math, renderer independent) and hit-testing.
//!
//! Angles are expressed as fractions of a full turn in `[0, 1)`, starting at
//! 12 o'clock and running clockwise. Ring 0 holds the focus node's children.

use crate::kind::Kind;
use crate::search::SearchResult;
use crate::tree::{Metric, NodeId, Tree};

/// Default number of rings.
pub const MAX_DEPTH: usize = 7;

/// What a segment stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Segment {
    Node(NodeId),
    /// `count` children of `parent` too small to draw individually.
    Small {
        parent: NodeId,
        count: u32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Arc {
    pub segment: Segment,
    pub start: f64,
    pub end: f64,
    pub size: u64,
    pub kind: Kind,
    /// Index of the top-level (ring 0) ancestor's arc, for shading.
    pub branch: u32,
    /// Contains a search match (always true without a search).
    pub relevant: bool,
}

impl Arc {
    pub fn node(&self) -> Option<NodeId> {
        match self.segment {
            Segment::Node(n) => Some(n),
            Segment::Small { .. } => None,
        }
    }
    pub fn span(&self) -> f64 {
        self.end - self.start
    }
    pub fn mid(&self) -> f64 {
        (self.start + self.end) * 0.5
    }
}

#[derive(Clone, Copy, Debug)]
pub struct LayoutParams {
    /// Number of rings to draw.
    pub max_depth: usize,
    /// Children narrower than this fraction of a turn are merged.
    pub min_span: f64,
}

impl Default for LayoutParams {
    fn default() -> Self {
        LayoutParams { max_depth: MAX_DEPTH, min_span: 0.6 / 360.0 }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub focus: NodeId,
    pub total: u64,
    pub rings: Vec<Vec<Arc>>,
}

impl Layout {
    /// Number of arcs over all rings.
    pub fn len(&self) -> usize {
        self.rings.iter().map(Vec::len).sum()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The arc at `ring` covering angle `frac`.
    pub fn hit(&self, ring: usize, frac: f64) -> Option<&Arc> {
        let arcs = self.rings.get(ring)?;
        let frac = frac.rem_euclid(1.0);
        let i = arcs.partition_point(|a| a.end <= frac);
        arcs.get(i).filter(|a| a.start <= frac && frac < a.end)
    }

    /// Ring index and arc of a segment, if visible.
    pub fn find(&self, seg: Segment) -> Option<(usize, &Arc)> {
        self.rings.iter().enumerate().find_map(|(r, arcs)| arcs.iter().find(|a| a.segment == seg).map(|a| (r, a)))
    }

    /// Visible descendants of the arc at `ring`: the outer-ring arcs inside
    /// its angular span.
    pub fn descendants<'a>(&'a self, ring: usize, arc: &Arc) -> impl Iterator<Item = (usize, &'a Arc)> + 'a {
        let (start, end) = (arc.start, arc.end);
        self.rings.iter().enumerate().skip(ring + 1).flat_map(move |(r, arcs)| {
            let first = arcs.partition_point(|x| x.end <= start + 1e-12);
            arcs[first..].iter().take_while(move |x| x.start < end - 1e-12).map(move |x| (r, x))
        })
    }
}

/// Compute the sunburst for `focus`; arcs without a match of `search`
/// are marked irrelevant.
pub fn sunburst(
    tree: &Tree,
    focus: NodeId,
    metric: Metric,
    params: LayoutParams,
    search: Option<&SearchResult>,
) -> Layout {
    let total = tree.size(focus, metric);
    let mut layout = Layout { focus, total, rings: vec![Vec::new(); params.max_depth] };
    if total > 0 {
        let mut ctx = Placer { tree, metric, params, search, rings: &mut layout.rings };
        ctx.place(focus, 0.0, 1.0, 0, 0);
    }
    while layout.rings.last().is_some_and(Vec::is_empty) {
        layout.rings.pop();
    }
    layout
}

struct Placer<'a> {
    tree: &'a Tree,
    metric: Metric,
    params: LayoutParams,
    search: Option<&'a SearchResult>,
    rings: &'a mut [Vec<Arc>],
}

impl Placer<'_> {
    fn relevant(&self, id: NodeId) -> bool {
        self.search.is_none_or(|s| s.is_relevant(id))
    }

    /// Add `arc` to ring `depth`; ring-0 arcs start a new branch. Returns
    /// the arc's branch.
    fn push(&mut self, depth: usize, mut arc: Arc) -> u32 {
        if depth == 0 {
            arc.branch = self.rings[0].len() as u32;
        }
        self.rings[depth].push(arc);
        arc.branch
    }

    fn place(&mut self, node: NodeId, start: f64, span: f64, depth: usize, branch: u32) {
        if depth >= self.params.max_depth {
            return;
        }
        let tree = self.tree;
        let size = tree.size(node, self.metric);
        if size == 0 {
            return;
        }
        let scale = span / size as f64;
        let mut cursor = start;
        let (mut small_size, mut small_count, mut small_relevant) = (0u64, 0u32, false);
        let mut last_small = None;
        for child in tree.children(node, self.metric) {
            let cs = tree.size(child, self.metric);
            if cs == 0 {
                continue;
            }
            let s = cs as f64 * scale;
            if s < self.params.min_span {
                small_size += cs;
                small_count += 1;
                small_relevant |= self.relevant(child);
                last_small = Some(child);
                continue;
            }
            let arc = Arc {
                segment: Segment::Node(child),
                start: cursor,
                end: cursor + s,
                size: cs,
                kind: tree.node(child).kind(),
                branch,
                relevant: self.relevant(child),
            };
            let b = self.push(depth, arc);
            if tree.node(child).is_dir() {
                self.place(child, cursor, s, depth + 1, b);
            }
            cursor += s;
        }
        let end = (cursor + small_size as f64 * scale).min(start + span);
        if small_count > 0 && end > cursor {
            let (segment, kind) = match last_small {
                Some(n) if small_count == 1 => (Segment::Node(n), tree.node(n).kind()),
                _ => (Segment::Small { parent: node, count: small_count }, Kind::SmallItems),
            };
            let arc = Arc { segment, start: cursor, end, size: small_size, kind, branch, relevant: small_relevant };
            self.push(depth, arc);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::test_util::{T::*, tree};

    #[test]
    fn layout_and_hit_roundtrip() {
        let t = tree(vec![
            D("a", vec![F("x.mov", 600), F("y.mov", 200)]),
            D("b", vec![F("w.png", 199)]),
            F("tiny1", 1),
            F("tiny2", 1),
        ]);
        let l = sunburst(&t, t.root(), Metric::Allocated, LayoutParams { max_depth: 4, min_span: 0.01 }, None);
        assert_eq!(l.total, 1001);
        assert_eq!(l.rings.len(), 2);
        // ring 0: a, b, small(2)
        assert_eq!(l.rings[0].len(), 3);
        assert!(matches!(l.rings[0][2].segment, Segment::Small { count: 2, .. }));
        assert!((l.rings[0].last().unwrap().end - 1.0).abs() < 1e-9);
        // Every arc's midpoint hits that arc.
        for (r, ring) in l.rings.iter().enumerate() {
            for a in ring {
                assert_eq!(l.hit(r, a.mid()).unwrap().segment, a.segment);
            }
            // Arcs are sorted and non-overlapping.
            for w in ring.windows(2) {
                assert!(w[0].end <= w[1].start + 1e-12);
            }
        }
        // Children lie within their parent's span.
        let a = &l.rings[0][0];
        for c in &l.rings[1] {
            if c.branch == 0 {
                assert!(c.start >= a.start - 1e-12 && c.end <= a.end + 1e-12);
            }
        }
        assert!(l.hit(5, 0.5).is_none());
        let a_id = a.node().unwrap();
        assert_eq!(l.find(Segment::Node(a_id)).unwrap().0, 0);
        let desc: Vec<_> = l.descendants(0, a).collect();
        assert_eq!(desc.len(), 2);
        assert!(desc.iter().all(|(r, _)| *r == 1));
    }

    #[test]
    fn small_items_keep_total() {
        let kids: Vec<_> = (0..1000).map(|_| F("f", 1)).collect();
        let t = tree(kids);
        let l = sunburst(&t, t.root(), Metric::Allocated, LayoutParams::default(), None);
        assert_eq!(l.rings[0].len(), 1);
        assert_eq!(l.rings[0][0].size, 1000);
        assert!((l.rings[0][0].span() - 1.0).abs() < 1e-9);
    }
}

#[cfg(test)]
mod search_tests {
    use super::*;
    use crate::tree::test_util::{T::*, tree};

    #[test]
    fn relevance_marks_small_buckets() {
        let mut kids = vec![D("big", vec![F("x.mov", 10_000)])];
        kids.extend((0..50).map(|_| F("noise", 1)));
        kids.push(F("needle", 1));
        let t = tree(kids);
        let s = crate::search::search(&t, "needle", &Default::default()).unwrap();
        let l = sunburst(&t, t.root(), Metric::Allocated, LayoutParams::default(), Some(&s));
        let big = &l.rings[0][0];
        assert!(!big.relevant);
        let small = l.rings[0].iter().find(|a| matches!(a.segment, Segment::Small { .. })).unwrap();
        assert!(small.relevant, "bucket containing the match stays lit");
    }
}
