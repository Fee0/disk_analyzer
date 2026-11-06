use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use crate::hidden::SpaceInfo;
use crate::platform::unix::c_path;

pub(super) fn space_info(path: &Path) -> io::Result<SpaceInfo> {
    let c = c_path(path.as_os_str().as_bytes())?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let fragment = s.f_frsize as u64;
    let total = s.f_blocks as u64 * fragment;
    let free = s.f_bfree as u64 * fragment;
    let available = s.f_bavail as u64 * fragment;
    Ok(SpaceInfo { total, used: total.saturating_sub(free), available, available_important: available })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_root_file_system_reports_its_size_and_usage() {
        let s = space_info(Path::new("/")).unwrap();
        assert!(s.total > 0 && s.used <= s.total && s.available <= s.total, "{s:?}");
        assert_eq!(s.purgeable(), 0, "Linux has no purgeable space");
    }
}
