use std::path::Path;

use tokio::fs;

use super::super::{
    FileEntry,
    command::git_output,
    parse::{parse_status_entries, to_file_entry},
};

pub async fn load_files_with_status(repo_root: &Path) -> color_eyre::Result<Vec<FileEntry>> {
    let output = git_output(
        repo_root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )
    .await?;
    let mut files = Vec::new();

    for entry in parse_status_entries(&output) {
        if entry.status == "!!" || is_directory_status_entry(repo_root, &entry.path).await {
            continue;
        }
        files.push(to_file_entry(entry));
    }

    Ok(files)
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
