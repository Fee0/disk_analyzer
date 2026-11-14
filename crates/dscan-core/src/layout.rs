use crate::age::{AgeBucket, Day};
use crate::exclusions::Exclusions;
use crate::kind::Kind;
use crate::paint::{ColorBy, Paint};
use crate::tree::{NodeId, Tree};

pub const MAX_DEPTH: usize = 7;

const DEGREES_PER_TURN: f64 = 360.0;
const NARROWEST_DRAWN_ARC_DEGREES: f64 = 0.6;
const TURN_FRACTION_EPSILON: f64 = 1e-12;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Segment {
    Node(NodeId),
    Small { parent: NodeId, count: u32, lit: bool },
}

impl Segment {
    pub fn node(&self) -> Option<NodeId> {
        match *self {
            Segment::Node(n) => Some(n),
            Segment::Small { .. } => None,
        }
    }
}

#[derive(Clone, Copy)]
struct SmallGroup {
    lit: bool,
    size: u64,
    count: u32,
    newest: Option<Day>,
    last: Option<NodeId>,
}

impl SmallGroup {
    fn empty(lit: bool) -> SmallGroup {
        SmallGroup { lit, size: 0, count: 0, newest: None, last: None }
    }

    fn add(&mut self, tree: &Tree, child: NodeId, size: u64) {
        self.size += size;
        self.count += 1;
        self.newest = self.newest.max(tree.newest_change(child));
        self.last = Some(child);
    }

    fn is_empty(&self) -> bool {
        self.count == 0
    }

    fn piece(&self, tree: &Tree, parent: NodeId, color_by: ColorBy) -> Option<Piece> {
        let last = self.last?;
        let (segment, kind, shown_age) = if self.count == 1 {
            (Segment::Node(last), tree.node(last).kind(), color_by.shown_age(tree, last))
        } else {
            let group = Segment::Small { parent, count: self.count, lit: self.lit };
            (group, Kind::SmallItems, color_by.shown_age_of_newest(tree, self.newest))
        };
        Some(Piece { segment, size: self.size, kind, shown_age, relevant: self.lit })
    }
}

pub(crate) struct SmallItems {
    lit: SmallGroup,
    dimmed: SmallGroup,
}

impl Default for SmallItems {
    fn default() -> Self {
        SmallItems { lit: SmallGroup::empty(true), dimmed: SmallGroup::empty(false) }
    }
}

impl SmallItems {
    pub(crate) fn add(&mut self, tree: &Tree, child: NodeId, size: u64, lit: bool) {
        let group = if lit { &mut self.lit } else { &mut self.dimmed };
        group.add(tree, child, size);
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.lit.is_empty() && self.dimmed.is_empty()
    }

    pub(crate) fn pieces(
        &self,
        tree: &Tree,
        parent: NodeId,
        color_by: ColorBy,
        big_enough_to_show: impl Fn(u64) -> bool,
    ) -> impl Iterator<Item = Piece> {
        let shows = |group: &SmallGroup| !group.is_empty() && big_enough_to_show(group.size);
        let groups = if shows(&self.lit) && shows(&self.dimmed) {
            let (larger, smaller) =
                if self.dimmed.size > self.lit.size { (self.dimmed, self.lit) } else { (self.lit, self.dimmed) };
            [Some(larger), Some(smaller)]
        } else {
            [Some(self.together()), None]
        };
        groups.into_iter().flatten().filter_map(move |group| group.piece(tree, parent, color_by))
    }

    fn together(&self) -> SmallGroup {
        let lit = !self.lit.is_empty();
        let colored_like = if lit { &self.lit } else { &self.dimmed };
        SmallGroup {
            lit,
            size: self.lit.size + self.dimmed.size,
            count: self.lit.count + self.dimmed.count,
            newest: colored_like.newest,
            last: colored_like.last,
        }
    }
}

pub(crate) struct Piece {
    pub(crate) segment: Segment,
    pub(crate) size: u64,
    pub(crate) kind: Kind,
    pub(crate) shown_age: Option<AgeBucket>,
    pub(crate) relevant: bool,
}

