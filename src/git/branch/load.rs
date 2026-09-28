use std::path::{Path, PathBuf};

use tokio::fs;

use super::super::{
    command::{git_output, git_output_raw},
    parse::{BRANCH_REF_FORMAT, parse_branch_refs},
    refs::resolve_current_branch_ref,
};
use super::{BranchSnapshot, HeadState, RepoOperation};

/// Git-dir entries whose presence means an operation is in progress, checked
/// in order. Rebase comes first because a conflicted rebase step can also
/// leave a cherry-pick marker behind.
const OPERATION_MARKERS: [(&str, RepoOperation); 6] = [
    ("rebase-merge", RepoOperation::Rebase),
    ("rebase-apply", RepoOperation::Rebase),
    ("MERGE_HEAD", RepoOperation::Merge),
    ("CHERRY_PICK_HEAD", RepoOperation::CherryPick),
    ("REVERT_HEAD", RepoOperation::Revert),
    ("BISECT_LOG", RepoOperation::Bisect),
];

pub async fn load_branch_snapshot(repo_root: &Path) -> color_eyre::Result<BranchSnapshot> {
    let refs = git_output(
        repo_root,
        &[
            "for-each-ref",
            BRANCH_REF_FORMAT,
            "--sort=-committerdate",
            "refs/heads",
            "refs/remotes",
        ],
    );
    let (refs, remotes, current_branch, git_paths, previous_branch) = tokio::try_join!(
        refs,
        git_output(repo_root, &["remote"]),
        resolve_current_branch_ref(repo_root),
        git_dir_paths(repo_root),
        previous_branch(repo_root),
    )?;

    let remotes = remotes
        .lines()
        .map(str::trim)
        .filter(|remote| !remote.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let mut branches = parse_branch_refs(&refs, &remotes);
    // Local branches first; for-each-ref already sorted each group by date.
    branches.sort_by_key(|branch| branch.is_remote());

    let head = match current_branch {
        Some(name) if branches.iter().any(|branch| branch.is_head) => HeadState::Branch(name),
        Some(name) => HeadState::Unborn(name),
        None => HeadState::Detached {
            short_hash: git_output(repo_root, &["rev-parse", "--short", "HEAD"])
                .await?
                .trim()
                .to_string(),
        },
    };
    let (fetch_head, markers) = git_paths;

    Ok(BranchSnapshot {
        previous_branch: previous_branch.filter(|previous| {
            head.branch_name() != Some(previous.as_str())
                && branches
                    .iter()
                    .any(|branch| !branch.is_remote() && &branch.name == previous)
        }),
        head,
        branches,
        remotes,
        last_fetch: fs::metadata(&fetch_head)
            .await
            .and_then(|metadata| metadata.modified())
            .ok(),
        operation: operation_in_progress(&markers).await,
    })
}

/// Resolves `FETCH_HEAD` and the operation markers inside the git directory.
/// `--git-path` handles linked worktrees, whose git dir is not `.git/`.
async fn git_dir_paths(
    repo_root: &Path,
) -> color_eyre::Result<(PathBuf, Vec<(PathBuf, RepoOperation)>)> {
    let mut args = vec!["rev-parse", "--git-path", "FETCH_HEAD"];
    for (marker, _) in OPERATION_MARKERS {
        args.extend(["--git-path", marker]);
    }
    let output = git_output(repo_root, &args).await?;
    let mut paths = output.lines().map(|path| repo_root.join(path.trim()));
    let fetch_head = paths.next().unwrap_or_default();
    let markers = paths
        .zip(OPERATION_MARKERS)
        .map(|(path, (_, operation))| (path, operation))
        .collect();
    Ok((fetch_head, markers))
}

async fn operation_in_progress(markers: &[(PathBuf, RepoOperation)]) -> Option<RepoOperation> {
    for (path, operation) in markers {
        if fs::try_exists(path).await.unwrap_or(false) {
            return Some(*operation);
        }
    }
    None
}

/// `@{-1}` when it names a branch. Missing history or a previously detached
/// HEAD is not an error, just no previous branch.
async fn previous_branch(repo_root: &Path) -> color_eyre::Result<Option<String>> {
    let output = git_output_raw(repo_root, &["rev-parse", "--symbolic-full-name", "@{-1}"]).await?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .trim()
        .strip_prefix("refs/heads/")
        .map(ToOwned::to_owned))
}
