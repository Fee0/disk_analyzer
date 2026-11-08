mod dir_info;
#[cfg(windows)]
mod dir_reader;
mod drives;
#[cfg(windows)]
mod handle;
mod launch;
pub(crate) mod paths;
mod protected;
#[cfg(windows)]
mod space_causes;
#[cfg(windows)]
mod volumes;

#[cfg(windows)]
pub(crate) use system::Windows;

#[cfg(windows)]
mod system {
    use std::borrow::Cow;
    use std::ffi::{OsStr, OsString};
    use std::io;
    use std::os::windows::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use windows_sys::Win32::Storage::FileSystem::FILE_READ_ATTRIBUTES;

    use super::handle::{Handle, wide_path};
    use super::{dir_reader, drives, launch, paths, protected, space_causes, volumes};
    use crate::hidden::{SpaceExplanation, SpaceInfo};
    use crate::platform::{DeviceId, DirEntry, Platform, ScanBoundaries, ScanExtent};
    use crate::protection::ProtectedLocations;
    use crate::volumes::Volume;

    pub(crate) struct Windows;

    impl Platform for Windows {
        const PATH_SEPARATOR: u8 = b'\\';
        const TRASH_NAME: &'static str = "Recycle Bin";
        const FULL_DISK_ACCESS_SETTINGS: Option<&'static str> = None;
        const PROTECTED: ProtectedLocations = protected::PROTECTED;
        const SYSTEM_FOLDERS_AT_SYSTEM_ROOT: &'static [&'static str] =
            &["Windows", "Recovery", "System Volume Information", "$Recycle.Bin", "Boot"];
        const HIDDEN_SPACE: SpaceExplanation = space_causes::HIDDEN_SPACE;
        const PURGEABLE_SPACE: SpaceExplanation = space_causes::PURGEABLE_SPACE;
        const PREVIEW_ACTION: &'static str = "Open";
        const REVEAL_ACTION: &'static str = "Show in Explorer";
        const COMMAND_KEY: &'static str = "Ctrl+";
        const RESCAN_SHORTCUT: &'static str = "F5";

        fn read_dir(path: &[u8], visit: impl FnMut(DirEntry<'_>)) -> io::Result<()> {
            dir_reader::read_dir(path, visit)
        }

        fn device_of(path: &Path) -> io::Result<DeviceId> {
            Handle::open_without_following_links(&wide_path(path), FILE_READ_ATTRIBUTES)?.volume()
        }

        fn canonical_root(path: &Path) -> io::Result<PathBuf> {
            Ok(PathBuf::from(path_without_verbatim_prefix(&std::fs::canonicalize(path)?)))
        }

        fn is_system_root(path: &Path) -> bool {
            path == Path::new(&drives::root_of(volumes::system_drive_letter()))
        }

        fn scan_boundaries(_root: &Path, _extent: ScanExtent) -> ScanBoundaries {
            ScanBoundaries::default()
        }

        fn space_info(path: &Path) -> io::Result<SpaceInfo> {
            volumes::space_info(path)
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
            match path.to_string_lossy() {
                Cow::Borrowed(text) => Cow::Borrowed(text.as_bytes()),
                Cow::Owned(text) => Cow::Owned(text.into_bytes()),
            }
        }

        fn os_str_from_bytes(bytes: &[u8]) -> Cow<'_, OsStr> {
            match String::from_utf8_lossy(bytes) {
                Cow::Borrowed(text) => Cow::Borrowed(OsStr::new(text)),
                Cow::Owned(text) => Cow::Owned(OsString::from(text)),
            }
        }

        fn would_run_a_program(path: &Path) -> bool {
            path.file_name().is_some_and(|name| launch::is_program_name(&name.to_string_lossy()))
        }

        fn preview(path: &Path) -> io::Result<()> {
            run_explorer_whose_exit_code_is_1_even_when_it_worked(&launch::quoted_for_explorer(
                &path_without_verbatim_prefix(path),
            ))
        }

        fn reveal(path: &Path) -> io::Result<()> {
            run_explorer_whose_exit_code_is_1_even_when_it_worked(&launch::explorer_select_argument(
                &path_without_verbatim_prefix(path),
            ))
        }
    }

    fn path_without_verbatim_prefix(path: &Path) -> String {
        paths::without_verbatim_prefix(&path.to_string_lossy()).into_owned()
    }

    fn run_explorer_whose_exit_code_is_1_even_when_it_worked(raw_argument: &str) -> io::Result<()> {
        Command::new("explorer.exe").raw_arg(raw_argument).spawn()?.wait().map(drop)
    }
}
