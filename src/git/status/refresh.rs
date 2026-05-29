use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use super::super::command::git_output_with_stdin;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum RefreshCandidates {
    RefreshNow,
    CheckIgnored(Vec<String>),
    NoRefresh,
}

pub(super) fn refresh_candidates_for_paths(
    repo_root: &Path,
    changed_paths: &[PathBuf],
) -> RefreshCandidates {
    if changed_paths.is_empty() {
        return RefreshCandidates::RefreshNow;
    }

    let mut candidate_paths = Vec::new();
    let mut seen_paths = HashSet::new();

    for path in changed_paths {
        let Ok(relative_path) = path.strip_prefix(repo_root) else {
            return RefreshCandidates::RefreshNow;
        };

        if relative_path.as_os_str().is_empty() {
            return RefreshCandidates::RefreshNow;
        }

        if relative_path
            .file_name()
            .is_some_and(|file_name| file_name == ".gitignore")
        {
            return RefreshCandidates::RefreshNow;
        }

        let relative = relative_path.to_string_lossy().replace('\\', "/");
        if seen_paths.insert(relative.clone()) {
            candidate_paths.push(relative);
        }
    }

    if candidate_paths.is_empty() {
        RefreshCandidates::NoRefresh
    } else {
        RefreshCandidates::CheckIgnored(candidate_paths)
    }
}

pub async fn should_refresh_for_paths(
    repo_root: &Path,
    changed_paths: &[PathBuf],
) -> color_eyre::Result<bool> {
    let candidate_paths = match refresh_candidates_for_paths(repo_root, changed_paths) {
        RefreshCandidates::RefreshNow => return Ok(true),
        RefreshCandidates::NoRefresh => return Ok(false),
        RefreshCandidates::CheckIgnored(candidate_paths) => candidate_paths,
    };

    let ignored_paths = git_check_ignored(repo_root, &candidate_paths).await?;
    Ok(candidate_paths
        .iter()
        .any(|path| !ignored_paths.contains(path)))
}

async fn git_check_ignored(
    repo_root: &Path,
    paths: &[String],
) -> color_eyre::Result<HashSet<String>> {
    let input = format!("{}\0", paths.join("\0"));
    let output = git_output_with_stdin(
        repo_root,
        &["check-ignore", "-z", "--stdin"],
        input.as_bytes(),
        &[0, 1],
    )
    .await?;

    Ok(output
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{RefreshCandidates, refresh_candidates_for_paths};

    fn path(value: &str) -> PathBuf {
        PathBuf::from(value)
    }

    #[test]
    fn empty_path_batch_refreshes_immediately() {
        assert_eq!(
            refresh_candidates_for_paths(Path::new("/repo"), &[]),
            RefreshCandidates::RefreshNow
        );
    }

    #[test]
    fn paths_outside_repo_refresh_immediately() {
        assert_eq!(
            refresh_candidates_for_paths(Path::new("/repo"), &[path("/tmp/file.rs")]),
            RefreshCandidates::RefreshNow
        );
    }

    #[test]
    fn repo_root_path_refreshes_immediately() {
        assert_eq!(
            refresh_candidates_for_paths(Path::new("/repo"), &[path("/repo")]),
            RefreshCandidates::RefreshNow
        );
    }

    #[test]
    fn gitignore_path_refreshes_immediately() {
        assert_eq!(
            refresh_candidates_for_paths(Path::new("/repo"), &[path("/repo/src/.gitignore")]),
            RefreshCandidates::RefreshNow
        );
    }

    #[test]
    fn repo_relative_paths_are_deduped_for_ignore_check() {
        assert_eq!(
            refresh_candidates_for_paths(
                Path::new("/repo"),
                &[path("/repo/src/lib.rs"), path("/repo/src/lib.rs")]
            ),
            RefreshCandidates::CheckIgnored(vec!["src/lib.rs".to_string()])
        );
    }
}
