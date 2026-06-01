//! Whole-diff text indexes for fast file selection.
//!
//! This module owns the cheap index that sits between raw `git diff` output
//! and the fully parsed review snapshot. It keeps one review-scope patch in
//! memory, records the byte range for each file patch, and lets callers build a
//! plain `DiffView` for a selected file without running another `git diff`
//! command.

use std::{collections::HashMap, ops::Range, path::Path, sync::Arc};

use color_eyre::eyre::WrapErr;

use super::{DiffView, preview::load_diff_preview_for_working_tree};
use crate::git::{
    BranchCompareSelection, CommitCompareSelection, FileEntry,
    command::git_output,
    parse::{build_branch_diff_range, resolve_diff_filetype},
};

#[derive(Debug, Clone)]
pub struct ReviewDiffTextIndex {
    diff: Arc<str>,
    files: HashMap<String, Range<usize>>,
    file_order: Vec<String>,
}

impl ReviewDiffTextIndex {
    pub fn from_diff_text_owned(diff: String) -> Self {
        let (files, file_order) = scan_file_ranges(&diff);
        Self {
            diff: Arc::from(diff),
            files,
            file_order,
        }
    }

    pub fn diff_text(&self) -> &str {
        &self.diff
    }

    pub fn file_count(&self) -> usize {
        self.file_order.len()
    }

    pub fn contains_file(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }

    pub fn build_diff_view(&self, file: &FileEntry) -> Option<DiffView> {
        let range = self.files.get(file.path.as_str())?.clone();
        Some(super::build_diff_view_from_diff_text(
            &self.diff[range],
            file.filetype.or_else(|| resolve_diff_filetype(&file.path)),
        ))
    }
}

pub async fn load_review_diff_text_index_for_working_tree(
    repo_root: &Path,
    files: &[FileEntry],
) -> color_eyre::Result<ReviewDiffTextIndex> {
    if files.iter().any(|file| is_unmerged_status(&file.status)) {
        return load_working_tree_text_index_file_by_file(repo_root, files).await;
    }

    let mut diff = String::new();
    if files.iter().any(|file| file.status != "??") {
        diff = git_output(
            repo_root,
            &["diff", "--no-color", "--find-renames", "HEAD", "--"],
        )
        .await?;
    }

    append_untracked_diffs(repo_root, files, &mut diff).await?;
    Ok(ReviewDiffTextIndex::from_diff_text_owned(diff))
}

pub async fn load_review_diff_text_index_for_commit_compare(
    repo_root: &Path,
    selection: &CommitCompareSelection,
) -> color_eyre::Result<ReviewDiffTextIndex> {
    let diff = git_output(
        repo_root,
        &[
            "diff",
            "--no-color",
            "--find-renames",
            selection.base_ref.as_str(),
            selection.commit_hash.as_str(),
        ],
    )
    .await?;
    Ok(ReviewDiffTextIndex::from_diff_text_owned(diff))
}

pub async fn load_review_diff_text_index_for_branch_compare(
    repo_root: &Path,
    selection: &BranchCompareSelection,
) -> color_eyre::Result<ReviewDiffTextIndex> {
    let diff_range = build_branch_diff_range(selection);
    let diff = git_output(
        repo_root,
        &["diff", "--no-color", "--find-renames", diff_range.as_str()],
    )
    .await?;
    Ok(ReviewDiffTextIndex::from_diff_text_owned(diff))
}

async fn load_working_tree_text_index_file_by_file(
    repo_root: &Path,
    files: &[FileEntry],
) -> color_eyre::Result<ReviewDiffTextIndex> {
    let mut diff = String::new();
    for file in files {
        let preview = load_diff_preview_for_working_tree(repo_root, file, false)
            .await
            .wrap_err_with(|| format!("failed to load preview for {}", file.path))?;
        append_preview_diff(&mut diff, &preview.diff);
    }
    Ok(ReviewDiffTextIndex::from_diff_text_owned(diff))
}

async fn append_untracked_diffs(
    repo_root: &Path,
    files: &[FileEntry],
    diff: &mut String,
) -> color_eyre::Result<()> {
    for file in files.iter().filter(|file| file.status == "??") {
        let preview = load_diff_preview_for_working_tree(repo_root, file, false)
            .await
            .wrap_err_with(|| format!("failed to load untracked preview for {}", file.path))?;
        append_preview_diff(diff, &preview.diff);
    }
    Ok(())
}

fn append_preview_diff(combined: &mut String, patch: &str) {
    if patch.trim().is_empty() {
        return;
    }
    if !combined.is_empty() && !combined.ends_with('\n') {
        combined.push('\n');
    }
    combined.push_str(patch);
    if !combined.ends_with('\n') {
        combined.push('\n');
    }
}

fn scan_file_ranges(diff: &str) -> (HashMap<String, Range<usize>>, Vec<String>) {
    let starts = diff_file_starts(diff);
    let mut files = HashMap::with_capacity(starts.len());
    let mut file_order = Vec::with_capacity(starts.len());

    for (index, start) in starts.iter().copied().enumerate() {
        let end = starts.get(index + 1).copied().unwrap_or(diff.len());
        let patch = &diff[start..end];
        let Some(path) = file_path_from_patch(patch) else {
            continue;
        };
        if !files.contains_key(path.as_str()) {
            file_order.push(path.clone());
        }
        files.insert(path, start..end);
    }

    (files, file_order)
}

