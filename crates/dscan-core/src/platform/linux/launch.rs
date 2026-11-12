use std::fmt::Write;

const FILE_URI_SCHEME: &str = "file://";
const KEPT_AS_IS_IN_A_FILE_URI_PATH: &[u8] = b"-._~/";
const LAUNCHER_EXTENSIONS: &[&[u8]] = &[b".desktop", b".appimage", b".exe", b".msi", b".jar"];
const PROGRAM_SIGNATURES: &[&[u8]] = &[b"\x7fELF", b"#!"];
pub(super) const LONGEST_PROGRAM_SIGNATURE: usize = 4;

pub(super) fn file_uri(path: &[u8]) -> String {
    let mut uri = String::with_capacity(FILE_URI_SCHEME.len() + path.len());
    uri.push_str(FILE_URI_SCHEME);
    for &byte in path {
        if byte.is_ascii_alphanumeric() || KEPT_AS_IS_IN_A_FILE_URI_PATH.contains(&byte) {
            uri.push(byte as char);
        } else {
            let _ = write!(uri, "%{byte:02X}");
        }
    }
    uri
}

fn ends_with_ignoring_ascii_case(name: &[u8], suffix: &[u8]) -> bool {
    name.len() >= suffix.len() && name[name.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
}

pub(super) fn looks_like_a_program(name: &[u8], first_bytes: &[u8], executable: bool) -> bool {
    let is_launcher = LAUNCHER_EXTENSIONS.iter().any(|extension| ends_with_ignoring_ascii_case(name, extension));
    let is_executable_program = executable && PROGRAM_SIGNATURES.iter().any(|s| first_bytes.starts_with(s));
    is_launcher || is_executable_program
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uris_percent_encode_everything_but_unreserved_bytes_and_slashes_so_commas_cannot_split_a_dbus_array() {
        assert_eq!(file_uri(b"/home/alex/My Files/a,b.txt"), "file:///home/alex/My%20Files/a%2Cb.txt");
        assert_eq!(file_uri("/tmp/ü".as_bytes()), "file:///tmp/%C3%BC");
        assert_eq!(file_uri(b"/x/\xff"), "file:///x/%FF", "names need not be UTF-8 on Linux");
    }

    #[test]
    fn reading_the_longest_signature_is_enough_to_recognise_every_program() {
        assert!(PROGRAM_SIGNATURES.iter().all(|s| s.len() <= LONGEST_PROGRAM_SIGNATURE));
    }

    #[test]
    fn only_executable_binaries_and_scripts_or_launchers_count_as_programs() {
        assert!(looks_like_a_program(b"tool", b"\x7fELF", true));
        assert!(looks_like_a_program(b"build.sh", b"#!/b", true));
        assert!(looks_like_a_program(b"Firefox.desktop", b"[Des", false));
        assert!(looks_like_a_program(b"Krita.AppImage", b"\x7fELF", false));
        assert!(looks_like_a_program(b"setup.EXE", b"MZ\x90\x00", false), "Wine runs it without the execute bit");
        assert!(looks_like_a_program(b"tool.jar", b"PK\x03\x04", false), "Java runs it without the execute bit");
        assert!(
            !looks_like_a_program(b"photo.jpg", b"\xff\xd8\xff\xe0", true),
            "FAT and NTFS mark every file executable"
        );
        assert!(!looks_like_a_program(b"tool", b"\x7fELF", false), "without the execute bit it cannot run");
    }
}
