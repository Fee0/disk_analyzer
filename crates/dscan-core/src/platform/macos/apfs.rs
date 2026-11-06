use rustc_hash::FxHashSet;

const DATA_VOLUME_MOUNT: &[u8] = b"/System/Volumes/Data";
const FIRMLINK_TARGET_SEPARATOR: u8 = b'\t';

pub(super) fn container_of(mounted_from: &str) -> Option<&str> {
    let dev = mounted_from.strip_prefix("/dev/")?;
    let disk_prefix = "disk";
    let rest = dev.strip_prefix(disk_prefix)?;
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    Some(&dev[..disk_prefix.len() + digits])
}

pub(super) fn firmlinked_duplicate_paths(firmlinks_file: Option<&[u8]>) -> FxHashSet<Vec<u8>> {
    let Some(content) = firmlinks_file else {
        return FxHashSet::from_iter([DATA_VOLUME_MOUNT.to_vec()]);
    };
    content
        .split(|&b| b == b'\n')
        .filter_map(|line| {
            let tab = line.iter().position(|&b| b == FIRMLINK_TARGET_SEPARATOR)?;
            let target = &line[tab + 1..];
            (!target.is_empty()).then(|| [DATA_VOLUME_MOUNT, b"/", target].concat())
        })
        .collect()
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
    fn firmlinked_folders_are_skipped_on_the_data_volume_because_the_walk_reaches_them_through_the_firmlink() {
        let firmlinks = b"/Applications\tApplications\n/Users\tUsers\n/Library\tLibrary\n";
        let skipped = firmlinked_duplicate_paths(Some(firmlinks));
        assert_eq!(skipped.len(), 3);
        assert!(skipped.contains(&b"/System/Volumes/Data/Users"[..]));
    }

    #[test]
    fn without_a_firmlinks_file_the_whole_data_volume_is_skipped_rather_than_counted_twice() {
        assert_eq!(firmlinked_duplicate_paths(None), FxHashSet::from_iter([b"/System/Volumes/Data".to_vec()]));
    }
}
