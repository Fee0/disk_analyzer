use std::ffi::CString;
use std::io;

use crate::platform::unix::{Fd, entry_with_unreadable_attributes, open_dir_without_following_symlink};
use crate::platform::{DeviceId, DirEntry, EntryType};

const ATTR_CMN_ERROR: u32 = 0x2000_0000;
const BUF_SIZE: usize = 256 * 1024;

const EF_MAY_SHARE_BLOCKS: u64 = 0x1;
const CLONE_INFO_ATTRIBUTES: u32 = libc::ATTR_CMNEXT_CLONEID | libc::ATTR_CMNEXT_EXT_FLAGS;
const RECORD_LENGTH_SIZE: usize = 4;
const STAT_BLOCK_SIZE: u64 = 512;

const VREG: u32 = 1;
const VDIR: u32 = 2;
const VLNK: u32 = 5;

#[derive(Debug, Clone, Copy)]
struct RawEntry<'a> {
    name: &'a [u8],
    error: u32,
    dev: i32,
    objtype: u32,
    ino: u64,
    links: u32,
    alloc: u64,
    logical: u64,
    data_stream: Option<u64>,
    may_share_blocks: bool,
    sizes_reported: bool,
}

impl<'a> RawEntry<'a> {
    fn attributes_unavailable(&self) -> bool {
        self.error != 0
    }

    fn as_dir_entry(&self) -> DirEntry<'a> {
        let device = DeviceId(self.dev as u64);
        if self.attributes_unavailable() {
            return entry_with_unreadable_attributes(self.name, device);
        }
        let is_regular_file = self.objtype == VREG;
        DirEntry {
            name: self.name,
            entry_type: Some(match self.objtype {
                VDIR => EntryType::Directory,
                VLNK => EntryType::Symlink,
                _ => EntryType::File,
            }),
            device,
            allocated: self.alloc,
            logical: self.logical,
            hard_link_id: (is_regular_file && self.links > 1).then_some(self.ino),
            shared_data_stream_id: self.data_stream.filter(|_| is_regular_file && self.may_share_blocks),
        }
    }

    fn needs_sizes_from_stat(&self) -> bool {
        !self.sizes_reported && !self.attributes_unavailable() && self.objtype != VDIR
    }

    fn take_sizes_from_stat(&mut self, directory: &Fd) {
        let Ok(name) = CString::new(self.name) else { return };
        let mut status: libc::stat = unsafe { std::mem::zeroed() };
        let found = unsafe { libc::fstatat(directory.0, name.as_ptr(), &mut status, libc::AT_SYMLINK_NOFOLLOW) } == 0;
        if found {
            self.logical = status.st_size as u64;
            self.alloc = status.st_blocks as u64 * STAT_BLOCK_SIZE;
            self.links = status.st_nlink as u32;
            self.ino = status.st_ino;
        }
    }
}

thread_local! {
    static BUF: std::cell::RefCell<Vec<u8>> = std::cell::RefCell::new(vec![0u8; BUF_SIZE]);
}

fn read_u32(bytes: &[u8], at: usize) -> u32 {
    u32::from_ne_bytes(bytes[at..at + 4].try_into().unwrap())
}

