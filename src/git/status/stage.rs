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

pub async fn toggle_file_stage(repo_root: &Path, file: &FileEntry) -> color_eyre::Result<()> {
    if is_file_staged(&file.status) {
        git_success(
            repo_root,
            &["restore", "--staged", "--", file.path.as_str()],
        )
        .await
    } else {
        git_success(repo_root, &["add", "--", file.path.as_str()]).await
    }
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
