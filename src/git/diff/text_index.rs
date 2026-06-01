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
    command::git_output_streamed,
    parse::{build_branch_diff_range, resolve_diff_filetype},
};

#[derive(Debug, Clone)]
pub struct ReviewDiffTextIndex {
    diff: Arc<str>,
    files: HashMap<String, Range<usize>>,
    file_order: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ReviewDiffStreamedFile {
    pub path: String,
    pub diff: String,
}

#[derive(Debug, Default, Clone)]
pub struct ReviewDiffPartialTextIndex {
    files: HashMap<String, Arc<str>>,
}

impl ReviewDiffPartialTextIndex {
    pub fn from_diff_text_owned(diff: String) -> Self {
        let (files, file_order) = scan_file_ranges(&diff);
        let mut partial = Self {
            files: HashMap::with_capacity(file_order.len()),
        };
        for path in file_order {
            let Some(range) = files.get(&path) else {
                continue;
            };
            partial.insert_file_diff(path, diff[range.clone()].to_string());
        }
        partial
    }

    pub fn insert_file_diff(&mut self, path: String, diff: String) {
        self.files.insert(path, Arc::from(diff));
    }

    pub fn contains_file(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }

    pub fn build_diff_view(&self, file: &FileEntry) -> Option<DiffView> {
        let diff = self.files.get(file.path.as_str())?;
        Some(super::build_diff_view_from_diff_text(
            diff,
            file.filetype.or_else(|| resolve_diff_filetype(&file.path)),
        ))
    }
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
    load_review_diff_text_index_for_working_tree_streaming(repo_root, files, |_| {}).await
}

pub async fn load_review_diff_text_index_for_working_tree_streaming<F>(
    repo_root: &Path,
    files: &[FileEntry],
    on_file: F,
) -> color_eyre::Result<ReviewDiffTextIndex>
where
    F: FnMut(ReviewDiffStreamedFile) + Send,
{
    if files.iter().any(|file| is_unmerged_status(&file.status)) {
        return load_working_tree_text_index_file_by_file(repo_root, files, on_file).await;
    }

    let mut diff = String::new();
    let mut on_file = on_file;
    if files.iter().any(|file| file.status != "??") {
        diff = stream_git_diff(
            repo_root,
            &["diff", "--no-color", "--find-renames", "HEAD", "--"],
            &mut on_file,
        )
        .await?;
    }

    append_untracked_diffs(repo_root, files, &mut diff, &mut on_file).await?;
    Ok(ReviewDiffTextIndex::from_diff_text_owned(diff))
}

pub async fn load_review_diff_text_index_for_commit_compare(
    repo_root: &Path,
    selection: &CommitCompareSelection,
) -> color_eyre::Result<ReviewDiffTextIndex> {
    load_review_diff_text_index_for_commit_compare_streaming(repo_root, selection, |_| {}).await
}

pub async fn load_review_diff_text_index_for_commit_compare_streaming<F>(
    repo_root: &Path,
    selection: &CommitCompareSelection,
    mut on_file: F,
) -> color_eyre::Result<ReviewDiffTextIndex>
where
    F: FnMut(ReviewDiffStreamedFile) + Send,
{
    let diff = stream_git_diff(
        repo_root,
        &[
            "diff",
            "--no-color",
            "--find-renames",
            selection.base_ref.as_str(),
            selection.commit_hash.as_str(),
        ],
        &mut on_file,
    )
    .await?;
    Ok(ReviewDiffTextIndex::from_diff_text_owned(diff))
}

pub async fn load_review_diff_text_index_for_branch_compare(
    repo_root: &Path,
    selection: &BranchCompareSelection,
) -> color_eyre::Result<ReviewDiffTextIndex> {
    load_review_diff_text_index_for_branch_compare_streaming(repo_root, selection, |_| {}).await
}

pub async fn load_review_diff_text_index_for_branch_compare_streaming<F>(
    repo_root: &Path,
    selection: &BranchCompareSelection,
    mut on_file: F,
) -> color_eyre::Result<ReviewDiffTextIndex>
where
    F: FnMut(ReviewDiffStreamedFile) + Send,
{
    let diff_range = build_branch_diff_range(selection);
    let diff = stream_git_diff(
        repo_root,
        &["diff", "--no-color", "--find-renames", diff_range.as_str()],
        &mut on_file,
    )
    .await?;
    Ok(ReviewDiffTextIndex::from_diff_text_owned(diff))
}

async fn load_working_tree_text_index_file_by_file<F>(
    repo_root: &Path,
    files: &[FileEntry],
    mut on_file: F,
) -> color_eyre::Result<ReviewDiffTextIndex>
where
    F: FnMut(ReviewDiffStreamedFile) + Send,
{
    let mut diff = String::new();
    for file in files {
        let preview = load_diff_preview_for_working_tree(repo_root, file, false)
            .await
            .wrap_err_with(|| format!("failed to load preview for {}", file.path))?;
        send_preview_diff(&mut on_file, file.path.clone(), &preview.diff);
        append_preview_diff(&mut diff, &preview.diff);
    }
    Ok(ReviewDiffTextIndex::from_diff_text_owned(diff))
}

async fn append_untracked_diffs<F>(
    repo_root: &Path,
    files: &[FileEntry],
    diff: &mut String,
    on_file: &mut F,
) -> color_eyre::Result<()>
where
    F: FnMut(ReviewDiffStreamedFile),
{
    for file in files.iter().filter(|file| file.status == "??") {
        let preview = load_diff_preview_for_working_tree(repo_root, file, false)
            .await
            .wrap_err_with(|| format!("failed to load untracked preview for {}", file.path))?;
        send_preview_diff(on_file, file.path.clone(), &preview.diff);
        append_preview_diff(diff, &preview.diff);
    }
    Ok(())
}

async fn stream_git_diff<F>(
    repo_root: &Path,
    args: &[&str],
    on_file: &mut F,
) -> color_eyre::Result<String>
where
    F: FnMut(ReviewDiffStreamedFile),
{
    let mut parser = DiffFileStreamParser::default();
    let diff = git_output_streamed(repo_root, args, |chunk| {
        parser.push_chunk(chunk, on_file);
        Ok(())
    })
    .await?;
    parser.finish(on_file);
    Ok(diff)
}

fn send_preview_diff<F>(on_file: &mut F, path: String, diff: &str)
where
    F: FnMut(ReviewDiffStreamedFile),
{
    if diff.trim().is_empty() {
        return;
    }
    on_file(ReviewDiffStreamedFile {
        path,
        diff: ensure_trailing_newline(diff),
    });
}

fn ensure_trailing_newline(diff: &str) -> String {
    let mut diff = diff.to_string();
    if !diff.ends_with('\n') {
        diff.push('\n');
    }
    diff
}

#[derive(Default)]
struct DiffFileStreamParser {
    current_patch: Vec<u8>,
    current_line: Vec<u8>,
}

impl DiffFileStreamParser {
    fn push_chunk<F>(&mut self, chunk: &[u8], on_file: &mut F)
    where
        F: FnMut(ReviewDiffStreamedFile),
    {
        let mut remaining = chunk;
        while !remaining.is_empty() {
            let Some(newline) = memchr::memchr(b'\n', remaining) else {
                self.current_line.extend_from_slice(remaining);
                break;
            };
            let line_end = newline + 1;
            self.current_line.extend_from_slice(&remaining[..line_end]);
            self.push_current_line(on_file);
            remaining = &remaining[line_end..];
        }
    }

