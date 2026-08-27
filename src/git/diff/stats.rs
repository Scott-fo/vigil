//! Fast review-scope diff statistics.
//!
//! This module gathers aggregate diff metrics without constructing a full
//! `ReviewDiffSnapshot`. The hot path uses `git diff --numstat -z`, which is
//! much smaller than patch output and is safe for paths containing whitespace or
//! unusual bytes. Callers should treat the returned line totals as changed-line
//! totals; richer rendered diff line counts are supplied later by the parsed
//! review snapshot when it finishes.
//!
//! Working-tree stats keep tracked edits and untracked new files as separate
//! scopes. Untracked files are counted as additions only.

use std::path::Path;

use color_eyre::eyre::WrapErr;
use tokio::{fs, io::AsyncReadExt, task::JoinSet};

use crate::git::{
    BranchCompareSelection, CommitCompareSelection, FileEntry, command::git_output_bytes,
    parse::build_branch_diff_range, status::is_untracked_status,
};

const UNTRACKED_STATS_CONCURRENCY: usize = 16;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DiffLineTotals {
    pub file_count: usize,
    pub additions: usize,
    pub deletions: usize,
    pub lines: usize,
    pub split_lines: usize,
}

impl DiffLineTotals {
    fn saturating_add_assign(&mut self, rhs: Self) {
        self.file_count = self.file_count.saturating_add(rhs.file_count);
        self.additions = self.additions.saturating_add(rhs.additions);
        self.deletions = self.deletions.saturating_add(rhs.deletions);
        self.lines = self.lines.saturating_add(rhs.lines);
        self.split_lines = self.split_lines.saturating_add(rhs.split_lines);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReviewDiffStats {
    pub file_count: usize,
    pub additions: usize,
    pub deletions: usize,
    pub lines: usize,
    pub split_lines: usize,
    pub tracked: Option<DiffLineTotals>,
    pub untracked: Option<DiffLineTotals>,
}

impl ReviewDiffStats {
    pub(crate) fn add_file(&mut self, additions: usize, deletions: usize) {
        self.file_count = self.file_count.saturating_add(1);
        self.additions = self.additions.saturating_add(additions);
        self.deletions = self.deletions.saturating_add(deletions);
        let changed_lines = additions.saturating_add(deletions);
        self.lines = self.lines.saturating_add(changed_lines);
        self.split_lines = self.split_lines.saturating_add(changed_lines);
    }

    pub(crate) fn with_working_tree_scopes(mut self, tracked: Self, untracked: Self) -> Self {
        self.tracked = Some(tracked.scope());
        self.untracked = Some(untracked.scope());
        self
    }

    pub fn has_working_tree_scopes(&self) -> bool {
        self.tracked.is_some() && self.untracked.is_some()
    }

    pub fn totals(&self) -> DiffLineTotals {
        self.scope()
    }

    fn scope(&self) -> DiffLineTotals {
        DiffLineTotals {
            file_count: self.file_count,
            additions: self.additions,
            deletions: self.deletions,
            lines: self.lines,
            split_lines: self.split_lines,
        }
    }
}

pub async fn load_review_diff_stats_for_working_tree(
    repo_root: &Path,
    files: &[FileEntry],
) -> color_eyre::Result<ReviewDiffStats> {
    let mut tracked = ReviewDiffStats::default();

    if files.iter().any(|file| !is_untracked_status(&file.status)) {
        let output = git_output_bytes(
            repo_root,
            &["diff", "--numstat", "-z", "--find-renames", "HEAD", "--"],
        )
        .await?;
        tracked = parse_numstat(&output);
    }

    let untracked = load_untracked_file_stats(repo_root, files).await?;
    Ok((tracked + untracked).with_working_tree_scopes(tracked, untracked))
}

pub async fn load_review_diff_stats_for_commit_compare(
    repo_root: &Path,
    selection: &CommitCompareSelection,
) -> color_eyre::Result<ReviewDiffStats> {
    let output = git_output_bytes(
        repo_root,
        &[
            "diff",
            "--numstat",
            "-z",
            "--find-renames",
            selection.base_ref.as_str(),
            selection.commit_hash.as_str(),
        ],
    )
    .await?;
    Ok(parse_numstat(&output))
}

pub async fn load_review_diff_stats_for_branch_compare(
    repo_root: &Path,
    selection: &BranchCompareSelection,
) -> color_eyre::Result<ReviewDiffStats> {
    let diff_range = build_branch_diff_range(selection);
    let output = git_output_bytes(
        repo_root,
        &[
            "diff",
            "--numstat",
            "-z",
            "--find-renames",
            diff_range.as_str(),
        ],
    )
    .await?;
    Ok(parse_numstat(&output))
}

async fn load_untracked_file_stats(
    repo_root: &Path,
    files: &[FileEntry],
) -> color_eyre::Result<ReviewDiffStats> {
    let mut stats = ReviewDiffStats::default();
    let mut jobs = JoinSet::new();

    for file in files
        .iter()
        .filter(|file| is_untracked_status(&file.status))
    {
        while jobs.len() >= UNTRACKED_STATS_CONCURRENCY {
            if let Some(result) = jobs.join_next().await {
                stats += result.wrap_err("untracked stats task failed")?;
            }
        }

        let path = repo_root.join(&file.path);
        jobs.spawn(async move { untracked_file_stats(path).await });
    }

    while let Some(result) = jobs.join_next().await {
        stats += result.wrap_err("untracked stats task failed")?;
    }

    Ok(stats)
}

async fn untracked_file_stats(path: std::path::PathBuf) -> ReviewDiffStats {
    let Ok(metadata) = fs::metadata(&path).await else {
        return ReviewDiffStats::default();
    };
    if metadata.is_dir() {
        return ReviewDiffStats::default();
    }

    let Ok(mut file) = fs::File::open(path).await else {
        return ReviewDiffStats::default();
    };

    let mut buffer = [0u8; 64 * 1024];
    let mut newlines = 0usize;
    let mut saw_byte = false;
    let mut last_byte = 0u8;

    loop {
        let Ok(read) = file.read(&mut buffer).await else {
            return ReviewDiffStats::default();
        };
        if read == 0 {
            break;
        }

        let chunk = &buffer[..read];
        if memchr::memchr(0, chunk).is_some() {
            let mut stats = ReviewDiffStats::default();
            stats.add_file(0, 0);
            return stats;
        }

        saw_byte = true;
        last_byte = chunk[read - 1];
        newlines = newlines.saturating_add(memchr::memchr_iter(b'\n', chunk).count());
    }

    let additions = text_line_count(saw_byte, newlines, last_byte);
    let mut stats = ReviewDiffStats::default();
    stats.add_file(additions, 0);
    stats
}

fn text_line_count(saw_byte: bool, newlines: usize, last_byte: u8) -> usize {
    if !saw_byte {
        return 0;
    }

    if last_byte == b'\n' {
        newlines
    } else {
        newlines.saturating_add(1)
    }
}

fn parse_numstat(output: &[u8]) -> ReviewDiffStats {
    let mut stats = ReviewDiffStats::default();
    let mut index = 0usize;

    while index < output.len() {
        let Some(additions_end) = find_byte(output, index, b'\t') else {
            break;
        };
        let Some(deletions_end) = find_byte(output, additions_end + 1, b'\t') else {
            break;
        };

        let additions = parse_count(&output[index..additions_end]).unwrap_or(0);
        let deletions = parse_count(&output[additions_end + 1..deletions_end]).unwrap_or(0);
        index = deletions_end + 1;

        if output.get(index) == Some(&0) {
            index += 1;
            let Some(old_path_end) = find_byte(output, index, 0) else {
                break;
            };
            let Some(new_path_end) = find_byte(output, old_path_end + 1, 0) else {
                break;
            };
            index = new_path_end + 1;
        } else {
            let Some(path_end) = find_byte(output, index, 0) else {
                break;
            };
            index = path_end + 1;
        }

        stats.add_file(additions, deletions);
    }

    stats
}

fn find_byte(bytes: &[u8], start: usize, byte: u8) -> Option<usize> {
    memchr::memchr(byte, bytes.get(start..)?).map(|offset| start + offset)
}

fn parse_count(bytes: &[u8]) -> Option<usize> {
    if bytes == b"-" {
        return None;
    }

    let mut parsed = 0usize;
    for byte in bytes {
        if !byte.is_ascii_digit() {
            return None;
        }
        parsed = parsed
            .saturating_mul(10)
            .saturating_add((byte - b'0') as usize);
    }
    Some(parsed)
}

impl std::ops::AddAssign for ReviewDiffStats {
    fn add_assign(&mut self, rhs: Self) {
        self.file_count = self.file_count.saturating_add(rhs.file_count);
        self.additions = self.additions.saturating_add(rhs.additions);
        self.deletions = self.deletions.saturating_add(rhs.deletions);
        self.lines = self.lines.saturating_add(rhs.lines);
        self.split_lines = self.split_lines.saturating_add(rhs.split_lines);
        self.tracked = merge_scope(self.tracked, rhs.tracked);
        self.untracked = merge_scope(self.untracked, rhs.untracked);
    }
}

fn merge_scope(lhs: Option<DiffLineTotals>, rhs: Option<DiffLineTotals>) -> Option<DiffLineTotals> {
    match (lhs, rhs) {
        (None, None) => None,
        (Some(scope), None) | (None, Some(scope)) => Some(scope),
        (Some(mut lhs), Some(rhs)) => {
            lhs.saturating_add_assign(rhs);
            Some(lhs)
        }
    }
}

impl std::ops::Add for ReviewDiffStats {
    type Output = ReviewDiffStats;

    fn add(mut self, rhs: Self) -> Self::Output {
        self += rhs;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numstat_parser_reads_normal_paths() {
        let stats = parse_numstat(b"12\t3\tsrc/lib.rs\0");

        assert_eq!(
            stats,
            ReviewDiffStats {
                file_count: 1,
                additions: 12,
                deletions: 3,
                lines: 15,
                split_lines: 15,
                ..ReviewDiffStats::default()
            }
        );
    }

    #[test]
    fn numstat_parser_reads_z_renames() {
        let stats = parse_numstat(b"1\t0\t\0old name.ts\0new name.ts\0");

        assert_eq!(stats.file_count, 1);
        assert_eq!(stats.additions, 1);
        assert_eq!(stats.deletions, 0);
    }

    #[test]
    fn numstat_parser_treats_binary_counts_as_zero() {
        let stats = parse_numstat(b"-\t-\tasset.bin\0");

        assert_eq!(stats.file_count, 1);
        assert_eq!(stats.additions, 0);
        assert_eq!(stats.deletions, 0);
    }

    #[test]
    fn text_line_count_matches_untracked_diff_line_count() {
        assert_eq!(line_count_for_test(b""), 0);
        assert_eq!(line_count_for_test(b"one"), 1);
        assert_eq!(line_count_for_test(b"one\n"), 1);
        assert_eq!(line_count_for_test(b"one\ntwo"), 2);
        assert_eq!(line_count_for_test(b"one\ntwo\n"), 2);
    }

    #[test]
    fn working_tree_scopes_keep_tracked_and_untracked_separate() {
        let mut tracked = ReviewDiffStats::default();
        tracked.add_file(4, 2);
        let mut untracked = ReviewDiffStats::default();
        untracked.add_file(9, 0);

        let stats = (tracked + untracked).with_working_tree_scopes(tracked, untracked);

        assert_eq!(stats.file_count, 2);
        assert_eq!(stats.additions, 13);
        assert_eq!(stats.deletions, 2);
        assert_eq!(
            stats.tracked,
            Some(DiffLineTotals {
                file_count: 1,
                additions: 4,
                deletions: 2,
                lines: 6,
                split_lines: 6,
            })
        );
        assert_eq!(
            stats.untracked,
            Some(DiffLineTotals {
                file_count: 1,
                additions: 9,
                deletions: 0,
                lines: 9,
                split_lines: 9,
            })
        );
    }

    fn line_count_for_test(bytes: &[u8]) -> usize {
        text_line_count(
            !bytes.is_empty(),
            memchr::memchr_iter(b'\n', bytes).count(),
            bytes.last().copied().unwrap_or_default(),
        )
    }
}
