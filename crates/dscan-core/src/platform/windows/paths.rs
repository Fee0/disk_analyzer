use std::borrow::Cow;

use crate::protection::PathSyntax;

const VERBATIM_UNC_PREFIX: &str = r"\\?\UNC\";
const VERBATIM_PREFIX: &str = r"\\?\";
const UNC_PREFIX: &str = r"\\";

pub(crate) const WINDOWS_PATHS: PathSyntax = PathSyntax { components, same_name: |a, b| a.eq_ignore_ascii_case(b) };

fn components(path: &str) -> Vec<&str> {
    let without_prefix =
        path.strip_prefix(VERBATIM_UNC_PREFIX).or_else(|| path.strip_prefix(VERBATIM_PREFIX)).unwrap_or(path);
    without_prefix.split(['\\', '/']).filter(|c| !c.is_empty()).collect()
}

pub(crate) fn without_verbatim_prefix(path: &str) -> Cow<'_, str> {
    if let Some(unc) = path.strip_prefix(VERBATIM_UNC_PREFIX) {
        return Cow::Owned(format!("{UNC_PREFIX}{unc}"));
    }
    match path.strip_prefix(VERBATIM_PREFIX) {
        Some(rest) if starts_with_drive_letter(rest) => Cow::Borrowed(rest),
        _ => Cow::Borrowed(path),
    }
}

pub(crate) fn verbatim(path: &str) -> Cow<'_, str> {
    if path.starts_with(VERBATIM_PREFIX) {
        Cow::Borrowed(path)
    } else if let Some(unc) = path.strip_prefix(UNC_PREFIX) {
        Cow::Owned(format!("{VERBATIM_UNC_PREFIX}{unc}"))
    } else {
        Cow::Owned(format!("{VERBATIM_PREFIX}{}", path.replace('/', "\\")))
    }
}

fn starts_with_drive_letter(path: &str) -> bool {
    matches!(path.as_bytes(), [letter, b':', ..] if letter.is_ascii_alphabetic())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbatim_prefixes_are_dropped_for_drives_and_unc_shares_but_kept_for_volume_guids() {
        assert_eq!(without_verbatim_prefix(r"\\?\C:\Users"), r"C:\Users");
        assert_eq!(without_verbatim_prefix(r"\\?\UNC\server\share\x"), r"\\server\share\x");
        assert_eq!(without_verbatim_prefix(r"C:\Users"), r"C:\Users");
        let guid = r"\\?\Volume{6b29fc40-ca47-1067-b31d-00dd010662da}\x";
        assert_eq!(without_verbatim_prefix(guid), guid);
    }

    #[test]
    fn the_verbatim_form_lifts_the_path_length_limit_and_accepts_only_backslashes() {
        assert_eq!(verbatim(r"C:\Users"), r"\\?\C:\Users");
        assert_eq!(verbatim("C:/Users/alex"), r"\\?\C:\Users\alex");
        assert_eq!(verbatim(r"\\server\share"), r"\\?\UNC\server\share");
        assert_eq!(verbatim(r"\\?\C:\x"), r"\\?\C:\x");
    }

    #[test]
    fn components_accept_either_separator_and_skip_verbatim_prefixes() {
        assert_eq!(components(r"\\?\C:\Users/alex\"), ["C:", "Users", "alex"]);
        assert_eq!(components(r"\\?\UNC\server\share"), ["server", "share"]);
        assert_eq!(components(r"C:\"), ["C:"]);
    }
}
