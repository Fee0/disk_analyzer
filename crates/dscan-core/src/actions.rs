//! The collection of items marked for removal, and moving them to the Trash.

use std::path::PathBuf;

use crate::tree::{Metric, NodeId, Tree};

/// A set of collected, non-overlapping tree nodes (no item is inside
/// another), in the order they were added.
#[derive(Clone, Debug, Default)]
pub struct Collection {
    items: Vec<NodeId>,
}

impl Collection {
    pub fn items(&self) -> &[NodeId] {
        &self.items
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    pub fn contains(&self, id: NodeId) -> bool {
        self.items.contains(&id)
    }

    /// Add `id` unless it can't be acted on or is already covered by a
    /// collected folder; adding a folder replaces items inside it.
    pub fn add(&mut self, tree: &Tree, id: NodeId) -> bool {
        if !tree.is_actionable(id) || self.items.iter().any(|&c| tree.is_ancestor_of(c, id)) {
            return false;
        }
        self.items.retain(|&c| !tree.is_ancestor_of(id, c));
        self.items.push(id);
        true
    }

    pub fn remove(&mut self, id: NodeId) {
        self.items.retain(|&c| c != id);
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Drop items that were removed from the tree (e.g. trashed).
    pub fn prune(&mut self, tree: &Tree) {
        self.items.retain(|&c| tree.is_actionable(c));
    }

    /// Bytes on disk the collected items occupy.
    pub fn size(&self, tree: &Tree) -> u64 {
        self.items.iter().map(|&c| tree.size(c, Metric::Allocated)).sum()
    }
}

/// Outcome of moving items to the Trash.
#[derive(Debug, Default)]
pub struct TrashReport {
    pub trashed: usize,
    /// Allocated bytes of the trashed items. They stay on disk until the
    /// Trash is emptied.
    pub bytes: u64,
    pub failed: Vec<(PathBuf, String)>,
}

/// Move `items` to the Trash and remove them from the tree. Items that
/// are no longer actionable are skipped; failures are reported and stay
/// in the tree.
pub fn move_to_trash(tree: &mut Tree, items: &[NodeId]) -> TrashReport {
    let mut report = TrashReport::default();
    let ctx = trash_ctx();
    for &id in items {
        if !tree.is_actionable(id) {
            continue;
        }
        let path = tree.path(id);
        let size = tree.size(id, Metric::Allocated);
        match ctx.delete(&path) {
            Ok(()) => {
                tree.remove(id);
                report.bytes += size;
                report.trashed += 1;
            }
            Err(e) => report.failed.push((path, e.to_string())),
        }
    }
    report
}

fn trash_ctx() -> trash::TrashContext {
    use trash::macos::{DeleteMethod, TrashContextExtMacos};
    let mut ctx = trash::TrashContext::default();
    // NSFileManager: fast and needs no Finder automation permission.
    ctx.set_delete_method(DeleteMethod::NsFileManager);
    ctx
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::test_util::{T::*, tree};

    #[test]
    fn collection_subsumes() {
        let t = tree(vec![D("a", vec![F("x", 5), D("b", vec![F("y", 3)])]), F("z", 1)]);
        let a = t.children(t.root(), Metric::Allocated).next().unwrap();
        let x = t.children(a, Metric::Allocated).next().unwrap();
        let mut c = Collection::default();
        assert!(!c.add(&t, t.root()));
        assert!(c.add(&t, x));
        assert!(c.add(&t, a), "folder replaces collected child");
        assert_eq!(c.items(), &[a]);
        assert!(!c.add(&t, x), "child of collected folder is covered");
        assert_eq!(c.size(&t), 8);
    }

    #[test]
    fn prune_drops_removed_items() {
        let mut t = tree(vec![D("a", vec![F("x", 5)]), F("z", 1)]);
        let [a, z] = [0, 1].map(|i| t.children(t.root(), Metric::Allocated).nth(i).unwrap());
        let mut c = Collection::default();
        c.add(&t, a);
        c.add(&t, z);
        t.remove(a);
        c.prune(&t);
        assert_eq!(c.items(), &[z]);
    }
}
