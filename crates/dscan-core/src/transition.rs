use std::collections::{HashMap, HashSet};

use crate::kind::Kind;
use crate::layout::{Arc, Layout, MAX_DEPTH, Segment};
use crate::tree::NodeId;

const LAYER_REVEAL_DELAY: f64 = 0.07;
const REVEAL_START_SCALE: f64 = 0.7;
const NEGLIGIBLE: f64 = 1e-9;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SegmentStyle {
    pub kind: Kind,
    pub branch: u32,
    pub relevant: bool,
}

impl SegmentStyle {
    fn of(arc: &Arc) -> Self {
        SegmentStyle { kind: arc.kind, branch: arc.branch, relevant: arc.relevant }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameArc {
    pub inner_ring: f64,
    pub outer_ring: f64,
    pub start: f64,
    pub end: f64,
    pub style_before: SegmentStyle,
    pub style_after: SegmentStyle,
}

impl FrameArc {
    pub fn span(&self) -> f64 {
        self.end - self.start
    }

    fn resting(ring: usize, arc: &Arc) -> Self {
        let style = SegmentStyle::of(arc);
        FrameArc {
            inner_ring: ring as f64,
            outer_ring: ring as f64 + 1.0,
            start: arc.start,
            end: arc.end,
            style_before: style,
            style_after: style,
        }
    }

    fn has_area(&self) -> bool {
        self.span() > NEGLIGIBLE && self.outer_ring - self.inner_ring > NEGLIGIBLE
    }
}

pub fn at_rest(layout: &Layout) -> Vec<FrameArc> {
    layout
        .rings
        .iter()
        .enumerate()
        .flat_map(|(ring, arcs)| arcs.iter().map(move |a| FrameArc::resting(ring, a)))
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransitionKind {
    Reveal,
    ZoomIn,
    ZoomOut,
    Morph,
}

#[derive(Clone, Debug)]
pub struct Transition {
    motion: Motion,
    total_before: u64,
    total_after: u64,
}

#[derive(Clone, Debug)]
enum Motion {
    Reveal(Layout),
    Zoom(Zoom),
    Morph(Vec<Vec<MorphArc>>),
}

impl Transition {
    pub fn reveal(layout: &Layout) -> Self {
        Transition { motion: Motion::Reveal(layout.clone()), total_before: 0, total_after: layout.total }
    }

    pub fn between(from: &Layout, to: &Layout) -> Self {
        let motion = if from.focus == to.focus {
            Motion::Morph(morph(from, to))
        } else if let Some((ring, anchor)) = from.find(Segment::Node(to.focus)) {
            Motion::Zoom(Zoom::new(from, to, ring, anchor, ZoomDirection::Inward))
        } else if let Some((ring, anchor)) = to.find(Segment::Node(from.focus)) {
            Motion::Zoom(Zoom::new(from, to, ring, anchor, ZoomDirection::Outward))
        } else {
            Motion::Reveal(to.clone())
        };
        Transition { motion, total_before: from.total, total_after: to.total }
    }

    pub fn total(&self, progress: f64) -> u64 {
        total_at(self.total_before, self.total_after, progress)
    }

    pub fn kind(&self) -> TransitionKind {
        match &self.motion {
            Motion::Reveal(_) => TransitionKind::Reveal,
            Motion::Zoom(Zoom { direction: ZoomDirection::Inward, .. }) => TransitionKind::ZoomIn,
            Motion::Zoom(Zoom { direction: ZoomDirection::Outward, .. }) => TransitionKind::ZoomOut,
            Motion::Morph(_) => TransitionKind::Morph,
        }
    }

    pub fn frame(&self, progress: f64) -> Vec<FrameArc> {
        let progress = progress.clamp(0.0, 1.0);
        match &self.motion {
            Motion::Reveal(layout) => reveal_frame(layout, progress),
            Motion::Zoom(zoom) => zoom.frame(progress),
            Motion::Morph(rings) => morph_frame(rings, progress),
        }
    }
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

pub(crate) fn total_at(before: u64, after: u64, progress: f64) -> u64 {
    lerp(before as f64, after as f64, progress.clamp(0.0, 1.0)).round() as u64
}

pub(crate) fn reveal_scale(t: f64) -> f64 {
    lerp(REVEAL_START_SCALE, 1.0, t)
}

pub(crate) struct StaggeredReveal {
    layer_duration: f64,
}

impl StaggeredReveal {
    pub(crate) fn up_to(deepest_layer: usize) -> Self {
        StaggeredReveal { layer_duration: (1.0 - LAYER_REVEAL_DELAY * deepest_layer as f64).max(NEGLIGIBLE) }
    }

    pub(crate) fn progress(&self, t: f64, layer: usize) -> f64 {
        ((t - layer as f64 * LAYER_REVEAL_DELAY) / self.layer_duration).clamp(0.0, 1.0)
    }
}

fn reveal_frame(layout: &Layout, t: f64) -> Vec<FrameArc> {
    let stagger = StaggeredReveal::up_to(layout.rings.len().saturating_sub(1));
    let scale = reveal_scale(t);
    let mut out = Vec::new();
    for (ring, arcs) in layout.rings.iter().enumerate() {
        let swept = stagger.progress(t, ring);
        out.extend(arcs.iter().map(|a| FrameArc {
            inner_ring: ring as f64 * scale,
            outer_ring: (ring as f64 + 1.0) * scale,
            start: a.start.min(swept),
            end: a.end.min(swept),
            ..FrameArc::resting(ring, a)
        }));
    }
    out.retain(FrameArc::has_area);
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Key {
    Node(NodeId),
    SmallItemsOf(NodeId),
}

fn key(segment: Segment) -> Key {
    match segment {
        Segment::Node(n) => Key::Node(n),
        Segment::Small { parent, .. } => Key::SmallItemsOf(parent),
    }
}

fn styles_by_key(layout: &Layout) -> HashMap<Key, SegmentStyle> {
    layout.rings.iter().flatten().map(|a| (key(a.segment), SegmentStyle::of(a))).collect()
}

#[derive(Clone, Copy, Debug)]
enum ZoomDirection {
    Inward,
    Outward,
}

#[derive(Clone, Copy, Debug)]
struct PlacedArc {
    ring: f64,
    start: f64,
    end: f64,
    style_before: SegmentStyle,
    style_after: SegmentStyle,
}

#[derive(Clone, Copy, Debug)]
struct Window {
    start: f64,
    width: f64,
    ring: f64,
}

#[derive(Clone, Debug)]
struct Zoom {
    arcs_in_outer_frame: Vec<PlacedArc>,
    anchor_ring: usize,
    anchor_start: f64,
    anchor_width: f64,
    direction: ZoomDirection,
}

impl Zoom {
    fn new(from: &Layout, to: &Layout, anchor_ring: usize, anchor: &Arc, direction: ZoomDirection) -> Self {
        let (outer, inner) = match direction {
            ZoomDirection::Inward => (from, to),
            ZoomDirection::Outward => (to, from),
        };
        let (styles_from, styles_to) = (styles_by_key(from), styles_by_key(to));
        let placed = |ring: f64, start: f64, end: f64, arc: &Arc| {
            let own = SegmentStyle::of(arc);
            let k = key(arc.segment);
            PlacedArc {
                ring,
                start,
                end,
                style_before: styles_from.get(&k).copied().unwrap_or(own),
                style_after: styles_to.get(&k).copied().unwrap_or(own),
            }
        };
        let replaced_by_inner: HashSet<Key> =
            outer.descendants(anchor_ring, anchor).map(|(_, a)| key(a.segment)).collect();
        let outer_arcs = outer.rings.iter().enumerate().flat_map(|(ring, arcs)| {
            arcs.iter().filter(|a| !replaced_by_inner.contains(&key(a.segment))).map(move |a| (ring, a))
        });
        let inner_arcs_in_outer_frame = inner.rings.iter().enumerate().flat_map(|(ring, arcs)| {
            arcs.iter().map(move |a| {
                (
                    anchor_ring + 1 + ring,
                    a,
                    lerp(anchor.start, anchor.end, a.start),
                    lerp(anchor.start, anchor.end, a.end),
                )
            })
        });
        let mut arcs_in_outer_frame: Vec<PlacedArc> =
            outer_arcs.map(|(ring, a)| placed(ring as f64, a.start, a.end, a)).collect();
        arcs_in_outer_frame
            .extend(inner_arcs_in_outer_frame.map(|(ring, a, start, end)| placed(ring as f64, start, end, a)));
        Zoom { arcs_in_outer_frame, anchor_ring, anchor_start: anchor.start, anchor_width: anchor.span(), direction }
    }

    fn window(&self, zoomed_in: f64) -> Window {
        let anchor_fills_the_turn = 1.0 - self.anchor_width < NEGLIGIBLE;
        let width = self.anchor_width.powf(zoomed_in);
        let start = if anchor_fills_the_turn {
            self.anchor_start * zoomed_in
        } else {
            self.anchor_start * (1.0 - width) / (1.0 - self.anchor_width)
        };
        Window { start, width, ring: (self.anchor_ring + 1) as f64 * zoomed_in }
    }

    fn frame(&self, t: f64) -> Vec<FrameArc> {
        let zoomed_in = match self.direction {
            ZoomDirection::Inward => t,
            ZoomDirection::Outward => 1.0 - t,
        };
        let w = self.window(zoomed_in);
        let outermost = MAX_DEPTH as f64;
        let mut out: Vec<FrameArc> = self
            .arcs_in_outer_frame
            .iter()
            .map(|a| {
                let ring = a.ring - w.ring;
                FrameArc {
                    inner_ring: ring.max(0.0),
                    outer_ring: (ring + 1.0).min(outermost),
                    start: ((a.start - w.start) / w.width).clamp(0.0, 1.0),
                    end: ((a.end - w.start) / w.width).clamp(0.0, 1.0),
                    style_before: a.style_before,
                    style_after: a.style_after,
                }
            })
            .collect();
        out.retain(FrameArc::has_area);
        out
    }
}

type Span = (f64, f64);

#[derive(Clone, Copy, Debug)]
enum Travel {
    Between { from: Span, to: Span },
    WithParent { parent_slot: usize, share_of_parent: Span },
}

#[derive(Clone, Copy, Debug)]
struct MorphArc {
    travel: Travel,
    style_before: SegmentStyle,
    style_after: SegmentStyle,
}

fn bounds(a: &Arc) -> Span {
    (a.start, a.end)
}

fn share_of(child: &Arc, parent: &Arc) -> Span {
    let width = parent.span().max(NEGLIGIBLE);
    ((child.start - parent.start) / width, (child.end - parent.start) / width)
}

struct Side<'a> {
    arcs: &'a [Arc],
    parents: Vec<Option<usize>>,
    by_key: HashMap<Key, usize>,
    slots: Vec<usize>,
}

impl<'a> Side<'a> {
    fn new(layout: &'a Layout, ring: usize) -> Self {
        let arcs = layout.rings.get(ring).map_or(&[][..], Vec::as_slice);
        let parents = match ring.checked_sub(1) {
            Some(parent_ring) => arcs.iter().map(|a| layout.hit_index(parent_ring, a.mid())).collect(),
            None => vec![None; arcs.len()],
        };
        Side {
            arcs,
            parents,
            by_key: arcs.iter().enumerate().map(|(i, a)| (key(a.segment), i)).collect(),
            slots: vec![0; arcs.len()],
        }
    }

    fn counterpart_index(&self, i: usize, other: &Side) -> Option<usize> {
        other.by_key.get(&key(self.arcs[i].segment)).copied()
    }

    fn siblings_after(&self, i: usize) -> impl Iterator<Item = usize> + '_ {
        (i + 1..self.arcs.len()).take_while(move |&j| self.parents[j] == self.parents[i])
    }

    fn siblings_before(&self, i: usize) -> impl Iterator<Item = usize> + '_ {
        (0..i).rev().take_while(move |&j| self.parents[j] == self.parents[i])
    }
}

#[derive(Clone, Copy)]
enum Lone {
    Leaving,
    Arriving,
}

fn point_where_a_lone_arc_leaves_or_arrives(
    own: &Side,
    other: &Side,
    i: usize,
    parent_counterpart: Option<&Arc>,
) -> f64 {
    let counterpart = |j: usize| own.counterpart_index(j, other).map(|k| &other.arcs[k]);
    if let Some(next_sibling) = own.siblings_after(i).find_map(counterpart) {
        return next_sibling.start;
    }
    if let Some(previous_sibling) = own.siblings_before(i).find_map(counterpart) {
        return previous_sibling.end;
    }
    parent_counterpart.map_or(own.arcs[i].mid(), Arc::mid)
}

fn lone_arc(own: &Side, other: &Side, i: usize, parent_ring: Option<(&Side, &Side)>, lone: Lone) -> MorphArc {
    let arc = &own.arcs[i];
    let style = SegmentStyle::of(arc);
    let parent = own.parents[i].zip(parent_ring);
    let parent_counterpart = parent.and_then(|(p, (own_parents, other_parents))| {
        own_parents.counterpart_index(p, other_parents).map(|k| &other_parents.arcs[k])
    });
    let travel = match parent {
        Some((p, (own_parents, _))) if parent_counterpart.is_none() => Travel::WithParent {
            parent_slot: own_parents.slots[p],
            share_of_parent: share_of(arc, &own_parents.arcs[p]),
        },
        _ => {
            let point = point_where_a_lone_arc_leaves_or_arrives(own, other, i, parent_counterpart);
            match lone {
                Lone::Leaving => Travel::Between { from: bounds(arc), to: (point, point) },
                Lone::Arriving => Travel::Between { from: (point, point), to: bounds(arc) },
            }
        }
    };
    MorphArc { travel, style_before: style, style_after: style }
}

fn morph(from: &Layout, to: &Layout) -> Vec<Vec<MorphArc>> {
    let rings = from.rings.len().max(to.rings.len());
    let mut out: Vec<Vec<MorphArc>> = Vec::with_capacity(rings);
    let mut parent_ring: Option<(Side, Side)> = None;
    for ring in 0..rings {
        let (mut old, mut new) = (Side::new(from, ring), Side::new(to, ring));
        let mut slots: Vec<MorphArc> = Vec::new();
        for i in 0..old.arcs.len() {
            let morph_arc = match old.counterpart_index(i, &new) {
                Some(j) => {
                    new.slots[j] = slots.len();
                    MorphArc {
                        travel: Travel::Between { from: bounds(&old.arcs[i]), to: bounds(&new.arcs[j]) },
                        style_before: SegmentStyle::of(&old.arcs[i]),
                        style_after: SegmentStyle::of(&new.arcs[j]),
                    }
                }
                None => lone_arc(&old, &new, i, parent_ring.as_ref().map(|(o, n)| (o, n)), Lone::Leaving),
            };
            old.slots[i] = slots.len();
            slots.push(morph_arc);
        }
        for j in 0..new.arcs.len() {
            if new.counterpart_index(j, &old).is_none() {
                new.slots[j] = slots.len();
                slots.push(lone_arc(&new, &old, j, parent_ring.as_ref().map(|(o, n)| (n, o)), Lone::Arriving));
            }
        }
        out.push(slots);
        parent_ring = Some((old, new));
    }
    out
}

fn morph_frame(rings: &[Vec<MorphArc>], t: f64) -> Vec<FrameArc> {
    let mut out = Vec::new();
    let mut parent_spans: Vec<Span> = Vec::new();
    for (ring, arcs) in rings.iter().enumerate() {
        let spans: Vec<Span> = arcs
            .iter()
            .map(|m| match m.travel {
                Travel::Between { from, to } => (lerp(from.0, to.0, t), lerp(from.1, to.1, t)),
                Travel::WithParent { parent_slot, share_of_parent: (s, e) } => {
                    let (parent_start, parent_end) = parent_spans[parent_slot];
                    (lerp(parent_start, parent_end, s), lerp(parent_start, parent_end, e))
                }
            })
            .collect();
        out.extend(arcs.iter().zip(&spans).map(|(m, &(start, end))| FrameArc {
            inner_ring: ring as f64,
            outer_ring: ring as f64 + 1.0,
            start,
            end,
            style_before: m.style_before,
            style_after: m.style_after,
        }));
        parent_spans = spans;
    }
    out.retain(FrameArc::has_area);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exclusions::Exclusions;
    use crate::layout::{LayoutParams, sunburst};
    use crate::tree::Tree;
    use crate::tree::test_util::{T::*, descend, tree};

    const SHOW_EVERY_ARC: LayoutParams = LayoutParams { max_depth: MAX_DEPTH, min_span: 1e-9 };
    const SAME_PLACE: f64 = 1e-9;

    fn sample() -> Tree {
        tree(vec![
            D("a", vec![F("x.mov", 600), D("deep", vec![F("y.mov", 150), F("z.rs", 50)]), F("w.png", 100)]),
            D("b", vec![F("v.png", 500)]),
            F("note.txt", 100),
        ])
    }

    fn layout(t: &Tree, focus: NodeId, excluded: &[NodeId]) -> Layout {
        sunburst(t, focus, SHOW_EVERY_ARC, None, &Exclusions::new(t, excluded))
    }

    fn assert_same_picture(actual: &[FrameArc], expected: &[FrameArc], what: &str) {
        let order = |arcs: &[FrameArc]| {
            let mut arcs = arcs.to_vec();
            arcs.sort_by(|a, b| a.inner_ring.total_cmp(&b.inner_ring).then(a.start.total_cmp(&b.start)));
            arcs
        };
        let (actual, expected) = (order(actual), order(expected));
        assert_eq!(actual.len(), expected.len(), "{what}: arc count\n{actual:#?}\nvs\n{expected:#?}");
        for (a, e) in actual.iter().zip(&expected) {
            let close =
                [(a.inner_ring, e.inner_ring), (a.outer_ring, e.outer_ring), (a.start, e.start), (a.end, e.end)]
                    .iter()
                    .all(|(x, y)| (x - y).abs() < SAME_PLACE);
            assert!(close, "{what}: {a:?} vs {e:?}");
        }
    }

    #[test]
    fn reveal_wipes_in_ring_by_ring_without_gaps_and_counts_the_total_up() {
        let t = sample();
        let l = layout(&t, t.root(), &[]);
        let reveal = Transition::reveal(&l);
        assert_eq!(reveal.kind(), TransitionKind::Reveal);
        assert!(reveal.frame(0.0).is_empty(), "nothing is drawn before the reveal starts");

        let halfway = reveal.frame(0.5);
        let thickness = halfway[0].outer_ring - halfway[0].inner_ring;
        let ring_of = |a: &FrameArc| (a.inner_ring / thickness).round() as usize;
        for a in &halfway {
            let resting = l.rings[ring_of(a)].iter().find(|r| (r.start - a.start).abs() < SAME_PLACE);
            assert!(
                resting.is_some_and(|r| a.end <= r.end + SAME_PLACE),
                "the wipe uncovers arcs where they belong instead of moving them: {a:?}"
            );
        }
        let uncovered = |ring: usize| halfway.iter().filter(|a| ring_of(a) == ring).map(|a| a.end).fold(0.0, f64::max);
        assert!(uncovered(0) < 1.0, "halfway, ring 0 has not closed yet");
        assert!((1..l.rings.len()).all(|r| uncovered(r) < uncovered(0)), "outer rings trail the inner one");
        let edge_to_edge = |a: &FrameArc| {
            let rings_inside = a.inner_ring / thickness;
            (a.outer_ring - a.inner_ring - thickness).abs() < SAME_PLACE
                && (rings_inside - rings_inside.round()).abs() < SAME_PLACE
        };
        assert!(halfway.iter().all(edge_to_edge), "rings sit edge to edge while they grow, with no gaps between them");

        assert_eq!(reveal.total(0.0), 0, "the total counts up from nothing");
        assert_eq!(reveal.total(1.0), l.total);
        assert_same_picture(&reveal.frame(1.0), &at_rest(&l), "a finished reveal is the chart");
    }

    #[test]
    fn zooming_into_a_folder_grows_its_arc_into_the_whole_chart() {
        let t = sample();
        let a = descend(&t, &["a"]);
        let (outer, inner) = (layout(&t, t.root(), &[]), layout(&t, a, &[]));
        let zoom = Transition::between(&outer, &inner);
        assert_eq!(zoom.kind(), TransitionKind::ZoomIn);
        assert_same_picture(&zoom.frame(0.0), &at_rest(&outer), "the zoom starts from the current chart");
        assert_same_picture(&zoom.frame(1.0), &at_rest(&inner), "the zoom ends on the folder's chart");

        let a_span_before = outer.find(Segment::Node(a)).unwrap().1.span();
        let halfway = zoom.frame(0.5);
        let x = descend(&t, &["a", "x.mov"]);
        let x_before = outer.find(Segment::Node(x)).unwrap().1.span();
        let x_after = inner.find(Segment::Node(x)).unwrap().1.span();
        let x_halfway = halfway
            .iter()
            .find(|f| f.inner_ring > 0.0 && f.inner_ring < 1.0 && f.style_after.kind == Kind::Video)
            .map(FrameArc::span);
        assert!(a_span_before < 1.0);
        assert!(
            x_halfway.is_some_and(|s| s > x_before && s < x_after),
            "a child of the folder widens and moves inward: {x_halfway:?} between {x_before} and {x_after}"
        );
    }

    #[test]
    fn going_up_shrinks_the_chart_back_into_its_folder_s_arc() {
        let t = sample();
        let a = descend(&t, &["a"]);
        let (outer, inner) = (layout(&t, t.root(), &[]), layout(&t, a, &[]));
        let zoom = Transition::between(&inner, &outer);
        assert_eq!(zoom.kind(), TransitionKind::ZoomOut);
        assert_same_picture(&zoom.frame(0.0), &at_rest(&inner), "going up starts from the folder's chart");
        assert_same_picture(&zoom.frame(1.0), &at_rest(&outer), "going up ends on the parent's chart");
    }

    #[test]
    fn jumping_between_unrelated_folders_reveals_the_new_chart() {
        let t = sample();
        let jump = Transition::between(&layout(&t, descend(&t, &["a"]), &[]), &layout(&t, descend(&t, &["b"]), &[]));
        assert_eq!(jump.kind(), TransitionKind::Reveal);
    }

    #[test]
    fn collecting_a_file_shrinks_its_arc_away_while_the_rest_grows_into_the_gap() {
        let t = sample();
        let x = descend(&t, &["a", "x.mov"]);
        let (before, after) = (layout(&t, t.root(), &[]), layout(&t, t.root(), &[x]));
        let morph = Transition::between(&before, &after);
        assert_eq!(morph.kind(), TransitionKind::Morph);
        assert_same_picture(&morph.frame(0.0), &at_rest(&before), "the morph starts from the current chart");
        assert_same_picture(&morph.frame(1.0), &at_rest(&after), "the morph ends without the collected file");
        assert_eq!(
            (morph.total(0.0), morph.total(1.0)),
            (before.total, after.total),
            "the total counts down to what stays"
        );

        let x_before = before.find(Segment::Node(x)).unwrap().1;
        let halfway = morph.frame(0.5);
        let x_halfway = halfway
            .iter()
            .filter(|f| f.inner_ring == 1.0 && f.style_before.kind == x_before.kind)
            .map(FrameArc::span)
            .fold(f64::MAX, f64::min);
        assert!(x_halfway > 0.0 && x_halfway < x_before.span(), "halfway the file is narrower but still there");
    }

    #[test]
    fn a_collected_folder_s_contents_shrink_inside_it_instead_of_flying_off() {
        let t = sample();
        let a = descend(&t, &["a"]);
        let (before, after) = (layout(&t, t.root(), &[]), layout(&t, t.root(), &[a]));
        let morph = Transition::between(&before, &after);
        assert_same_picture(&morph.frame(1.0), &at_rest(&after), "the folder and its contents are gone at the end");
        for t in [0.25, 0.5, 0.75] {
            let frame = morph.frame(t);
            let a_now = frame.iter().find(|f| f.inner_ring == 0.0 && f.style_before.kind == Kind::Video).unwrap();
            for inside in frame.iter().filter(|f| f.inner_ring > 0.0 && f.style_before.kind != Kind::Image) {
                assert!(
                    inside.start >= a_now.start - SAME_PLACE && inside.end <= a_now.end + SAME_PLACE,
                    "at {t} every arc of the folder stays within it: {inside:?} outside {a_now:?}"
                );
            }
        }
    }

    #[test]
    fn uncollecting_grows_the_item_back_from_where_it_belongs() {
        let t = sample();
        let x = descend(&t, &["a", "x.mov"]);
        let (with_x_collected, restored) = (layout(&t, t.root(), &[x]), layout(&t, t.root(), &[]));
        let morph = Transition::between(&with_x_collected, &restored);
        assert_same_picture(&morph.frame(0.0), &at_rest(&with_x_collected), "starts without the file");
        assert_same_picture(&morph.frame(1.0), &at_rest(&restored), "ends with the file back");
    }
}
