use std::collections::HashMap;
use std::fs::FileType;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::Acquire, Ordering::Relaxed, Ordering::Release};

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
    scanned_as_folder: bool,
}

impl RemovalTarget {
    fn file_type_unless_replaced_since_the_scan(&self) -> Result<FileType, String> {
        let now = std::fs::symlink_metadata(&self.path).map_err(|e| e.to_string())?;
        if now.is_dir() == self.scanned_as_folder { Ok(now.file_type()) } else { Err(CHANGED_SINCE_THE_SCAN.into()) }
    }
}

const CHANGED_SINCE_THE_SCAN: &str = "it changed since the scan; scan again before removing it";

pub struct RemovalPlan {
    targets: Vec<RemovalTarget>,
    refused: Vec<(PathBuf, String)>,
    progress: Arc<RemovalProgress>,
}

impl RemovalPlan {
    pub fn new(tree: &Tree, items: &[NodeId], removal: Removal) -> Self {
        let mut targets = Vec::new();
        let mut refused = Vec::new();
        for &id in items.iter().filter(|&&id| tree.is_actionable(id)) {
            let path = tree.path(id);
            if is_protected(&path) {
                refused.push((path, "protected system location".into()));
            } else {
                let scanned_as_folder = tree.node(id).is_dir();
                targets.push(RemovalTarget { id, path, size: tree.size(id), scanned_as_folder });
            }
        }
        let progress = Arc::new(RemovalProgress::new(removal, &targets));
        RemovalPlan { targets, refused, progress }
    }

    pub fn progress(&self) -> Arc<RemovalProgress> {
        Arc::clone(&self.progress)
    }

    pub fn carry_out(self) -> RemovalReport {
        let RemovalPlan { targets, refused, progress } = self;
        let removal = progress.removal;
        let mut report = RemovalReport { removal, removed: Vec::new(), bytes: 0, failed: refused };
        let trash = Current::trash_context();
        for target in targets {
            let outcome = target.file_type_unless_replaced_since_the_scan().and_then(|file_type| match removal {
                Removal::MoveToTrash => trash.delete(&target.path).map_err(|e| e.to_string()),
                Removal::DeletePermanently => delete_permanently(&target.path, file_type).map_err(|e| e.to_string()),
            });
            let RemovalTarget { id, path, size, .. } = target;
            let position = progress.items_finished.load(Relaxed);
            match outcome {
                Ok(()) => {
                    report.removed.push(id);
                    report.bytes += size;
                }
                Err(e) => {
                    progress.failed[position].store(true, Relaxed);
                    report.failed.push((path, e));
                }
            }
            progress.items_finished.fetch_add(1, Release);
        }
        report
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemRemoval {
    Removed,
    Failed,
    Underway,
    Waiting,
}

#[derive(Debug)]
pub struct RemovalProgress {
    removal: Removal,
    order: Vec<NodeId>,
    sizes: Vec<u64>,
    position: HashMap<NodeId, usize>,
    failed: Vec<AtomicBool>,
    items_finished: AtomicUsize,
}

impl RemovalProgress {
    fn new(removal: Removal, targets: &[RemovalTarget]) -> Self {
        let order: Vec<NodeId> = targets.iter().map(|target| target.id).collect();
        RemovalProgress {
            removal,
            sizes: targets.iter().map(|target| target.size).collect(),
            position: order.iter().enumerate().map(|(at, &id)| (id, at)).collect(),
            failed: targets.iter().map(|_| AtomicBool::new(false)).collect(),
            order,
            items_finished: AtomicUsize::new(0),
        }
    }

    pub fn items_finished(&self) -> usize {
        self.items_finished.load(Acquire)
    }

    pub fn at(&self, items_finished: usize) -> RemovalStatus<'_> {
        RemovalStatus { progress: self, items_finished: items_finished.min(self.order.len()) }
    }

    pub fn holds_anything_inside(&self, tree: &Tree, folder: NodeId) -> bool {
        self.order.iter().any(|&item| tree.is_ancestor_of(folder, item))
    }
}

pub struct RemovalStatus<'a> {
    progress: &'a RemovalProgress,
    items_finished: usize,
}

