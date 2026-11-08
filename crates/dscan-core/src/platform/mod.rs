#[cfg(any(target_os = "linux", test))]
pub(crate) mod linux;
#[cfg(any(target_os = "macos", test))]
pub(crate) mod macos;
#[cfg(unix)]
mod unix;
#[cfg(any(unix, test))]
pub(crate) mod unix_syntax;
#[cfg(any(windows, test))]
pub(crate) mod windows;

use std::borrow::Cow;
use std::ffi::OsStr;
use std::fs::FileType;
use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::hidden::{SpaceExplanation, SpaceInfo};
use crate::protection::ProtectedLocations;
use crate::volumes::Volume;

#[cfg(target_os = "linux")]
pub(crate) type Current = linux::Linux;
#[cfg(target_os = "macos")]
pub(crate) type Current = macos::MacOs;
#[cfg(windows)]
pub(crate) type Current = windows::Windows;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct DeviceId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct BlockSpace(pub u128);

#[derive(Clone, Copy, Debug)]
pub(crate) struct SharedBlocks<'a> {
    pub space: BlockSpace,
    pub ranges: &'a [Range<u64>],
}

impl SharedBlocks<'_> {
    pub const NONE_KNOWN: SharedBlocks<'static> = SharedBlocks { space: BlockSpace(0), ranges: &[] };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EntryType {
    File,
    Directory,
    Symlink,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct DirEntry<'a> {
    pub name: &'a [u8],
    pub entry_type: Option<EntryType>,
    pub device: DeviceId,
    pub allocated: u64,
    pub hard_link_id: Option<u64>,
    pub shared_data_stream_id: Option<u64>,
    pub shared_blocks: SharedBlocks<'a>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScanExtent {
    Folder,
    WholeDisk,
}

#[derive(Debug, Default)]
pub(crate) struct ScanBoundaries {
    pub other_devices_of_the_disk: FxHashSet<DeviceId>,
    pub duplicate_views: FxHashSet<Vec<u8>>,
    pub devices_mounted_on: FxHashMap<Vec<u8>, DeviceId>,
}

pub(crate) trait Platform {
    const PATH_SEPARATOR: u8;
    const TRASH_NAME: &'static str;
    const FULL_DISK_ACCESS_SETTINGS: Option<&'static str>;
    const PROTECTED: ProtectedLocations;
    const SYSTEM_FOLDERS_AT_SYSTEM_ROOT: &'static [&'static str];
    const HIDDEN_SPACE: SpaceExplanation;
    const PURGEABLE_SPACE: SpaceExplanation;
    const PREVIEW_ACTION: &'static str;
    const REVEAL_ACTION: &'static str;
    const COMMAND_KEY: &'static str;
    const RESCAN_SHORTCUT: &'static str;

    fn read_dir(path: &[u8], visit: impl FnMut(DirEntry<'_>)) -> io::Result<()>;
    fn device_of(path: &Path) -> io::Result<DeviceId>;
    fn canonical_root(path: &Path) -> io::Result<PathBuf>;
    fn is_system_root(path: &Path) -> bool;
    fn scan_boundaries(root: &Path, extent: ScanExtent) -> ScanBoundaries;
    fn space_info(path: &Path) -> io::Result<SpaceInfo>;
    fn mounted_volumes() -> Vec<Volume>;
    fn has_full_disk_access() -> bool;
    fn trash_context() -> trash::TrashContext;
    fn path_bytes(path: &Path) -> Cow<'_, [u8]>;
    fn os_str_from_bytes(bytes: &[u8]) -> Cow<'_, OsStr>;
    fn would_run_a_program(path: &Path) -> bool;
    fn preview(path: &Path) -> io::Result<()>;
    fn reveal(path: &Path) -> io::Result<()>;
    fn remove_link(path: &Path, link: FileType) -> io::Result<()>;
}

pub(crate) fn answered_within<T: Send + 'static>(
    limit: Duration,
    question: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    let (answer, answered) = mpsc::channel();
    std::thread::spawn(move || answer.send(question()));
    answered.recv_timeout(limit).ok()
}
