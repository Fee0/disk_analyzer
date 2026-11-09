use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};

use crate::tree::{NodeId, Tree};

#[derive(Clone, Debug, Default)]
pub struct Exclusions {
    excluded_items: HashSet<NodeId>,
    excluded_bytes_below_or_at: HashMap<NodeId, u64>,
}

impl Exclusions {
    pub fn new(tree: &Tree, excluded: &[NodeId]) -> Self {
        let excluded_items: HashSet<NodeId> = excluded.iter().copied().filter(|&item| tree.is_live(item)).collect();
        let mut excluded_bytes_below_or_at: HashMap<NodeId, u64> = HashMap::new();
        for &item in &excluded_items {
            let item_bytes = tree.size(item);
            for holder in tree.ancestors(item) {
                *excluded_bytes_below_or_at.entry(holder).or_default() += item_bytes;
            }
        }
        Exclusions { excluded_items, excluded_bytes_below_or_at }
    }

    fn touches(&self, id: NodeId) -> bool {
        self.excluded_bytes_below_or_at.contains_key(&id)
    }

    pub fn size(&self, tree: &Tree, id: NodeId) -> u64 {
        let excluded = self.excluded_bytes_below_or_at.get(&id).copied().unwrap_or(0);
        tree.size(id).saturating_sub(excluded)
    }

    pub fn children_with_sizes<'t>(&self, tree: &'t Tree, id: NodeId) -> impl Iterator<Item = (NodeId, u64)> + 't {
        if !self.touches(id) {
            return ChildOrder::AsScanned(tree.children(id).map(move |c| (c, tree.size(c))));
        }
        let hidden_by_exclusion = |c: NodeId, size: u64| {
            let emptied_by_exclusion = size == 0 && tree.size(c) > 0;
            self.excluded_items.contains(&c) || emptied_by_exclusion
        };
        let mut kept: Vec<(NodeId, u64)> = tree
            .children(id)
            .map(|c| (c, self.size(tree, c)))
            .filter(|&(c, size)| !hidden_by_exclusion(c, size))
            .collect();
        kept.sort_by_key(|&(_, size)| Reverse(size));
        ChildOrder::Resorted(kept.into_iter())
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

    fn listed_names(ex: &Exclusions, t: &Tree, folder: NodeId) -> Vec<String> {
        ex.children_with_sizes(t, folder).map(|(c, _)| t.name(c).into_owned()).collect()
    }

    #[test]
    fn excluding_an_item_shrinks_every_ancestor_and_hides_the_item() {
        let t = tree(vec![D("a", vec![F("big", 600), F("mid", 300)]), D("b", vec![F("w", 500)])]);
        let (a, big) = (descend(&t, &["a"]), descend(&t, &["a", "big"]));
        let ex = Exclusions::new(&t, &[big]);

        assert_eq!(ex.size(&t, big), 0);
        assert_eq!(ex.size(&t, a), t.size(a) - t.size(big));
        assert_eq!(ex.size(&t, t.root()), t.size(t.root()) - t.size(big), "the root shrinks too");
        let names = |p| listed_names(&ex, &t, p);
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
        assert_eq!(
            listed_names(&ex, &t, a),
            ["empty"],
            "only what the exclusion emptied is hidden, not what was empty to begin with"
        );
    }

    #[test]
    fn excluding_an_empty_item_hides_it_but_keeps_the_empty_folder_holding_it() {
        let t = tree(vec![D("a", vec![F("photo", 600), D("cache", vec![F("lock", 0), F("pid", 0)])])]);
        let (a, cache, lock) =
            (descend(&t, &["a"]), descend(&t, &["a", "cache"]), descend(&t, &["a", "cache", "lock"]));
        let ex = Exclusions::new(&t, &[lock]);
        let names = |p| listed_names(&ex, &t, p);
        assert_eq!(names(cache), ["pid"], "the excluded empty file is hidden");
        assert_eq!(names(a), ["photo", "cache"], "its folder was empty before and stays listed");
    }

    #[test]
    fn without_exclusions_sizes_and_children_are_the_tree_s_own() {
        let t = tree(vec![D("a", vec![F("x", 5)]), F("z", 9)]);
        let ex = Exclusions::default();
        assert_eq!(ex.size(&t, t.root()), t.size(t.root()));
        assert!(ex.children_with_sizes(&t, t.root()).map(|(c, _)| c).eq(t.children(t.root())));
    }
}
