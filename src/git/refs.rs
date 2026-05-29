//! Branch/reference listing and branch comparison file lists.
//!
//! This module owns ref-shaped repository facts: which refs are useful for
//! comparison, which ref is currently checked out, and which files differ
//! between two branch comparison endpoints.

use std::path::Path;

use color_eyre::eyre::WrapErr;

use super::{
    BranchCompareRefs, BranchCompareSelection, FileEntry,
    changed_files::load_diff_name_status_files,
    command::{git_output, git_output_raw, stderr_error},
    parse::build_branch_diff_range,
};

pub async fn list_comparable_refs(repo_root: &Path) -> color_eyre::Result<Vec<String>> {
    let output = git_output(
        repo_root,
        &[
            "for-each-ref",
            "--format=%(refname)\t%(refname:short)",
            "refs/heads",
            "refs/remotes",
        ],
    )
    .await?;

    let mut refs = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let (full_ref, short_ref) = line.split_once('\t')?;
            let short_ref = short_ref.trim();
            if short_ref.is_empty() || short_ref == "HEAD" {
                return None;
            }
            if full_ref.starts_with("refs/remotes/")
                && (!short_ref.contains('/') || short_ref.ends_with("/HEAD"))
            {
                return None;
            }
            Some(short_ref.to_string())
        })
        .collect::<Vec<_>>();

    refs.sort();
    refs.dedup();
    Ok(refs)
}

pub async fn load_branch_compare_refs(repo_root: &Path) -> color_eyre::Result<BranchCompareRefs> {
    Ok(BranchCompareRefs {
        refs: list_comparable_refs(repo_root).await?,
        current_ref: resolve_current_branch_ref(repo_root).await?,
    })
}

pub async fn load_files_with_branch_diff(
    repo_root: &Path,
    selection: &BranchCompareSelection,
) -> color_eyre::Result<Vec<FileEntry>> {
    load_diff_name_status_files(
        repo_root,
        &[
            "diff",
            "--name-status",
            "--find-renames",
            "-z",
            build_branch_diff_range(selection).as_str(),
        ],
    )
    .await
}

pub(crate) async fn resolve_current_branch_ref(
    repo_root: &Path,
) -> color_eyre::Result<Option<String>> {
    let output = git_output_raw(repo_root, &["symbolic-ref", "--quiet", "--short", "HEAD"])
        .await
        .wrap_err("failed to resolve current branch")?;

    match output.status.code() {
        Some(0) => {
            let current_ref = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if current_ref.is_empty() {
                Ok(None)
            } else {
                Ok(Some(current_ref))
            }
        }
        Some(1) => Ok(None),
        _ => Err(stderr_error(&output)),
    }
}