    fn finish<F>(&mut self, on_file: &mut F)
    where
        F: FnMut(ReviewDiffStreamedFile),
    {
        if !self.current_line.is_empty() {
            self.push_current_line(on_file);
        }
        self.finish_current_patch(on_file);
    }

    fn push_current_line<F>(&mut self, on_file: &mut F)
    where
        F: FnMut(ReviewDiffStreamedFile),
    {
        if self.current_line.starts_with(b"diff --git ") && !self.current_patch.is_empty() {
            self.finish_current_patch(on_file);
        }
        self.current_patch.extend_from_slice(&self.current_line);
        self.current_line.clear();
    }

    fn finish_current_patch<F>(&mut self, on_file: &mut F)
    where
        F: FnMut(ReviewDiffStreamedFile),
    {
        if self.current_patch.is_empty() {
            return;
        }

        let diff = String::from_utf8_lossy(&self.current_patch).into_owned();
        self.current_patch.clear();
        let Some(path) = file_path_from_patch(&diff) else {
            return;
        };
        on_file(ReviewDiffStreamedFile { path, diff });
    }
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

    #[test]
    fn stream_parser_emits_completed_files_across_chunk_boundaries() {
        let mut parser = DiffFileStreamParser::default();
        let mut files = Vec::new();
        let mut on_file = |file| files.push(file);

        for chunk in [
            "diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n",
            "+++ b/src/a.rs\n@@ -1,1 +1,1 @@\n-old\n+new\n",
            "diff --git a/src/b.rs b/src/b.rs\n--- a/src/b.rs\n+++ b/src/b.rs\n",
            "@@ -0,0 +1,1 @@\n+added\n",
        ] {
            parser.push_chunk(chunk.as_bytes(), &mut on_file);
        }
        parser.finish(&mut on_file);

        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, "src/a.rs");
        assert!(files[0].diff.contains("+new\n"));
        assert_eq!(files[1].path, "src/b.rs");
        assert!(files[1].diff.contains("+added\n"));
    }
}
