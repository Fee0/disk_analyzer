use std::io;
use std::path::{Path, PathBuf};

use windows_sys::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
};

use super::drives;
use super::handle::wide_nul_terminated;
use crate::hidden::SpaceInfo;
use crate::platform::answered_within;
use crate::volumes::{MountedFileSystem, NETWORK_ANSWER_LIMIT, Volume};

const MAX_LABEL_UNITS: usize = 261;
const MAX_FILE_SYSTEM_NAME_UNITS: usize = 261;
const SYSTEM_DRIVE_VARIABLE: &str = "SystemDrive";

pub(super) fn space_info(path: &Path) -> io::Result<SpaceInfo> {
    let wide = wide_nul_terminated(&path.to_string_lossy());
    let (mut available_to_user, mut total, mut free) = (0u64, 0u64, 0u64);
    if unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut available_to_user, &mut total, &mut free) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(SpaceInfo {
        total,
        used: total.saturating_sub(free),
        available: available_to_user,
        available_important: available_to_user,
    })
}

fn from_wide(buffer: &[u16]) -> String {
    let length = buffer.iter().position(|&unit| unit == 0).unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..length])
}

fn label_and_file_system_of(root: &[u16]) -> (String, String) {
    let mut label = [0u16; MAX_LABEL_UNITS];
    let mut file_system = [0u16; MAX_FILE_SYSTEM_NAME_UNITS];
    let ok = unsafe {
        GetVolumeInformationW(
            root.as_ptr(),
            label.as_mut_ptr(),
            label.len() as u32,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            file_system.as_mut_ptr(),
            file_system.len() as u32,
        )
    };
    if ok == 0 {
        return (String::new(), String::new());
    }
    (from_wide(&label), from_wide(&file_system))
}

struct DriveFacts {
    label: String,
    file_system: String,
    space: SpaceInfo,
}

fn facts_of(root: &str) -> Option<DriveFacts> {
    let space = space_info(Path::new(root)).ok().filter(|s| s.total > 0)?;
    let (label, file_system) = label_and_file_system_of(&wide_nul_terminated(root));
    Some(DriveFacts { label, file_system, space })
}

fn facts_of_a_share_that_may_not_answer(root: &str) -> Option<DriveFacts> {
    let root = root.to_owned();
    answered_within(NETWORK_ANSWER_LIMIT, move || facts_of(&root)).flatten()
}

pub(super) fn system_drive_letter() -> char {
    drives::system_drive_letter(std::env::var(SYSTEM_DRIVE_VARIABLE).ok().as_deref())
}

pub(super) fn mounted_volumes() -> Vec<Volume> {
    drives::letters_in(unsafe { GetLogicalDrives() })
        .filter_map(|letter| {
            let root = drives::root_of(letter);
            let drive_type = unsafe { GetDriveTypeW(wide_nul_terminated(&root).as_ptr()) };
            if !drives::holds_browsable_files(drive_type) {
                return None;
            }
            let is_network = drive_type == drives::DRIVE_REMOTE;
            let facts = if is_network { facts_of_a_share_that_may_not_answer(&root) } else { facts_of(&root) }?;
            let mounted = MountedFileSystem {
                name: drives::display_name(&facts.label, letter),
                mount: PathBuf::from(root),
                file_system: facts.file_system,
                is_network,
            };
            Some(Volume::new(mounted, &facts.space))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_system_drive_is_listed_as_a_local_file_system() {
        let volumes = mounted_volumes();
        let system = volumes.iter().find(|v| v.is_root).unwrap_or_else(|| panic!("{volumes:?}"));
        assert!(system.total > 0 && !system.is_network, "{system:?}");
        assert!(!system.file_system.is_empty(), "Windows names the file system, e.g. NTFS: {system:?}");
    }
}
