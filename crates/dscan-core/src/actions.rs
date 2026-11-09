use std::path::{Path, PathBuf};

use crate::exclusions::Exclusions;
use crate::platform::{Current, Platform};
use crate::protection::is_protected;
use crate::tree::{NodeId, Tree};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    NotActionable,
    AlreadyCoveredByCollectedFolder,
    Protected,
}

#[derive(Clone, Debug, Default)]
pub struct Collection {
    items: Vec<NodeId>,
    exclusions: Exclusions,
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

    pub fn covers(&self, tree: &Tree, id: NodeId) -> bool {
        self.items.iter().any(|&c| tree.is_ancestor_of(c, id))
    }

    pub fn exclusions(&self) -> &Exclusions {
        &self.exclusions
    }

    fn exclude_the_items(&mut self, tree: &Tree) {
        self.exclusions = Exclusions::new(tree, &self.items);
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
        self.exclude_the_items(tree);
        Ok(())
    }

    pub fn remove(&mut self, tree: &Tree, id: NodeId) {
        self.items.retain(|&c| c != id);
        self.exclude_the_items(tree);
    }

    pub fn clear(&mut self) {
        *self = Collection::default();
    }

    pub fn prune(&mut self, tree: &Tree) {
        self.items.retain(|&c| tree.is_actionable(c));
        self.exclude_the_items(tree);
    }

    pub fn size(&self, tree: &Tree) -> u64 {
        self.items.iter().map(|&c| tree.size(c)).sum()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Removal {
    MoveToTrash,
    DeletePermanently,
}

struct RemovalTarget {
    id: NodeId,
    path: PathBuf,
    size: u64,
}

pub struct RemovalPlan {
    removal: Removal,
    targets: Vec<RemovalTarget>,
    refused: Vec<(PathBuf, String)>,
}

impl RemovalPlan {
    pub fn new(tree: &Tree, items: &[NodeId], removal: Removal) -> Self {
        let mut plan = RemovalPlan { removal, targets: Vec::new(), refused: Vec::new() };
        for &id in items.iter().filter(|&&id| tree.is_actionable(id)) {
            let path = tree.path(id);
            if is_protected(&path) {
                plan.refused.push((path, "protected system location".into()));
            } else {
                plan.targets.push(RemovalTarget { id, path, size: tree.size(id) });
            }
        }
        plan
    }

    pub fn carry_out(self) -> RemovalReport {
        let RemovalPlan { removal, targets, refused } = self;
        let mut report = RemovalReport { removal, removed: Vec::new(), bytes: 0, failed: refused };
        let trash = Current::trash_context();
        for RemovalTarget { id, path, size } in targets {
            let outcome = match removal {
                Removal::MoveToTrash => trash.delete(&path).map_err(|e| e.to_string()),
                Removal::DeletePermanently => delete_permanently(&path).map_err(|e| e.to_string()),
            };
            match outcome {
                Ok(()) => {
                    report.removed.push(id);
                    report.bytes += size;
                }
                Err(e) => report.failed.push((path, e)),
            }
        }
        report
    }
}

#[derive(Debug)]
pub struct RemovalReport {
    pub removal: Removal,
    removed: Vec<NodeId>,
    pub bytes: u64,
    pub failed: Vec<(PathBuf, String)>,
}

impl RemovalReport {
    pub fn removed(&self) -> usize {
        self.removed.len()
    }

    pub fn apply_to(&self, tree: &mut Tree) {
        for &id in &self.removed {
            tree.remove(id);
        }
    }
}

fn delete_permanently(path: &Path) -> std::io::Result<()> {
    let file_type = std::fs::symlink_metadata(path)?.file_type();
    if file_type.is_symlink() {
        Current::remove_link(path, file_type)
    } else if file_type.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::test_util::{T::*, descend, scan_folder, tree};

    #[test]
    fn collection_never_holds_an_item_inside_another_collected_item() {
        let t = tree(vec![D("a", vec![F("x", 5), D("b", vec![F("y", 3)])]), F("z", 1)]);
        let a = t.children(t.root()).next().unwrap();
        let x = t.children(a).next().unwrap();
        let mut c = Collection::default();
        assert_eq!(c.add(&t, t.root()), Err(Refusal::NotActionable));
        assert_eq!(c.add(&t, x), Ok(()));
        assert_eq!(c.add(&t, a), Ok(()), "folder replaces collected child");
        assert_eq!(c.items(), &[a]);
        assert_eq!(c.add(&t, x), Err(Refusal::AlreadyCoveredByCollectedFolder));
        assert!(c.covers(&t, x) && c.covers(&t, a), "a collected folder covers itself and what is inside it");
        assert_eq!(c.size(&t), 8, "size counts allocated bytes");
    }

    #[test]
    fn what_a_collection_excludes_follows_every_change_to_its_items() {
        let mut t = tree(vec![D("a", vec![F("x", 5), F("y", 3)]), F("z", 1)]);
        let (a, x, y, z) =
            (descend(&t, &["a"]), descend(&t, &["a", "x"]), descend(&t, &["a", "y"]), descend(&t, &["z"]));
        let shown = |c: &Collection, t: &Tree| c.exclusions().size(t, t.root());
        let mut c = Collection::default();
        c.add(&t, x).unwrap();
        c.add(&t, z).unwrap();
        assert_eq!(shown(&c, &t), 3, "adding excludes the item");
        c.remove(&t, z);
        assert_eq!(shown(&c, &t), 4, "removing brings the item back");
        c.add(&t, y).unwrap();
        t.remove(x);
        c.prune(&t);
        assert_eq!(c.items(), &[y]);
        assert_eq!(c.exclusions().size(&t, a), 0, "pruning recounts against the changed tree");
        c.clear();
        assert_eq!(shown(&c, &t), t.size(t.root()), "a cleared collection excludes nothing");
    }

    #[test]
    fn delete_permanently_removes_files_and_folders_and_the_report_updates_the_tree() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("old/nested")).unwrap();
        std::fs::write(root.join("old/nested/a.bin"), vec![1u8; 50_000]).unwrap();
        std::fs::write(root.join("big.bin"), vec![1u8; 80_000]).unwrap();
        std::fs::write(root.join("keep.txt"), b"keep").unwrap();
        let mut t = scan_folder(root);
        let items = [descend(&t, &["old"]), descend(&t, &["big.bin"])];
        let total_before = t.size(t.root());

        let report = RemovalPlan::new(&t, &items, Removal::DeletePermanently).carry_out();
        assert!(items.iter().all(|&id| t.is_live(id)), "carrying out the plan leaves the tree alone");
        report.apply_to(&mut t);

        assert_eq!(report.removed(), 2, "{:?}", report.failed);
        assert!(!root.join("old").exists() && !root.join("big.bin").exists());
        assert!(root.join("keep.txt").exists());
        assert_eq!(t.size(t.root()), total_before - report.bytes);
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
        let t = scan_folder(root);
        let link = descend(&t, &["link"]);

        let report = RemovalPlan::new(&t, &[link], Removal::DeletePermanently).carry_out();

        assert_eq!(report.removed(), 1, "{:?}", report.failed);
        assert!(root.join("link").symlink_metadata().is_err(), "the symlink itself is deleted");
        assert!(root.join("target-folder/keep.txt").exists(), "a symlink's target is never followed");
    }

