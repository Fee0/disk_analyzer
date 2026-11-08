use std::ffi::CStr;
use std::io;
use std::mem::ManuallyDrop;
use std::ops::Range;

use super::extents::{self, Batch, FIEMAP_BUFFER_WORDS, FileSystemBlocks};
use crate::platform::unix::{
    Fd, allocated_bytes, entry_with_unreadable_attributes, hard_link_id, open_dir_without_following_symlink, stat_at,
};
use crate::platform::{DeviceId, DirEntry, EntryType, SharedBlocks};

const FS_IOC_FIEMAP: u32 = 0xC020_660B;
const BTRFS_IOC_FS_INFO: u32 = 0x8400_941F;
const NO_ERROR: libc::c_int = 0;

struct Dir(*mut libc::DIR);

impl Drop for Dir {
    fn drop(&mut self) {
        unsafe { libc::closedir(self.0) };
    }
}

impl Dir {
    fn open(path: &[u8]) -> io::Result<Dir> {
        let fd = open_dir_without_following_symlink(path)?;
        let dir = unsafe { libc::fdopendir(fd.0) };
        if dir.is_null() {
            return Err(io::Error::last_os_error());
        }
        let _now_closed_by_closedir = ManuallyDrop::new(fd);
        Ok(Dir(dir))
    }

    fn fd(&self) -> libc::c_int {
        unsafe { libc::dirfd(self.0) }
    }

    fn next_name(&mut self) -> io::Result<Option<&CStr>> {
        unsafe { *libc::__errno_location() = NO_ERROR };
        let entry = unsafe { libc::readdir(self.0) };
        if entry.is_null() {
            let error = io::Error::last_os_error();
            return if error.raw_os_error() == Some(NO_ERROR) { Ok(None) } else { Err(error) };
        }
        Ok(Some(unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }))
    }
}

fn is_self_or_parent(name: &[u8]) -> bool {
    name == b"." || name == b".."
}

fn entry_from_stat<'a>(name: &'a [u8], status: &libc::stat) -> DirEntry<'a> {
    let file_type = status.st_mode & libc::S_IFMT;
    let is_regular_file = file_type == libc::S_IFREG;
    DirEntry {
        name,
        entry_type: Some(match file_type {
            libc::S_IFDIR => EntryType::Directory,
            libc::S_IFLNK => EntryType::Symlink,
            _ => EntryType::File,
        }),
        device: DeviceId(status.st_dev),
        allocated: allocated_bytes(status),
        hard_link_id: hard_link_id(is_regular_file, status.st_nlink, status.st_ino),
        shared_data_stream_id: None,
        shared_blocks: SharedBlocks::NONE_KNOWN,
    }
}

fn block_sharing_of(dir: libc::c_int, device: DeviceId) -> FileSystemBlocks {
    let mut file_system: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstatfs(dir, &mut file_system) } != 0 {
        return FileSystemBlocks::NeverShared;
    }
    extents::block_sharing(file_system.f_type as i64, device.0, || btrfs_fs_info(dir))
}

fn btrfs_fs_info(dir: libc::c_int) -> Option<[u8; extents::BTRFS_FS_INFO_BYTES]> {
    let mut info = [0u8; extents::BTRFS_FS_INFO_BYTES];
    let answered = unsafe { libc::ioctl(dir, BTRFS_IOC_FS_INFO as _, info.as_mut_ptr()) } == 0;
    answered.then_some(info)
}

fn has_data_blocks(status: &libc::stat) -> bool {
    status.st_mode & libc::S_IFMT == libc::S_IFREG && status.st_blocks > 0
}

fn shared_extents_of(dir: libc::c_int, name: &CStr, shared: &mut Vec<Range<u64>>) {
    shared.clear();
    let flags = libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_NOCTTY | libc::O_CLOEXEC;
    let fd = unsafe { libc::openat(dir, name.as_ptr(), flags) };
    if fd < 0 {
        return;
    }
    let file = Fd(fd);
    let mut buffer = [0u64; FIEMAP_BUFFER_WORDS];
    let mut from = 0;
    loop {
        extents::ask_for_extents_from(&mut buffer, from);
        if unsafe { libc::ioctl(file.0, FS_IOC_FIEMAP as _, buffer.as_mut_ptr()) } != 0 {
            return;
        }
        match extents::collect_shared_extents(&buffer, shared) {
            Batch::Complete => return,
            Batch::ContinueFrom(offset) => from = offset,
        }
    }
}

pub(super) fn read_dir(path: &[u8], mut visit: impl FnMut(DirEntry<'_>)) -> io::Result<()> {
    let mut dir = Dir::open(path)?;
    let dir_fd = dir.fd();
    let own_device = DeviceId(stat_at(dir_fd, c".")?.st_dev);
    let sharing = block_sharing_of(dir_fd, own_device);
    let mut shared = Vec::new();
    while let Some(name) = dir.next_name()? {
        let name_bytes = name.to_bytes();
        if is_self_or_parent(name_bytes) {
            continue;
        }
        match stat_at(dir_fd, name) {
            Ok(status) => {
                let mut entry = entry_from_stat(name_bytes, &status);
                if let FileSystemBlocks::MayBeShared(space) = sharing
                    && has_data_blocks(&status)
                {
                    shared_extents_of(dir_fd, name, &mut shared);
                    entry.shared_blocks = SharedBlocks { space, ranges: &shared };
                }
                visit(entry)
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(_) => visit(entry_with_unreadable_attributes(name_bytes, own_device)),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStrExt;

    #[test]
    fn reads_names_types_sizes_and_hard_link_ids_without_following_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        std::fs::write(r.join("a.bin"), vec![1u8; 10_000]).unwrap();
        std::fs::hard_link(r.join("a.bin"), r.join("a-link.bin")).unwrap();
        std::fs::write(r.join("single.txt"), b"x").unwrap();
        std::fs::create_dir(r.join("sub")).unwrap();
        std::os::unix::fs::symlink(r.join("sub"), r.join("sym")).unwrap();

        let mut seen = std::collections::BTreeMap::new();
        read_dir(r.as_os_str().as_bytes(), |e| {
            seen.insert(String::from_utf8_lossy(e.name).into_owned(), (e.entry_type, e.allocated, e.hard_link_id));
        })
        .unwrap();

        assert_eq!(seen.keys().collect::<Vec<_>>(), ["a-link.bin", "a.bin", "single.txt", "sub", "sym"]);
        let (entry_type, allocated, hard_link_id) = seen["a.bin"];
        assert_eq!(entry_type, Some(EntryType::File));
        assert!(allocated >= 10_000);
        assert!(hard_link_id.is_some() && hard_link_id == seen["a-link.bin"].2, "both names share one inode");
        assert_eq!(seen["single.txt"].2, None, "a file with one name needs no hard-link bookkeeping");
        assert_eq!(seen["sub"].0, Some(EntryType::Directory));
        assert_eq!(seen["sym"].0, Some(EntryType::Symlink));
    }

    #[test]
    fn a_missing_folder_is_an_error() {
        assert!(read_dir(b"/definitely/not/here", |_| {}).is_err());
    }
}
