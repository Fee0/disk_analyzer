use crate::hidden::{Cause, Remedy, SpaceExplanation};

pub(super) const HIDDEN_SPACE: SpaceExplanation = SpaceExplanation {
    summary: "Space in use on this disk that the scan could not attribute to any file it could read.",
    causes: &[
        Cause {
            text: "Folders macOS keeps private until you grant Full Disk Access, such as Mail, Messages and Safari data.",
            remedy: Some(Remedy::GrantFullDiskAccess),
        },
        Cause {
            text: "Folders only the system can read, even with Full Disk Access. Running the scan as an administrator includes them.",
            remedy: None,
        },
        Cause {
            text: "Local Time Machine and APFS snapshots. List them with “tmutil listlocalsnapshots /” in Terminal.",
            remedy: None,
        },
        Cause {
            text: "Space APFS uses for itself: metadata, and system volumes such as Recovery and VM.",
            remedy: None,
        },
    ],
};

pub(super) const PURGEABLE_SPACE: SpaceExplanation = SpaceExplanation {
    summary: "Space macOS counts as available because it can free it when an app needs the room. Nothing needs to be done: macOS frees it automatically.",
    causes: &[
        Cause { text: "Local Time Machine snapshots, which macOS removes when space runs low.", remedy: None },
        Cause {
            text: "Files kept in iCloud that macOS can download again, when Optimize Mac Storage is on.",
            remedy: None,
        },
        Cause { text: "Caches and temporary files the system manages.", remedy: None },
    ],
};
