pub mod actions;
pub mod hidden;
pub mod kind;
pub mod layout;
mod platform;
mod protection;
pub mod scan;
pub mod search;
pub mod tree;
pub mod volumes;

pub use actions::{Collection, Refusal, Removal};
pub use kind::Kind;
pub use layout::{Arc, Layout, LayoutParams, MAX_DEPTH, Segment, sunburst};
pub use protection::is_protected;
pub use scan::{CancelToken, Progress, ProgressSnapshot, ScanError, ScanOptions, scan};
pub use search::{SearchResult, search};
pub use tree::{Metric, NodeId, ScanStats, Tree};
pub use volumes::{TRASH_NAME, Volume, has_full_disk_access, list_volumes};

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
