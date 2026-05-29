mod conflict;
mod refresh;
mod stage;

use crate::git;

pub(super) fn merge_conflict_resolution_label(
    resolution: git::MergeConflictResolution,
) -> &'static str {
    match resolution {
        git::MergeConflictResolution::Current => "current",
        git::MergeConflictResolution::Incoming => "incoming",
        git::MergeConflictResolution::Both => "both",
    }
}
