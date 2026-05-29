use std::path::Path;

use color_eyre::eyre::eyre;

use super::super::{
    BlameCommitDetails, BlameTarget, CommitCompareSelection, EMPTY_TREE_HASH,
    command::git_output,
    parse::{is_uncommitted_blame_hash, parse_blame_porcelain_header, parse_commit_show_output},
};

pub async fn load_blame_commit_details(
    repo_root: &Path,
    target: &BlameTarget,
) -> color_eyre::Result<BlameCommitDetails> {
    let blame_output = git_output(
        repo_root,
        &[
            "blame",
            "--porcelain",
            "-L",
            &format!("{0},{0}", target.line_number),
            "--",
            target.file_path.as_str(),
        ],
    )
    .await?;

    let header = parse_blame_porcelain_header(&blame_output).ok_or_else(|| {
        eyre!(
            "unable to parse blame output for {}:{}",
            target.file_path,
            target.line_number
        )
    })?;

    if is_uncommitted_blame_hash(&header.commit_hash) {
        return Ok(BlameCommitDetails {
            target: target.clone(),
            commit_hash: header.commit_hash,
            short_hash: "working-tree".to_string(),
            author: if header.author.is_empty() {
                "Uncommitted".to_string()
            } else {
                header.author
            },
            date: header.date,
            subject: if header.summary.is_empty() {
                "Uncommitted line changes".to_string()
            } else {
                header.summary
            },
            description: "This line has uncommitted changes. Commit comparison is unavailable."
                .to_string(),
            is_uncommitted: true,
            compare_selection: None,
        });
    }

    let show_output = git_output(
        repo_root,
        &[
            "show",
            "-s",
            "--date=short",
            "--format=%H%x1f%h%x1f%P%x1f%ad%x1f%an%x1f%s%x1f%b",
            header.commit_hash.as_str(),
        ],
    )
    .await?;

    let commit = parse_commit_show_output(&show_output)
        .ok_or_else(|| eyre!("unable to parse commit metadata for {}", header.commit_hash))?;
    let subject = if commit.subject.is_empty() {
        header.summary
    } else {
        commit.subject
    };
    let description = if commit.description.trim().is_empty() {
        "No commit description.".to_string()
    } else {
        commit.description
    };
    let compare_base = commit
        .parent_hashes
        .first()
        .cloned()
        .unwrap_or_else(|| EMPTY_TREE_HASH.to_string());
    let commit_hash = commit.commit_hash;
    let short_hash = commit.short_hash;

    Ok(BlameCommitDetails {
        target: target.clone(),
        commit_hash: commit_hash.clone(),
        short_hash: short_hash.clone(),
        author: if commit.author.is_empty() {
            header.author
        } else {
            commit.author
        },
        date: if commit.date.is_empty() {
            header.date
        } else {
            commit.date
        },
        description,
        is_uncommitted: false,
        compare_selection: Some(CommitCompareSelection {
            base_ref: compare_base,
            commit_hash: commit_hash.clone(),
            short_hash: short_hash.clone(),
            subject: subject.clone(),
        }),
        subject,
    })
}
