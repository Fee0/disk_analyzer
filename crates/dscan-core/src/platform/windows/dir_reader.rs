use std::cell::RefCell;
use std::io;

use windows_sys::Win32::Foundation::{
    ERROR_INVALID_LEVEL, ERROR_INVALID_PARAMETER, ERROR_NO_MORE_FILES, ERROR_NOT_SUPPORTED,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_INFO_BY_HANDLE_CLASS, FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FileIdBothDirectoryInfo,
    FileIdExtdDirectoryInfo, GetFileInformationByHandleEx,
};

use super::dir_info::{self, FILE_ID_BOTH_DIR_INFO_WITHOUT_UNIQUE_IDS, FILE_ID_EXTD_DIR_INFO, Layout, Record};
use super::handle::{Handle, wide_nul_terminated};
use super::paths;
use crate::platform::{DeviceId, DirEntry, SharedBlocks};

const BUFFER_BYTES: usize = 64 * 1024;

thread_local! {
    static BUFFER: RefCell<Vec<u64>> = RefCell::new(vec![0u64; BUFFER_BYTES / size_of::<u64>()]);
    static NAME: RefCell<String> = const { RefCell::new(String::new()) };
}

#[derive(Clone, Copy)]
struct Listing {
    class: FILE_INFO_BY_HANDLE_CLASS,
    layout: Layout,
}

const WITH_UNIQUE_128_BIT_IDS: Listing = Listing { class: FileIdExtdDirectoryInfo, layout: FILE_ID_EXTD_DIR_INFO };
const FOR_FILE_SYSTEMS_WITHOUT_UNIQUE_IDS: Listing =
    Listing { class: FileIdBothDirectoryInfo, layout: FILE_ID_BOTH_DIR_INFO_WITHOUT_UNIQUE_IDS };

enum Batch {
    Filled,
    End,
}

fn next_batch(directory: &Handle, listing: Listing, buffer: &mut [u64]) -> io::Result<Batch> {
    let ok = unsafe {
        GetFileInformationByHandleEx(
            directory.0,
            listing.class,
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

fn file_system_lacks_the_listing(error: &io::Error) -> bool {
    [ERROR_INVALID_PARAMETER, ERROR_INVALID_LEVEL, ERROR_NOT_SUPPORTED]
        .iter()
        .any(|&code| error.raw_os_error() == Some(code as i32))
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
            |listing, buffer| visit_batch(buffer, listing.layout, volume, &mut visit),
        )
    })
}

fn read_batches(
    buffer: &mut [u64],
    mut next_batch: impl FnMut(Listing, &mut [u64]) -> io::Result<Batch>,
    mut visit_batch: impl FnMut(Listing, &[u64]),
) -> io::Result<()> {
    let mut listing = WITH_UNIQUE_128_BIT_IDS;
    let mut may_fall_back = true;
    loop {
        match next_batch(listing, buffer) {
            Ok(Batch::Filled) => visit_batch(listing, buffer),
            Ok(Batch::End) => return Ok(()),
            Err(e) if may_fall_back && file_system_lacks_the_listing(&e) => {
                listing = FOR_FILE_SYSTEMS_WITHOUT_UNIQUE_IDS;
            }
            Err(e) => return Err(e),
        }
        may_fall_back = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::EntryType;
    use std::mem::offset_of;
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

    fn lacks_the_listing() -> io::Error {
        io::Error::from_raw_os_error(ERROR_NOT_SUPPORTED as i32)
    }

    #[test]
    fn a_file_system_lacking_both_listings_fails_instead_of_retrying_forever() {
        let mut asked = Vec::new();
        let outcome = read_batches(
            &mut [0u64; 1],
            |listing, _| {
                asked.push(listing.class);
                assert!(asked.len() <= 2, "gave up after the fallback failed too");
                Err(lacks_the_listing())
            },
            |_, _| panic!("nothing was listed"),
        );
        assert!(outcome.is_err_and(|e| file_system_lacks_the_listing(&e)));
        assert_eq!(asked, [WITH_UNIQUE_128_BIT_IDS.class, FOR_FILE_SYSTEMS_WITHOUT_UNIQUE_IDS.class]);
    }

    #[test]
    fn falls_back_once_and_lists_every_batch_with_the_fallback() {
        let mut answers = vec![Ok(Batch::End), Ok(Batch::Filled), Ok(Batch::Filled), Err(lacks_the_listing())];
        let mut visited = Vec::new();
        read_batches(&mut [0u64; 1], |_, _| answers.pop().unwrap(), |listing, _| visited.push(listing.class)).unwrap();
        assert_eq!(visited, [FOR_FILE_SYSTEMS_WITHOUT_UNIQUE_IDS.class; 2]);
    }

    #[test]
    fn only_the_first_batch_may_fall_back() {
        let mut answers = vec![Err(lacks_the_listing()), Ok(Batch::Filled)];
        let outcome = read_batches(&mut [0u64; 1], |_, _| answers.pop().unwrap(), |_, _| {});
        assert!(outcome.is_err(), "a later failure is a real error, not a missing listing");
    }
}
