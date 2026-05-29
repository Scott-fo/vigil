//! Repository-level git operations.
//!
//! This module owns repository initialization, remotes, root resolution, and
//! raw revision file reads. Product-specific queries live in sibling modules
//! such as `commit`, `refs`, `status`, and `worktree`.

use std::path::{Path, PathBuf};

use color_eyre::eyre::WrapErr;

use super::command::{git_output, git_output_bytes, git_success};

pub async fn push_to_remote(repo_root: &Path) -> color_eyre::Result<()> {
    git_success(repo_root, &["push"]).await
}

pub async fn pull_from_remote(repo_root: &Path) -> color_eyre::Result<()> {
    git_success(repo_root, &["pull"]).await
}

pub async fn init_repo(repo_root: &Path) -> color_eyre::Result<()> {
    git_success(repo_root, &["init"]).await
}

pub async fn load_revision_file_bytes(
    repo_root: &Path,
    revision: &str,
    file_path: &str,
) -> color_eyre::Result<Vec<u8>> {
    let object = format!("{}:{}", revision.trim(), file_path);
    git_output_bytes(repo_root, &["cat-file", "-p", object.as_str()])
        .await
        .wrap_err_with(|| format!("failed to read {object}"))
}

pub async fn revision_matches_head(repo_root: &Path, revision: &str) -> color_eyre::Result<bool> {
    let revision_hash = git_output(repo_root, &["rev-parse", "--verify", revision.trim()]).await?;
    let head_hash = git_output(repo_root, &["rev-parse", "--verify", "HEAD"]).await?;

    Ok(revision_hash.trim() == head_hash.trim())
}

pub async fn resolve_repo_root() -> color_eyre::Result<PathBuf> {
    resolve_repo_root_from(Path::new(".")).await
}

pub async fn resolve_repo_root_from(probe_path: &Path) -> color_eyre::Result<PathBuf> {
    let output = git_output(probe_path, &["rev-parse", "--show-toplevel"])
        .await
        .wrap_err("failed to resolve git repository root")?;

    Ok(PathBuf::from(output.trim()))
}
