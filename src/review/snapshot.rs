use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use color_eyre::eyre::WrapErr;
use tokio::fs;

use crate::git::{self, FileEntry};

use super::{ReviewScope, ReviewSnapshot};

#[derive(Debug, Clone)]
pub struct BuildReviewSnapshotOptions {
    pub repo_root: PathBuf,
    pub worktree_root: PathBuf,
    pub scope: ReviewScope,
    pub files: Vec<FileEntry>,
    pub extra_context: String,
}

pub async fn build_review_snapshot(
    options: BuildReviewSnapshotOptions,
) -> color_eyre::Result<ReviewSnapshot> {
    let head_sha = resolve_optional_revision(&options.repo_root, "HEAD").await;
    let branch = resolve_current_branch(&options.repo_root).await;
    let scope = hydrate_scope(&options.repo_root, options.scope).await?;
    let files = options
        .files
        .iter()
        .map(|file| file.path.clone())
        .collect::<Vec<_>>();
    let patch = load_review_patch(&options.repo_root, &scope, &options.files).await?;
    let created_at_ms = now_ms();

    let id = snapshot_id(
        &options.repo_root,
        &options.worktree_root,
        head_sha.as_deref(),
        branch.as_deref(),
        &scope,
        &files,
        &patch,
    )?;

    Ok(ReviewSnapshot {
        id,
        repo_root: options.repo_root,
        worktree_root: options.worktree_root,
        head_sha,
        branch,
        scope,
        files,
        extra_context: options.extra_context,
        patch,
        created_at_ms,
    })
}

async fn hydrate_scope(repo_root: &Path, scope: ReviewScope) -> color_eyre::Result<ReviewScope> {
    match scope {
        ReviewScope::WorkingTree => Ok(ReviewScope::WorkingTree),
        ReviewScope::CommitCompare {
            base_ref,
            commit_hash,
            short_hash,
            subject,
            ..
        } => {
            let base_sha = resolve_optional_revision(repo_root, &base_ref).await;
            Ok(ReviewScope::CommitCompare {
                base_ref,
                base_sha,
                commit_hash,
                short_hash,
                subject,
            })
        }
        ReviewScope::BranchCompare {
            source_ref,
            destination_ref,
            ..
        } => {
            let source_sha = resolve_optional_revision(repo_root, &source_ref).await;
            let destination_sha = resolve_optional_revision(repo_root, &destination_ref).await;
            let merge_base = git::git_output(
                repo_root,
                &["merge-base", destination_ref.as_str(), source_ref.as_str()],
            )
            .await
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
            Ok(ReviewScope::BranchCompare {
                source_ref,
                source_sha,
                destination_ref,
                destination_sha,
                merge_base,
            })
        }
    }
}

