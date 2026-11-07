use std::path::{Path, PathBuf};

use crate::exclusions::Exclusions;
use crate::platform::{Current, Platform};
use crate::protection::is_protected;
use crate::tree::{Metric, NodeId, Tree};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    NotActionable,
    AlreadyCoveredByCollectedFolder,
    Protected,
}

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

    pub fn covers(&self, tree: &Tree, id: NodeId) -> bool {
        self.items.iter().any(|&c| tree.is_ancestor_of(c, id))
    }

    pub fn exclusions(&self, tree: &Tree) -> Exclusions {
        Exclusions::new(tree, &self.items)
    }

    pub fn add(&mut self, tree: &Tree, id: NodeId) -> Result<(), Refusal> {
        if !tree.is_actionable(id) {
            return Err(Refusal::NotActionable);
        }
        if self.covers(tree, id) {
            return Err(Refusal::AlreadyCoveredByCollectedFolder);
        }
        if is_protected(&tree.path(id)) {
            return Err(Refusal::Protected);
        }
        self.items.retain(|&c| !tree.is_ancestor_of(id, c));
        self.items.push(id);
        Ok(())
    }

    pub fn remove(&mut self, id: NodeId) {
        self.items.retain(|&c| c != id);
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn prune(&mut self, tree: &Tree) {
        self.items.retain(|&c| tree.is_actionable(c));
    }

    pub fn size(&self, tree: &Tree) -> u64 {
        self.items.iter().map(|&c| tree.size(c, Metric::Allocated)).sum()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Removal {
    MoveToTrash,
    DeletePermanently,
}

#[derive(Debug, Default)]
pub struct RemovalReport {
    pub removed: usize,
    pub bytes: u64,
    pub failed: Vec<(PathBuf, String)>,
}

pub fn remove_from_disk(tree: &mut Tree, items: &[NodeId], removal: Removal) -> RemovalReport {
    let mut report = RemovalReport::default();
    let trash = Current::trash_context();
    for &id in items {
        if !tree.is_actionable(id) {
            continue;
        }
        let path = tree.path(id);
        if is_protected(&path) {
            report.failed.push((path, "protected system location".into()));
            continue;
        }
        let size = tree.size(id, Metric::Allocated);
        let outcome = match removal {
            Removal::MoveToTrash => trash.delete(&path).map_err(|e| e.to_string()),
            Removal::DeletePermanently => delete_permanently(&path).map_err(|e| e.to_string()),
        };
        match outcome {
            Ok(()) => {
                tree.remove(id);
                report.bytes += size;
                report.removed += 1;
            }
            Err(e) => report.failed.push((path, e)),
        }
    }
    report
}

fn delete_permanently(path: &Path) -> std::io::Result<()> {
    let is_real_directory = std::fs::symlink_metadata(path)?.is_dir();
    if is_real_directory { std::fs::remove_dir_all(path) } else { std::fs::remove_file(path) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::test_util::{T::*, tree};

    #[test]
    fn collection_never_holds_an_item_inside_another_collected_item() {
        let t = tree(vec![D("a", vec![F("x", 5), D("b", vec![F("y", 3)])]), F("z", 1)]);
        let a = t.children(t.root(), Metric::Allocated).next().unwrap();
        let x = t.children(a, Metric::Allocated).next().unwrap();
        let mut c = Collection::default();
        assert_eq!(c.add(&t, t.root()), Err(Refusal::NotActionable));
        assert_eq!(c.add(&t, x), Ok(()));
        assert_eq!(c.add(&t, a), Ok(()), "folder replaces collected child");
        assert_eq!(c.items(), &[a]);
        assert_eq!(c.add(&t, x), Err(Refusal::AlreadyCoveredByCollectedFolder));
        assert!(c.covers(&t, x) && c.covers(&t, a), "a collected folder covers itself and what is inside it");
        assert_eq!(c.size(&t), 8, "size counts allocated bytes");
    }

    fn child(t: &Tree, name: &str) -> NodeId {
        t.children(t.root(), Metric::Allocated).find(|&c| t.name(c) == name).unwrap()
    }

    fn scan_folder(root: &Path) -> Tree {
        crate::scan(&crate::ScanOptions::folder(root), &Default::default(), &Default::default()).unwrap()
    }

    #[test]
    fn delete_permanently_removes_files_and_folders_and_updates_the_tree() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("old/nested")).unwrap();
        std::fs::write(root.join("old/nested/a.bin"), vec![1u8; 50_000]).unwrap();
        std::fs::write(root.join("big.bin"), vec![1u8; 80_000]).unwrap();
        std::fs::write(root.join("keep.txt"), b"keep").unwrap();
        let mut t = scan_folder(root);
        let items = [child(&t, "old"), child(&t, "big.bin")];
        let total_before = t.size(t.root(), Metric::Allocated);

        let report = remove_from_disk(&mut t, &items, Removal::DeletePermanently);

        assert_eq!(report.removed, 2, "{:?}", report.failed);
        assert!(!root.join("old").exists() && !root.join("big.bin").exists());
        assert!(root.join("keep.txt").exists());
        assert_eq!(t.size(t.root(), Metric::Allocated), total_before - report.bytes);
        assert!(items.iter().all(|&id| !t.is_live(id)));
    }

    #[cfg(unix)]
    #[test]
    fn delete_permanently_removes_a_symlink_but_never_its_target() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir(root.join("target-folder")).unwrap();
        std::fs::write(root.join("target-folder/keep.txt"), b"keep").unwrap();
        std::os::unix::fs::symlink(root.join("target-folder"), root.join("link")).unwrap();
        let mut t = scan_folder(root);
        let link = child(&t, "link");

        let report = remove_from_disk(&mut t, &[link], Removal::DeletePermanently);

        assert_eq!(report.removed, 1, "{:?}", report.failed);
        assert!(root.join("link").symlink_metadata().is_err(), "the symlink itself is deleted");
        assert!(root.join("target-folder/keep.txt").exists(), "a symlink's target is never followed");
    }

    #[cfg(unix)]
    #[test]
    fn refuses_protected_items() {
        let t = crate::tree::test_util::tree_at(
            "/",
            vec![D("usr", vec![D("lib", vec![F("libc", 9)])]), D("opt", vec![D("tool", vec![F("bin", 5)])])],
        );
        let child = |p: NodeId, name: &str| t.children(p, Metric::Allocated).find(|&c| t.name(c) == name).unwrap();
        let usr = child(t.root(), "usr");
        let opt = child(t.root(), "opt");
        let mut c = Collection::default();
        assert_eq!(c.add(&t, usr), Err(Refusal::Protected));
        assert_eq!(c.add(&t, child(child(usr, "lib"), "libc")), Err(Refusal::Protected));
        assert_eq!(c.add(&t, opt), Err(Refusal::Protected));
        assert_eq!(c.add(&t, child(opt, "tool")), Ok(()), "what users install under /opt is theirs");
    }

    #[test]
    fn prune_drops_removed_items() {
        let mut t = tree(vec![D("a", vec![F("x", 5)]), F("z", 1)]);
        let [a, z] = [0, 1].map(|i| t.children(t.root(), Metric::Allocated).nth(i).unwrap());
        let mut c = Collection::default();
        c.add(&t, z).unwrap();
        c.add(&t, a).unwrap();
        assert_eq!(c.items(), &[z, a], "items keep the order they were added in");
        t.remove(a);
        c.prune(&t);
        assert_eq!(c.items(), &[z]);
    }
}
