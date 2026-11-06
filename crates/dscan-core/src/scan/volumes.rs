//! Mounted volume discovery and APFS container topology.

use std::ffi::CStr;
use std::path::{Path, PathBuf};

use rustc_hash::FxHashSet;

use crate::hidden;

/// A mounted, user-visible APFS volume.
#[derive(Clone, Debug)]
pub struct Volume {
    pub name: String,
    pub mount: PathBuf,
    /// Container capacity in bytes.
    pub total: u64,
    /// Bytes in use in the container (all volumes).
    pub used: u64,
    /// Bytes available, including purgeable space.
    pub available: u64,
    pub is_root: bool,
}

struct Mount {
    on: PathBuf,
    from: String,
    fstype: String,
    flags: u32,
}

fn cstr(buf: &[libc::c_char]) -> String {
    unsafe { CStr::from_ptr(buf.as_ptr()) }.to_string_lossy().into_owned()
}

fn mounts() -> Vec<Mount> {
    let mut ptr: *mut libc::statfs = std::ptr::null_mut();
    let n = unsafe { libc::getmntinfo(&mut ptr, libc::MNT_NOWAIT) };
    if n <= 0 || ptr.is_null() {
        return Vec::new();
    }
    let list = unsafe { std::slice::from_raw_parts(ptr, n as usize) };
    list.iter()
        .map(|s| Mount {
            on: PathBuf::from(cstr(&s.f_mntonname)),
            from: cstr(&s.f_mntfromname),
            fstype: cstr(&s.f_fstypename),
            flags: s.f_flags,
        })
        .collect()
}

/// `/dev/disk3s1s1` -> `disk3`
fn container_of(from: &str) -> Option<&str> {
    let dev = from.strip_prefix("/dev/")?;
    let rest = dev.strip_prefix("disk")?;
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    Some(&dev[..4 + digits])
}

/// Device ids of all mounted volumes that share an APFS container with the
/// volume mounted at `root` (System, Data, Preboot, VM, Update, …).
pub fn container_devices(root: &Path) -> FxHashSet<i32> {
    let all = mounts();
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

/// Paths under `/System/Volumes/Data` that are also reachable through an
/// APFS firmlink from `/`; skipping them avoids counting data twice.
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

/// User-visible APFS volumes: the startup volume plus anything under /Volumes.
pub fn list_volumes() -> Vec<Volume> {
    let mut out: Vec<Volume> = mounts()
        .into_iter()
        .filter(|m| m.fstype == "apfs")
        .filter_map(|m| {
            let is_root = m.on == Path::new("/");
            let visible = m.flags & (libc::MNT_DONTBROWSE as u32) == 0;
            if !(is_root || (visible && m.on.starts_with("/Volumes"))) {
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

/// Can we read a TCC-protected location? (Proxy for Full Disk Access.)
pub fn has_full_disk_access() -> bool {
    let Some(home) = std::env::var_os("HOME") else { return true };
    ["Library/Mail", "Library/Safari"]
        .iter()
        .map(|p| Path::new(&home).join(p))
        .find(|p| p.exists())
        .is_none_or(|p| std::fs::read_dir(p).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_parsing() {
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
    fn root_container_includes_data() {
        let devs = container_devices(Path::new("/"));
        assert!(devs.len() >= 2, "expected System + Data volumes, got {devs:?}");
    }
}
