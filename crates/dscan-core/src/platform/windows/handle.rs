use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, GetFileInformationByHandle, OPEN_EXISTING,
};

use super::paths;
use crate::platform::DeviceId;

pub(super) struct Handle(pub HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

pub(super) fn wide_nul_terminated(path: &str) -> Vec<u16> {
    std::ffi::OsStr::new(path).encode_wide().chain(std::iter::once(0)).collect()
}

pub(super) fn wide_path(path: &Path) -> Vec<u16> {
    wide_nul_terminated(&paths::verbatim(&path.to_string_lossy()))
}

impl Handle {
    pub fn open_without_following_links(wide_path: &[u16], access: u32) -> io::Result<Handle> {
        let handle = unsafe {
            CreateFileW(
                wide_path.as_ptr(),
                access,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE { Err(io::Error::last_os_error()) } else { Ok(Handle(handle)) }
    }

    pub fn volume(&self) -> io::Result<DeviceId> {
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        if unsafe { GetFileInformationByHandle(self.0, &mut info) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(DeviceId(u64::from(info.dwVolumeSerialNumber)))
    }
}
