use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

use crate::tree::{Metric, NodeId, Tree};

const PROTECTED_FOLDERS: &[&str] = &[
    "/Applications",
    "/Library",
    "/System",
    "/Users",
    "/Users/Shared",
    "/Volumes",
    "/bin",
    "/cores",
    "/dev",
    "/etc",
    "/opt",
    "/private",
    "/private/etc",
    "/private/tmp",
    "/private/var",
    "/sbin",
    "/tmp",
    "/usr",
    "/usr/local",
    "/var",
];

const PROTECTED_SYSTEM_TREES: &[&str] = &["/System", "/bin", "/sbin", "/usr", "/private/var/db"];
const USER_TREES_INSIDE_SYSTEM_TREES: &[&str] = &["/usr/local"];

const STANDARD_HOME_FOLDERS: &[&str] = &[
    "Applications",
    "Desktop",
    "Documents",
    "Downloads",
    "Library",
    "Movies",
    "Music",
    "Pictures",
    "Public",
    ".Trash",
];

pub fn is_protected(path: &Path) -> bool {
    is_protected_folder(path)
        || is_inside_protected_system_tree(path)
        || is_home_folder_or_standard_home_folder(path)
        || is_volume_root(path)
}

fn is_protected_folder(path: &Path) -> bool {
    PROTECTED_FOLDERS.iter().any(|folder| path == Path::new(folder))
}

fn is_inside_protected_system_tree(path: &Path) -> bool {
    let is_inside_any = |trees: &[&str]| trees.iter().any(|tree| path.starts_with(tree));
    is_inside_any(PROTECTED_SYSTEM_TREES) && !is_inside_any(USER_TREES_INSIDE_SYSTEM_TREES)
}

fn is_home_folder_or_standard_home_folder(path: &Path) -> bool {
    match named_components(path)[..] {
        [users, _home] if users == "Users" => true,
        [users, _home, folder] if users == "Users" => STANDARD_HOME_FOLDERS.iter().any(|standard| folder == *standard),
        _ => false,
    }
}

fn is_volume_root(path: &Path) -> bool {
    matches!(named_components(path)[..], [volumes, _volume] if volumes == "Volumes")
}

fn named_components(path: &Path) -> Vec<&OsStr> {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(name) => Some(name),
            _ => None,
        })
        .collect()
}

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

    pub fn add(&mut self, tree: &Tree, id: NodeId) -> Result<(), Refusal> {
        if !tree.is_actionable(id) {
            return Err(Refusal::NotActionable);
        }
        if self.items.iter().any(|&c| tree.is_ancestor_of(c, id)) {
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
    let trash = trash_context_needing_no_finder_automation_permission();
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

fn trash_context_needing_no_finder_automation_permission() -> trash::TrashContext {
    use trash::macos::{DeleteMethod, TrashContextExtMacos};
    let mut ctx = trash::TrashContext::default();
    ctx.set_delete_method(DeleteMethod::NsFileManager);
    ctx
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::test_util::{T::*, tree, tree_at};

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
        assert_eq!(c.size(&t), 8, "size counts allocated bytes");
    }

    #[test]
    fn delete_permanently_removes_files_and_folders_and_updates_the_tree() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("old/nested")).unwrap();
        std::fs::write(root.join("old/nested/a.bin"), vec![1u8; 50_000]).unwrap();
        std::fs::write(root.join("big.bin"), vec![1u8; 80_000]).unwrap();
        std::fs::write(root.join("keep.txt"), b"keep").unwrap();
        std::os::unix::fs::symlink(root.join("keep.txt"), root.join("link-to-keep")).unwrap();
        let mut t = crate::scan(&crate::ScanOptions::folder(root), &Default::default(), &Default::default()).unwrap();
        let child =
            |t: &Tree, name: &str| t.children(t.root(), Metric::Allocated).find(|&c| t.name(c) == name).unwrap();
        let items = [child(&t, "old"), child(&t, "big.bin"), child(&t, "link-to-keep")];
        let total_before = t.size(t.root(), Metric::Allocated);

        let report = remove_from_disk(&mut t, &items, Removal::DeletePermanently);

        assert_eq!(report.removed, 3, "{:?}", report.failed);
        assert!(!root.join("old").exists() && !root.join("big.bin").exists());
        assert!(root.join("link-to-keep").symlink_metadata().is_err(), "the symlink itself is deleted");
        assert!(root.join("keep.txt").exists(), "a symlink's target is never followed");
        assert_eq!(t.size(t.root(), Metric::Allocated), total_before - report.bytes);
        assert!(items.iter().all(|&id| !t.is_live(id)));
    }

    #[test]
    fn protected_locations() {
        for p in [
            "/System",
            "/System/Library/CoreServices/Finder.app",
            "/System/Volumes/Data/.Spotlight-V100",
            "/Library",
            "/Applications",
            "/usr",
            "/usr/lib/libSystem.B.dylib",
            "/usr/local",
            "/private/var",
            "/private/var/db/receipts",
            "/Users/alex",
            "/Users/alex/Library",
            "/Users/alex/Downloads",
            "/Users/alex/.Trash",
            "/Volumes/Backup",
        ] {
            assert!(is_protected(Path::new(p)), "{p} should be protected");
        }
        for p in [
            "/Applications/Some.app",
            "/Library/Caches/com.example",
            "/usr/local/Cellar",
            "/private/var/folders/xy",
            "/Users/alex/Downloads/big.dmg",
            "/Users/alex/Library/Caches",
            "/Users/alex/code",
            "/Volumes/Backup/old",
            "/opt/homebrew",
        ] {
            assert!(!is_protected(Path::new(p)), "{p} should be collectable");
        }
    }

    #[test]
    fn refuses_protected_items() {
        let t = tree_at(
            "/",
            vec![
                D("System", vec![F("kernel", 9)]),
                D("Users", vec![D("alex", vec![D("Movies", vec![F("a.mov", 5)])])]),
            ],
        );
        let child = |p: NodeId, name: &str| t.children(p, Metric::Allocated).find(|&c| t.name(c) == name).unwrap();
        let system = child(t.root(), "System");
        let movies = child(child(child(t.root(), "Users"), "alex"), "Movies");
        let mut c = Collection::default();
        assert_eq!(c.add(&t, system), Err(Refusal::Protected));
        assert_eq!(c.add(&t, child(system, "kernel")), Err(Refusal::Protected));
        assert_eq!(c.add(&t, movies), Err(Refusal::Protected));
        assert_eq!(c.add(&t, child(movies, "a.mov")), Ok(()), "contents of a home folder are fine");
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
