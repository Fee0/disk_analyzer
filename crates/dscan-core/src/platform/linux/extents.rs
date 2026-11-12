use std::ops::Range;

use crate::platform::BlockSpace;

pub(super) const BTRFS_SUPER_MAGIC: i64 = 0x9123_683E;
pub(super) const XFS_SUPER_MAGIC: i64 = 0x5846_5342;

pub(super) const BTRFS_FS_INFO_BYTES: usize = 1024;
const BTRFS_FS_INFO_FSID: Range<usize> = 16..32;

const FIEMAP_HEADER_BYTES: usize = 32;
const FIEMAP_EXTENT_BYTES: usize = 56;
const FM_START: usize = 0;
const FM_LENGTH: usize = 8;
const FM_MAPPED_EXTENTS: usize = 20;
const FM_EXTENT_COUNT: usize = 24;
const FE_LOGICAL: usize = 0;
const FE_PHYSICAL: usize = 8;
const FE_LENGTH: usize = 16;
const FE_FLAGS: usize = 40;

const FIEMAP_EXTENT_LAST: u32 = 0x1;
const FIEMAP_EXTENT_UNKNOWN: u32 = 0x2;
const FIEMAP_EXTENT_DELALLOC: u32 = 0x4;
const FIEMAP_EXTENT_ENCODED: u32 = 0x8;
const FIEMAP_EXTENT_DATA_INLINE: u32 = 0x200;
const FIEMAP_EXTENT_SHARED: u32 = 0x2000;
const WITHOUT_A_DISK_ADDRESS: u32 = FIEMAP_EXTENT_UNKNOWN | FIEMAP_EXTENT_DELALLOC | FIEMAP_EXTENT_DATA_INLINE;

const SMALLEST_EXTENT_ON_DISK: u64 = 4096;
const LONGEST_COMPRESSED_EXTENT: u64 = 128 << 10;
const COMPRESSED_SLOT_PER_DISK_BYTE: u64 = LONGEST_COMPRESSED_EXTENT / SMALLEST_EXTENT_ON_DISK;
const COMPRESSED_EXTENTS_ADDRESS_SPACE: u64 = 1 << 63;

fn claimed_range(physical: u64, length: u64, flags: u32) -> Range<u64> {
    let length_says_nothing_about_the_space_on_disk = flags & FIEMAP_EXTENT_ENCODED != 0;
    if length_says_nothing_about_the_space_on_disk {
        let own_slot = COMPRESSED_EXTENTS_ADDRESS_SPACE | physical.wrapping_mul(COMPRESSED_SLOT_PER_DISK_BYTE);
        own_slot..own_slot.saturating_add(length.min(LONGEST_COMPRESSED_EXTENT))
    } else {
        physical..physical.saturating_add(length)
    }
}

const EXTENTS_PER_CALL: usize = 128;
const BYTES_PER_WORD: usize = 8;
pub(super) const FIEMAP_BUFFER_WORDS: usize =
    (FIEMAP_HEADER_BYTES + EXTENTS_PER_CALL * FIEMAP_EXTENT_BYTES).div_ceil(BYTES_PER_WORD);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FileSystemBlocks {
    MayBeShared(BlockSpace),
    NeverShared,
}

pub(super) fn block_sharing(
    file_system_magic: i64,
    device: u64,
    btrfs_info: impl FnOnce() -> Option<[u8; BTRFS_FS_INFO_BYTES]>,
) -> FileSystemBlocks {
    match file_system_magic {
        BTRFS_SUPER_MAGIC => match btrfs_info() {
            Some(info) => FileSystemBlocks::MayBeShared(BlockSpace(btrfs_fsid(&info))),
            None => FileSystemBlocks::NeverShared,
        },
        XFS_SUPER_MAGIC => FileSystemBlocks::MayBeShared(BlockSpace(u128::from(device))),
        _ => FileSystemBlocks::NeverShared,
    }
}

fn btrfs_fsid(info: &[u8; BTRFS_FS_INFO_BYTES]) -> u128 {
    u128::from_le_bytes(info[BTRFS_FS_INFO_FSID].try_into().unwrap())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Batch {
    Complete,
    ContinueFrom(u64),
}

fn bytes_mut(buffer: &mut [u64]) -> &mut [u8] {
    unsafe { std::slice::from_raw_parts_mut(buffer.as_mut_ptr().cast(), std::mem::size_of_val(buffer)) }
}

fn bytes(buffer: &[u64]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast(), std::mem::size_of_val(buffer)) }
}

