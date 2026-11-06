use std::path::PathBuf;

use crate::hidden::SpaceInfo;
use crate::platform::{Current, Platform};

#[derive(Clone, Debug)]
pub struct Volume {
    pub name: String,
    pub mount: PathBuf,
    pub total: u64,
    pub used: u64,
    pub available: u64,
    pub is_root: bool,
}

impl Volume {
    pub(crate) fn new(name: String, mount: PathBuf, space: &SpaceInfo, is_root: bool) -> Self {
        Volume {
            name,
            mount,
            total: space.total,
            used: space.used,
            available: space.available_important.max(space.available),
            is_root,
        }
    }
}

pub fn list_volumes() -> Vec<Volume> {
    let mut volumes = Current::mounted_volumes();
    volumes.sort_by_key(|v| (!v.is_root, v.name.clone()));
    volumes
}

pub fn has_full_disk_access() -> bool {
    Current::has_full_disk_access()
}

pub const TRASH_NAME: &str = Current::TRASH_NAME;

#[cfg(test)]
mod tests {
    #[test]
    fn the_system_volume_is_listed_first_with_its_space() {
        let volumes = super::list_volumes();
        let first = volumes.first().expect("at least the system volume");
        assert!(first.is_root && first.total > 0 && first.used > 0 && first.used <= first.total, "{first:?}");
    }
}