fn diff_file_starts(diff: &str) -> Vec<usize> {
    let mut starts = Vec::new();
    let bytes = diff.as_bytes();
    let mut line_start = 0usize;

    while line_start < bytes.len() {
        let line_end = memchr::memchr(b'\n', &bytes[line_start..])
            .map(|offset| line_start + offset + 1)
            .unwrap_or(bytes.len());
        if bytes[line_start..line_end].starts_with(b"diff --git ") {
            starts.push(line_start);
        }
        line_start = line_end;
    }

    starts
}

fn file_path_from_patch(patch: &str) -> Option<String> {
    let mut old_path = None::<String>;
    let mut diff_header_path = None::<String>;

    for line in patch.lines() {
        if let Some((_, next)) = parse_git_diff_names(line) {
            diff_header_path = Some(next);
            continue;
        }

        if let Some(path) = parse_unified_header_path(line, "--- ") {
            if path != "/dev/null" {
                old_path = Some(path);
            }
            continue;
        }

        if let Some(path) = parse_unified_header_path(line, "+++ ") {
            return Some(if path == "/dev/null" {
                old_path.or(diff_header_path)?
            } else {
                path
            });
        }

        if line.starts_with("@@ ") {
            break;
        }
    }

    diff_header_path.or(old_path)
}

fn parse_git_diff_names(line: &str) -> Option<(String, String)> {
    let mut rest = line.strip_prefix("diff --git ")?;
    let prev = parse_git_header_path(&mut rest)?;
    rest = rest.trim_start();
    let next = parse_git_header_path(&mut rest)?;
    Some((prev, next))
}

fn parse_git_header_path(rest: &mut &str) -> Option<String> {
    let value = rest.trim_start();
    if let Some(after_quote) = value.strip_prefix('"') {
        for (index, ch) in after_quote.char_indices() {
            if ch == '"' {
                let path = &after_quote[..index];
                *rest = &after_quote[index + ch.len_utf8()..];
                return strip_git_side_prefix(path).map(ToOwned::to_owned);
            }
        }
        None
    } else {
        let end = value.find(char::is_whitespace).unwrap_or(value.len());
        let path = &value[..end];
        *rest = &value[end..];
        strip_git_side_prefix(path).map(ToOwned::to_owned)
    }
}

fn parse_unified_header_path(line: &str, prefix: &str) -> Option<String> {
    let rest = line.strip_prefix(prefix)?.trim_start();
    let path = rest
        .split('\t')
        .next()
        .unwrap_or(rest)
        .split('\r')
        .next()
        .unwrap_or(rest)
        .trim();
    Some(strip_git_side_prefix(path).unwrap_or(path).to_string())
}

fn strip_git_side_prefix(path: &str) -> Option<&str> {
    path.strip_prefix("a/")
        .or_else(|| path.strip_prefix("b/"))
        .or_else(|| (path == "/dev/null").then_some(path))
}

fn is_unmerged_status(status: &str) -> bool {
    status.contains('U')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_index_slices_files_from_whole_diff() {
        let diff = concat!(
            "diff --git a/src/a.rs b/src/a.rs\n",
            "--- a/src/a.rs\n",
            "+++ b/src/a.rs\n",
            "@@ -1,1 +1,1 @@\n",
            "-old\n",
            "+new\n",
            "diff --git a/src/b.rs b/src/b.rs\n",
            "--- a/src/b.rs\n",
            "+++ b/src/b.rs\n",
            "@@ -1,0 +1,1 @@\n",
            "+added\n",
        );

        let index = ReviewDiffTextIndex::from_diff_text_owned(diff.to_string());

        assert_eq!(index.file_count(), 2);
        assert!(index.contains_file("src/a.rs"));
        assert!(index.contains_file("src/b.rs"));
        assert!(!index.contains_file("src/missing.rs"));
    }

    #[test]
    fn text_index_uses_old_path_for_deletions() {
        let diff = concat!(
            "diff --git a/src/deleted.rs b/src/deleted.rs\n",
            "deleted file mode 100644\n",
            "--- a/src/deleted.rs\n",
            "+++ /dev/null\n",
            "@@ -1,1 +0,0 @@\n",
            "-old\n",
        );

        let index = ReviewDiffTextIndex::from_diff_text_owned(diff.to_string());

        assert!(index.contains_file("src/deleted.rs"));
    }

    #[test]
    fn text_index_uses_new_path_for_renames() {
        let diff = concat!(
            "diff --git a/src/old.rs b/src/new.rs\n",
            "similarity index 98%\n",
            "rename from src/old.rs\n",
            "rename to src/new.rs\n",
            "--- a/src/old.rs\n",
            "+++ b/src/new.rs\n",
            "@@ -1,1 +1,1 @@\n",
            "-old\n",
            "+new\n",
        );

        let index = ReviewDiffTextIndex::from_diff_text_owned(diff.to_string());

        assert!(index.contains_file("src/new.rs"));
        assert!(!index.contains_file("src/old.rs"));
    }
}
