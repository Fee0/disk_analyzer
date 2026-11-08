use std::borrow::Cow;
use std::ffi::{CStr, CString, OsStr};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::{DeviceId, DirEntry, SharedBlocks};

const STAT_BLOCK_SIZE: u64 = 512;

pub(super) struct Fd(pub libc::c_int);

impl Drop for Fd {
    fn drop(&mut self) {
        unsafe { libc::close(self.0) };
    }
}

pub(super) fn remove_link(path: &Path) -> io::Result<()> {
    std::fs::remove_file(path)
}

pub(super) fn entry_with_unreadable_attributes(name: &[u8], device: DeviceId) -> DirEntry<'_> {
    DirEntry {
        name,
        entry_type: None,
        device,
        allocated: 0,
        hard_link_id: None,
        shared_data_stream_id: None,
        shared_blocks: SharedBlocks::NONE_KNOWN,
    }
}

pub(super) fn c_path(path: &[u8]) -> io::Result<CString> {
    CString::new(path).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains a NUL byte"))
}

pub(super) fn open_dir_without_following_symlink(path: &[u8]) -> io::Result<Fd> {
    let path = c_path(path)?;
    let fd =
        unsafe { libc::open(path.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC) };
    if fd < 0 { Err(io::Error::last_os_error()) } else { Ok(Fd(fd)) }
}

pub(super) fn stat_at(dir: libc::c_int, name: &CStr) -> io::Result<libc::stat> {
    let mut status: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstatat(dir, name.as_ptr(), &mut status, libc::AT_SYMLINK_NOFOLLOW) } == 0 {
        Ok(status)
    } else {
        Err(io::Error::last_os_error())
    }
}

pub(super) fn allocated_bytes(status: &libc::stat) -> u64 {
    status.st_blocks as u64 * STAT_BLOCK_SIZE
}

pub(super) fn hard_link_id(is_regular_file: bool, links: impl Into<u64>, inode: u64) -> Option<u64> {
    (is_regular_file && links.into() > 1).then_some(inode)
}

pub(super) fn device_of(path: &Path) -> io::Result<DeviceId> {
    std::fs::symlink_metadata(path).map(|m| DeviceId(m.dev()))
}

pub(super) fn canonical_root(path: &Path) -> io::Result<PathBuf> {
    std::fs::canonicalize(path)
}

pub(super) fn path_bytes(path: &Path) -> Cow<'_, [u8]> {
    Cow::Borrowed(path.as_os_str().as_bytes())
}

pub(super) fn os_str_from_bytes(bytes: &[u8]) -> Cow<'_, OsStr> {
    Cow::Borrowed(OsStr::from_bytes(bytes))
}

pub(super) fn run_until_done(program: &str, args: &[&OsStr]) -> io::Result<()> {
    let status =
        Command::new(program).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status()?;
    if status.success() { Ok(()) } else { Err(io::Error::other(format!("{program} failed ({status})"))) }
}
