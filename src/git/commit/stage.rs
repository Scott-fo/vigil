use std::path::Path;

use color_eyre::eyre::eyre;

use super::super::command::git_success;

pub async fn commit_staged_changes(repo_root: &Path, message: &str) -> color_eyre::Result<()> {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return Err(eyre!("Commit message is required."));
    }

    git_success(repo_root, &["commit", "-m", trimmed]).await
}
