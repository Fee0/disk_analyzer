const DRIVE_LETTERS: std::ops::RangeInclusive<u8> = b'A'..=b'Z';
const UNLABELED_DRIVE_NAME: &str = "Local Disk";
const DEFAULT_SYSTEM_DRIVE_LETTER: char = 'C';

pub(super) const DRIVE_REMOVABLE: u32 = 2;
pub(super) const DRIVE_FIXED: u32 = 3;
pub(super) const DRIVE_REMOTE: u32 = 4;
pub(super) const DRIVE_RAMDISK: u32 = 6;

pub(super) fn letters_in(logical_drives_bitmask: u32) -> impl Iterator<Item = char> {
    DRIVE_LETTERS.enumerate().filter(move |(bit, _)| logical_drives_bitmask & (1 << bit) != 0).map(|(_, l)| l as char)
}

pub(super) fn root_of(letter: char) -> String {
    format!(r"{letter}:\")
}

pub(super) fn holds_browsable_files(drive_type: u32) -> bool {
    matches!(drive_type, DRIVE_REMOVABLE | DRIVE_FIXED | DRIVE_REMOTE | DRIVE_RAMDISK)
}

pub(super) fn display_name(label: &str, letter: char) -> String {
    let label = if label.trim().is_empty() { UNLABELED_DRIVE_NAME } else { label.trim() };
    format!("{label} ({letter}:)")
}

pub(super) fn system_drive_letter(system_drive_variable: Option<&str>) -> char {
    system_drive_variable
        .and_then(|value| value.chars().next())
        .filter(char::is_ascii_alphabetic)
        .map_or(DEFAULT_SYSTEM_DRIVE_LETTER, |letter| letter.to_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_set_bit_of_the_logical_drive_mask_is_a_drive_letter_from_a() {
        assert_eq!(letters_in(0b1101).collect::<String>(), "ACD");
        assert_eq!(letters_in(1 << 25).collect::<String>(), "Z");
        assert_eq!(root_of('D'), r"D:\");
    }

    #[test]
    fn optical_drives_and_unknown_types_are_not_offered() {
        const DRIVE_UNKNOWN: u32 = 0;
        const DRIVE_NO_ROOT_DIR: u32 = 1;
        const DRIVE_CDROM: u32 = 5;
        assert!([DRIVE_FIXED, DRIVE_REMOVABLE, DRIVE_REMOTE, DRIVE_RAMDISK].into_iter().all(holds_browsable_files));
        assert!(![DRIVE_UNKNOWN, DRIVE_NO_ROOT_DIR, DRIVE_CDROM].into_iter().any(holds_browsable_files));
    }

    #[test]
    fn drives_are_named_like_explorer_does() {
        assert_eq!(display_name("Data", 'D'), "Data (D:)");
        assert_eq!(display_name("", 'C'), "Local Disk (C:)");
    }

    #[test]
    fn the_system_drive_comes_from_the_environment_with_c_as_the_fallback() {
        assert_eq!(system_drive_letter(Some("d:")), 'D');
        assert_eq!(system_drive_letter(None), 'C');
        assert_eq!(system_drive_letter(Some("")), 'C');
    }
}
