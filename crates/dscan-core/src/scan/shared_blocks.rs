use std::collections::BTreeMap;
use std::ops::Range;

use parking_lot::Mutex;
use rustc_hash::{FxHashMap, FxHasher};

use crate::platform::{BlockSpace, SharedBlocks};

const SHARDS: usize = 64;
const REGION_BYTES: u64 = 1 << 30;

type ClaimedRanges = BTreeMap<u64, u64>;

pub(super) struct SharedBlockLedger {
    shards: Vec<Mutex<FxHashMap<(BlockSpace, u64), ClaimedRanges>>>,
}

impl SharedBlockLedger {
    pub fn new() -> Self {
        SharedBlockLedger { shards: (0..SHARDS).map(|_| Mutex::new(FxHashMap::default())).collect() }
    }

    pub fn claim_returning_bytes_counted_before(&self, blocks: SharedBlocks<'_>) -> u64 {
        blocks
            .ranges
            .iter()
            .flat_map(|range| split_at_regions(range.clone()))
            .map(|piece| self.claim_piece(blocks.space, piece))
            .sum()
    }

    fn claim_piece(&self, space: BlockSpace, piece: Range<u64>) -> u64 {
        let region = piece.start / REGION_BYTES;
        let mut shard = self.shards[shard_of(space, region)].lock();
        claim(shard.entry((space, region)).or_default(), piece)
    }
}

fn shard_of(space: BlockSpace, region: u64) -> usize {
    use std::hash::{Hash, Hasher};
    let mut hasher = FxHasher::default();
    (space, region).hash(&mut hasher);
    hasher.finish() as usize % SHARDS
}

fn split_at_regions(range: Range<u64>) -> impl Iterator<Item = Range<u64>> {
    let mut start = range.start;
    std::iter::from_fn(move || {
        if start >= range.end {
            return None;
        }
        let region_end = (start / REGION_BYTES + 1).saturating_mul(REGION_BYTES);
        let end = region_end.min(range.end);
        let piece = start..end;
        start = end;
        Some(piece)
    })
}

fn claim(claimed: &mut ClaimedRanges, wanted: Range<u64>) -> u64 {
    if wanted.is_empty() {
        return 0;
    }
    let reaches_into_wanted = claimed.range(..=wanted.start).next_back().filter(|&(_, &end)| end >= wanted.start);
    let first_touching = reaches_into_wanted.map_or(wanted.start, |(&start, _)| start);
    let touching: Vec<(u64, u64)> = claimed.range(first_touching..=wanted.end).map(|(&s, &e)| (s, e)).collect();
    let (mut merged_start, mut merged_end, mut counted_before) = (wanted.start, wanted.end, 0);
    for (start, end) in touching {
        counted_before += end.min(wanted.end).saturating_sub(start.max(wanted.start));
        merged_start = merged_start.min(start);
        merged_end = merged_end.max(end);
        claimed.remove(&start);
    }
    claimed.insert(merged_start, merged_end);
    counted_before
}

#[cfg(test)]
mod tests {
    use super::*;

    const DISK: BlockSpace = BlockSpace(1);
    const OTHER_DISK: BlockSpace = BlockSpace(2);

    fn claim_all_on(ledger: &SharedBlockLedger, space: BlockSpace, ranges: &[Range<u64>]) -> u64 {
        ledger.claim_returning_bytes_counted_before(SharedBlocks { space, ranges })
    }

    fn claim_on(ledger: &SharedBlockLedger, space: BlockSpace, range: Range<u64>) -> u64 {
        claim_all_on(ledger, space, std::slice::from_ref(&range))
    }

    #[test]
    fn the_first_file_pays_for_shared_blocks_and_later_files_pay_only_for_blocks_nobody_counted() {
        let ledger = SharedBlockLedger::new();
        assert_eq!(claim_on(&ledger, DISK, 0..100), 0, "first sight");
        assert_eq!(claim_on(&ledger, DISK, 0..100), 100, "a full clone was counted already");
        assert_eq!(claim_on(&ledger, DISK, 50..150), 50, "only the overlap was counted already");
        assert_eq!(claim_on(&ledger, DISK, 0..200), 150);
        assert_eq!(claim_all_on(&ledger, DISK, &[300..400, 350..360]), 10, "a file can share blocks with itself");
    }

    #[test]
    fn ranges_that_touch_without_overlapping_count_nothing_twice_and_merge() {
        let mut claimed = ClaimedRanges::new();
        assert_eq!(claim(&mut claimed, 10..20), 0);
        assert_eq!(claim(&mut claimed, 30..40), 0);
        assert_eq!(claim(&mut claimed, 20..30), 0, "fills the gap exactly");
        assert_eq!(claimed.into_iter().collect::<Vec<_>>(), [(10, 40)]);
    }

    #[test]
    fn a_range_spanning_several_claimed_ranges_counts_each_overlap() {
        let mut claimed = ClaimedRanges::new();
        for r in [0..10, 20..30, 40..50] {
            claim(&mut claimed, r);
        }
        assert_eq!(claim(&mut claimed, 5..45), 5 + 10 + 5);
        assert_eq!(claim(&mut claimed, 0..50), 50);
        assert_eq!(claim(&mut claimed, 7..7), 0, "an empty range claims nothing");
    }

    #[test]
    fn block_spaces_are_separate_file_systems() {
        let ledger = SharedBlockLedger::new();
        claim_on(&ledger, DISK, 0..100);
        assert_eq!(claim_on(&ledger, OTHER_DISK, 0..100), 0);
        assert_eq!(claim_on(&ledger, BlockSpace(u128::from(u64::MAX) + 1), 0..100), 0);
    }

    #[test]
    fn ranges_crossing_region_borders_are_claimed_piece_by_piece() {
        let ledger = SharedBlockLedger::new();
        let across = REGION_BYTES - 10..2 * REGION_BYTES + 10;
        assert_eq!(claim_on(&ledger, DISK, across.clone()), 0);
        assert_eq!(claim_on(&ledger, DISK, REGION_BYTES - 5..REGION_BYTES + 5), 10);
        assert_eq!(claim_on(&ledger, DISK, across.clone()), across.end - across.start);
        assert_eq!(split_at_regions(u64::MAX - 5..u64::MAX).count(), 1, "the last region ends at the top of the space");
    }
}