impl RemovalStatus<'_> {
    pub fn removal(&self) -> Removal {
        self.progress.removal
    }

    pub fn total_items(&self) -> usize {
        self.progress.order.len()
    }

    pub fn current(&self) -> Option<NodeId> {
        self.progress.order.get(self.items_finished).copied()
    }

    pub fn position_of_current(&self) -> usize {
        (self.items_finished + 1).min(self.total_items())
    }

    fn finished(&self) -> impl Iterator<Item = (u64, bool)> + '_ {
        let progress = self.progress;
        (0..self.items_finished).map(|at| (progress.sizes[at], progress.failed[at].load(Relaxed)))
    }

    pub fn bytes_removed(&self) -> u64 {
        self.finished().filter(|&(_, failed)| !failed).map(|(size, _)| size).sum()
    }

    pub fn items_failed(&self) -> usize {
        self.finished().filter(|&(_, failed)| failed).count()
    }

    pub fn total_bytes(&self) -> u64 {
        self.progress.sizes.iter().sum()
    }

    pub fn fraction(&self) -> f32 {
        let attempted: u64 = self.finished().map(|(size, _)| size).sum();
        match self.total_bytes() {
            0 => self.items_finished as f32 / self.total_items().max(1) as f32,
            total => attempted as f32 / total as f32,
        }
    }

    pub fn of_item(&self, id: NodeId) -> Option<ItemRemoval> {
        let &position = self.progress.position.get(&id)?;
        Some(match position.cmp(&self.items_finished) {
            std::cmp::Ordering::Less if self.progress.failed[position].load(Relaxed) => ItemRemoval::Failed,
            std::cmp::Ordering::Less => ItemRemoval::Removed,
            std::cmp::Ordering::Equal => ItemRemoval::Underway,
            std::cmp::Ordering::Greater => ItemRemoval::Waiting,
        })
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

fn delete_permanently(path: &Path, file_type: FileType) -> std::io::Result<()> {
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

    #[test]
    fn progress_follows_plan_order_and_never_counts_a_failed_item_as_removed() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("gone.bin"), vec![1u8; 40_000]).unwrap();
        std::fs::write(root.join("big.bin"), vec![1u8; 80_000]).unwrap();
        let t = scan_folder(root);
        let (gone, big) = (descend(&t, &["gone.bin"]), descend(&t, &["big.bin"]));
        let plan = RemovalPlan::new(&t, &[gone, big], Removal::DeletePermanently);
        let progress = plan.progress();
        assert!(progress.holds_anything_inside(&t, t.root()) && progress.holds_anything_inside(&t, big));

        let start = progress.at(progress.items_finished());
        assert_eq!(start.removal(), Removal::DeletePermanently);
        assert_eq!((start.current(), start.position_of_current(), start.total_items()), (Some(gone), 1, 2));
        assert_eq!(start.of_item(gone), Some(ItemRemoval::Underway), "the first collected item goes first");
        assert_eq!(start.of_item(big), Some(ItemRemoval::Waiting));
        assert_eq!(start.of_item(t.root()), None, "only planned items have a stage");
        assert_eq!(
            (start.bytes_removed(), start.total_bytes(), start.fraction()),
            (0, t.size(gone) + t.size(big), 0.0)
        );

        std::fs::remove_file(root.join("gone.bin")).unwrap();
        let report = plan.carry_out();
        assert_eq!((report.removed(), report.failed.len()), (1, 1), "an item already gone fails");

        let halfway = progress.at(1);
        assert_eq!(halfway.of_item(gone), Some(ItemRemoval::Failed));
        assert_eq!((halfway.current(), halfway.position_of_current()), (Some(big), 2));
        assert_eq!((halfway.bytes_removed(), halfway.items_failed()), (0, 1), "a failed item frees nothing");
        assert!(halfway.fraction() > 0.0, "a failed item is still work done");

        let end = progress.at(progress.items_finished());
        assert_eq!(progress.items_finished(), 2, "every item is attempted, so progress reaches the end");
        assert_eq!(end.of_item(big), Some(ItemRemoval::Removed));
        assert_eq!((end.current(), end.position_of_current(), end.fraction()), (None, 2, 1.0));
        assert_eq!(end.bytes_removed(), report.bytes, "progress and the report agree on what was freed");
    }

    #[test]
    fn a_file_replaced_by_a_folder_since_the_scan_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("report"), vec![1u8; 10_000]).unwrap();
        let t = scan_folder(root);
        let report_file = descend(&t, &["report"]);
        std::fs::remove_file(root.join("report")).unwrap();
        std::fs::create_dir(root.join("report")).unwrap();
        std::fs::write(root.join("report/new-work.txt"), b"written after the scan").unwrap();

        let report = RemovalPlan::new(&t, &[report_file], Removal::DeletePermanently).carry_out();

        assert_eq!((report.removed(), report.failed.len()), (0, 1));
        assert_eq!(report.failed[0].1, CHANGED_SINCE_THE_SCAN);
        assert!(root.join("report/new-work.txt").exists(), "what replaced the scanned file is not deleted");
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
