use crate::hidden::{Cause, SpaceExplanation};

pub(super) const HIDDEN_SPACE: SpaceExplanation = SpaceExplanation {
    summary: "Space in use on this drive that the scan could not attribute to any file it could read.",
    causes: &[
        Cause {
            text: "Folders that only an administrator can read. Run the app as administrator and scan again to include them.",
            remedy: None,
        },
        Cause {
            text: "System Restore points and other shadow copies, kept in System Volume Information. Manage them under System Protection.",
            remedy: None,
        },
        Cause { text: "NTFS metadata, such as the master file table and the change journal.", remedy: None },
        Cause { text: "Files deleted while a program still had them open.", remedy: None },
    ],
};

pub(super) const PURGEABLE_SPACE: SpaceExplanation =
    SpaceExplanation { summary: "Windows does not report purgeable space, so none is shown.", causes: &[] };
