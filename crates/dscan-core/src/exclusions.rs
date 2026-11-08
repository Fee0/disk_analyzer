use std::cmp::Reverse;
use std::collections::HashMap;

use crate::tree::{NodeId, Tree};

#[derive(Clone, Debug, Default)]
pub struct Exclusions {
    excluded_bytes_below_or_at: HashMap<NodeId, u64>,
}

impl Exclusions {
    pub fn new(tree: &Tree, excluded: &[NodeId]) -> Self {
        let mut excluded_bytes_below_or_at: HashMap<NodeId, u64> = HashMap::new();
        for &item in excluded.iter().filter(|&&item| tree.is_live(item)) {
            let item_bytes = tree.size(item);
            for holder in tree.ancestors(item) {
                *excluded_bytes_below_or_at.entry(holder).or_default() += item_bytes;
            }
        }
        Exclusions { excluded_bytes_below_or_at }
    }

    pub fn is_empty(&self) -> bool {
        self.excluded_bytes_below_or_at.is_empty()
    }

    fn touches(&self, id: NodeId) -> bool {
        !self.is_empty() && self.excluded_bytes_below_or_at.contains_key(&id)
    }

    pub fn size(&self, tree: &Tree, id: NodeId) -> u64 {
        if self.is_empty() {
            return tree.size(id);
        }
        let excluded = self.excluded_bytes_below_or_at.get(&id).copied().unwrap_or(0);
        tree.size(id).saturating_sub(excluded)
    }

    pub fn children_with_sizes<'t>(&self, tree: &'t Tree, id: NodeId) -> impl Iterator<Item = (NodeId, u64)> + 't {
        if !self.touches(id) {
            return ChildOrder::AsScanned(tree.children(id).map(move |c| (c, tree.size(c))));
        }
        let emptied_by_exclusion = |c: NodeId, size: u64| size == 0 && self.touches(c);
        let mut kept: Vec<(NodeId, u64)> = tree
            .children(id)
            .map(|c| (c, self.size(tree, c)))
            .filter(|&(c, size)| !emptied_by_exclusion(c, size))
            .collect();
        kept.sort_by_key(|&(_, size)| Reverse(size));
        ChildOrder::Resorted(kept.into_iter())
    }

    pub fn children<'t>(&self, tree: &'t Tree, id: NodeId) -> impl Iterator<Item = NodeId> + 't {
        self.children_with_sizes(tree, id).map(|(child, _)| child)
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

        assert_eq!(ex.size(&t, big), 0);
        assert_eq!(ex.size(&t, a), t.size(a) - t.size(big));
        assert_eq!(ex.size(&t, t.root()), t.size(t.root()) - t.size(big), "the root shrinks too");
        let names = |p| ex.children(&t, p).map(|c| t.name(c).into_owned()).collect::<Vec<_>>();
        assert_eq!(names(a), ["mid"], "an excluded item is no longer listed");
        assert_eq!(names(t.root()), ["b", "a"], "a shrunken folder moves to its new rank among its siblings");
        assert!(
            ex.children_with_sizes(&t, t.root()).all(|(c, size)| size == ex.size(&t, c)),
            "listed sizes are the sizes after exclusion"
        );
    }

    #[test]
    fn excluding_an_item_keeps_its_siblings_that_take_no_space() {
        let t = tree(vec![D("a", vec![F("photo", 600), F("empty", 0), D("emptied", vec![F("inside", 50)])])]);
        let (a, photo, inside) =
            (descend(&t, &["a"]), descend(&t, &["a", "photo"]), descend(&t, &["a", "emptied", "inside"]));
        let ex = Exclusions::new(&t, &[photo, inside]);
        let names = ex.children(&t, a).map(|c| t.name(c).into_owned()).collect::<Vec<_>>();
        assert_eq!(names, ["empty"], "only what the exclusion emptied is hidden, not what was empty to begin with");
    }

    #[test]
    fn without_exclusions_sizes_and_children_are_the_tree_s_own() {
        let t = tree(vec![D("a", vec![F("x", 5)]), F("z", 9)]);
        let ex = Exclusions::default();
        assert!(ex.is_empty());
        assert_eq!(ex.size(&t, t.root()), t.size(t.root()));
        assert!(ex.children(&t, t.root()).eq(t.children(t.root())));
    }
}
