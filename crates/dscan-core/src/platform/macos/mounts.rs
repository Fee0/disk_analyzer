use std::ffi::CStr;
use std::path::{Path, PathBuf};

use rustc_hash::{FxHashMap, FxHashSet};

use super::{apfs, file_systems, space};
use crate::hidden::SpaceInfo;
use crate::platform::{DeviceId, answered_within, unix};
use crate::volumes::{MountedFileSystem, NETWORK_ANSWER_LIMIT, Volume};

const SPARE_SLOTS_FOR_MOUNTS_APPEARING_BETWEEN_CALLS: usize = 4;
const TCC_PROTECTED_HOME_FOLDERS: [&str; 2] = ["Library/Mail", "Library/Safari"];
const DEFAULT_STARTUP_DISK_NAME: &str = "Macintosh HD";
const USER_VOLUMES_FOLDER: &str = "/Volumes";

struct Mount {
    on: PathBuf,
    from: String,
    fstype: String,
    flags: u32,
    space_when_listed: SpaceInfo,
}

impl Mount {
    fn is_network(&self) -> bool {
        self.flags & (libc::MNT_LOCAL as u32) == 0
    }

    fn is_user_visible(&self) -> bool {
        let browsable = self.flags & (libc::MNT_DONTBROWSE as u32) == 0;
        self.is_root() || (browsable && self.on.starts_with(USER_VOLUMES_FOLDER))
    }

    fn is_root(&self) -> bool {
        self.on == Path::new("/")
    }
}

fn space_of_a_share_that_may_not_answer(m: &Mount) -> SpaceInfo {
    let on = m.on.clone();
    answered_within(NETWORK_ANSWER_LIMIT, move || space::space_info(&on).ok()).flatten().unwrap_or(m.space_when_listed)
}

fn space_reported_by(s: &libc::statfs) -> SpaceInfo {
    let block = s.f_bsize as u64;
    let available = s.f_bavail * block;
    let total = s.f_blocks * block;
    SpaceInfo { total, used: total.saturating_sub(s.f_bfree * block), available, available_important: available }
}

fn cstr(buf: &[libc::c_char]) -> String {
    unsafe { CStr::from_ptr(buf.as_ptr()) }.to_string_lossy().into_owned()
}

fn mounts_via_getfsstat_into_own_buffer() -> Vec<Mount> {
    let list = loop {
        let n = unsafe { libc::getfsstat(std::ptr::null_mut(), 0, libc::MNT_NOWAIT) };
        if n <= 0 {
            return Vec::new();
        }
        let cap = n as usize + SPARE_SLOTS_FOR_MOUNTS_APPEARING_BETWEEN_CALLS;
        let mut buf: Vec<libc::statfs> = Vec::with_capacity(cap);
        let bytes = (cap * std::mem::size_of::<libc::statfs>()) as libc::c_int;
        let got = unsafe { libc::getfsstat(buf.as_mut_ptr(), bytes, libc::MNT_NOWAIT) };
        if got < 0 {
            return Vec::new();
        }
        let buffer_was_large_enough = (got as usize) < cap;
        if buffer_was_large_enough {
            unsafe { buf.set_len(got as usize) };
            break buf;
        }
    };
    list.iter()
        .map(|s| Mount {
            on: PathBuf::from(cstr(&s.f_mntonname)),
            from: cstr(&s.f_mntfromname),
            fstype: cstr(&s.f_fstypename),
            flags: s.f_flags,
            space_when_listed: space_reported_by(s),
        })
        .collect()
}

pub(super) fn other_volumes_of_the_same_container(root: &Path) -> FxHashSet<DeviceId> {
    let all = mounts_via_getfsstat_into_own_buffer();
    let Some(root_from) = all.iter().find(|m| m.on == root).map(|m| m.from.clone()) else {
        return FxHashSet::default();
    };
    let Some(container) = apfs::container_of(&root_from) else {
        return FxHashSet::default();
    };
    all.iter()
        .filter(|m| m.fstype == "apfs" && apfs::container_of(&m.from) == Some(container))
        .filter_map(|m| unix::device_of(&m.on).ok())
        .collect()
}

pub(super) fn devices_mounted_on() -> FxHashMap<Vec<u8>, DeviceId> {
    use std::os::unix::ffi::OsStrExt;
    mounts_via_getfsstat_into_own_buffer()
        .into_iter()
        .filter_map(|m| Some((m.on.as_os_str().as_bytes().to_vec(), unix::device_of(&m.on).ok()?)))
        .collect()
}

pub(super) fn mounted_volumes() -> Vec<Volume> {
    mounts_via_getfsstat_into_own_buffer()
        .into_iter()
        .filter(Mount::is_user_visible)
        .filter_map(|m| {
            let (space, localized_name) = match m.is_network() {
                true => (space_of_a_share_that_may_not_answer(&m), None),
                false => (space::space_info(&m.on).ok()?, space::volume_name(&m.on)),
            };
            let name = localized_name.unwrap_or_else(|| match m.is_root() {
                true => DEFAULT_STARTUP_DISK_NAME.into(),
                false => m.on.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            });
            let is_root = m.is_root();
            let mounted = MountedFileSystem {
                name,
                file_system: file_systems::display_name(&m.fstype),
                is_network: m.is_network(),
                mount: m.on,
            };
            Some(Volume::new(mounted, &space, is_root))
        })
        .collect()
}

pub(super) fn has_full_disk_access() -> bool {
    let Some(home) = std::env::var_os("HOME") else { return true };
    TCC_PROTECTED_HOME_FOLDERS
        .iter()
        .map(|p| Path::new(&home).join(p))
        .find(|p| p.exists())
        .is_none_or(|p| std::fs::read_dir(p).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_volume_listed() {
        let v = mounted_volumes();
        let root = v.iter().find(|v| v.is_root).expect("the startup disk is listed");
        assert!(root.total > 0 && root.used > 0);
        assert!(root.file_system == "APFS" && !root.is_network, "{root:?}");
    }

    #[test]
    fn mount_listing_is_thread_safe_unlike_getmntinfo_which_shares_one_buffer() {
        std::thread::scope(|s| {
            for _ in 0..8 {
                s.spawn(|| {
                    for _ in 0..200 {
                        assert!(
                            mounts_via_getfsstat_into_own_buffer().iter().any(|m| m.on == Path::new("/")),
                            "root mount missing"
                        );
                    }
                });
            }
        });
    }

    #[test]
    fn mount_points_map_to_the_device_mounted_there_not_the_one_they_cover() {
        let mounted = devices_mounted_on();
        let root = unix::device_of(Path::new("/")).unwrap();
        assert_eq!(mounted.get(&b"/"[..]), Some(&root));
        let preboot =
            mounted.get(&b"/System/Volumes/Preboot"[..]).expect("Preboot is mounted on every Apple silicon Mac");
        assert_ne!(
            *preboot, root,
            "a separate volume, although getattrlistbulk lists its folder with the root's device"
        );
    }

    #[test]
    fn the_startup_disk_container_holds_the_system_and_the_data_volume() {
        let devices = other_volumes_of_the_same_container(Path::new("/"));
        assert!(devices.len() >= 2, "expected System + Data volumes, got {devices:?}");
    }
}
