use crate::platform::unix_syntax::UNIX_PATHS;
use crate::protection::ProtectedLocations;

pub(crate) const PROTECTED: ProtectedLocations = ProtectedLocations {
    syntax: UNIX_PATHS,
    exact: &[
        "/",
        "/Applications",
        "/Library",
        "/System",
        "/Users",
        "/Users/Shared",
        "/Volumes",
        "/bin",
        "/cores",
        "/dev",
        "/etc",
        "/opt",
        "/private",
        "/private/etc",
        "/private/tmp",
        "/private/var",
        "/sbin",
        "/tmp",
        "/usr",
        "/usr/local",
        "/var",
        "/Users/*",
        "/Users/*/Applications",
        "/Users/*/Desktop",
        "/Users/*/Documents",
        "/Users/*/Downloads",
        "/Users/*/Library",
        "/Users/*/Movies",
        "/Users/*/Music",
        "/Users/*/Pictures",
        "/Users/*/Public",
        "/Users/*/.Trash",
        "/Volumes/*",
    ],
    trees: &[
        "/System",
        "/bin",
        "/sbin",
        "/usr",
        "/private/etc",
        "/private/var/db",
        "/private/var/root",
        "/Library/Keychains",
        "/Users/*/Library/Keychains",
    ],
    user_trees_inside_system_trees: &["/usr/local"],
    listed_location_of: &[],
};

#[cfg(test)]
mod tests {
    use super::PROTECTED;

    #[test]
    fn system_folders_home_folders_and_volume_roots_are_protected_but_their_user_content_is_not() {
        for p in [
            "/System",
            "/System/Library/CoreServices/Finder.app",
            "/System/Volumes/Data/.Spotlight-V100",
            "/Library",
            "/Applications",
            "/usr",
            "/usr/lib/libSystem.B.dylib",
            "/usr/local",
            "/private/var",
            "/private/var/db/receipts",
            "/private/etc/sudoers",
            "/private/var/root/Library",
            "/Library/Keychains/System.keychain",
            "/Users/alex/Library/Keychains/login.keychain-db",
            "/Users/alex",
            "/Users/alex/Library",
            "/Users/alex/Downloads",
            "/Users/alex/.Trash",
            "/Volumes/Backup",
        ] {
            assert!(PROTECTED.protects(p), "{p} should be protected");
        }
        for p in [
            "/Applications/Some.app",
            "/Library/Caches/com.example",
            "/usr/local/Cellar",
            "/private/var/folders/xy",
            "/Users/alex/Downloads/big.dmg",
            "/Users/alex/Library/Caches",
            "/Users/alex/code",
            "/Volumes/Backup/old",
            "/opt/homebrew",
        ] {
            assert!(!PROTECTED.protects(p), "{p} should be collectable");
        }
    }
}
