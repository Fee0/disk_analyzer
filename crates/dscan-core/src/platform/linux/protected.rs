use crate::platform::unix_syntax::UNIX_PATHS;
use crate::protection::ProtectedLocations;

pub(crate) const PROTECTED: ProtectedLocations = ProtectedLocations {
    syntax: UNIX_PATHS,
    exact: &[
        "/",
        "/bin",
        "/boot",
        "/dev",
        "/etc",
        "/home",
        "/lib",
        "/lib32",
        "/lib64",
        "/libx32",
        "/media",
        "/mnt",
        "/opt",
        "/proc",
        "/root",
        "/run",
        "/sbin",
        "/snap",
        "/srv",
        "/sys",
        "/tmp",
        "/usr",
        "/usr/local",
        "/var",
        "/var/cache",
        "/var/lib",
        "/var/log",
        "/var/tmp",
        "/home/*",
        "/home/*/Desktop",
        "/home/*/Documents",
        "/home/*/Downloads",
        "/home/*/Music",
        "/home/*/Pictures",
        "/home/*/Public",
        "/home/*/Templates",
        "/home/*/Videos",
        "/home/*/.config",
        "/home/*/.local",
        "/home/*/.local/share",
        "/home/*/.local/share/Trash",
        "/media/*",
        "/media/*/*",
        "/mnt/*",
        "/run/media/*/*",
    ],
    trees: &[
        "/bin",
        "/boot",
        "/dev",
        "/etc",
        "/lib",
        "/lib32",
        "/lib64",
        "/libx32",
        "/proc",
        "/run",
        "/sbin",
        "/snap",
        "/sys",
        "/usr",
        "/var/lib/dpkg",
        "/var/lib/pacman",
        "/var/lib/rpm",
    ],
    user_trees_inside_system_trees: &["/usr/local", "/run/media"],
};

#[cfg(test)]
mod tests {
    use super::PROTECTED;

    #[test]
    fn system_folders_home_folders_and_mounted_media_are_protected_but_their_user_content_is_not() {
        for p in [
            "/usr",
            "/usr/lib/x86_64-linux-gnu/libc.so.6",
            "/etc/fstab",
            "/boot/vmlinuz",
            "/var/lib/dpkg/status",
            "/home",
            "/home/alex",
            "/home/alex/Downloads",
            "/home/alex/.config",
            "/home/alex/.local/share/Trash",
            "/media/alex/USB",
            "/mnt/backup",
            "/run/media/alex/USB",
            "/var/log",
        ] {
            assert!(PROTECTED.protects(p), "{p} should be protected");
        }
        for p in [
            "/home/alex/Downloads/big.iso",
            "/home/alex/.cache",
            "/home/alex/.local/share/Steam",
            "/home/alex/code/target",
            "/usr/local/lib/node_modules",
            "/var/lib/docker",
            "/var/log/journal",
            "/var/cache/apt/archives",
            "/opt/google",
            "/media/alex/USB/old",
            "/run/media/alex/USB/photos",
        ] {
            assert!(!PROTECTED.protects(p), "{p} should be collectable");
        }
    }
}
