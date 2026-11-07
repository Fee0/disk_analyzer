mod apfs;
#[cfg(target_os = "macos")]
mod bulk;
#[cfg(target_os = "macos")]
mod mounts;
mod protected;
#[cfg(target_os = "macos")]
mod space;

#[cfg(target_os = "macos")]
pub(crate) use system::MacOs;

#[cfg(target_os = "macos")]
mod system {
    use std::borrow::Cow;
    use std::ffi::OsStr;
    use std::io;
    use std::path::{Path, PathBuf};

    use super::{apfs, bulk, mounts, protected, space};
    use crate::hidden::SpaceInfo;
    use crate::platform::{DeviceId, DirEntry, Platform, ScanBoundaries, ScanExtent, unix};
    use crate::protection::ProtectedLocations;
    use crate::volumes::Volume;

    const FIRMLINKS_FILE: &str = "/usr/share/firmlinks";

    pub(crate) struct MacOs;

    impl Platform for MacOs {
        const PATH_SEPARATOR: u8 = b'/';
        const TRASH_NAME: &'static str = "Trash";
        const FULL_DISK_ACCESS_SETTINGS: Option<&'static str> =
            Some("x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles");
        const PROTECTED: ProtectedLocations = protected::PROTECTED;
        const SYSTEM_FOLDERS_AT_SYSTEM_ROOT: &'static [&'static str] =
            &["System", "usr", "bin", "sbin", "private", "cores", "etc", "var"];

        fn read_dir(path: &[u8], visit: impl FnMut(DirEntry<'_>)) -> io::Result<()> {
            bulk::read_dir(path, visit)
        }

        fn device_of(path: &Path) -> io::Result<DeviceId> {
            unix::device_of(path)
        }

        fn canonical_root(path: &Path) -> io::Result<PathBuf> {
            unix::canonical_root(path)
        }

        fn is_system_root(path: &Path) -> bool {
            path == Path::new("/")
        }

        fn scan_boundaries(root: &Path, extent: ScanExtent) -> ScanBoundaries {
            ScanBoundaries {
                other_devices_of_the_disk: match extent {
                    ScanExtent::WholeDisk => mounts::other_volumes_of_the_same_container(root),
                    ScanExtent::Folder => Default::default(),
                },
                duplicate_views: apfs::firmlinked_duplicate_paths(
                    std::fs::read(FIRMLINKS_FILE).ok().as_deref(),
                    &unix::path_bytes(root),
                ),
                devices_mounted_on: mounts::devices_mounted_on(),
            }
        }

        fn space_info(path: &Path) -> io::Result<SpaceInfo> {
            space::space_info(path)
        }

        fn mounted_volumes() -> Vec<Volume> {
            mounts::mounted_volumes()
        }

        fn has_full_disk_access() -> bool {
            mounts::has_full_disk_access()
        }

        fn trash_context() -> trash::TrashContext {
            use trash::macos::{DeleteMethod, TrashContextExtMacos};
            let mut context_needing_no_finder_automation_permission = trash::TrashContext::default();
            context_needing_no_finder_automation_permission.set_delete_method(DeleteMethod::NsFileManager);
            context_needing_no_finder_automation_permission
        }

        fn path_bytes(path: &Path) -> Cow<'_, [u8]> {
            unix::path_bytes(path)
        }

        fn os_str_from_bytes(bytes: &[u8]) -> Cow<'_, OsStr> {
            unix::os_str_from_bytes(bytes)
        }
    }
}
