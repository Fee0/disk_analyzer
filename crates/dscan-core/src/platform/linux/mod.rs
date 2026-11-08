#[cfg(target_os = "linux")]
mod dir_reader;
mod extents;
mod launch;
mod mountinfo;
mod protected;
#[cfg(target_os = "linux")]
mod space;
#[cfg(target_os = "linux")]
mod space_causes;
#[cfg(target_os = "linux")]
mod volumes;

#[cfg(target_os = "linux")]
pub(crate) use system::Linux;

#[cfg(target_os = "linux")]
mod system {
    use std::borrow::Cow;
    use std::ffi::OsStr;
    use std::io::{self, Read};
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    use super::{dir_reader, launch, protected, space, space_causes, volumes};
    use crate::hidden::{SpaceExplanation, SpaceInfo};
    use crate::platform::{DeviceId, DirEntry, Platform, ScanBoundaries, ScanExtent, unix};
    use crate::protection::ProtectedLocations;
    use crate::volumes::Volume;

    const ANY_EXECUTE_PERMISSION: u32 = 0o111;
    const OPENER: &str = "xdg-open";
    const DBUS_SEND: &str = "dbus-send";
    const SHOW_ITEMS_IN_THE_FILE_MANAGER: [&str; 4] =
        ["--session", "--print-reply", "--dest=org.freedesktop.FileManager1", "/org/freedesktop/FileManager1"];
    const SHOW_ITEMS_METHOD: &str = "org.freedesktop.FileManager1.ShowItems";
    const NO_STARTUP_ID: &str = "string:";

    pub(crate) struct Linux;

    fn show_in_the_file_manager(path: &Path) -> io::Result<()> {
        let items = format!("array:string:{}", launch::file_uri(&unix::path_bytes(path)));
        let mut args: Vec<&OsStr> = SHOW_ITEMS_IN_THE_FILE_MANAGER.iter().map(OsStr::new).collect();
        args.extend([OsStr::new(SHOW_ITEMS_METHOD), OsStr::new(&items), OsStr::new(NO_STARTUP_ID)]);
        unix::run_until_done(DBUS_SEND, &args)
    }

    fn open_the_folder_holding(path: &Path) -> io::Result<()> {
        unix::run_until_done(OPENER, &[path.parent().unwrap_or(path).as_os_str()])
    }

    impl Platform for Linux {
        const PATH_SEPARATOR: u8 = b'/';
        const TRASH_NAME: &'static str = "Trash";
        const FULL_DISK_ACCESS_SETTINGS: Option<&'static str> = None;
        const PROTECTED: ProtectedLocations = protected::PROTECTED;
        const SYSTEM_FOLDERS_AT_SYSTEM_ROOT: &'static [&'static str] =
            &["bin", "boot", "etc", "lib", "lib32", "lib64", "libx32", "sbin", "usr"];
        const HIDDEN_SPACE: SpaceExplanation = space_causes::HIDDEN_SPACE;
        const PURGEABLE_SPACE: SpaceExplanation = space_causes::PURGEABLE_SPACE;
        const PREVIEW_ACTION: &'static str = "Open";
        const REVEAL_ACTION: &'static str = "Show in File Manager";
        const COMMAND_KEY: &'static str = "Ctrl+";
        const RESCAN_SHORTCUT: &'static str = "F5";

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

        fn scan_boundaries(root: &Path, extent: ScanExtent) -> ScanBoundaries {
            volumes::scan_boundaries(root, extent)
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

        fn would_run_a_program(path: &Path) -> bool {
            let Ok(metadata) = std::fs::metadata(path) else { return false };
            if metadata.is_dir() {
                return false;
            }
            let executable = metadata.permissions().mode() & ANY_EXECUTE_PERMISSION != 0;
            let name = path.file_name().map(|n| unix::path_bytes(Path::new(n)).into_owned()).unwrap_or_default();
            let mut first_bytes = [0u8; launch::LONGEST_PROGRAM_SIGNATURE];
            match std::fs::File::open(path).and_then(|mut file| file.read(&mut first_bytes)) {
                Ok(read) => launch::looks_like_a_program(&name, &first_bytes[..read], executable),
                Err(_) => executable,
            }
        }

        fn preview(path: &Path) -> io::Result<()> {
            unix::run_until_done(OPENER, &[path.as_os_str()])
        }

        fn reveal(path: &Path) -> io::Result<()> {
            show_in_the_file_manager(path).or_else(|_| open_the_folder_holding(path))
        }

        fn remove_link(path: &Path) -> io::Result<()> {
            unix::remove_link(path)
        }
    }
}
