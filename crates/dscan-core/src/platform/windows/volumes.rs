use std::io;
use std::path::{Path, PathBuf};

use windows_sys::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
};

use super::drives;
use super::handle::wide_nul_terminated;
use crate::hidden::SpaceInfo;
use crate::volumes::Volume;

const MAX_LABEL_UNITS: usize = 261;
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

fn label_of(root: &[u16]) -> String {
    let mut label = [0u16; MAX_LABEL_UNITS];
    let ok = unsafe {
        GetVolumeInformationW(
            root.as_ptr(),
            label.as_mut_ptr(),
            label.len() as u32,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
        )
    };
    if ok == 0 {
        return String::new();
    }
    let length = label.iter().position(|&unit| unit == 0).unwrap_or(label.len());
    String::from_utf16_lossy(&label[..length])
}

pub(super) fn system_drive_letter() -> char {
    drives::system_drive_letter(std::env::var(SYSTEM_DRIVE_VARIABLE).ok().as_deref())
}

pub(super) fn mounted_volumes() -> Vec<Volume> {
    let system_drive = system_drive_letter();
    drives::letters_in(unsafe { GetLogicalDrives() })
        .filter_map(|letter| {
            let root = drives::root_of(letter);
            let wide_root = wide_nul_terminated(&root);
            if !drives::holds_browsable_files(unsafe { GetDriveTypeW(wide_root.as_ptr()) }) {
                return None;
            }
            let space = space_info(Path::new(&root)).ok().filter(|s| s.total > 0)?;
            let name = drives::display_name(&label_of(&wide_root), letter);
            Some(Volume::new(name, PathBuf::from(root), &space, letter == system_drive))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_system_drive_is_listed() {
        assert!(mounted_volumes().iter().any(|v| v.is_root && v.total > 0), "{:?}", mounted_volumes());
    }
}