impl Piece {
    pub(crate) fn of_node(tree: &Tree, id: NodeId, size: u64, paint: &Paint<'_>) -> Piece {
        let node = tree.node(id);
        Piece {
            segment: Segment::Node(id),
            size,
            kind: node.kind(),
            shown_age: paint.color_by.shown_age(tree, id),
            relevant: paint.lights(tree, id),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Arc {
    pub segment: Segment,
    pub start: f64,
    pub end: f64,
    pub size: u64,
    pub kind: Kind,
    pub shown_age: Option<AgeBucket>,
    pub branch: u32,
    pub relevant: bool,
}

impl Arc {
    pub fn node(&self) -> Option<NodeId> {
        self.segment.node()
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
        self.hit_index(ring, turn_fraction).map(|i| &self.rings[ring][i])
    }

    pub fn hit_index(&self, ring: usize, turn_fraction: f64) -> Option<usize> {
        let arcs = self.rings.get(ring)?;
        let turn_fraction = turn_fraction.rem_euclid(1.0);
        let i = arcs.partition_point(|a| a.end <= turn_fraction);
        arcs.get(i).filter(|a| a.start <= turn_fraction && turn_fraction < a.end).map(|_| i)
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

pub fn partition(tree: &Tree, focus: NodeId, params: LayoutParams, paint: Paint<'_>, excluded: &Exclusions) -> Layout {
    let total = excluded.size(tree, focus);
    let mut layout = Layout { focus, total, rings: vec![Vec::new(); params.max_depth] };
    if total > 0 {
        let mut placer = Placer { tree, params, paint, excluded, rings: &mut layout.rings };
        placer.place(focus, 0.0, 1.0, 0, 0);
    }
    while layout.rings.last().is_some_and(Vec::is_empty) {
        layout.rings.pop();
    }
    layout
}

struct Placer<'a> {
    tree: &'a Tree,
    params: LayoutParams,
    paint: Paint<'a>,
    excluded: &'a Exclusions,
    rings: &'a mut [Vec<Arc>],
}

impl Placer<'_> {
    fn is_relevant(&self, id: NodeId) -> bool {
        self.paint.lights(self.tree, id)
    }

    fn arc(&self, piece: Piece, start: f64, end: f64, branch: u32) -> Arc {
        let Piece { segment, size, kind, shown_age, relevant } = piece;
        Arc { segment, start, end, size, kind, shown_age, branch, relevant }
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
        let size = self.excluded.size(tree, node);
        if size == 0 {
            return;
        }
        let scale = span / size as f64;
        let mut cursor = start;
        let mut small = SmallItems::default();
        for (child, cs) in self.excluded.children_with_sizes(tree, node) {
            if cs == 0 {
                continue;
            }
            let s = cs as f64 * scale;
            if s < self.params.min_span {
                small.add(tree, child, cs, self.is_relevant(child));
                continue;
            }
            let arc = self.arc(Piece::of_node(tree, child, cs, &self.paint), cursor, cursor + s, branch);
            let b = self.push_returning_branch(depth, arc);
            if tree.node(child).is_dir() {
                self.place(child, cursor, s, depth + 1, b);
            }
            cursor += s;
        }
        let min_span = self.params.min_span;
        let wide_enough = |size: u64| size as f64 * scale >= min_span;
        for piece in small.pieces(tree, node, self.paint.color_by, wide_enough) {
            let end = (cursor + piece.size as f64 * scale).min(start + span);
            if end > cursor {
                let arc = self.arc(piece, cursor, end, branch);
                self.push_returning_branch(depth, arc);
                cursor = end;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::test_util::{T::*, descend, tree};

    #[test]
    fn layout_and_hit_roundtrip() {
        let t = tree(vec![
            D("a", vec![F("x.mov", 600), F("y.mov", 200)]),
            D("b", vec![F("w.png", 199)]),
            F("tiny1", 1),
            F("tiny2", 1),
        ]);
        let l = partition(
            &t,
            t.root(),
            LayoutParams { max_depth: 4, min_span: 0.01 },
            Paint::default(),
            &Exclusions::default(),
        );
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
    fn excluded_items_leave_the_chart_and_their_space_goes_to_the_rest() {
        let t = tree(vec![D("a", vec![F("x.mov", 600), F("y.mov", 200)]), D("b", vec![F("w.png", 400)])]);
        let x = descend(&t, &["a", "x.mov"]);
        let params = LayoutParams { max_depth: 4, min_span: 0.01 };
        let l = partition(&t, t.root(), params, Paint::default(), &Exclusions::new(&t, &[x]));

        assert_eq!(l.total, t.size(t.root()) - t.size(x));
        assert!(l.find(Segment::Node(x)).is_none(), "an excluded file has no arc");
        let ring0: Vec<_> = l.rings[0].iter().map(|arc| t.name(arc.node().unwrap()).into_owned()).collect();
        assert_eq!(ring0, ["b", "a"], "the shrunken folder is re-ranked");
        assert!((l.rings[0].last().unwrap().end - 1.0).abs() < 1e-9, "the remaining arcs still fill the turn");
    }

    #[test]
    fn small_items_keep_total() {
        let kids: Vec<_> = (0..1000).map(|_| F("f", 1)).collect();
        let t = tree(kids);
        let l = partition(&t, t.root(), LayoutParams::default(), Paint::default(), &Exclusions::default());
        assert_eq!(l.rings[0].len(), 1);
        assert_eq!(l.rings[0][0].size, 1000);
        assert!((l.rings[0][0].span() - 1.0).abs() < 1e-9);
    }
}

#[cfg(test)]
mod search_tests {
    use super::*;
    use crate::tree::test_util::{T::*, tree};

    #[derive(Debug, PartialEq)]
    struct SmallGroupArc {
        count: u32,
        size: u64,
        lit: bool,
        shown_age: Option<AgeBucket>,
    }

    fn small_groups(t: &Tree, paint: Paint<'_>) -> (Layout, Vec<SmallGroupArc>) {
        let l = partition(t, t.root(), LayoutParams::default(), paint, &Exclusions::default());
        let groups = l.rings[0]
            .iter()
            .filter_map(|a| match a.segment {
                Segment::Small { count, lit, .. } => {
                    assert_eq!(lit, a.relevant, "a group is lit exactly when it is drawn lit");
                    Some(SmallGroupArc { count, size: a.size, lit, shown_age: a.shown_age })
                }
                Segment::Node(_) => None,
            })
            .collect();
        (l, groups)
    }

    fn group(count: u32, size: u64, lit: bool) -> SmallGroupArc {
        SmallGroupArc { count, size, lit, shown_age: None }
    }

    fn aged_group(count: u32, size: u64, lit: bool, age: AgeBucket) -> SmallGroupArc {
        SmallGroupArc { shown_age: Some(age), ..group(count, size, lit) }
    }

    fn noise_and_needles(needles: usize, needle_size: u64) -> Tree {
        let mut kids = vec![D("big", vec![F("x.mov", 10_000)])];
        kids.extend((0..50).map(|_| F("noise", 10)));
        kids.extend((0..needles).map(|_| F("needle", needle_size)));
        tree(kids)
    }

    #[test]
    fn a_search_splits_the_small_items_into_a_dimmed_group_and_a_lit_group_of_matches_largest_first() {
        let t = noise_and_needles(5, 10);
        let s = crate::search::search(&t, "needle", &Default::default()).unwrap();
        let (l, groups) = small_groups(&t, Paint { search: Some(&s), ..Paint::default() });
        assert!(!l.rings[0][0].relevant, "the big folder holds no match");
        assert_eq!(groups, [group(50, 500, false), group(5, 50, true)], "the matches are lit and sized on their own");
        let (_, unsearched) = small_groups(&t, Paint::default());
        assert_eq!(unsearched, [group(55, 550, true)], "without a filter they stay one group");
    }

    #[test]
    fn a_kind_filter_lights_the_folders_holding_it_and_splits_the_small_items_like_a_search() {
        let mut kids = vec![D("big", vec![F("x.pdf", 9_000), F("clip.mov", 1)])];
        kids.extend((0..50).map(|_| F("noise", 10)));
        kids.extend((0..5).map(|_| F("clip.mov", 10)));
        let t = tree(kids);
        let (l, groups) = small_groups(&t, Paint { only_kind: Some(Kind::Video), ..Paint::default() });
        assert!(l.rings[0][0].relevant, "the folder of documents holds a video, so it stays lit");
        assert_eq!(groups, [group(50, 500, false), group(5, 50, true)]);
    }

    #[test]
    fn matches_too_small_to_see_on_their_own_keep_the_whole_group_lit_instead_of_vanishing() {
        let t = noise_and_needles(2, 1);
        let s = crate::search::search(&t, "needle", &Default::default()).unwrap();
        let (_, groups) = small_groups(&t, Paint { search: Some(&s), ..Paint::default() });
        assert_eq!(groups, [group(52, 502, true)]);
    }

    #[test]
    fn small_items_take_their_newest_member_s_age_and_an_age_filter_splits_off_the_old_ones() {
        use crate::tree::test_util::tree_scanned_on;
        let day = Day::from_days_since_1970;
        let with_fresh = |fresh: usize| {
            let mut kids = vec![Aged("old.mov", 10_000, day(15_000))];
            kids.extend((0..50).map(|_| Aged("old-noise", 1, day(15_000))));
            kids.extend((0..fresh).map(|_| Aged("fresh", 1, day(19_995))));
            tree_scanned_on(day(20_000), kids)
        };
        let by_age = Paint { color_by: ColorBy::Age, ..Paint::default() };
        let older_than_two_years = Paint { aged_at_least: Some(AgeBucket::UnderFiveYears), ..by_age };
        let (_, unfiltered) = small_groups(&with_fresh(30), by_age);
        assert_eq!(
            unfiltered,
            [aged_group(80, 80, true, AgeBucket::UnderAMonth)],
            "one group, as new as its newest member"
        );
        let (l, filtered) = small_groups(&with_fresh(30), older_than_two_years);
        assert_eq!(
            filtered,
            [aged_group(50, 50, true, AgeBucket::FiveYearsOrMore), aged_group(30, 30, false, AgeBucket::UnderAMonth)],
            "the old ones stay lit in their own old color, the fresh ones are dimmed"
        );
        assert_eq!(l.rings[0][0].shown_age, Some(AgeBucket::FiveYearsOrMore));
        assert!(l.rings[0][0].relevant, "old enough to stay lit");
        let (_, too_few_fresh) = small_groups(&with_fresh(3), older_than_two_years);
        assert_eq!(
            too_few_fresh,
            [aged_group(53, 53, true, AgeBucket::FiveYearsOrMore)],
            "too few fresh ones to show apart, so they join the lit group, which takes the old ones' color"
        );
    }
}
