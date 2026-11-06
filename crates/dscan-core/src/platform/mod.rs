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
use std::io;
use std::path::{Path, PathBuf};

use rustc_hash::{FxHashMap, FxHashSet};

use crate::hidden::SpaceInfo;
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
    pub logical: u64,
    pub hard_link_id: Option<u64>,
    pub shared_data_stream_id: Option<u64>,
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
    const PROTECTED: ProtectedLocations;
    const SYSTEM_FOLDERS_AT_SYSTEM_ROOT: &'static [&'static str];

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
}
