use crate::platform::EntryType;

pub(super) const NEXT_ENTRY_OFFSET: usize = 0;
pub(super) const END_OF_FILE: usize = 40;
pub(super) const ALLOCATION_SIZE: usize = 48;
pub(super) const FILE_ATTRIBUTES: usize = 56;
pub(super) const FILE_NAME_LENGTH: usize = 60;

const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const REPARSE_TAG_NAME_SURROGATE_BIT: u32 = 0x2000_0000;
const UTF16_UNIT_BYTES: usize = 2;
const FILE_ID_128_BYTES: usize = 16;
const LAST_RECORD: u32 = 0;
const NO_FILE_ID: u128 = 0;
const FOLD_MULTIPLIER: u64 = 0x9E37_79B9_7F4A_7C15;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Layout {
    pub reparse_tag: usize,
    pub unique_file_id: Option<usize>,
    pub file_name: usize,
}

pub(super) const FILE_ID_EXTD_DIR_INFO: Layout = Layout { reparse_tag: 68, unique_file_id: Some(72), file_name: 88 };
pub(super) const FILE_ID_BOTH_DIR_INFO_WITHOUT_UNIQUE_IDS: Layout =
    Layout { reparse_tag: 64, unique_file_id: None, file_name: 104 };

#[derive(Clone, Copy, Debug)]
pub(super) struct Record<'a> {
    name_utf16le: &'a [u8],
    pub attributes: u32,
    pub reparse_tag: u32,
    pub end_of_file: u64,
    pub allocation_size: u64,
    pub file_id: Option<u128>,
}

impl Record<'_> {
    pub fn name_units(&self) -> impl Iterator<Item = u16> + '_ {
        self.name_utf16le.as_chunks::<UTF16_UNIT_BYTES>().0.iter().map(|&unit| u16::from_le_bytes(unit))
    }

    pub fn is_self_or_parent(&self) -> bool {
        let dot = u16::from(b'.');
        let name: Vec<u16> = self.name_units().take(3).collect();
        name == [dot] || name == [dot, dot]
    }

    pub fn entry_type(&self) -> EntryType {
        let points_elsewhere = self.attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
            && self.reparse_tag & REPARSE_TAG_NAME_SURROGATE_BIT != 0;
        if points_elsewhere {
            EntryType::Symlink
        } else if self.attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
            EntryType::Directory
        } else {
            EntryType::File
        }
    }

    pub fn hard_link_id(&self) -> Option<u64> {
        let is_file = self.entry_type() == EntryType::File;
        self.file_id.filter(|&id| is_file && id != NO_FILE_ID).map(fold_to_64_bits)
    }
}

fn fold_to_64_bits(id: u128) -> u64 {
    let (high, low) = ((id >> 64) as u64, id as u64);
    low ^ high.wrapping_mul(FOLD_MULTIPLIER)
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

fn u64_at(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap())
}

fn u128_at(bytes: &[u8], at: usize) -> u128 {
    u128::from_le_bytes(bytes[at..at + FILE_ID_128_BYTES].try_into().unwrap())
}

