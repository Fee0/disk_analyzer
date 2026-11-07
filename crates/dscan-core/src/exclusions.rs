use std::cmp::Reverse;
use std::collections::HashMap;

use crate::tree::{Metric, NodeId, Tree};

type BytesPerMetric = [u64; Metric::ALL.len()];

#[derive(Clone, Debug, Default)]
pub struct Exclusions {
    excluded_below_or_at: HashMap<NodeId, BytesPerMetric>,
}

impl Exclusions {
    pub fn new(tree: &Tree, excluded: &[NodeId]) -> Self {
        let mut excluded_below_or_at: HashMap<NodeId, BytesPerMetric> = HashMap::new();
        for &item in excluded.iter().filter(|&&item| tree.is_live(item)) {
            let item_bytes = Metric::ALL.map(|m| tree.size(item, m));
            for holder in tree.ancestors(item) {
                let held = excluded_below_or_at.entry(holder).or_default();
                for (held, item) in held.iter_mut().zip(item_bytes) {
                    *held += item;
                }
            }
        }
        Exclusions { excluded_below_or_at }
    }

    pub fn is_empty(&self) -> bool {
        self.excluded_below_or_at.is_empty()
    }

    fn touches(&self, id: NodeId) -> bool {
        !self.is_empty() && self.excluded_below_or_at.contains_key(&id)
    }

    pub fn size(&self, tree: &Tree, id: NodeId, m: Metric) -> u64 {
        if self.is_empty() {
            return tree.size(id, m);
        }
        let excluded = self.excluded_below_or_at.get(&id).map_or(0, |bytes| bytes[m.slot()]);
        tree.size(id, m).saturating_sub(excluded)
    }

    pub fn children_with_sizes<'t>(
        &self,
        tree: &'t Tree,
        id: NodeId,
        m: Metric,
    ) -> impl Iterator<Item = (NodeId, u64)> + 't {
        if !self.touches(id) {
            return ChildOrder::AsScanned(tree.children(id, m).map(move |c| (c, tree.size(c, m))));
        }
        let mut kept: Vec<(NodeId, u64)> =
            tree.children(id, m).map(|c| (c, self.size(tree, c, m))).filter(|&(_, size)| size > 0).collect();
        kept.sort_by_key(|&(_, size)| Reverse(size));
        ChildOrder::Resorted(kept.into_iter())
    }

    pub fn children<'t>(&self, tree: &'t Tree, id: NodeId, m: Metric) -> impl Iterator<Item = NodeId> + 't {
        self.children_with_sizes(tree, id, m).map(|(child, _)| child)
    }
}

enum ChildOrder<S, R> {
    AsScanned(S),
    Resorted(R),
}

impl<S: Iterator<Item = (NodeId, u64)>, R: Iterator<Item = (NodeId, u64)>> Iterator for ChildOrder<S, R> {
    type Item = (NodeId, u64);

    fn next(&mut self) -> Option<(NodeId, u64)> {
        match self {
            ChildOrder::AsScanned(children) => children.next(),
            ChildOrder::Resorted(children) => children.next(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::test_util::{T::*, descend, tree};

    #[test]
    fn excluding_an_item_shrinks_every_ancestor_and_hides_the_item() {
        let t = tree(vec![D("a", vec![F("big", 600), F("mid", 300)]), D("b", vec![F("w", 500)])]);
        let (a, big) = (descend(&t, &["a"]), descend(&t, &["a", "big"]));
        let ex = Exclusions::new(&t, &[big]);

        assert_eq!(ex.size(&t, big, Metric::Allocated), 0);
        assert_eq!(ex.size(&t, a, Metric::Allocated), t.size(a, Metric::Allocated) - t.size(big, Metric::Allocated));
        assert_eq!(
            ex.size(&t, t.root(), Metric::Logical),
            t.size(t.root(), Metric::Logical) - t.size(big, Metric::Logical),
            "both metrics shrink"
        );
        let names = |p| ex.children(&t, p, Metric::Allocated).map(|c| t.name(c).into_owned()).collect::<Vec<_>>();
        assert_eq!(names(a), ["mid"], "an excluded item is no longer listed");
        assert_eq!(names(t.root()), ["b", "a"], "a shrunken folder moves to its new rank among its siblings");
        assert!(
            ex.children_with_sizes(&t, t.root(), Metric::Allocated)
                .all(|(c, size)| size == ex.size(&t, c, Metric::Allocated)),
            "listed sizes are the sizes after exclusion"
        );
    }

    #[test]
    fn without_exclusions_sizes_and_children_are_the_tree_s_own() {
        let t = tree(vec![D("a", vec![F("x", 5)]), F("z", 9)]);
        let ex = Exclusions::default();
        assert!(ex.is_empty());
        assert_eq!(ex.size(&t, t.root(), Metric::Allocated), t.size(t.root(), Metric::Allocated));
        assert!(ex.children(&t, t.root(), Metric::Allocated).eq(t.children(t.root(), Metric::Allocated)));
    }
}
