use std::path::Path;

use super::super::{
    CommitSearchEntry, EMPTY_TREE_HASH, command::git_output, parse::parse_commit_log_entries,
};

pub async fn list_searchable_commits(
    repo_root: &Path,
    limit: usize,
) -> color_eyre::Result<Vec<CommitSearchEntry>> {
    let output = git_output(
        repo_root,
        &[
            "log",
            &format!("--max-count={}", limit.max(1)),
            "--date=short",
            "--pretty=format:%H%x1f%P%x1f%h%x1f%ad%x1f%an%x1f%s%x1e",
        ],
    )
    .await?;

    Ok(parse_commit_log_entries(&output))
}

pub fn resolve_commit_base_ref(commit: &CommitSearchEntry) -> String {
    commit
        .parent_hashes
        .first()
        .cloned()
        .unwrap_or_else(|| EMPTY_TREE_HASH.to_string())
}
