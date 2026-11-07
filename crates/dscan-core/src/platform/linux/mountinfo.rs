use std::path::{Path, PathBuf};

const OPTIONAL_FIELDS_END: &[u8] = b"-";
const FIELDS_BEFORE_OPTIONAL: usize = 6;
const MOUNT_ROOT_FIELD: usize = 3;
const MOUNT_POINT_FIELD: usize = 4;
const OCTAL_ESCAPE_DIGITS: usize = 3;
const LOOP_DEVICE_PREFIX: &str = "/dev/loop";
const BLOCK_DEVICE_PREFIX: &str = "/dev/";
const READ_ONLY_IMAGE_FILE_SYSTEMS: &[&str] = &["squashfs", "erofs"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Mount {
    pub fs_root: PathBuf,
    pub mount_point: PathBuf,
    pub fs_type: String,
    pub source: String,
}

impl Mount {
    fn shows_the_same_file_system_as(&self, other: &Mount) -> bool {
        self.source == other.source && self.fs_type == other.fs_type
    }

    fn is_on_a_real_disk(&self) -> bool {
        self.source.starts_with(BLOCK_DEVICE_PREFIX)
            && !self.source.starts_with(LOOP_DEVICE_PREFIX)
            && !READ_ONLY_IMAGE_FILE_SYSTEMS.contains(&self.fs_type.as_str())
    }
}

pub(super) fn parse(mountinfo: &[u8]) -> Vec<Mount> {
    mountinfo.split(|&b| b == b'\n').filter_map(parse_line).collect()
}

fn parse_line(line: &[u8]) -> Option<Mount> {
    let fields: Vec<&[u8]> = line.split(|&b| b == b' ').filter(|f| !f.is_empty()).collect();
    let separator = fields.iter().skip(FIELDS_BEFORE_OPTIONAL).position(|&f| f == OPTIONAL_FIELDS_END)?;
    let after_separator = FIELDS_BEFORE_OPTIONAL + separator + 1;
    Some(Mount {
        fs_root: unescape(fields.get(MOUNT_ROOT_FIELD)?),
        mount_point: unescape(fields.get(MOUNT_POINT_FIELD)?),
        fs_type: String::from_utf8_lossy(fields.get(after_separator)?).into_owned(),
        source: String::from_utf8_lossy(&unescape_bytes(fields.get(after_separator + 1)?)).into_owned(),
    })
}

fn unescape(field: &[u8]) -> PathBuf {
    path_from_bytes(unescape_bytes(field))
}

#[cfg(unix)]
fn path_from_bytes(bytes: Vec<u8>) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;
    PathBuf::from(std::ffi::OsString::from_vec(bytes))
}

#[cfg(not(unix))]
fn path_from_bytes(bytes: Vec<u8>) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(&bytes).into_owned())
}

fn unescape_bytes(field: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(field.len());
    let mut i = 0;
    while i < field.len() {
        let octal = field.get(i + 1..i + 1 + OCTAL_ESCAPE_DIGITS);
        match octal.and_then(|digits| u8::from_str_radix(std::str::from_utf8(digits).ok()?, 8).ok()) {
            Some(byte) if field[i] == b'\\' => {
                out.push(byte);
                i += 1 + OCTAL_ESCAPE_DIGITS;
            }
            _ => {
                out.push(field[i]);
                i += 1;
            }
        }
    }
    out
}

