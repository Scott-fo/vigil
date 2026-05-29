//! Git worktree listing.
//!
//! This module turns `git worktree list --porcelain` into sorted worktree
//! entries, including dirty-state summaries used by the worktree modal.

use std::path::Path;

use tokio::fs;

use super::{
    WorktreeEntry,
    command::git_output,
    parse::{parse_status_entries, parse_worktree_entries},
};

pub async fn list_worktrees(repo_root: &Path) -> color_eyre::Result<Vec<WorktreeEntry>> {
    let output = git_output(repo_root, &["worktree", "list", "--porcelain", "-z"]).await?;
    let mut entries = parse_worktree_entries(&output);
    let current_root = fs::canonicalize(repo_root)
        .await
        .unwrap_or_else(|_| repo_root.to_path_buf());

    for entry in &mut entries {
        if entry.bare || entry.prunable {
            continue;
        }

        let status_output = git_output(
            &entry.path,
            &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        )
        .await?;
        entry.change_count = parse_status_entries(&status_output)
            .into_iter()
            .filter(|status| status.status != "!!")
            .count();
        entry.dirty = entry.change_count > 0;
    }

    entries.sort_by(|a, b| {
        let a_current = a.path == repo_root || a.path == current_root;
        let b_current = b.path == repo_root || b.path == current_root;
        b_current
            .cmp(&a_current)
            .then_with(|| b.dirty.cmp(&a.dirty))
            .then_with(|| a.branch.cmp(&b.branch))
            .then_with(|| a.path.cmp(&b.path))
    });

    Ok(entries)
}
