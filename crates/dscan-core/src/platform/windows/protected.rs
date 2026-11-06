use super::paths::WINDOWS_PATHS;
use crate::protection::ProtectedLocations;

pub(crate) const PROTECTED: ProtectedLocations = ProtectedLocations {
    syntax: WINDOWS_PATHS,
    exact: &[
        "*",
        "*/Program Files",
        "*/Program Files (x86)",
        "*/ProgramData",
        "*/Users",
        "*/Users/Public",
        "*/Users/*",
        "*/Users/*/AppData",
        "*/Users/*/AppData/Local",
        "*/Users/*/AppData/LocalLow",
        "*/Users/*/AppData/Roaming",
        "*/Users/*/Desktop",
        "*/Users/*/Documents",
        "*/Users/*/Downloads",
        "*/Users/*/Music",
        "*/Users/*/OneDrive",
        "*/Users/*/Pictures",
        "*/Users/*/Videos",
        "*/Windows/Temp",
    ],
    trees: &[
        "*/$Recycle.Bin",
        "*/Boot",
        "*/Recovery",
        "*/System Volume Information",
        "*/Windows",
        "*/hiberfil.sys",
        "*/pagefile.sys",
        "*/swapfile.sys",
    ],
    user_trees_inside_system_trees: &["*/Windows/Temp"],
};

#[cfg(test)]
mod tests {
    use super::PROTECTED;

    #[test]
    fn drive_roots_system_folders_and_user_folders_are_protected_but_their_user_content_is_not() {
        for p in [
            r"C:\",
            r"D:\",
            r"C:\Windows",
            r"C:\Windows\System32\kernel32.dll",
            r"C:\Windows\WinSxS",
            r"c:\windows\temp",
            r"C:\Program Files",
            r"C:\ProgramData",
            r"C:\Users",
            r"C:\Users\alex",
            r"C:\Users\alex\AppData\Local",
            r"C:\Users\alex\Downloads",
            r"C:\pagefile.sys",
            r"C:\System Volume Information",
            r"\\?\C:\Windows\explorer.exe",
        ] {
            assert!(PROTECTED.protects(p), "{p} should be protected");
        }
        for p in [
            r"C:\Windows\Temp\setup.log",
            r"C:\Program Files\Some App",
            r"C:\ProgramData\Package Cache",
            r"C:\Users\alex\AppData\Local\Temp",
            r"C:\Users\alex\Downloads\big.iso",
            r"C:\Users\alex\source",
            r"D:\Games",
        ] {
            assert!(!PROTECTED.protects(p), "{p} should be collectable");
        }
    }
}
