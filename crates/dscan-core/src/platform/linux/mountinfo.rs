use std::path::{Path, PathBuf};

const OPTIONAL_FIELDS_END: &[u8] = b"-";
const FIELDS_BEFORE_OPTIONAL: usize = 6;
const MOUNT_ROOT_FIELD: usize = 3;
const MOUNT_POINT_FIELD: usize = 4;
const OCTAL_ESCAPE_DIGITS: usize = 3;
const LOOP_DEVICE_PREFIX: &str = "/dev/loop";
const BLOCK_DEVICE_PREFIX: &str = "/dev/";
const READ_ONLY_IMAGE_FILE_SYSTEMS: &[&str] = &["squashfs", "erofs"];
const NETWORK_FILE_SYSTEMS: &[&str] = &[
    "nfs",
    "nfs4",
    "cifs",
    "smb3",
    "smbfs",
    "afs",
    "ceph",
    "fuse.sshfs",
    "fuse.rclone",
    "fuse.s3fs",
    "fuse.gcsfuse",
    "fuse.glusterfs",
    "fuse.ceph-fuse",
];
const FUSE_TYPE: &str = "fuse";
const FUSE_SUBTYPE_PREFIX: &str = "fuse.";
const SYSTEM_MOUNT_LOCATIONS: &[&str] = &["/proc", "/sys", "/dev", "/run", "/tmp", "/var", "/snap"];
const REMOVABLE_MEDIA_UNDER_RUN: &str = "/run/media";

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

    pub fn is_on_a_block_device(&self) -> bool {
        self.source.starts_with(BLOCK_DEVICE_PREFIX)
    }

    fn is_on_a_real_disk(&self) -> bool {
        self.is_on_a_block_device()
            && !self.source.starts_with(LOOP_DEVICE_PREFIX)
            && !READ_ONLY_IMAGE_FILE_SYSTEMS.contains(&self.fs_type.as_str())
    }

    pub fn is_network(&self) -> bool {
        NETWORK_FILE_SYSTEMS.contains(&self.fs_type.as_str())
    }

    fn is_fuse(&self) -> bool {
        self.fs_type == FUSE_TYPE || self.fs_type.starts_with(FUSE_SUBTYPE_PREFIX)
    }

    fn is_in_a_system_location(&self) -> bool {
        let point = &self.mount_point;
        !point.starts_with(REMOVABLE_MEDIA_UNDER_RUN)
            && SYSTEM_MOUNT_LOCATIONS.iter().any(|location| point.starts_with(location))
    }

    fn is_a_user_visible_volume(&self) -> bool {
        let is_share_or_user_mount = self.is_network() || self.is_fuse();
        self.mount_point == Path::new("/")
            || self.is_on_a_real_disk()
            || (is_share_or_user_mount && !self.is_in_a_system_location())
    }

    pub fn file_system_name(&self) -> String {
        let name = match self.fs_type.as_str() {
            "btrfs" => "Btrfs",
            "xfs" => "XFS",
            "zfs" => "ZFS",
            "f2fs" => "F2FS",
            "vfat" | "msdos" => "FAT",
            "exfat" => "exFAT",
            "ntfs" | "ntfs3" => "NTFS",
            "fuseblk" | "fuse" => "FUSE",
            "iso9660" => "ISO 9660",
            "udf" => "UDF",
            "nfs" | "nfs4" => "NFS",
            "cifs" | "smb3" | "smbfs" => "SMB",
            other => other.strip_prefix(FUSE_SUBTYPE_PREFIX).unwrap_or(other),
        };
        name.to_owned()
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
    let scanned_part = scanned_part_of(root_mount, root);
    let mut shown: Vec<&Mount> = vec![&scanned_part];
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

fn scanned_part_of(root_mount: &Mount, root: &Path) -> Mount {
    let inside_the_mount = root.strip_prefix(&root_mount.mount_point).unwrap_or(Path::new(""));
    Mount { fs_root: root_mount.fs_root.join(inside_the_mount), mount_point: root.to_path_buf(), ..root_mount.clone() }
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

pub(super) fn volume_mounts<Facts>(
    mounts: &[Mount],
    facts_of_a_mounted_folder: impl Fn(&Mount) -> Option<Facts>,
) -> Vec<(&Mount, Facts)> {
    let mut candidates: Vec<(&Mount, Facts)> = mounts
        .iter()
        .filter(|m| m.is_a_user_visible_volume())
        .filter_map(|m| Some((m, facts_of_a_mounted_folder(m)?)))
        .collect();
    candidates.sort_by_key(|(m, _)| m.mount_point.components().count());
    let mut volumes: Vec<(&Mount, Facts)> = Vec::new();
    for (mount, facts) in candidates {
        if !volumes.iter().any(|(v, _)| v.shows_the_same_file_system_as(mount)) {
            volumes.push((mount, facts));
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
    fn a_folder_scan_enters_a_bind_mount_of_content_outside_the_folder_and_skips_one_of_content_inside_it() {
        let binds = format!(
            "{BTRFS_LAPTOP}47 22 0:21 /@/srv/data /srv/again rw,relatime shared:37 - btrfs /dev/nvme0n1p2 rw,subvol=/@
"
        );
        let found = same_file_system_mounts_below(&parse(binds.as_bytes()), Path::new("/srv"));
        assert_eq!(found.entered, [PathBuf::from("/srv/shared")], "/var/lib/shared is not inside /srv");
        assert_eq!(found.duplicate_views, [PathBuf::from("/srv/again")], "/srv/data is scanned where it is");
    }

    fn volume_points(mounts: &[Mount], is_folder: impl Fn(&Path) -> bool) -> Vec<&Path> {
        volume_mounts(mounts, |m| is_folder(&m.mount_point).then_some(()))
            .iter()
            .map(|(m, _)| m.mount_point.as_path())
            .collect()
    }

    #[test]
    fn volumes_are_one_per_real_file_system_at_its_shortest_mount_point() {
        let mounts = mounts();
        let volumes = volume_points(&mounts, |_| true);
        assert_eq!(volumes, [Path::new("/"), Path::new("/boot/efi"), Path::new("/media/alex/USB Stick")]);
    }

    const SHARES_AND_USER_MOUNTS: &str = r"50 22 0:60 / /mnt/nas rw,relatime shared:40 - nfs4 nas.local:/export/media rw
51 22 0:61 / /media/alex/Backup rw shared:41 - cifs //server/backup rw,vers=3.1.1
52 41 0:62 / /home/alex/remote rw,nosuid shared:42 - fuse.sshfs alex@host:/srv rw,user_id=1000
53 22 0:63 / /run/user/1000/gvfs rw,nosuid shared:43 - fuse.gvfsd-fuse gvfsd-fuse rw,user_id=1000
54 22 0:64 / /run/user/1000/doc rw,nosuid shared:44 - fuse.portal portal rw,user_id=1000
55 22 0:65 / /var/lib/lxcfs rw,nosuid shared:45 - fuse.lxcfs lxcfs rw
56 22 0:66 / /tmp/.mount_KritaX ro,nosuid shared:46 - fuse.Krita.AppImage Krita.AppImage ro
57 22 0:67 / /run/media/alex/Phone rw,nosuid shared:47 - fuse.jmtpfs jmtpfs rw
58 22 8:33 / /media/alex/Old\040Disk rw shared:48 - fuseblk /dev/sdc1 rw,user_id=0
";

    #[test]
    fn network_shares_and_user_fuse_mounts_are_volumes_but_desktop_plumbing_is_not() {
        let all = parse(format!("{BTRFS_LAPTOP}{SHARES_AND_USER_MOUNTS}").as_bytes());
        let volumes = volume_points(&all, |_| true);
        let offered =
            ["/mnt/nas", "/media/alex/Backup", "/home/alex/remote", "/run/media/alex/Phone", "/media/alex/Old Disk"];
        for shown in offered {
            assert!(volumes.contains(&Path::new(shown)), "{shown} should be offered: {volumes:?}");
        }
        for plumbing in ["/run/user/1000/gvfs", "/run/user/1000/doc", "/var/lib/lxcfs", "/tmp/.mount_KritaX"] {
            assert!(!volumes.contains(&Path::new(plumbing)), "{plumbing} is not a place users keep files");
        }
    }

    #[test]
    fn shares_are_marked_as_network_and_file_systems_get_their_usual_names() {
        let all = parse(SHARES_AND_USER_MOUNTS.as_bytes());
        let at = |point: &str| all.iter().find(|m| m.mount_point == Path::new(point)).unwrap();
        assert!(at("/mnt/nas").is_network() && at("/home/alex/remote").is_network());
        assert!(!at("/media/alex/Old Disk").is_network() && !at("/run/media/alex/Phone").is_network());
        assert_eq!(at("/mnt/nas").file_system_name(), "NFS");
        assert_eq!(at("/media/alex/Backup").file_system_name(), "SMB");
        assert_eq!(at("/home/alex/remote").file_system_name(), "sshfs");
        assert_eq!(mounts()[0].file_system_name(), "Btrfs");
        assert_eq!(mounts()[7].file_system_name(), "exFAT");
    }

    #[test]
    fn mounts_of_single_files_like_dockers_etc_hosts_are_not_volumes() {
        let docker = parse(
            b"1 0 0:50 / / rw - overlay overlay rw\n\
              2 1 254:1 /docker/containers/abc/hosts /etc/hosts rw - ext4 /dev/vda1 rw\n\
              3 1 254:1 /volumes/data /data rw - ext4 /dev/vda1 rw\n",
        );
        let volumes = volume_points(&docker, |p| p != Path::new("/etc/hosts"));
        assert_eq!(volumes, [Path::new("/"), Path::new("/data")]);
    }

    #[test]
    fn udev_labels_decode_hex_escapes() {
        assert_eq!(decode_udev_label(r"My\x20Disk"), "My Disk");
        assert_eq!(decode_udev_label("plain"), "plain");
        assert_eq!(decode_udev_label(r"trailing\x2"), r"trailing\x2");
    }
}
