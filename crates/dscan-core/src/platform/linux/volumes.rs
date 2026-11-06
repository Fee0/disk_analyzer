use std::path::{Path, PathBuf};

use rustc_hash::FxHashSet;

use super::{mountinfo, space};
use crate::platform::{DeviceId, ScanBoundaries, ScanExtent, unix};
use crate::volumes::Volume;

const MOUNTINFO: &str = "/proc/self/mountinfo";
const LABELS: &str = "/dev/disk/by-label";
const ROOT_FILE_SYSTEM_NAME: &str = "Root";

fn mounts() -> Vec<mountinfo::Mount> {
    std::fs::read(MOUNTINFO).map(|text| mountinfo::parse(&text)).unwrap_or_default()
}

pub(super) fn scan_boundaries(root: &Path, extent: ScanExtent) -> ScanBoundaries {
    let found = mountinfo::same_file_system_mounts_below(&mounts(), root);
    ScanBoundaries {
        other_devices_of_the_disk: match extent {
            ScanExtent::WholeDisk => found.entered.iter().filter_map(|m| unix::device_of(m).ok()).collect(),
            ScanExtent::Folder => Default::default(),
        },
        duplicate_views: found.duplicate_views.iter().map(|m| unix::path_bytes(m).into_owned()).collect(),
        devices_mounted_on: Default::default(),
    }
}

fn label_of(source: &str) -> Option<String> {
    let device = std::fs::canonicalize(source).ok()?;
    std::fs::read_dir(LABELS).ok()?.flatten().find_map(|link| {
        let points_at_device = std::fs::canonicalize(link.path()).ok()? == device;
        points_at_device.then(|| mountinfo::decode_udev_label(&link.file_name().to_string_lossy()))
    })
}

fn display_name(mount: &mountinfo::Mount) -> String {
    label_of(&mount.source).unwrap_or_else(|| match mount.mount_point.file_name() {
        Some(name) => name.to_string_lossy().into_owned(),
        None => ROOT_FILE_SYSTEM_NAME.to_owned(),
    })
}

pub(super) fn mounted_volumes() -> Vec<Volume> {
    let mounts = mounts();
    let mut seen_devices: FxHashSet<DeviceId> = FxHashSet::default();
    mountinfo::volume_mounts(&mounts, Path::is_dir)
        .into_iter()
        .filter(|m| unix::device_of(&m.mount_point).is_ok_and(|device| seen_devices.insert(device)))
        .filter_map(|m| {
            let space = space::space_info(&m.mount_point).ok().filter(|s| s.total > 0)?;
            let is_root = m.mount_point == Path::new("/");
            Some(Volume::new(display_name(m), PathBuf::from(&m.mount_point), &space, is_root))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_root_file_system_is_a_volume() {
        assert!(mounted_volumes().iter().any(|v| v.is_root && v.mount == Path::new("/")));
    }

    #[test]
    fn a_whole_disk_scan_of_root_never_crosses_into_proc_or_sys() {
        let boundaries = scan_boundaries(Path::new("/"), ScanExtent::WholeDisk);
        for pseudo in ["/proc", "/sys"] {
            let device = unix::device_of(Path::new(pseudo)).unwrap();
            assert!(!boundaries.other_devices_of_the_disk.contains(&device), "{pseudo}");
        }
    }
}
