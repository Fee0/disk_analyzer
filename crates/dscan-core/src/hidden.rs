use crate::kind::Kind;
use crate::platform::{Current, Platform};
use crate::tree::{NodeId, Tree};

pub const HIDDEN_SPACE_NAME: &str = "Hidden space";
pub const PURGEABLE_SPACE_NAME: &str = "Purgeable space";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Remedy {
    GrantFullDiskAccess,
}

#[derive(Clone, Copy, Debug)]
pub struct Cause {
    pub text: &'static str,
    pub remedy: Option<Remedy>,
}

#[derive(Clone, Copy, Debug)]
pub struct SpaceExplanation {
    pub summary: &'static str,
    pub causes: &'static [Cause],
}

pub fn explain(tree: &Tree, id: NodeId) -> Option<SpaceExplanation> {
    match tree.node(id).kind() {
        Kind::Hidden => Some(Current::HIDDEN_SPACE),
        Kind::Purgeable => Some(Current::PURGEABLE_SPACE),
        _ => None,
    }
}

pub fn unreadable_items_behind(tree: &Tree, id: NodeId) -> Option<u64> {
    let unreadable = tree.stats.errors;
    (tree.node(id).kind() == Kind::Hidden && unreadable > 0).then_some(unreadable)
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SpaceInfo {
    pub total: u64,
    pub used: u64,
    pub available: u64,
    pub available_important: u64,
}

impl SpaceInfo {
    pub(crate) fn without_purgeable(total: u64, free: u64, available: u64) -> Self {
        SpaceInfo { total, used: total.saturating_sub(free), available, available_important: available }
    }

    pub fn purgeable(&self) -> u64 {
        self.available_important.saturating_sub(self.available)
    }

    pub fn split_unscanned(&self, scanned: u64) -> (u64, u64) {
        let unscanned = self.used.saturating_sub(scanned);
        let purgeable_outside_scan = self.purgeable().min(unscanned);
        let hidden = unscanned - purgeable_outside_scan;
        (hidden, purgeable_outside_scan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_unscanned_clamps_purgeable_to_unscanned_because_the_scan_may_see_purgeable_files() {
        let s = SpaceInfo { total: 100, used: 80, available: 10, available_important: 15 };
        assert_eq!(s.split_unscanned(60), (15, 5), "20 unscanned: (hidden 15, purgeable 5)");
        assert_eq!(s.split_unscanned(78), (0, 2), "only 2 unscanned, so purgeable is clamped to 2");
        assert_eq!(s.split_unscanned(90), (0, 0), "scanned more than used: nothing unscanned");
    }

    #[test]
    fn only_the_synthetic_hidden_and_purgeable_entries_are_explained() {
        use crate::tree::{DirNode, ScanStats, Synthetic, build};
        let mut root = DirNode::new(b"root"[..].into());
        root.push_file(b"real.mov", Kind::Video, 0, 10);
        root.finish(None);
        let synthetic = vec![
            Synthetic { name: HIDDEN_SPACE_NAME, kind: Kind::Hidden, size: 5 },
            Synthetic { name: PURGEABLE_SPACE_NAME, kind: Kind::Purgeable, size: 3 },
        ];
        let stats = ScanStats { errors: 3, ..ScanStats::default() };
        let tree = build(root, "/r".into(), "root", true, synthetic, stats);
        let named = |name| crate::tree::test_util::descend(&tree, &[name]);
        assert_eq!(
            unreadable_items_behind(&tree, named(HIDDEN_SPACE_NAME)),
            Some(3),
            "unreadable items show up as hidden"
        );
        assert_eq!(unreadable_items_behind(&tree, named(PURGEABLE_SPACE_NAME)), None);

        let hidden = explain(&tree, named(HIDDEN_SPACE_NAME)).expect("hidden space is explained");
        assert!(!hidden.causes.is_empty(), "every OS names what hidden space can be");
        assert!(explain(&tree, named(PURGEABLE_SPACE_NAME)).is_some());
        assert!(explain(&tree, named("real.mov")).is_none(), "real files need no explanation");
        assert!(explain(&tree, tree.root()).is_none());
    }

    #[test]
    fn full_disk_access_is_offered_as_a_remedy_only_where_the_os_has_it() {
        let causes = Current::HIDDEN_SPACE.causes.iter().chain(Current::PURGEABLE_SPACE.causes);
        let offers_it = causes.into_iter().any(|c| c.remedy == Some(Remedy::GrantFullDiskAccess));
        assert_eq!(offers_it, Current::FULL_DISK_ACCESS_SETTINGS.is_some());
    }
}
