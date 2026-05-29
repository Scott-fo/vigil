use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use crate::git::git_output;

pub(super) async fn collect_watch_directories(repo_root: &Path) -> Result<Vec<PathBuf>, String> {
    let visible_paths = git_visible_paths(repo_root).await?;
    let mut directories = HashSet::from([repo_root.to_path_buf()]);

    for path in visible_paths {
        let mut current = repo_root.join(path);
        while let Some(parent) = current.parent() {
            if !parent.starts_with(repo_root) {
                break;
            }
            directories.insert(parent.to_path_buf());
            if parent == repo_root {
                break;
            }
            current = parent.to_path_buf();
        }
    }

    let mut directories = directories.into_iter().collect::<Vec<_>>();
    directories.sort();
    Ok(directories)
}

async fn git_visible_paths(repo_root: &Path) -> Result<Vec<PathBuf>, String> {
    let output = git_output(
        repo_root,
        &[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ],
    )
    .await
    .map_err(|error| error.to_string())?;

    Ok(output
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .collect())
}
