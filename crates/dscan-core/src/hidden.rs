use std::io;
use std::path::Path;

use crate::platform::{Current, Platform};

#[derive(Clone, Copy, Debug, Default)]
pub struct SpaceInfo {
    pub total: u64,
    pub used: u64,
    pub available: u64,
    pub available_important: u64,
}

impl SpaceInfo {
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

pub(crate) fn space_info(path: &Path) -> io::Result<SpaceInfo> {
    Current::space_info(path)
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
}
