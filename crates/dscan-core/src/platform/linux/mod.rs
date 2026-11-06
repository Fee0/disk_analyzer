#[cfg(target_os = "linux")]
mod dir_reader;
mod mountinfo;
mod protected;
#[cfg(target_os = "linux")]
mod space;
#[cfg(target_os = "linux")]
mod volumes;

#[cfg(target_os = "linux")]
pub(crate) use system::Linux;

#[cfg(target_os = "linux")]
mod system {
    use std::borrow::Cow;
    use std::ffi::OsStr;
    use std::io;
    use std::path::{Path, PathBuf};

    use super::{dir_reader, protected, space, volumes};
    use crate::hidden::SpaceInfo;
    use crate::platform::{DeviceId, DirEntry, Platform, WholeDiskBoundaries, unix};
    use crate::protection::ProtectedLocations;
    use crate::volumes::Volume;

    pub(crate) struct Linux;

    impl Platform for Linux {
        const PATH_SEPARATOR: u8 = b'/';
        const TRASH_NAME: &'static str = "Trash";
        const PROTECTED: ProtectedLocations = protected::PROTECTED;
        const SYSTEM_FOLDERS_AT_SYSTEM_ROOT: &'static [&'static str] =
            &["bin", "boot", "etc", "lib", "lib32", "lib64", "libx32", "sbin", "usr"];

        fn read_dir(path: &[u8], visit: impl FnMut(DirEntry<'_>)) -> io::Result<()> {
            dir_reader::read_dir(path, visit)
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

        fn whole_disk_boundaries(root: &Path) -> WholeDiskBoundaries {
            volumes::whole_disk_boundaries(root)
        }

        fn space_info(path: &Path) -> io::Result<SpaceInfo> {
            space::space_info(path)
        }

        fn mounted_volumes() -> Vec<Volume> {
            volumes::mounted_volumes()
        }

        fn has_full_disk_access() -> bool {
            true
        }

        fn trash_context() -> trash::TrashContext {
            trash::TrashContext::default()
        }

        fn path_bytes(path: &Path) -> Cow<'_, [u8]> {
            unix::path_bytes(path)
        }

        fn os_str_from_bytes(bytes: &[u8]) -> Cow<'_, OsStr> {
            unix::os_str_from_bytes(bytes)
        }
    }
}
