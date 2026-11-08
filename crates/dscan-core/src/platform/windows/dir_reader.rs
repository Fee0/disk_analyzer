use std::cell::RefCell;
use std::io;

use windows_sys::Win32::Foundation::ERROR_NO_MORE_FILES;
use windows_sys::Win32::Storage::FileSystem::{
    FILE_INFO_BY_HANDLE_CLASS, FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FileIdBothDirectoryInfo,
    FileIdExtdDirectoryInfo, GetFileInformationByHandleEx,
};

use super::dir_info::{self, FILE_ID_BOTH_DIR_INFO_WITHOUT_UNIQUE_IDS, FILE_ID_EXTD_DIR_INFO, Layout, Record};
use super::handle::{Handle, wide_nul_terminated};
use super::listing::{Batch, Listing, read_batches};
use super::paths;
use crate::platform::{DeviceId, DirEntry, SharedBlocks};

const BUFFER_BYTES: usize = 64 * 1024;

thread_local! {
    static BUFFER: RefCell<Vec<u64>> = RefCell::new(vec![0u64; BUFFER_BYTES / size_of::<u64>()]);
    static NAME: RefCell<String> = const { RefCell::new(String::new()) };
}

fn information_class(listing: Listing) -> FILE_INFO_BY_HANDLE_CLASS {
    match listing {
        Listing::WithUnique128BitIds => FileIdExtdDirectoryInfo,
        Listing::ForFileSystemsWithoutUniqueIds => FileIdBothDirectoryInfo,
    }
}

fn record_layout(listing: Listing) -> Layout {
    match listing {
        Listing::WithUnique128BitIds => FILE_ID_EXTD_DIR_INFO,
        Listing::ForFileSystemsWithoutUniqueIds => FILE_ID_BOTH_DIR_INFO_WITHOUT_UNIQUE_IDS,
    }
}

fn next_batch(directory: &Handle, listing: Listing, buffer: &mut [u64]) -> io::Result<Batch> {
    let ok = unsafe {
        GetFileInformationByHandleEx(
            directory.0,
            information_class(listing),
            buffer.as_mut_ptr().cast(),
            std::mem::size_of_val(buffer) as u32,
        )
    };
    if ok != 0 {
        return Ok(Batch::Filled);
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) { Ok(Batch::End) } else { Err(error) }
}

fn as_bytes(buffer: &[u64]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast(), std::mem::size_of_val(buffer)) }
}

fn entry<'a>(record: &Record<'_>, name: &'a str, volume: DeviceId) -> DirEntry<'a> {
    DirEntry {
        name: name.as_bytes(),
        entry_type: Some(record.entry_type()),
        device: volume,
        allocated: record.allocation_size,
        hard_link_id: record.hard_link_id(),
        shared_data_stream_id: None,
        shared_blocks: SharedBlocks::NONE_KNOWN,
    }
}

fn visit_batch(buffer: &[u64], layout: Layout, volume: DeviceId, visit: &mut impl FnMut(DirEntry<'_>)) {
    for record in dir_info::records(as_bytes(buffer), layout).filter(|r| !r.is_self_or_parent()) {
        NAME.with_borrow_mut(|name| {
            name.clear();
            name.extend(char::decode_utf16(record.name_units()).map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER)));
            visit(entry(&record, name, volume));
        });
    }
}

