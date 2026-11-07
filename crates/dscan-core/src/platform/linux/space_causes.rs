use crate::hidden::{Cause, SpaceExplanation};

pub(super) const HIDDEN_SPACE: SpaceExplanation = SpaceExplanation {
    summary: "Space in use on this file system that the scan could not attribute to any file it could read.",
    causes: &[
        Cause {
            text: "Folders your user cannot read. Run the app as root and scan again to include them.",
            remedy: None,
        },
        Cause {
            text: "Snapshots and subvolumes that are not mounted, as on btrfs or ZFS. Tools such as Snapper or Timeshift manage them.",
            remedy: None,
        },
        Cause { text: "Files hidden under a folder that another file system is mounted on.", remedy: None },
        Cause {
            text: "Files deleted while a program still has them open. The space comes back when the program closes them.",
            remedy: None,
        },
        Cause { text: "File system metadata, such as the journal and inode tables.", remedy: None },
    ],
};

pub(super) const PURGEABLE_SPACE: SpaceExplanation =
    SpaceExplanation { summary: "Linux does not report purgeable space, so none is shown.", causes: &[] };
