use std::ffi::CString;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use objc2::rc::autoreleasepool;
use objc2_foundation::{NSArray, NSNumber, NSString, NSURL, NSURLResourceKey};

#[derive(Clone, Copy, Debug, Default)]
pub struct SpaceInfo {
    pub total: u64,
    pub used: u64,
    pub available: u64,
    pub available_important: u64,
}

impl SpaceInfo {
    pub fn purgeable(&self) -> u64 {
        self.available_important.saturating_sub(self.available)
    }

    pub fn split_unscanned(&self, scanned: u64) -> (u64, u64) {
        let unscanned = self.used.saturating_sub(scanned);
        let purgeable_outside_scan = self.purgeable().min(unscanned);
        let hidden = unscanned - purgeable_outside_scan;
        (hidden, purgeable_outside_scan)
    }
}

pub fn space_info(path: &Path) -> io::Result<SpaceInfo> {
    let c = CString::new(path.as_os_str().as_bytes()).map_err(io::Error::other)?;
    let mut s: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statfs(c.as_ptr(), &mut s) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let bs = s.f_bsize as u64;
    let total = s.f_blocks * bs;
    let free = s.f_bfree * bs;
    let available = s.f_bavail * bs;
    let important = url_u64(path, unsafe { objc2_foundation::NSURLVolumeAvailableCapacityForImportantUsageKey })
        .unwrap_or(available);
    Ok(SpaceInfo { total, used: total.saturating_sub(free), available, available_important: important })
}

pub fn volume_name(path: &Path) -> Option<String> {
    let key = unsafe { objc2_foundation::NSURLVolumeLocalizedNameKey };
    url_resource(path, key, |s: &NSString| s.to_string())
}

fn url_u64(path: &Path, key: &NSURLResourceKey) -> Option<u64> {
    url_resource(path, key, |n: &NSNumber| n.unsignedLongLongValue())
}

fn url_resource<T: objc2::Message + objc2::DowncastTarget, R>(
    path: &Path,
    key: &NSURLResourceKey,
    f: impl FnOnce(&T) -> R,
) -> Option<R> {
    autoreleasepool(|_| {
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        let dict = url.resourceValuesForKeys_error(&NSArray::from_slice(&[key])).ok()?;
        let value = dict.objectForKey(key)?.downcast::<T>().ok()?;
        Some(f(&value))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_volume_reports_space_and_a_localized_name() {
        let s = space_info(Path::new("/")).unwrap();
        assert!(s.total > 0 && s.used > 0 && s.used <= s.total);
        assert!(s.available_important >= s.available || s.available_important > 0);
        assert!(volume_name(Path::new("/")).is_some());
    }

    #[test]
    fn split_unscanned_clamps_purgeable_to_unscanned_because_the_scan_may_see_purgeable_files() {
        let s = SpaceInfo { total: 100, used: 80, available: 10, available_important: 15 };
        assert_eq!(s.split_unscanned(60), (15, 5), "20 unscanned: (hidden 15, purgeable 5)");
        assert_eq!(s.split_unscanned(78), (0, 2), "only 2 unscanned, so purgeable is clamped to 2");
        assert_eq!(s.split_unscanned(90), (0, 0), "scanned more than used: nothing unscanned");
    }
}