async fn resolve_optional_revision(repo_root: &Path, revision: &str) -> Option<String> {
    git::git_output(repo_root, &["rev-parse", "--verify", revision])
        .await
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

async fn resolve_current_branch(repo_root: &Path) -> Option<String> {
    git::git_output(repo_root, &["branch", "--show-current"])
        .await
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

async fn load_review_patch(
    repo_root: &Path,
    scope: &ReviewScope,
    files: &[FileEntry],
) -> color_eyre::Result<String> {
    match scope {
        ReviewScope::WorkingTree => load_working_tree_review_patch(repo_root, files).await,
        ReviewScope::CommitCompare {
            base_ref,
            commit_hash,
            ..
        } => {
            let args = diff_args_with_paths(
                &[base_ref.as_str(), commit_hash.as_str()],
                tracked_review_paths(files),
            );
            let arg_refs = args.iter().map(String::as_str).collect::<Vec<_>>();
            git::git_output(repo_root, &arg_refs)
                .await
                .wrap_err("failed to load review patch")
        }
        ReviewScope::BranchCompare {
            source_ref,
            destination_ref,
            ..
        } => {
            let comparison = format!("{destination_ref}...{source_ref}");
            let args = diff_args_with_paths(&[comparison.as_str()], tracked_review_paths(files));
            let arg_refs = args.iter().map(String::as_str).collect::<Vec<_>>();
            git::git_output(repo_root, &arg_refs)
                .await
                .wrap_err("failed to load review patch")
        }
    }
}

async fn load_working_tree_review_patch(
    repo_root: &Path,
    files: &[FileEntry],
) -> color_eyre::Result<String> {
    let tracked_paths = tracked_review_paths(files);
    let mut patch = String::new();

    if !tracked_paths.is_empty() {
        if repo_has_head(repo_root).await {
            let args = diff_args_with_paths(&["HEAD"], tracked_paths);
            let arg_refs = args.iter().map(String::as_str).collect::<Vec<_>>();
            patch = git::git_output(repo_root, &arg_refs)
                .await
                .wrap_err("failed to load review patch")?;
        } else {
            let cached_args = diff_args_with_paths(&["--cached"], tracked_paths.clone());
            let cached_refs = cached_args.iter().map(String::as_str).collect::<Vec<_>>();
            append_patch(
                &mut patch,
                &git::git_output(repo_root, &cached_refs)
                    .await
                    .wrap_err("failed to load staged review patch")?,
            );

            let worktree_args = diff_args_with_paths(&[], tracked_paths);
            let worktree_refs = worktree_args.iter().map(String::as_str).collect::<Vec<_>>();
            append_patch(
                &mut patch,
                &git::git_output(repo_root, &worktree_refs)
                    .await
                    .wrap_err("failed to load unstaged review patch")?,
            );
        }
    }

    for file in files.iter().filter(|file| file.status == "??") {
        let untracked = load_untracked_file_patch(repo_root, &file.path).await?;
        append_patch(&mut patch, &untracked);
    }

    Ok(patch)
}

fn tracked_review_paths(files: &[FileEntry]) -> Vec<String> {
    files
        .iter()
        .filter(|file| file.status != "??")
        .map(|file| file.path.clone())
        .collect()
}

async fn repo_has_head(repo_root: &Path) -> bool {
    resolve_optional_revision(repo_root, "HEAD").await.is_some()
}

fn diff_args_with_paths(revisions: &[&str], paths: Vec<String>) -> Vec<String> {
    let mut args = vec![
        "diff".to_string(),
        "--no-color".to_string(),
        "--find-renames".to_string(),
    ];
    args.extend(revisions.iter().map(|revision| (*revision).to_string()));
    args.push("--".to_string());
    args.extend(paths);
    args
}

fn append_patch(patch: &mut String, addition: &str) {
    if addition.is_empty() {
        return;
    }
    if !patch.ends_with('\n') && !patch.is_empty() {
        patch.push('\n');
    }
    patch.push_str(addition);
}

async fn load_untracked_file_patch(
    repo_root: &Path,
    file_path: &str,
) -> color_eyre::Result<String> {
    let full_path = repo_root.join(file_path);
    let metadata = match fs::metadata(&full_path).await {
        Ok(metadata) if metadata.is_file() => metadata,
        _ => return Ok(String::new()),
    };
    let bytes = fs::read(&full_path)
        .await
        .wrap_err_with(|| format!("failed to read untracked file {file_path}"))?;
    if bytes.contains(&0) {
        return Ok(format!(
            "diff --git a/{file_path} b/{file_path}\nnew file mode {:o}\nBinary files /dev/null and b/{file_path} differ\n",
            git_file_mode(&metadata)
        ));
    }

    let contents = String::from_utf8_lossy(&bytes);
    let mut patch = format!(
        "diff --git a/{file_path} b/{file_path}\nnew file mode {:o}\n--- /dev/null\n+++ b/{file_path}\n@@ -0,0 +1,{} @@\n",
        git_file_mode(&metadata),
        contents.lines().count()
    );
    for line in contents.lines() {
        patch.push('+');
        patch.push_str(line);
        patch.push('\n');
    }
    if !contents.ends_with('\n') {
        patch.push_str("\\ No newline at end of file\n");
    }
    Ok(patch)
}

#[cfg(unix)]
fn git_file_mode(metadata: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;

    if metadata.permissions().mode() & 0o111 == 0 {
        0o100644
    } else {
        0o100755
    }
}

#[cfg(not(unix))]
fn git_file_mode(_metadata: &std::fs::Metadata) -> u32 {
    0o100644
}

fn snapshot_id(
    repo_root: &Path,
    worktree_root: &Path,
    head_sha: Option<&str>,
    branch: Option<&str>,
    scope: &ReviewScope,
    files: &[String],
    patch: &str,
) -> color_eyre::Result<String> {
    let mut input = String::new();
    input.push_str(&repo_root.display().to_string());
    input.push('\n');
    input.push_str(&worktree_root.display().to_string());
    input.push('\n');
    input.push_str(head_sha.unwrap_or(""));
    input.push('\n');
    input.push_str(branch.unwrap_or(""));
    input.push('\n');
    input.push_str(&serde_json::to_string(scope)?);
    input.push('\n');
    for file in files {
        input.push_str(file);
        input.push('\n');
    }
    input.push_str(patch);
    Ok(format!("v1-{:016x}", fnv1a64(input.as_bytes())))
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_hash_is_stable_for_same_inputs() {
        let scope = ReviewScope::WorkingTree;
        let files = vec!["src/lib.rs".to_string()];
        let first = snapshot_id(
            Path::new("/repo"),
            Path::new("/repo"),
            Some("abc"),
            Some("main"),
            &scope,
            &files,
            "diff",
        )
        .expect("hash");
        let second = snapshot_id(
            Path::new("/repo"),
            Path::new("/repo"),
            Some("abc"),
            Some("main"),
            &scope,
            &files,
            "diff",
        )
        .expect("hash");

        assert_eq!(first, second);
        assert!(first.starts_with("v1-"));
    }
}
