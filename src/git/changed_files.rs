//! Changed-file listing for comparison views.
//!
//! This module turns `git diff --name-status -z` output into Vigil's `FileEntry`
//! interface. Commit and branch comparison modules share it so rename/copy
//! handling stays in one place.

use std::path::Path;

use super::{
    FileEntry,
    command::git_output,
    parse::{parse_diff_name_status_entries, to_file_entry},
};

pub(crate) async fn load_diff_name_status_files(
    repo_root: &Path,
    args: &[&str],
) -> color_eyre::Result<Vec<FileEntry>> {
    let output = git_output(repo_root, args).await?;
    Ok(parse_diff_name_status_entries(&output)
        .into_iter()
        .map(to_file_entry)
        .collect())
}
