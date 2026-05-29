use std::path::Path;

use super::super::{CommitCompareSelection, FileEntry, changed_files::load_diff_name_status_files};

pub async fn load_files_with_commit_diff(
    repo_root: &Path,
    selection: &CommitCompareSelection,
) -> color_eyre::Result<Vec<FileEntry>> {
    load_diff_name_status_files(
        repo_root,
        &[
            "diff",
            "--name-status",
            "--find-renames",
            "-z",
            selection.base_ref.as_str(),
            selection.commit_hash.as_str(),
        ],
    )
    .await
}