fn mount_holding(mounts: &[Mount], path: &Path) -> Option<usize> {
    mounts
        .iter()
        .enumerate()
        .filter(|(_, m)| path.starts_with(&m.mount_point))
        .max_by_key(|&(index, m)| (m.mount_point.components().count(), index))
        .map(|(index, _)| index)
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct SameFileSystemMounts {
    pub entered: Vec<PathBuf>,
    pub duplicate_views: Vec<PathBuf>,
}

pub(super) fn same_file_system_mounts_below(mounts: &[Mount], root: &Path) -> SameFileSystemMounts {
    let mut found = SameFileSystemMounts::default();
    let Some(root_index) = mount_holding(mounts, root) else { return found };
    let root_mount = &mounts[root_index];
    let mut shown: Vec<&Mount> = vec![root_mount];
    for (index, mount) in mounts.iter().enumerate() {
        let below_root = mount.mount_point.starts_with(root) && mount.mount_point != root;
        if index == root_index || !below_root || !mount.shows_the_same_file_system_as(root_mount) {
            continue;
        }
        if shown.iter().any(|s| mount.fs_root.starts_with(&s.fs_root)) {
            found.duplicate_views.push(mount.mount_point.clone());
        } else {
            shown.push(mount);
            found.entered.push(mount.mount_point.clone());
        }
    }
    found.duplicate_views.extend(places_inside_a_mount_that_show_another_shown_mount(&shown));
    found
}

fn places_inside_a_mount_that_show_another_shown_mount(shown: &[&Mount]) -> Vec<PathBuf> {
    let mut places = Vec::new();
    for outer in shown {
        for inner in shown {
            if let Ok(inside) = inner.fs_root.strip_prefix(&outer.fs_root)
                && !inside.as_os_str().is_empty()
            {
                places.push(outer.mount_point.join(inside));
            }
        }
    }
    places
}

pub(super) fn volume_mounts(mounts: &[Mount], is_directory: impl Fn(&Path) -> bool) -> Vec<&Mount> {
    let mut candidates: Vec<&Mount> = mounts
        .iter()
        .filter(|m| m.mount_point == Path::new("/") || m.is_on_a_real_disk())
        .filter(|m| is_directory(&m.mount_point))
        .collect();
    candidates.sort_by_key(|m| m.mount_point.components().count());
    let mut volumes: Vec<&Mount> = Vec::new();
    for mount in candidates {
        if !volumes.iter().any(|v| v.shows_the_same_file_system_as(mount)) {
            volumes.push(mount);
        }
    }
    volumes
}

pub(super) fn decode_udev_label(escaped: &str) -> String {
    let mut out = Vec::with_capacity(escaped.len());
    let bytes = escaped.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes.get(i + 2..i + 4).and_then(|h| u8::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok());
        match hex {
            Some(byte) if bytes[i..].starts_with(b"\\x") => {
                out.push(byte);
                i += 4;
            }
            _ => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const BTRFS_LAPTOP: &str = "\
22 1 0:21 /@ / rw,relatime shared:1 - btrfs /dev/nvme0n1p2 rw,ssd,subvol=/@
23 22 0:22 / /proc rw,nosuid shared:5 - proc proc rw
24 22 0:5 / /dev rw,nosuid shared:2 - devtmpfs devtmpfs rw,size=8000k
40 22 259:1 / /boot/efi rw,relatime shared:30 - vfat /dev/nvme0n1p1 rw
41 22 0:21 /@home /home rw,relatime shared:31 - btrfs /dev/nvme0n1p2 rw,ssd,subvol=/@home
42 22 0:21 /@/var/lib/shared /srv/shared rw,relatime shared:32 - btrfs /dev/nvme0n1p2 rw,subvol=/@
43 22 7:0 / /snap/core22/1380 ro,nodev shared:33 - squashfs /dev/loop0 ro
44 41 8:17 / /media/alex/USB\\040Stick rw,nosuid shared:34 - exfat /dev/sdb1 rw
45 22 0:40 / /tmp rw shared:35 - tmpfs tmpfs rw
";

    fn mounts() -> Vec<Mount> {
        parse(BTRFS_LAPTOP.as_bytes())
    }

    #[test]
    fn parses_fields_after_a_variable_number_of_optional_fields_and_decodes_octal_escapes() {
        let m = mounts();
        assert_eq!(m.len(), 9);
        assert_eq!(
            m[0],
            Mount {
                fs_root: "/@".into(),
                mount_point: "/".into(),
                fs_type: "btrfs".into(),
                source: "/dev/nvme0n1p2".into(),
            }
        );
        assert_eq!(m[7].mount_point, Path::new("/media/alex/USB Stick"));
        assert_eq!(parse(b"garbage line\n\n").len(), 0);
    }

    #[test]
    fn a_whole_disk_scan_enters_other_subvolumes_of_its_file_system_and_skips_bind_mounts_of_what_it_already_sees() {
        let found = same_file_system_mounts_below(&mounts(), Path::new("/"));
        assert_eq!(found.entered, [PathBuf::from("/home")], "@home is separate content on the same disk");
        assert_eq!(found.duplicate_views, [PathBuf::from("/srv/shared")], "a bind mount of /@/var/lib/shared");
    }

    #[test]
    fn a_mount_of_the_whole_btrfs_pool_is_entered_but_subvolumes_already_shown_elsewhere_are_skipped_inside_it() {
        let pool = format!(
            "{BTRFS_LAPTOP}46 22 0:21 / /mnt/pool rw,relatime shared:36 - btrfs /dev/nvme0n1p2 rw,subvol=/
"
        );
        let found = same_file_system_mounts_below(&parse(pool.as_bytes()), Path::new("/"));
        assert_eq!(
            found.entered,
            [PathBuf::from("/home"), PathBuf::from("/mnt/pool")],
            "the pool holds snapshots and other subvolumes"
        );
        for already_shown in ["/mnt/pool/@", "/mnt/pool/@home"] {
            assert!(
                found.duplicate_views.contains(&PathBuf::from(already_shown)),
                "{already_shown} is / or /home again"
            );
        }
    }

    #[test]
    fn other_partitions_and_pseudo_file_systems_are_never_part_of_the_root_scan() {
        let found = same_file_system_mounts_below(&mounts(), Path::new("/"));
        for elsewhere in ["/boot/efi", "/proc", "/dev", "/tmp", "/media/alex/USB Stick"] {
            assert!(!found.entered.contains(&PathBuf::from(elsewhere)), "{elsewhere}");
        }
    }

    #[test]
    fn a_scan_of_a_folder_inside_a_mount_uses_the_mount_holding_it() {
        let found = same_file_system_mounts_below(&mounts(), Path::new("/home/alex"));
        assert_eq!(found, SameFileSystemMounts::default());
    }

    #[test]
    fn volumes_are_one_per_real_file_system_at_its_shortest_mount_point() {
        let mounts = mounts();
        let volumes: Vec<&Path> = volume_mounts(&mounts, |_| true).iter().map(|m| m.mount_point.as_path()).collect();
        assert_eq!(volumes, [Path::new("/"), Path::new("/boot/efi"), Path::new("/media/alex/USB Stick")]);
    }

    #[test]
    fn mounts_of_single_files_like_dockers_etc_hosts_are_not_volumes() {
        let docker = parse(
            b"1 0 0:50 / / rw - overlay overlay rw\n\
              2 1 254:1 /docker/containers/abc/hosts /etc/hosts rw - ext4 /dev/vda1 rw\n\
              3 1 254:1 /volumes/data /data rw - ext4 /dev/vda1 rw\n",
        );
        let volumes: Vec<&Path> =
            volume_mounts(&docker, |p| p != Path::new("/etc/hosts")).iter().map(|m| m.mount_point.as_path()).collect();
        assert_eq!(volumes, [Path::new("/"), Path::new("/data")]);
    }

    #[test]
    fn udev_labels_decode_hex_escapes() {
        assert_eq!(decode_udev_label(r"My\x20Disk"), "My Disk");
        assert_eq!(decode_udev_label("plain"), "plain");
        assert_eq!(decode_udev_label(r"trailing\x2"), r"trailing\x2");
    }
}
