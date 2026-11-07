use std::io;
use std::path::Path;

use crate::platform::{Current, Platform};

pub const PREVIEW_ACTION: &str = Current::PREVIEW_ACTION;

pub const REVEAL_ACTION: &str = Current::REVEAL_ACTION;

#[derive(Debug, thiserror::Error)]
pub enum DesktopError {
    #[error("it is a program, and opening it would run it")]
    WouldRunAProgram,
    #[error(transparent)]
    Launch(#[from] io::Error),
}

pub fn preview(path: &Path) -> Result<(), DesktopError> {
    if Current::would_run_a_program(path) {
        return Err(DesktopError::WouldRunAProgram);
    }
    Ok(Current::preview(path)?)
}

pub fn reveal(path: &Path) -> Result<(), DesktopError> {
    Ok(Current::reveal(path)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn previewing_a_program_is_refused_where_opening_it_would_run_it() {
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join(if cfg!(windows) { "install.bat" } else { "install" });
        std::fs::write(&program, "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        assert!(matches!(preview(&program), Err(DesktopError::WouldRunAProgram)), "the program must not be started");
    }

    #[test]
    fn actions_are_named_for_the_platform() {
        assert!(!PREVIEW_ACTION.is_empty() && !REVEAL_ACTION.is_empty());
        let expected_reveal = if cfg!(target_os = "macos") {
            "Show in Finder"
        } else if cfg!(windows) {
            "Show in Explorer"
        } else {
            "Show in File Manager"
        };
        assert_eq!(REVEAL_ACTION, expected_reveal);
    }
}