    #[cfg(windows)]
    #[test]
    fn delete_permanently_removes_a_junction_but_never_its_target() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir(root.join("target-folder")).unwrap();
        std::fs::write(root.join("target-folder/keep.txt"), b"keep").unwrap();
        crate::platform::windows::test_fixtures::make_junction(&root.join("junction"), &root.join("target-folder"))
            .expect("a junction needs no admin rights");
        let t = scan_folder(root);
        let junction = descend(&t, &["junction"]);
        assert!(!t.node(junction).is_dir(), "a junction is listed as a link, not as a folder");

        let report = RemovalPlan::new(&t, &[junction], Removal::DeletePermanently).carry_out();

        assert_eq!(report.removed(), 1, "{:?}", report.failed);
        assert!(root.join("junction").symlink_metadata().is_err(), "the junction itself is deleted");
        assert!(root.join("target-folder/keep.txt").exists(), "a junction's target is never followed");
    }

    #[cfg(unix)]
    #[test]
    fn refuses_protected_items() {
        let t = crate::tree::test_util::tree_at(
            "/",
            vec![D("usr", vec![D("lib", vec![F("libc", 9)])]), D("opt", vec![D("tool", vec![F("bin", 5)])])],
        );
        let mut c = Collection::default();
        assert_eq!(c.add(&t, descend(&t, &["usr"])), Err(Refusal::Protected));
        assert_eq!(c.add(&t, descend(&t, &["usr", "lib", "libc"])), Err(Refusal::Protected));
        assert_eq!(c.add(&t, descend(&t, &["opt"])), Err(Refusal::Protected));
        assert_eq!(c.add(&t, descend(&t, &["opt", "tool"])), Ok(()), "what users install under /opt is theirs");
    }

    #[test]
    fn prune_drops_removed_items() {
        let mut t = tree(vec![D("a", vec![F("x", 5)]), F("z", 1)]);
        let [a, z] = [0, 1].map(|i| t.children(t.root()).nth(i).unwrap());
        let mut c = Collection::default();
        c.add(&t, z).unwrap();
        c.add(&t, a).unwrap();
        assert_eq!(c.items(), &[z, a], "items keep the order they were added in");
        t.remove(a);
        c.prune(&t);
        assert_eq!(c.items(), &[z]);
    }
}
