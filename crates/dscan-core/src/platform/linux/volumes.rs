use std::path::{Path, PathBuf};

use rustc_hash::FxHashSet;

use super::{mountinfo, space};
use crate::hidden::SpaceInfo;
use crate::platform::{DeviceId, ScanBoundaries, ScanExtent, asked_of_a_share_that_may_not_answer, unix};
use crate::volumes::{MountedFileSystem, Volume};

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
        folders_never_entered: found.duplicate_views.iter().map(|m| unix::path_bytes(m).into_owned()).collect(),
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
    let label = mount.is_on_a_block_device().then(|| label_of(&mount.source)).flatten();
    label.unwrap_or_else(|| match mount.mount_point.file_name() {
        Some(name) => name.to_string_lossy().into_owned(),
        None => ROOT_FILE_SYSTEM_NAME.to_owned(),
    })
}

struct FolderFacts {
    device: DeviceId,
    space: SpaceInfo,
}

fn facts_of(mount_point: &Path) -> Option<FolderFacts> {
    if !mount_point.is_dir() {
        return None;
    }
    let device = unix::device_of(mount_point).ok()?;
    let space = space::space_info(mount_point).ok().filter(|s| s.total > 0)?;
    Some(FolderFacts { device, space })
}

fn facts_of_a_share_that_may_not_answer(mount_point: &Path) -> Option<FolderFacts> {
    asked_of_a_share_that_may_not_answer(mount_point.to_owned(), |mount_point| facts_of(mount_point))
}

pub(super) fn mounted_volumes() -> Vec<Volume> {
    let mounts = mounts();
    let mut seen_devices: FxHashSet<DeviceId> = FxHashSet::default();
    let facts = |m: &mountinfo::Mount| match m.is_network() {
        true => facts_of_a_share_that_may_not_answer(&m.mount_point),
        false => facts_of(&m.mount_point),
    };
    mountinfo::volume_mounts(&mounts, facts)
        .into_iter()
        .filter(|(_, facts)| seen_devices.insert(facts.device))
        .map(|(m, facts)| {
            let mounted = MountedFileSystem {
                name: display_name(m),
                mount: PathBuf::from(&m.mount_point),
                file_system: m.file_system_name(),
                is_network: m.is_network(),
            };
            Volume::new(mounted, &facts.space)
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
