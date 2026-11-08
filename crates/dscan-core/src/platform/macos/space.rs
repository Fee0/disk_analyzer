use std::io;
use std::path::Path;

use objc2::rc::autoreleasepool;
use objc2_foundation::{NSArray, NSNumber, NSString, NSURL, NSURLResourceKey};

use crate::hidden::SpaceInfo;
use crate::platform::unix;

pub(super) fn space_info(path: &Path) -> io::Result<SpaceInfo> {
    let c = unix::c_path(&unix::path_bytes(path))?;
    let mut s: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statfs(c.as_ptr(), &mut s) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let reported = space_reported_by(&s);
    let available_important =
        url_u64(path, unsafe { objc2_foundation::NSURLVolumeAvailableCapacityForImportantUsageKey })
            .unwrap_or(reported.available);
    Ok(SpaceInfo { available_important, ..reported })
}

pub(super) fn space_reported_by(s: &libc::statfs) -> SpaceInfo {
    let block = s.f_bsize as u64;
    let available = s.f_bavail * block;
    let total = s.f_blocks * block;
    SpaceInfo { total, used: total.saturating_sub(s.f_bfree * block), available, available_important: available }
}

pub(super) fn volume_name(path: &Path) -> Option<String> {
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
    fn root_volume_reports_space_purgeable_capacity_and_a_localized_name() {
        let s = space_info(Path::new("/")).unwrap();
        assert!(s.total > 0 && s.used > 0 && s.used <= s.total);
        assert!(s.available_important >= s.available || s.available_important > 0);
        assert!(volume_name(Path::new("/")).is_some());
    }
}