fn put_u64(bytes: &mut [u8], at: usize, value: u64) {
    bytes[at..at + 8].copy_from_slice(&value.to_ne_bytes());
}

fn put_u32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_ne_bytes());
}

fn u64_at(bytes: &[u8], at: usize) -> u64 {
    u64::from_ne_bytes(bytes[at..at + 8].try_into().unwrap())
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_ne_bytes(bytes[at..at + 4].try_into().unwrap())
}

pub(super) fn ask_for_extents_from(buffer: &mut [u64; FIEMAP_BUFFER_WORDS], file_offset: u64) {
    let bytes = bytes_mut(buffer);
    bytes[..FIEMAP_HEADER_BYTES].fill(0);
    put_u64(bytes, FM_START, file_offset);
    put_u64(bytes, FM_LENGTH, u64::MAX - file_offset);
    put_u32(bytes, FM_EXTENT_COUNT, EXTENTS_PER_CALL as u32);
}

pub(super) fn collect_shared_extents(buffer: &[u64; FIEMAP_BUFFER_WORDS], shared: &mut Vec<Range<u64>>) -> Batch {
    let bytes = bytes(buffer);
    let mapped = (u32_at(bytes, FM_MAPPED_EXTENTS) as usize).min(EXTENTS_PER_CALL);
    let mut next = Batch::Complete;
    for index in 0..mapped {
        let extent = &bytes[FIEMAP_HEADER_BYTES + index * FIEMAP_EXTENT_BYTES..][..FIEMAP_EXTENT_BYTES];
        let (physical, length, flags) =
            (u64_at(extent, FE_PHYSICAL), u64_at(extent, FE_LENGTH), u32_at(extent, FE_FLAGS));
        if flags & FIEMAP_EXTENT_SHARED != 0 && flags & WITHOUT_A_DISK_ADDRESS == 0 {
            shared.push(claimed_range(physical, length, flags));
        }
        let is_last_of_the_file = flags & FIEMAP_EXTENT_LAST != 0;
        let filled_the_buffer = index + 1 == EXTENTS_PER_CALL;
        next = match (is_last_of_the_file, filled_the_buffer) {
            (false, true) => Batch::ContinueFrom(u64_at(extent, FE_LOGICAL).saturating_add(length)),
            _ => Batch::Complete,
        };
    }
    next
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Extent {
        logical: u64,
        physical: u64,
        length: u64,
        flags: u32,
    }

    fn answer(extents: &[Extent]) -> [u64; FIEMAP_BUFFER_WORDS] {
        let mut buffer = [0u64; FIEMAP_BUFFER_WORDS];
        ask_for_extents_from(&mut buffer, 0);
        let bytes = bytes_mut(&mut buffer);
        put_u32(bytes, FM_MAPPED_EXTENTS, extents.len() as u32);
        for (index, e) in extents.iter().enumerate() {
            let at = FIEMAP_HEADER_BYTES + index * FIEMAP_EXTENT_BYTES;
            put_u64(bytes, at + FE_LOGICAL, e.logical);
            put_u64(bytes, at + FE_PHYSICAL, e.physical);
            put_u64(bytes, at + FE_LENGTH, e.length);
            put_u32(bytes, at + FE_FLAGS, e.flags);
        }
        buffer
    }

    fn extent(logical: u64, physical: u64, flags: u32) -> Extent {
        Extent { logical, physical, length: 4096, flags }
    }

    #[test]
    fn the_request_asks_for_every_extent_from_the_offset_to_the_end_of_the_file() {
        let mut buffer = [u64::MAX; FIEMAP_BUFFER_WORDS];
        ask_for_extents_from(&mut buffer, 8192);
        let bytes = bytes(&buffer);
        assert_eq!(u64_at(bytes, FM_START), 8192);
        assert_eq!(u64_at(bytes, FM_START) + u64_at(bytes, FM_LENGTH), u64::MAX);
        assert_eq!(u32_at(bytes, FM_EXTENT_COUNT) as usize, EXTENTS_PER_CALL);
        assert_eq!(u32_at(bytes, FM_MAPPED_EXTENTS), 0, "the kernel fills in the count");
    }

    #[test]
    fn only_shared_extents_with_a_disk_address_are_collected() {
        let buffer = answer(&[
            extent(0, 1 << 20, FIEMAP_EXTENT_SHARED),
            extent(4096, 2 << 20, 0),
            extent(8192, 0, FIEMAP_EXTENT_SHARED | FIEMAP_EXTENT_DATA_INLINE),
            extent(12288, 0, FIEMAP_EXTENT_SHARED | FIEMAP_EXTENT_DELALLOC | FIEMAP_EXTENT_UNKNOWN),
            extent(16384, 3 << 20, FIEMAP_EXTENT_SHARED | FIEMAP_EXTENT_LAST),
        ]);
        let mut shared = Vec::new();
        assert_eq!(collect_shared_extents(&buffer, &mut shared), Batch::Complete);
        assert_eq!(shared, [(1 << 20)..(1 << 20) + 4096, (3 << 20)..(3 << 20) + 4096]);
    }

    #[test]
    fn compressed_extents_lying_next_to_each_other_on_disk_never_look_shared_with_each_other() {
        let compressed = FIEMAP_EXTENT_SHARED | FIEMAP_EXTENT_ENCODED;
        let logical_length_of_a_compressed_extent = LONGEST_COMPRESSED_EXTENT;
        let neighbours_on_disk =
            [1 << 20, (1 << 20) + SMALLEST_EXTENT_ON_DISK, (1 << 20) + 2 * SMALLEST_EXTENT_ON_DISK];
        let extents: Vec<Extent> = neighbours_on_disk
            .iter()
            .enumerate()
            .map(|(i, &physical)| Extent {
                logical: i as u64 * logical_length_of_a_compressed_extent,
                physical,
                length: logical_length_of_a_compressed_extent,
                flags: compressed,
            })
            .collect();
        let mut shared = Vec::new();
        collect_shared_extents(&answer(&extents), &mut shared);
        for pair in shared.windows(2) {
            assert!(pair[0].end <= pair[1].start, "{pair:?} overlap although they are different extents");
        }
        let mut same_extent_again = Vec::new();
        collect_shared_extents(&answer(&extents[..1]), &mut same_extent_again);
        assert_eq!(same_extent_again[0], shared[0], "the same extent in another file claims the same range");
        let plain = claimed_range(1 << 20, 4096, FIEMAP_EXTENT_SHARED);
        assert!(shared.iter().all(|c| c.start >= plain.end), "compressed extents never meet uncompressed ones");
    }

    #[test]
    fn a_full_buffer_without_the_last_extent_continues_after_it() {
        let extents: Vec<Extent> =
            (0..EXTENTS_PER_CALL as u64).map(|i| extent(i * 8192, i * 4096, FIEMAP_EXTENT_SHARED)).collect();
        let mut shared = Vec::new();
        let last_logical = (EXTENTS_PER_CALL as u64 - 1) * 8192;
        assert_eq!(collect_shared_extents(&answer(&extents), &mut shared), Batch::ContinueFrom(last_logical + 4096));
        assert_eq!(shared.len(), EXTENTS_PER_CALL);
        assert_eq!(collect_shared_extents(&answer(&[]), &mut shared), Batch::Complete, "a file without extents");
    }

    #[test]
    fn btrfs_shares_blocks_across_its_subvolumes_and_xfs_within_its_device() {
        let mut info = [0u8; BTRFS_FS_INFO_BYTES];
        info[BTRFS_FS_INFO_FSID].copy_from_slice(&7u128.to_le_bytes());
        assert_eq!(block_sharing(BTRFS_SUPER_MAGIC, 40, || Some(info)), FileSystemBlocks::MayBeShared(BlockSpace(7)));
        assert_eq!(
            block_sharing(BTRFS_SUPER_MAGIC, 40, || None),
            FileSystemBlocks::NeverShared,
            "no file system id, no ledger key"
        );
        assert_eq!(block_sharing(XFS_SUPER_MAGIC, 40, || None), FileSystemBlocks::MayBeShared(BlockSpace(40)));
        const EXT4_SUPER_MAGIC: i64 = 0xEF53;
        assert_eq!(block_sharing(EXT4_SUPER_MAGIC, 40, || None), FileSystemBlocks::NeverShared);
    }
}
