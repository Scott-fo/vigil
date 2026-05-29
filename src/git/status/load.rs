use std::path::{Path, PathBuf};

use tokio::fs;

use super::super::{
    FileEntry,
    command::git_output,
    parse::{parse_status_entries, to_file_entry},
    repo::resolve_repo_root_from,
};

#[derive(Debug, Clone)]
pub struct WorkingTreeStatus {
    pub repo_root: PathBuf,
    pub files: Vec<FileEntry>,
}

pub async fn load_working_tree_status(repo_path: &Path) -> color_eyre::Result<WorkingTreeStatus> {
    let status_output = git_output(
        repo_path,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    );
    let resolved_root = resolve_repo_root_from(repo_path);
    let (output, repo_root) = tokio::try_join!(status_output, resolved_root)?;

    Ok(WorkingTreeStatus {
        files: status_files_from_output(&repo_root, &output).await,
        repo_root,
    })
}

pub async fn load_files_with_status(repo_root: &Path) -> color_eyre::Result<Vec<FileEntry>> {
    let output = git_output(
        repo_root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )
    .await?;
    Ok(status_files_from_output(repo_root, &output).await)
}

async fn status_files_from_output(repo_root: &Path, output: &str) -> Vec<FileEntry> {
    let mut files = Vec::new();

    for entry in parse_status_entries(output) {
        if entry.status == "!!" || is_directory_status_entry(repo_root, &entry.path).await {
            continue;
        }
        files.push(to_file_entry(entry));
    }

    files
}

pub async fn load_status_for_path(
    repo_root: &Path,
    file_path: &str,
) -> color_eyre::Result<Option<FileEntry>> {
    let output = git_output(
        repo_root,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--",
            file_path,
        ],
    )
    .await?;

    for entry in parse_status_entries(&output) {
        if entry.status == "!!" || is_directory_status_entry(repo_root, &entry.path).await {
            continue;
        }
        return Ok(Some(to_file_entry(entry)));
    }

    Ok(None)
}

async fn is_directory_status_entry(repo_root: &Path, path: &str) -> bool {
    match fs::metadata(repo_root.join(path)).await {
        Ok(metadata) => metadata.is_dir(),
        Err(_) => false,
    }
}