pub(super) fn read_dir(path: &[u8], mut visit: impl FnMut(DirEntry<'_>)) -> io::Result<()> {
    let wide = wide_nul_terminated(&paths::verbatim(&String::from_utf8_lossy(path)));
    let directory = Handle::open_without_following_links(&wide, FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES)?;
    let volume = directory.volume()?;
    BUFFER.with_borrow_mut(|buffer| {
        read_batches(
            buffer,
            |listing, buffer| next_batch(&directory, listing, buffer),
            |listing, buffer| visit_batch(buffer, record_layout(listing), volume, &mut visit),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::EntryType;
    use crate::platform::windows::listing;
    use std::mem::offset_of;
    use windows_sys::Win32::Foundation::{ERROR_INVALID_LEVEL, ERROR_INVALID_PARAMETER, ERROR_NOT_SUPPORTED};
    use windows_sys::Win32::Storage::FileSystem::{FILE_ID_BOTH_DIR_INFO, FILE_ID_EXTD_DIR_INFO as Extd};

    #[test]
    fn record_offsets_match_the_windows_definitions() {
        assert_eq!(dir_info::NEXT_ENTRY_OFFSET, offset_of!(Extd, NextEntryOffset));
        assert_eq!(dir_info::ALLOCATION_SIZE, offset_of!(Extd, AllocationSize));
        assert_eq!(dir_info::FILE_ATTRIBUTES, offset_of!(Extd, FileAttributes));
        assert_eq!(dir_info::FILE_NAME_LENGTH, offset_of!(Extd, FileNameLength));
        assert_eq!(FILE_ID_EXTD_DIR_INFO.reparse_tag, offset_of!(Extd, ReparsePointTag));
        assert_eq!(FILE_ID_EXTD_DIR_INFO.unique_file_id, Some(offset_of!(Extd, FileId)));
        assert_eq!(FILE_ID_EXTD_DIR_INFO.file_name, offset_of!(Extd, FileName));
        assert_eq!(dir_info::ALLOCATION_SIZE, offset_of!(FILE_ID_BOTH_DIR_INFO, AllocationSize));
        assert_eq!(dir_info::FILE_NAME_LENGTH, offset_of!(FILE_ID_BOTH_DIR_INFO, FileNameLength));
        assert_eq!(FILE_ID_BOTH_DIR_INFO_WITHOUT_UNIQUE_IDS.reparse_tag, offset_of!(FILE_ID_BOTH_DIR_INFO, EaSize));
        assert_eq!(FILE_ID_BOTH_DIR_INFO_WITHOUT_UNIQUE_IDS.file_name, offset_of!(FILE_ID_BOTH_DIR_INFO, FileName));
    }

    #[test]
    fn listing_error_codes_match_the_windows_definitions() {
        assert_eq!(listing::ERROR_NOT_SUPPORTED, ERROR_NOT_SUPPORTED as i32);
        assert_eq!(listing::ERROR_INVALID_PARAMETER, ERROR_INVALID_PARAMETER as i32);
        assert_eq!(listing::ERROR_INVALID_LEVEL, ERROR_INVALID_LEVEL as i32);
    }

    #[test]
    fn reads_names_types_sizes_and_shared_file_ids_of_hard_links() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        std::fs::write(r.join("a.bin"), vec![1u8; 10_000]).unwrap();
        std::fs::hard_link(r.join("a.bin"), r.join("a-link.bin")).unwrap();
        std::fs::write(r.join("ünïcode.txt"), b"x").unwrap();
        std::fs::create_dir(r.join("sub")).unwrap();

        let mut seen = std::collections::BTreeMap::new();
        let root = r.to_string_lossy().into_owned();
        read_dir(root.as_bytes(), |e| {
            seen.insert(String::from_utf8(e.name.to_vec()).unwrap(), (e.entry_type, e.allocated, e.hard_link_id));
        })
        .unwrap();

        assert_eq!(seen.keys().collect::<Vec<_>>(), ["a-link.bin", "a.bin", "sub", "ünïcode.txt"]);
        assert_eq!(seen["a.bin"].0, Some(EntryType::File));
        assert!(seen["a.bin"].1 >= 10_000, "a file takes at least its length on disk");
        assert_eq!(seen["sub"].0, Some(EntryType::Directory));
        assert!(seen["a.bin"].2.is_some(), "NTFS lists 128-bit file ids");
        assert_eq!(seen["a.bin"].2, seen["a-link.bin"].2, "both names of a hard link share a file id");
        assert_ne!(seen["a.bin"].2, seen["ünïcode.txt"].2);
        assert_eq!(seen["sub"].2, None);
    }
}
