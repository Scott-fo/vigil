//! Parsed review-scope diff snapshots.
//!
//! A snapshot owns the structured file metadata parsed from one review diff.
//! Callers can build individual file views from this metadata without running
//! another `git diff` process or reparsing a per-file patch string.

use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use color_eyre::eyre::WrapErr;
use tokio::task;

use super::{
    DiffPreviewData, DiffSearchIndex, DiffView, FileDiffMetadata,
    build_diff_view_from_file_metadata, parse_patch_files,
    preview::load_diff_preview_for_working_tree, stats::ReviewDiffStats,
};
use crate::git::{
    BranchCompareSelection, CommitCompareSelection, FileEntry, command::git_output,
    parse::build_branch_diff_range, status::is_untracked_status,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DiffFileMetrics {
    pub split_line_count: usize,
    pub unified_line_count: usize,
    pub addition_line_count: usize,
    pub deletion_line_count: usize,
    pub new_side_line_count: usize,
    pub old_side_line_count: usize,
}

#[derive(Debug, Clone, Default)]
pub struct ReviewDiffSnapshot {
    generation: u64,
    files: HashMap<String, FileDiffMetadata>,
    file_order: Vec<String>,
    metrics: HashMap<String, DiffFileMetrics>,
}

impl ReviewDiffSnapshot {
    pub fn from_diff_text(diff: &str, cache_key_prefix: Option<&str>) -> color_eyre::Result<Self> {
        let parsed = parse_patch_files(diff, cache_key_prefix, true)?;
        let mut snapshot = Self::default();
        for patch in parsed {
            for file in patch.files {
                snapshot.insert_file(file);
            }
        }
        Ok(snapshot)
    }

    pub fn with_generation(mut self, generation: u64) -> Self {
        self.generation = generation;
        self
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn file_count(&self) -> usize {
        self.file_order.len()
    }

    pub fn line_count(&self) -> usize {
        self.metrics
            .values()
            .map(|metrics| metrics.unified_line_count)
            .sum()
    }

    pub fn stats(&self) -> ReviewDiffStats {
        self.metrics
            .values()
            .fold(ReviewDiffStats::default(), |mut stats, metrics| {
                add_file_metrics(&mut stats, metrics);
                stats
            })
    }

    pub fn stats_for_working_tree(&self, files: &[FileEntry]) -> ReviewDiffStats {
        let untracked_paths = files
            .iter()
            .filter(|file| is_untracked_status(&file.status))
            .map(|file| file.path.as_str())
            .collect::<HashSet<_>>();

        let mut tracked = ReviewDiffStats::default();
        let mut untracked = ReviewDiffStats::default();
        for (path, metrics) in &self.metrics {
            if untracked_paths.contains(path.as_str()) {
                add_file_metrics(&mut untracked, metrics);
            } else {
                add_file_metrics(&mut tracked, metrics);
            }
        }

        (tracked + untracked).with_working_tree_scopes(tracked, untracked)
    }

    pub fn contains_file(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }

    pub fn metrics_for_file(&self, path: &str) -> Option<DiffFileMetrics> {
        self.metrics.get(path).copied()
    }

    pub fn build_diff_view(&self, file: &FileEntry) -> Option<DiffView> {
        self.files
            .get(file.path.as_str())
            .map(build_diff_view_from_file_metadata)
    }

    pub fn build_search_index(&self) -> DiffSearchIndex {
        DiffSearchIndex::from_file_metadata(self.files_in_order())
    }

    fn files_in_order(&self) -> impl Iterator<Item = &FileDiffMetadata> {
        self.file_order
            .iter()
            .filter_map(|path| self.files.get(path))
    }

    fn append_preview_data(&mut self, preview: &DiffPreviewData) -> color_eyre::Result<()> {
        if preview.merge_conflict.is_some() || preview.diff.trim().is_empty() {
            return Ok(());
        }

        let parsed = parse_patch_files(&preview.diff, None, true)?;
        for patch in parsed {
            for file in patch.files {
                self.insert_file(file);
            }
        }
        Ok(())
    }

    fn insert_file(&mut self, file: FileDiffMetadata) {
        if !self.files.contains_key(file.name.as_str()) {
            self.file_order.push(file.name.clone());
        }
        self.metrics
            .insert(file.name.clone(), DiffFileMetrics::from_file(&file));
        self.files.insert(file.name.clone(), file);
    }
}

impl DiffFileMetrics {
    fn from_file(file: &FileDiffMetadata) -> Self {
        Self {
            split_line_count: file.split_line_count,
            unified_line_count: file.unified_line_count,
            addition_line_count: file.hunks.iter().map(|hunk| hunk.addition_lines).sum(),
            deletion_line_count: file.hunks.iter().map(|hunk| hunk.deletion_lines).sum(),
            new_side_line_count: file.addition_lines.len(),
            old_side_line_count: file.deletion_lines.len(),
        }
    }
}

pub async fn load_review_diff_snapshot_for_working_tree(
    repo_root: &Path,
    files: &[FileEntry],
) -> color_eyre::Result<ReviewDiffSnapshot> {
    if files.iter().any(|file| is_unmerged_status(&file.status)) {
        return load_working_tree_snapshot_file_by_file(repo_root, files).await;
    }

    let mut snapshot = ReviewDiffSnapshot::default();

    if files.iter().any(|file| !is_untracked_status(&file.status)) {
        let diff = git_output(
            repo_root,
            &["diff", "--no-color", "--find-renames", "HEAD", "--"],
        )
        .await?;
        snapshot = snapshot_from_diff_text(diff, Some("working-tree")).await?;
    }

    for file in files
        .iter()
        .filter(|file| is_untracked_status(&file.status))
    {
        let preview = load_diff_preview_for_working_tree(repo_root, file, false).await?;
        snapshot.append_preview_data(&preview)?;
    }

    Ok(snapshot)
}

async fn load_working_tree_snapshot_file_by_file(
    repo_root: &Path,
    files: &[FileEntry],
) -> color_eyre::Result<ReviewDiffSnapshot> {
    let mut snapshot = ReviewDiffSnapshot::default();
    for file in files {
        let preview = load_diff_preview_for_working_tree(repo_root, file, false).await?;
        snapshot.append_preview_data(&preview)?;
    }
    Ok(snapshot)
}

pub async fn load_review_diff_snapshot_for_commit_compare(
    repo_root: &Path,
    selection: &CommitCompareSelection,
) -> color_eyre::Result<ReviewDiffSnapshot> {
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
    snapshot_from_diff_text(diff, Some("commit")).await
}

pub async fn load_review_diff_snapshot_for_branch_compare(
    repo_root: &Path,
    selection: &BranchCompareSelection,
) -> color_eyre::Result<ReviewDiffSnapshot> {
    let diff_range = build_branch_diff_range(selection);
    let diff = git_output(
        repo_root,
        &["diff", "--no-color", "--find-renames", diff_range.as_str()],
    )
    .await?;
    snapshot_from_diff_text(diff, Some("branch")).await
}

async fn snapshot_from_diff_text(
    diff: String,
    cache_key_prefix: Option<&'static str>,
) -> color_eyre::Result<ReviewDiffSnapshot> {
    task::spawn_blocking(move || ReviewDiffSnapshot::from_diff_text(&diff, cache_key_prefix))
        .await
        .wrap_err("review diff snapshot parse task failed")?
}

fn is_unmerged_status(status: &str) -> bool {
    status.contains('U')
}

fn add_file_metrics(stats: &mut ReviewDiffStats, metrics: &DiffFileMetrics) {
    stats.file_count = stats.file_count.saturating_add(1);
    stats.additions = stats.additions.saturating_add(metrics.addition_line_count);
    stats.deletions = stats.deletions.saturating_add(metrics.deletion_line_count);
    stats.lines = stats.lines.saturating_add(metrics.unified_line_count);
    stats.split_lines = stats.split_lines.saturating_add(metrics.split_line_count);
}