pub(super) fn records(buffer: &[u8], layout: Layout) -> impl Iterator<Item = Record<'_>> {
    let mut next = Some(0usize);
    std::iter::from_fn(move || {
        let at = next?;
        let record = buffer.get(at..).filter(|r| r.len() >= layout.file_name)?;
        let name_length = u32_at(record, FILE_NAME_LENGTH) as usize;
        let name_utf16le = record.get(layout.file_name..layout.file_name + name_length)?;
        next = match u32_at(record, NEXT_ENTRY_OFFSET) {
            LAST_RECORD => None,
            offset => Some(at + offset as usize),
        };
        Some(Record {
            name_utf16le,
            attributes: u32_at(record, FILE_ATTRIBUTES),
            reparse_tag: u32_at(record, layout.reparse_tag),
            end_of_file: u64_at(record, END_OF_FILE),
            allocation_size: u64_at(record, ALLOCATION_SIZE),
            file_id: layout.unique_file_id.map(|at| u128_at(record, at)),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECORD_ALIGNMENT: usize = 8;
    const FILE_ATTRIBUTE_NORMAL: u32 = 0x80;
    const IO_REPARSE_TAG_SYMLINK: u32 = 0xA000_000C;
    const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;
    const IO_REPARSE_TAG_CLOUD_6: u32 = 0x9000_601A;

    struct Fake<'a> {
        name: &'a str,
        attributes: u32,
        reparse_tag: u32,
        end_of_file: u64,
        allocation_size: u64,
        file_id: u128,
    }

    fn fake(name: &str, attributes: u32, reparse_tag: u32) -> Fake<'_> {
        Fake { name, attributes, reparse_tag, end_of_file: 0, allocation_size: 0, file_id: 0 }
    }

    fn buffer_with(layout: Layout, records: &[Fake<'_>]) -> Vec<u8> {
        let mut buffer = Vec::new();
        for (i, fake) in records.iter().enumerate() {
            let name: Vec<u8> = fake.name.encode_utf16().flat_map(u16::to_le_bytes).collect();
            let mut record = vec![0u8; (layout.file_name + name.len()).next_multiple_of(RECORD_ALIGNMENT)];
            let is_last = i + 1 == records.len();
            let next = if is_last { LAST_RECORD } else { record.len() as u32 };
            record[NEXT_ENTRY_OFFSET..][..4].copy_from_slice(&next.to_le_bytes());
            record[END_OF_FILE..][..8].copy_from_slice(&fake.end_of_file.to_le_bytes());
            record[ALLOCATION_SIZE..][..8].copy_from_slice(&fake.allocation_size.to_le_bytes());
            record[FILE_ATTRIBUTES..][..4].copy_from_slice(&fake.attributes.to_le_bytes());
            record[FILE_NAME_LENGTH..][..4].copy_from_slice(&(name.len() as u32).to_le_bytes());
            record[layout.reparse_tag..][..4].copy_from_slice(&fake.reparse_tag.to_le_bytes());
            if let Some(at) = layout.unique_file_id {
                record[at..][..FILE_ID_128_BYTES].copy_from_slice(&fake.file_id.to_le_bytes());
            }
            record[layout.file_name..][..name.len()].copy_from_slice(&name);
            buffer.extend(record);
        }
        buffer
    }

    #[test]
    fn walks_every_record_by_its_next_entry_offset_and_reads_its_fields() {
        let refs_style_id = (7u128 << 64) | 0x1234;
        let buffer = buffer_with(
            FILE_ID_EXTD_DIR_INFO,
            &[
                fake(".", FILE_ATTRIBUTE_DIRECTORY, 0),
                Fake {
                    name: "Ünïcode.bin",
                    attributes: FILE_ATTRIBUTE_NORMAL,
                    reparse_tag: 0,
                    end_of_file: 10_000,
                    allocation_size: 12_288,
                    file_id: refs_style_id,
                },
            ],
        );
        let parsed: Vec<Record<'_>> = records(&buffer, FILE_ID_EXTD_DIR_INFO).collect();
        assert_eq!(parsed.len(), 2);
        assert!(parsed[0].is_self_or_parent());
        let file = parsed[1];
        assert_eq!(String::from_utf16(&file.name_units().collect::<Vec<_>>()).unwrap(), "Ünïcode.bin");
        assert_eq!((file.end_of_file, file.allocation_size), (10_000, 12_288));
        assert_eq!(file.file_id, Some(refs_style_id));
        assert_eq!(file.entry_type(), EntryType::File);
        assert!(!file.is_self_or_parent());
    }

    fn record(attributes: u32, file_id: u128) -> Record<'static> {
        Record {
            name_utf16le: &[],
            attributes,
            reparse_tag: 0,
            end_of_file: 0,
            allocation_size: 0,
            file_id: Some(file_id),
        }
    }

    #[test]
    fn ntfs_ids_keep_their_value_and_refs_ids_that_differ_only_in_the_high_half_stay_apart() {
        let ntfs = record(FILE_ATTRIBUTE_NORMAL, 0x0005_0000_0000_1234);
        assert_eq!(ntfs.hard_link_id(), Some(0x0005_0000_0000_1234));
        let in_one_folder = record(FILE_ATTRIBUTE_NORMAL, (1u128 << 64) | 42);
        let in_another_folder = record(FILE_ATTRIBUTE_NORMAL, (2u128 << 64) | 42);
        assert_ne!(in_one_folder.hard_link_id(), in_another_folder.hard_link_id());
    }

    #[test]
    fn folders_files_without_an_id_and_listings_without_unique_ids_have_no_hard_link_id() {
        assert_eq!(record(FILE_ATTRIBUTE_DIRECTORY, 9).hard_link_id(), None);
        assert_eq!(record(FILE_ATTRIBUTE_NORMAL, NO_FILE_ID).hard_link_id(), None);
        let buffer =
            buffer_with(FILE_ID_BOTH_DIR_INFO_WITHOUT_UNIQUE_IDS, &[fake("fat.txt", FILE_ATTRIBUTE_NORMAL, 0)]);
        let fat = records(&buffer, FILE_ID_BOTH_DIR_INFO_WITHOUT_UNIQUE_IDS).next().unwrap();
        assert_eq!(fat.hard_link_id(), None, "FAT and exFAT have no hard links and no unique ids");
    }

    #[test]
    fn symlinks_and_junctions_are_not_followed_but_cloud_placeholder_folders_are() {
        let reparse_dir = FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT;
        for layout in [FILE_ID_EXTD_DIR_INFO, FILE_ID_BOTH_DIR_INFO_WITHOUT_UNIQUE_IDS] {
            let buffer = buffer_with(
                layout,
                &[
                    fake("junction", reparse_dir, IO_REPARSE_TAG_MOUNT_POINT),
                    fake("dir-symlink", reparse_dir, IO_REPARSE_TAG_SYMLINK),
                    fake("file-symlink", FILE_ATTRIBUTE_REPARSE_POINT, IO_REPARSE_TAG_SYMLINK),
                    fake("OneDrive", reparse_dir, IO_REPARSE_TAG_CLOUD_6),
                    fake("plain", FILE_ATTRIBUTE_DIRECTORY, 0),
                ],
            );
            let types: Vec<EntryType> = records(&buffer, layout).map(|r| r.entry_type()).collect();
            use EntryType::{Directory, Symlink};
            assert_eq!(types, [Symlink, Symlink, Symlink, Directory, Directory], "{layout:?}");
        }
    }

    #[test]
    fn a_truncated_buffer_ends_the_listing_instead_of_reading_past_it() {
        let buffer =
            buffer_with(FILE_ID_EXTD_DIR_INFO, &[fake("first", FILE_ATTRIBUTE_NORMAL, 0), fake("second", 0x20, 0)]);
        let cut_inside_the_second_name = buffer.len() - 12;
        assert_eq!(records(&buffer[..cut_inside_the_second_name], FILE_ID_EXTD_DIR_INFO).count(), 1);
        assert_eq!(records(&[], FILE_ID_EXTD_DIR_INFO).count(), 0);
    }
}
