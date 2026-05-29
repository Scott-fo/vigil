//! Branch merge preparation.
//!
//! This module owns the repository mutations needed to prepare a branch merge
//! for review. It validates refs, switches to the destination branch when
//! needed, and runs `git merge --no-ff --no-commit` so callers can inspect the
//! working tree before creating the merge commit.

use std::{path::Path, process::Output};

use color_eyre::eyre::{WrapErr, bail, eyre};

use super::{
    BranchMergeOutcome, BranchMergeRequest,
    command::{git_output, git_output_raw, git_success, stderr_error},
    refs::resolve_current_branch_ref,
};

pub async fn prepare_branch_merge(
    repo_root: &Path,
    request: &BranchMergeRequest,
) -> color_eyre::Result<BranchMergeOutcome> {
    let source_ref = normalized_ref(&request.source_ref, "source")?;
    let destination_ref = normalized_ref(&request.destination_ref, "destination")?;
    if source_ref == destination_ref {
        bail!("source and destination refs must differ");
    }

    ensure_source_ref_exists(repo_root, source_ref).await?;
    ensure_destination_is_local_branch(repo_root, destination_ref).await?;
    ensure_working_tree_clean(repo_root).await?;
    switch_to_destination_branch(repo_root, destination_ref).await?;

    let output = git_output_raw(repo_root, &["merge", "--no-ff", "--no-commit", source_ref])
        .await
        .wrap_err("failed to start branch merge")?;

    match output.status.code() {
        Some(0) => {
            if merge_in_progress(repo_root).await? {
                Ok(BranchMergeOutcome::Prepared {
                    source_ref: source_ref.to_string(),
                    destination_ref: destination_ref.to_string(),
                })
            } else {
                Ok(BranchMergeOutcome::AlreadyUpToDate {
                    source_ref: source_ref.to_string(),
                    destination_ref: destination_ref.to_string(),
                })
            }
        }
        Some(1) if merge_in_progress(repo_root).await.unwrap_or(false) => {
            Ok(BranchMergeOutcome::Conflicted {
                source_ref: source_ref.to_string(),
                destination_ref: destination_ref.to_string(),
            })
        }
        _ => Err(output_error("failed to start branch merge", &output)),
    }
}

fn normalized_ref<'a>(value: &'a str, role: &str) -> color_eyre::Result<&'a str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        bail!("select a {role} ref");
    }
    Ok(trimmed)
}

async fn ensure_source_ref_exists(repo_root: &Path, source_ref: &str) -> color_eyre::Result<()> {
    let revision = format!("{source_ref}^{{commit}}");
    let output = git_output_raw(
        repo_root,
        &["rev-parse", "--verify", "--quiet", revision.as_str()],
    )
    .await
    .wrap_err("failed to validate source ref")?;
    if output.status.success() {
        Ok(())
    } else {
        bail!("source ref is not a commit: {source_ref}");
    }
}

async fn ensure_destination_is_local_branch(
    repo_root: &Path,
    destination_ref: &str,
) -> color_eyre::Result<()> {
    let full_ref = format!("refs/heads/{destination_ref}");
    let output = git_output_raw(
        repo_root,
        &["show-ref", "--verify", "--quiet", full_ref.as_str()],
    )
    .await
    .wrap_err("failed to validate destination branch")?;

    match output.status.code() {
        Some(0) => Ok(()),
        Some(1) => bail!("destination must be a local branch: {destination_ref}"),
        _ => Err(stderr_error(&output)),
    }
}

async fn ensure_working_tree_clean(repo_root: &Path) -> color_eyre::Result<()> {
    let output = git_output(
        repo_root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )
    .await
    .wrap_err("failed to inspect working tree")?;
    if output.is_empty() {
        Ok(())
    } else {
        bail!("working tree must be clean before merging");
    }
}

async fn switch_to_destination_branch(
    repo_root: &Path,
    destination_ref: &str,
) -> color_eyre::Result<()> {
    if resolve_current_branch_ref(repo_root)
        .await
        .ok()
        .flatten()
        .as_deref()
        == Some(destination_ref)
    {
        return Ok(());
    }

    git_success(repo_root, &["switch", destination_ref])
        .await
        .wrap_err_with(|| format!("failed to switch to {destination_ref}"))
}

async fn merge_in_progress(repo_root: &Path) -> color_eyre::Result<bool> {
    let output = git_output_raw(
        repo_root,
        &["rev-parse", "--verify", "--quiet", "MERGE_HEAD"],
    )
    .await
    .wrap_err("failed to inspect merge state")?;
    Ok(output.status.success())
}

fn output_error(context: &str, output: &Output) -> color_eyre::Report {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    match (stderr.is_empty(), stdout.is_empty()) {
        (false, false) => eyre!("{context}: {stderr}\n{stdout}"),
        (false, true) => eyre!("{context}: {stderr}"),
        (true, false) => eyre!("{context}: {stdout}"),
        (true, true) => eyre!("{context}"),
    }
}
