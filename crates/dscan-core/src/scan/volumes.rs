use std::ffi::CStr;
use std::path::{Path, PathBuf};

use rustc_hash::FxHashSet;

use crate::hidden;

#[derive(Clone, Debug)]
pub struct Volume {
    pub name: String,
    pub mount: PathBuf,
    pub total: u64,
    pub used: u64,
    pub available: u64,
    pub is_root: bool,
}

const SPARE_SLOTS_FOR_MOUNTS_APPEARING_BETWEEN_CALLS: usize = 4;
const TCC_PROTECTED_HOME_FOLDERS: [&str; 2] = ["Library/Mail", "Library/Safari"];

struct Mount {
    on: PathBuf,
    from: String,
    fstype: String,
    flags: u32,
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
        })
        .collect()
}

fn container_of(from: &str) -> Option<&str> {
    let dev = from.strip_prefix("/dev/")?;
    let disk_prefix = "disk";
    let rest = dev.strip_prefix(disk_prefix)?;
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    Some(&dev[..disk_prefix.len() + digits])
}

pub fn container_devices(root: &Path) -> FxHashSet<i32> {
    let all = mounts_via_getfsstat_into_own_buffer();
    let Some(root_from) = all.iter().find(|m| m.on == root).map(|m| m.from.clone()) else {
        return FxHashSet::default();
    };
    let Some(container) = container_of(&root_from) else {
        return FxHashSet::default();
    };
    all.iter()
        .filter(|m| m.fstype == "apfs" && container_of(&m.from) == Some(container))
        .filter_map(|m| super::dev_of(&m.on).ok())
        .collect()
}

pub fn firmlink_skip_paths() -> FxHashSet<Vec<u8>> {
    let mut out = FxHashSet::default();
    match std::fs::read("/usr/share/firmlinks") {
        Ok(content) => {
            for line in content.split(|&b| b == b'\n') {
                if let Some(tab) = line.iter().position(|&b| b == b'\t') {
                    let target = &line[tab + 1..];
                    if !target.is_empty() {
                        let mut p = b"/System/Volumes/Data/".to_vec();
                        p.extend_from_slice(target);
                        out.insert(p);
                    }
                }
            }
        }
        Err(_) => {
            out.insert(b"/System/Volumes/Data".to_vec());
        }
    }
    out
}

pub fn list_volumes() -> Vec<Volume> {
    let mut out: Vec<Volume> = mounts_via_getfsstat_into_own_buffer()
        .into_iter()
        .filter(|m| m.fstype == "apfs")
        .filter_map(|m| {
            let is_root = m.on == Path::new("/");
            let browsable = m.flags & (libc::MNT_DONTBROWSE as u32) == 0;
            let user_visible = is_root || (browsable && m.on.starts_with("/Volumes"));
            if !user_visible {
                return None;
            }
            let space = hidden::space_info(&m.on).ok()?;
            let name = hidden::volume_name(&m.on).unwrap_or_else(|| {
                if is_root {
                    "Macintosh HD".into()
                } else {
                    m.on.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
                }
            });
            Some(Volume {
                name,
                mount: m.on,
                total: space.total,
                used: space.used,
                available: space.available_important.max(space.available),
                is_root,
            })
        })
        .collect();
    out.sort_by_key(|v| (!v.is_root, v.name.clone()));
    out
}

pub fn has_full_disk_access() -> bool {
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
    fn container_of_takes_the_whole_disk_from_a_volume_device() {
        assert_eq!(container_of("/dev/disk3s1s1"), Some("disk3"));
        assert_eq!(container_of("/dev/disk12s5"), Some("disk12"));
        assert_eq!(container_of("devfs"), None);
        assert_eq!(container_of("com.apple.TimeMachine.x@/dev/disk3s5"), None);
    }

    #[test]
    fn root_volume_listed() {
        let v = list_volumes();
        assert!(v.iter().any(|v| v.is_root && v.total > 0 && v.used > 0));
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
    fn root_container_includes_data() {
        let devs = container_devices(Path::new("/"));
        assert!(devs.len() >= 2, "expected System + Data volumes, got {devs:?}");
    }
}