pub(super) fn read_dir(path: &[u8], mut visit: impl FnMut(DirEntry<'_>)) -> io::Result<()> {
    let fd = open_dir_without_following_symlink(path)?;
    let mut visit_raw = |entry: RawEntry<'_>| visit(entry.as_dir_entry());
    match read_entries(&fd, AttributeRequest::WithCloneInfo, &mut visit_raw) {
        Err(e) if file_system_rejects_attribute_request(&e) => {
            read_entries(&fd, AttributeRequest::Basic, &mut visit_raw)
        }
        other => other,
    }
}

#[derive(Clone, Copy)]
enum AttributeRequest {
    WithCloneInfo,
    Basic,
}

fn file_system_rejects_attribute_request(error: &io::Error) -> bool {
    error.raw_os_error() == Some(libc::EINVAL)
}

fn attribute_list(request: AttributeRequest) -> (libc::attrlist, u64) {
    let mut attrs: libc::attrlist = unsafe { std::mem::zeroed() };
    attrs.bitmapcount = libc::ATTR_BIT_MAP_COUNT;
    attrs.commonattr = libc::ATTR_CMN_RETURNED_ATTRS
        | ATTR_CMN_ERROR
        | libc::ATTR_CMN_NAME
        | libc::ATTR_CMN_DEVID
        | libc::ATTR_CMN_OBJTYPE
        | libc::ATTR_CMN_FILEID;
    attrs.fileattr = libc::ATTR_FILE_LINKCOUNT | libc::ATTR_FILE_TOTALSIZE | libc::ATTR_FILE_ALLOCSIZE;
    let options = match request {
        AttributeRequest::WithCloneInfo => request_extended_common_attributes(&mut attrs, CLONE_INFO_ATTRIBUTES),
        AttributeRequest::Basic => 0,
    };
    (attrs, options)
}

fn request_extended_common_attributes(attrs: &mut libc::attrlist, wanted: u32) -> u64 {
    attrs.forkattr = wanted;
    libc::FSOPT_ATTR_CMN_EXTENDED as u64
}

fn read_entries(fd: &Fd, request: AttributeRequest, f: &mut impl FnMut(RawEntry<'_>)) -> io::Result<()> {
    let (mut attrs, options) = attribute_list(request);
    BUF.with_borrow_mut(|buf| {
        loop {
            let n = unsafe {
                libc::getattrlistbulk(
                    fd.0,
                    &mut attrs as *mut _ as *mut libc::c_void,
                    buf.as_mut_ptr() as *mut libc::c_void,
                    buf.len(),
                    options,
                )
            };
            if n < 0 {
                let err = io::Error::last_os_error();
                if err.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(err);
            }
            if n == 0 {
                return Ok(());
            }
            let mut p = 0usize;
            for _ in 0..n {
                let len = read_u32(buf, p) as usize;
                if len == 0 || p + len > buf.len() {
                    return Err(io::Error::other("malformed getattrlistbulk record"));
                }
                let mut entry = parse_entry(&buf[p..p + len]);
                if entry.needs_sizes_from_stat() {
                    entry.take_sizes_from_stat(fd);
                }
                if !entry.name.is_empty() {
                    f(entry);
                }
                p += len;
            }
        }
    })
}

struct ReturnedAttributes {
    common: u32,
    file: u32,
    extended_common: u32,
}

impl ReturnedAttributes {
    const COMMON_FIELD: usize = 0;
    const FILE_FIELD: usize = 3;
    const FORK_FIELD: usize = 4;
    const FIELD_COUNT: usize = 5;
    const END: usize = RECORD_LENGTH_SIZE + 4 * Self::FIELD_COUNT;

    fn read(record: &[u8]) -> Self {
        let field = |index: usize| read_u32(record, RECORD_LENGTH_SIZE + 4 * index);
        ReturnedAttributes {
            common: field(Self::COMMON_FIELD),
            file: field(Self::FILE_FIELD),
            extended_common: field(Self::FORK_FIELD),
        }
    }
}

struct Cursor<'a> {
    record: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take<const N: usize>(&mut self) -> [u8; N] {
        let bytes = self.record[self.at..self.at + N].try_into().unwrap();
        self.at += N;
        bytes
    }
    fn u32(&mut self) -> u32 {
        u32::from_ne_bytes(self.take())
    }
    fn i32(&mut self) -> i32 {
        i32::from_ne_bytes(self.take())
    }
    fn u64(&mut self) -> u64 {
        u64::from_ne_bytes(self.take())
    }
    fn name(&mut self) -> &'a [u8] {
        let reference_at = self.at;
        let offset = self.i32() as isize;
        let length_with_nul = self.u32() as usize;
        let start = (reference_at as isize + offset) as usize;
        match length_with_nul.checked_sub(1) {
            Some(length) if start + length_with_nul <= self.record.len() => &self.record[start..start + length],
            _ => &[],
        }
    }
}

