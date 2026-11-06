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

fn is_same_or_inside(path: &[u8], folder: &[u8]) -> bool {
    let folder = folder.strip_suffix(b"/").unwrap_or(folder);
    path.strip_prefix(folder).is_some_and(|rest| rest.is_empty() || rest.starts_with(b"/"))
}

pub(super) fn firmlinked_duplicate_paths(firmlinks_file: Option<&[u8]>, scan_root: &[u8]) -> FxHashSet<Vec<u8>> {
    let Some(content) = firmlinks_file else {
        let scan_reaches_the_data_volume_twice = scan_root == b"/";
        return match scan_reaches_the_data_volume_twice {
            true => FxHashSet::from_iter([DATA_VOLUME_MOUNT.to_vec()]),
            false => FxHashSet::default(),
        };
    };
    content
        .split(|&b| b == b'\n')
        .filter_map(|line| {
            let tab = line.iter().position(|&b| b == FIRMLINK_TARGET_SEPARATOR)?;
            let (source, target) = (&line[..tab], &line[tab + 1..]);
            let scan_reaches_the_firmlink = is_same_or_inside(source, scan_root);
            (!target.is_empty() && scan_reaches_the_firmlink).then(|| [DATA_VOLUME_MOUNT, b"/", target].concat())
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

    const FIRMLINKS: &[u8] = b"/Applications\tApplications\n/Users\tUsers\n/usr/local\tusr/local\n";

    #[test]
    fn firmlinked_folders_are_skipped_on_the_data_volume_because_the_walk_reaches_them_through_the_firmlink() {
        let skipped = firmlinked_duplicate_paths(Some(FIRMLINKS), b"/");
        assert_eq!(skipped.len(), 3);
        assert!(skipped.contains(&b"/System/Volumes/Data/Users"[..]));
        assert!(skipped.contains(&b"/System/Volumes/Data/usr/local"[..]));
    }

    #[test]
    fn a_firmlink_target_counts_as_a_duplicate_only_when_the_scan_also_reaches_the_firmlink() {
        let from_usr = firmlinked_duplicate_paths(Some(FIRMLINKS), b"/usr");
        assert_eq!(from_usr, FxHashSet::from_iter([b"/System/Volumes/Data/usr/local".to_vec()]));
        assert!(firmlinked_duplicate_paths(Some(FIRMLINKS), b"/System/Volumes/Data").is_empty());
        assert!(firmlinked_duplicate_paths(Some(FIRMLINKS), b"/Users/alex").is_empty());
    }

    #[test]
    fn inside_compares_whole_components() {
        assert!(is_same_or_inside(b"/usr/local", b"/usr"));
        assert!(is_same_or_inside(b"/usr", b"/usr/"));
        assert!(is_same_or_inside(b"/Users", b"/"));
        assert!(!is_same_or_inside(b"/usrx", b"/usr"));
    }

    #[test]
    fn without_a_firmlinks_file_a_root_scan_skips_the_whole_data_volume_rather_than_counting_it_twice() {
        let skipped = firmlinked_duplicate_paths(None, b"/");
        assert_eq!(skipped, FxHashSet::from_iter([b"/System/Volumes/Data".to_vec()]));
        assert!(firmlinked_duplicate_paths(None, b"/Users").is_empty());
    }
}
