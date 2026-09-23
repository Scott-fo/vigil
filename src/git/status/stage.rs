use std::path::Path;

use super::super::{FileEntry, command::git_success};

pub fn is_untracked_status(status: &str) -> bool {
    status == "??"
}

pub fn is_file_staged(status: &str) -> bool {
    if is_untracked_status(status) {
        return false;
    }

    let index_status = status.chars().next().unwrap_or(' ');
    index_status != ' '
}

pub fn is_file_fully_staged(status: &str) -> bool {
    if is_untracked_status(status) {
        return false;
    }

    let mut chars = status.chars();
    let index_status = chars.next().unwrap_or(' ');
    let worktree_status = chars.next().unwrap_or(' ');
    index_status != ' ' && worktree_status == ' '
}

/// Where a working-tree file's changes live relative to the index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageState {
    /// Nothing staged: unstaged edits, untracked files, and merge conflicts.
    Unstaged,
    /// Some changes staged and more edits on top in the working tree.
    PartiallyStaged,
    /// Every change is in the index.
    Staged,
}

pub fn stage_state(status: &str) -> StageState {
    let mut chars = status.chars();
    let index_status = chars.next().unwrap_or(' ');
    let worktree_status = chars.next().unwrap_or(' ');
    let conflicted = index_status == 'U'
        || worktree_status == 'U'
        || matches!((index_status, worktree_status), ('A', 'A') | ('D', 'D'));
    if is_untracked_status(status) || conflicted || index_status == ' ' {
        StageState::Unstaged
    } else if worktree_status == ' ' {
        StageState::Staged
    } else {
        StageState::PartiallyStaged
    }
}

/// Stages every working-tree change to `file`, including untracked files.
pub async fn stage_file(repo_root: &Path, file: &FileEntry) -> color_eyre::Result<()> {
    git_success(repo_root, &["add", "--", file.path.as_str()]).await
}

/// Moves every staged change to `file` back to the working tree.
pub async fn unstage_file(repo_root: &Path, file: &FileEntry) -> color_eyre::Result<()> {
    git_success(
        repo_root,
        &["restore", "--staged", "--", file.path.as_str()],
    )
    .await
}

pub async fn stage_all_changes(repo_root: &Path) -> color_eyre::Result<()> {
    git_success(repo_root, &["add", "-A"]).await
}

pub async fn unstage_all_changes(repo_root: &Path) -> color_eyre::Result<()> {
    git_success(repo_root, &["restore", "--staged", "--", "."]).await
}

pub async fn discard_file_changes(repo_root: &Path, file: &FileEntry) -> color_eyre::Result<()> {
    if is_untracked_status(&file.status) {
        git_success(repo_root, &["clean", "-f", "--", file.path.as_str()]).await
    } else {
        git_success(
            repo_root,
            &[
                "restore",
                "--source=HEAD",
                "--staged",
                "--worktree",
                "--",
                file.path.as_str(),
            ],
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::{StageState, stage_state};

    #[test]
    fn stage_state_follows_index_and_worktree_columns() {
        assert_eq!(stage_state("M "), StageState::Staged);
        assert_eq!(stage_state("A "), StageState::Staged);
        assert_eq!(stage_state("R "), StageState::Staged);
        assert_eq!(stage_state("MM"), StageState::PartiallyStaged);
        assert_eq!(stage_state("AM"), StageState::PartiallyStaged);
        assert_eq!(stage_state(" M"), StageState::Unstaged);
        assert_eq!(stage_state("??"), StageState::Unstaged);
    }

    #[test]
    fn merge_conflicts_are_never_treated_as_staged() {
        for status in ["UU", "AA", "DD", "AU", "UD"] {
            assert_eq!(stage_state(status), StageState::Unstaged, "{status}");
        }
    }
}
