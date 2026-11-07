pub(super) fn display_name(fstypename: &str) -> String {
    let name = match fstypename {
        "apfs" => "APFS",
        "hfs" => "Mac OS Extended",
        "msdos" => "FAT",
        "exfat" => "exFAT",
        "ntfs" => "NTFS",
        "smbfs" => "SMB",
        "nfs" => "NFS",
        "afpfs" => "AFP",
        "webdav" => "WebDAV",
        "ftp" => "FTP",
        "cd9660" => "ISO 9660",
        "udf" => "UDF",
        "cddafs" => "Audio CD",
        "macfuse" | "osxfuse" | "fuse-t" | "osxfusefs" => "FUSE",
        other => other,
    };
    name.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_systems_get_the_names_disk_utility_uses() {
        assert_eq!(display_name("apfs"), "APFS");
        assert_eq!(display_name("hfs"), "Mac OS Extended");
        assert_eq!(display_name("smbfs"), "SMB");
        assert_eq!(display_name("zfs"), "zfs", "unknown types keep the name the kernel reports");
    }
}
