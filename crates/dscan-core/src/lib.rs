//! dscan-core: fast APFS-aware disk usage scanning and analysis.
//!
//! This crate contains everything that is not presentation: scanning,
//! the result tree, sunburst geometry, search and file actions. It has no
//! GUI dependencies.

pub mod actions;
pub mod hidden;
pub mod kind;
pub mod layout;
pub mod scan;
pub mod search;
pub mod tree;

pub use actions::Collection;
pub use kind::Kind;
pub use layout::{Arc, Layout, LayoutParams, MAX_DEPTH, Segment, sunburst};
pub use scan::volumes::{Volume, has_full_disk_access, list_volumes};
pub use scan::{CancelToken, Progress, ProgressSnapshot, ScanError, ScanOptions, scan};
pub use search::{SearchResult, search};
pub use tree::{Metric, NodeId, ScanStats, Tree};

/// Human readable size using decimal units like Finder ("238.4 MB").
pub fn format_size(bytes: u64) -> String {
    use humansize::{DECIMAL, FormatSizeOptions, format_size as fs};
    fs(bytes, FormatSizeOptions::from(DECIMAL).decimal_places(1).space_after_value(true))
}

#[cfg(test)]
mod tests {
    #[test]
    fn formats() {
        assert_eq!(super::format_size(238_400_000), "238.4 MB");
        assert_eq!(super::format_size(0), "0 B");
    }
}