fn parse_entry(record: &[u8]) -> RawEntry<'_> {
    let returned = ReturnedAttributes::read(record);
    let mut cursor = Cursor { record, at: ReturnedAttributes::END };
    let mut e = RawEntry {
        name: &[],
        error: 0,
        dev: 0,
        objtype: 0,
        ino: 0,
        links: 1,
        alloc: 0,
        logical: 0,
        data_stream: None,
        may_share_blocks: false,
        sizes_reported: false,
    };
    if returned.common & ATTR_CMN_ERROR != 0 {
        e.error = cursor.u32();
    }
    if returned.common & libc::ATTR_CMN_NAME != 0 {
        e.name = cursor.name();
    }
    if returned.common & libc::ATTR_CMN_DEVID != 0 {
        e.dev = cursor.i32();
    }
    if returned.common & libc::ATTR_CMN_OBJTYPE != 0 {
        e.objtype = cursor.u32();
    }
    if returned.common & libc::ATTR_CMN_FILEID != 0 {
        e.ino = cursor.u64();
    }
    if returned.file & libc::ATTR_FILE_LINKCOUNT != 0 {
        e.links = cursor.u32();
    }
    if returned.file & libc::ATTR_FILE_TOTALSIZE != 0 {
        e.logical = cursor.u64();
    }
    if returned.file & libc::ATTR_FILE_ALLOCSIZE != 0 {
        e.alloc = cursor.u64();
        e.sizes_reported = true;
    }
    if returned.extended_common & libc::ATTR_CMNEXT_CLONEID != 0 {
        e.data_stream = Some(cursor.u64());
    }
    if returned.extended_common & libc::ATTR_CMNEXT_EXT_FLAGS != 0 {
        e.may_share_blocks = cursor.u64() & EF_MAY_SHARE_BLOCKS != 0;
    }
    e
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStrExt;

    #[test]
    fn reads_entries_with_sizes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), vec![1u8; 10_000]).unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        let mut seen = Vec::new();
        read_dir(dir.path().as_os_str().as_bytes(), |e| {
            seen.push((e.name.to_vec(), e.entry_type, e.logical, e.allocated))
        })
        .unwrap();
        seen.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].0, b"a.txt");
        assert_eq!(seen[0].1, Some(EntryType::File));
        assert_eq!(seen[0].2, 10_000);
        assert!(seen[0].3 >= 10_000);
        assert_eq!(seen[1].0, b"sub");
        assert_eq!(seen[1].1, Some(EntryType::Directory));
    }

    #[derive(Clone, Copy)]
    struct Entry {
        entry_type: Option<EntryType>,
        logical: u64,
        allocated: u64,
        shared_data_stream_id: Option<u64>,
    }

    fn entries(dir: &std::path::Path) -> std::collections::HashMap<String, Entry> {
        let mut seen = std::collections::HashMap::new();
        read_dir(dir.as_os_str().as_bytes(), |e| {
            let entry = Entry {
                entry_type: e.entry_type,
                logical: e.logical,
                allocated: e.allocated,
                shared_data_stream_id: e.shared_data_stream_id,
            };
            seen.insert(String::from_utf8_lossy(e.name).into_owned(), entry);
        })
        .unwrap();
        seen
    }

    fn clone_file(dir: &std::path::Path, from: &str, to: &str) {
        let c = |name: &str| CString::new(dir.join(name).as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::clonefile(c(from).as_ptr(), c(to).as_ptr(), 0) }, 0, "clonefile failed");
    }

    fn overwrite_part_of(path: &std::path::Path) {
        use std::io::{Seek, Write};
        let mut file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        file.seek(std::io::SeekFrom::Start(100_000)).unwrap();
        file.write_all(&[1u8; 8192]).unwrap();
        file.sync_all().unwrap();
    }

    fn original_clone_and_unrelated_file() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("orig.bin"), vec![7u8; 200_000]).unwrap();
        std::fs::write(dir.path().join("other.bin"), vec![7u8; 200_000]).unwrap();
        clone_file(dir.path(), "orig.bin", "clone.bin");
        dir
    }

    #[test]
    fn file_sizes_parse_correctly_alongside_clone_info() {
        let dir = original_clone_and_unrelated_file();
        for (name, e) in entries(dir.path()) {
            assert_eq!(e.entry_type, Some(EntryType::File), "{name}");
            assert_eq!(e.logical, 200_000, "{name}");
            assert!(e.allocated >= 200_000, "{name}");
        }
    }

    #[test]
    fn pure_clones_share_the_data_stream_and_files_that_share_no_blocks_report_none() {
        let dir = original_clone_and_unrelated_file();
        let seen = entries(dir.path());
        let (orig, clone, other) = (seen["orig.bin"], seen["clone.bin"], seen["other.bin"]);
        assert!(orig.shared_data_stream_id.is_some(), "either side may be met first by the walk");
        assert_eq!(orig.shared_data_stream_id, clone.shared_data_stream_id);
        assert_eq!(other.shared_data_stream_id, None);
    }

    #[test]
    fn editing_a_clone_gives_it_its_own_data_stream() {
        let dir = original_clone_and_unrelated_file();
        let original_stream = entries(dir.path())["orig.bin"].shared_data_stream_id;
        overwrite_part_of(&dir.path().join("clone.bin"));
        let seen = entries(dir.path());
        assert_eq!(seen["orig.bin"].shared_data_stream_id, original_stream);
        assert_ne!(
            seen["clone.bin"].shared_data_stream_id, original_stream,
            "a partial clone is never taken for a pure one"
        );
    }
}
