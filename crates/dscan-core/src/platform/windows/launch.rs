use std::path::Path;

const PROGRAM_EXTENSIONS: &[&str] = &[
    "exe",
    "com",
    "bat",
    "cmd",
    "msi",
    "msp",
    "msix",
    "msixbundle",
    "appx",
    "appxbundle",
    "appinstaller",
    "application",
    "appref-ms",
    "scr",
    "pif",
    "cpl",
    "msc",
    "hta",
    "ps1",
    "psm1",
    "vbs",
    "vbe",
    "js",
    "jse",
    "wsf",
    "wsh",
    "reg",
    "lnk",
    "url",
    "jar",
    "py",
    "pyw",
    "msu",
    "chm",
    "scf",
    "sct",
    "wsc",
    "gadget",
    "diagcab",
    "settingcontent-ms",
];

pub(crate) fn is_program_name(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|extension| PROGRAM_EXTENSIONS.iter().any(|p| p.eq_ignore_ascii_case(extension)))
}

pub(crate) fn quoted_for_explorer(path: &str) -> String {
    format!("\"{path}\"")
}

pub(crate) fn explorer_select_argument(path: &str) -> String {
    format!("/select,{}", quoted_for_explorer(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn programs_scripts_installers_and_shortcuts_are_recognised_whatever_their_case() {
        for program in [
            "setup.exe",
            "INSTALL.MSI",
            "run.Bat",
            "build.cmd",
            "tool.ps1",
            "game.lnk",
            "site.url",
            "help.chm",
            "kb.msu",
        ] {
            assert!(is_program_name(program), "{program} would run when opened");
        }
        for document in ["photo.jpg", "report.pdf", "notes", "archive.exe.zip", "movie.mov"] {
            assert!(!is_program_name(document), "{document} opens in a viewer");
        }
    }

    #[test]
    fn explorer_gets_every_path_quoted_because_it_splits_unquoted_ones_at_commas() {
        assert_eq!(quoted_for_explorer(r"C:\a, b\c.txt"), r#""C:\a, b\c.txt""#);
        assert_eq!(explorer_select_argument(r"C:\a,b\c.txt"), r#"/select,"C:\a,b\c.txt""#);
    }
}
