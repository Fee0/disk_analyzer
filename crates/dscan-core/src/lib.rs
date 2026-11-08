pub mod actions;
pub mod desktop;
pub mod exclusions;
pub mod hidden;
pub mod kind;
pub mod layout;
mod platform;
mod protection;
pub mod scan;
pub mod search;
pub mod transition;
pub mod tree;
pub mod volumes;

pub use actions::{Collection, Refusal, Removal, RemovalPlan, RemovalReport};
pub use desktop::{COMMAND_KEY, DesktopError, PREVIEW_ACTION, RESCAN_SHORTCUT, REVEAL_ACTION};
pub use exclusions::Exclusions;
pub use hidden::{Cause, Remedy, SpaceExplanation};
pub use kind::Kind;
pub use layout::{Arc, Layout, LayoutParams, MAX_DEPTH, Segment, sunburst};
pub use scan::{CancelToken, Progress, ProgressSnapshot, ScanError, ScanOptions, scan};
pub use search::{SearchResult, search};
pub use transition::{ArcStyle, FrameArc, Transition, TransitionKind};
pub use tree::{Accounting, NodeId, ScanStats, Tree};
pub use volumes::{FULL_DISK_ACCESS_SETTINGS, TRASH_NAME, Volume, has_full_disk_access, list_volumes};

pub fn canonical_path(path: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
    use platform::{Current, Platform};
    Current::canonical_root(path)
}

pub fn existing_folder(path: std::path::PathBuf) -> Option<std::path::PathBuf> {
    path.is_dir().then(|| canonical_path(&path).unwrap_or(path))
}

pub fn format_size(bytes: u64) -> String {
    use humansize::{DECIMAL, FormatSizeOptions, format_size as fs};
    fs(bytes, FormatSizeOptions::from(DECIMAL).decimal_places(1).space_after_value(true))
}

#[cfg(test)]
mod tests {
    #[test]
    fn format_size_uses_decimal_units_with_one_decimal_like_finder() {
        assert_eq!(super::format_size(238_400_000), "238.4 MB", "1 MB is 10^6 bytes, as in Finder");
        assert_eq!(super::format_size(0), "0 B");
    }
}
