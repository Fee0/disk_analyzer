use crate::kind::Kind;
use crate::search::SearchResult;
use crate::tree::{Metric, NodeId, Tree};

pub const MAX_DEPTH: usize = 7;

const DEGREES_PER_TURN: f64 = 360.0;
const NARROWEST_DRAWN_ARC_DEGREES: f64 = 0.6;
const TURN_FRACTION_EPSILON: f64 = 1e-12;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Segment {
    Node(NodeId),
    Small { parent: NodeId, count: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Arc {
    pub segment: Segment,
    pub start: f64,
    pub end: f64,
    pub size: u64,
    pub kind: Kind,
    pub branch: u32,
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
    pub max_depth: usize,
    pub min_span: f64,
}

impl Default for LayoutParams {
    fn default() -> Self {
        LayoutParams { max_depth: MAX_DEPTH, min_span: NARROWEST_DRAWN_ARC_DEGREES / DEGREES_PER_TURN }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub focus: NodeId,
    pub total: u64,
    pub rings: Vec<Vec<Arc>>,
}

impl Layout {
    pub fn len(&self) -> usize {
        self.rings.iter().map(Vec::len).sum()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn hit(&self, ring: usize, turn_fraction: f64) -> Option<&Arc> {
        let arcs = self.rings.get(ring)?;
        let turn_fraction = turn_fraction.rem_euclid(1.0);
        let i = arcs.partition_point(|a| a.end <= turn_fraction);
        arcs.get(i).filter(|a| a.start <= turn_fraction && turn_fraction < a.end)
    }

    pub fn find(&self, seg: Segment) -> Option<(usize, &Arc)> {
        self.rings.iter().enumerate().find_map(|(r, arcs)| arcs.iter().find(|a| a.segment == seg).map(|a| (r, a)))
    }

    pub fn descendants<'a>(&'a self, ring: usize, arc: &Arc) -> impl Iterator<Item = (usize, &'a Arc)> + 'a {
        let (start, end) = (arc.start, arc.end);
        self.rings.iter().enumerate().skip(ring + 1).flat_map(move |(r, arcs)| {
            let first = arcs.partition_point(|x| x.end <= start + TURN_FRACTION_EPSILON);
            arcs[first..].iter().take_while(move |x| x.start < end - TURN_FRACTION_EPSILON).map(move |x| (r, x))
        })
    }
}

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
        let mut placer = Placer { tree, metric, params, search, rings: &mut layout.rings };
        placer.place(focus, 0.0, 1.0, 0, 0);
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
    fn is_relevant(&self, id: NodeId) -> bool {
        self.search.is_none_or(|s| s.is_relevant(id))
    }

    fn push_returning_branch(&mut self, depth: usize, mut arc: Arc) -> u32 {
        let starts_a_new_branch = depth == 0;
        if starts_a_new_branch {
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
                small_relevant |= self.is_relevant(child);
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
                relevant: self.is_relevant(child),
            };
            let b = self.push_returning_branch(depth, arc);
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
            self.push_returning_branch(depth, arc);
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
        assert_eq!(l.rings[0].len(), 3, "ring 0 holds the focus's children: a, b and one bucket of small items");
        assert!(matches!(l.rings[0][2].segment, Segment::Small { count: 2, .. }));
        assert!((l.rings[0].last().unwrap().end - 1.0).abs() < 1e-9, "ring 0 covers the full turn");
        assert!(l.rings.iter().flatten().all(|a| a.relevant), "without a search every arc is relevant");
        for (r, ring) in l.rings.iter().enumerate() {
            for a in ring {
                assert_eq!(l.hit(r, a.mid()).unwrap().segment, a.segment, "an arc's midpoint hits that arc");
            }
            for w in ring.windows(2) {
                assert!(w[0].end <= w[1].start + TURN_FRACTION_EPSILON, "arcs are sorted and non-overlapping");
            }
        }
        let a = &l.rings[0][0];
        for c in &l.rings[1] {
            if c.branch == 0 {
                assert!(
                    c.start >= a.start - TURN_FRACTION_EPSILON && c.end <= a.end + TURN_FRACTION_EPSILON,
                    "children lie within their parent's span"
                );
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
