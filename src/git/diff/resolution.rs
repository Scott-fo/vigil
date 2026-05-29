use std::collections::HashSet;

use color_eyre::eyre::eyre;

use super::{
    DiffHunkResolution, FileDiffMetadata, Hunk, HunkContent, MergeConflictResolution,
    ProcessFileConflictData,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NormalizedDiffResolution {
    Deletions,
    Additions,
    Both,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ResolveCursor {
    next_addition_line_index: usize,
    next_deletion_line_index: usize,
    next_addition_start: usize,
    next_deletion_start: usize,
    split_line_count: usize,
    unified_line_count: usize,
}

#[inline]
pub fn diff_accept_reject_hunk(
    diff: &FileDiffMetadata,
    hunk_index: usize,
    resolution: DiffHunkResolution,
) -> color_eyre::Result<FileDiffMetadata> {
    let hunk = diff
        .hunks
        .get(hunk_index)
        .ok_or_else(|| eyre!("diffAcceptRejectHunk: Invalid hunk index"))?;
    resolve_diff_region(
        diff,
        hunk_index,
        0,
        hunk.hunk_content.len().saturating_sub(1),
        normalize_diff_resolution(resolution),
    )
}

#[inline]
pub fn diff_accept_reject_content(
    diff: &FileDiffMetadata,
    hunk_index: usize,
    content_index: usize,
    resolution: DiffHunkResolution,
) -> color_eyre::Result<FileDiffMetadata> {
    resolve_diff_region(
        diff,
        hunk_index,
        content_index,
        content_index,
        normalize_diff_resolution(resolution),
    )
}

#[inline]
pub fn resolve_conflict(
    diff: &FileDiffMetadata,
    conflict: &ProcessFileConflictData,
    resolution: MergeConflictResolution,
) -> color_eyre::Result<FileDiffMetadata> {
    let mut indexes_to_delete = HashSet::new();
    if let Some(base_content_index) = conflict.base_content_index {
        indexes_to_delete.insert(base_content_index);
    }
    if conflict.end_marker_content_index != conflict.end_content_index {
        indexes_to_delete.insert(conflict.end_marker_content_index);
    }

    resolve_diff_region_with_deleted_indexes(
        diff,
        conflict.hunk_index,
        conflict.start_content_index,
        conflict.end_content_index,
        normalize_merge_conflict_resolution(resolution),
        &indexes_to_delete,
    )
}

#[inline]
fn resolve_diff_region(
    diff: &FileDiffMetadata,
    hunk_index: usize,
    start_content_index: usize,
    end_content_index: usize,
    resolution: NormalizedDiffResolution,
) -> color_eyre::Result<FileDiffMetadata> {
    resolve_diff_region_with_deleted_indexes(
        diff,
        hunk_index,
        start_content_index,
        end_content_index,
        resolution,
        &HashSet::new(),
    )
}

#[inline]
fn resolve_diff_region_with_deleted_indexes(
    diff: &FileDiffMetadata,
    hunk_index: usize,
    start_content_index: usize,
    end_content_index: usize,
    resolution: NormalizedDiffResolution,
    indexes_to_delete: &HashSet<usize>,
) -> color_eyre::Result<FileDiffMetadata> {
    let current_hunk = diff
        .hunks
        .get(hunk_index)
        .ok_or_else(|| eyre!("resolveRegion: Invalid hunk index: {hunk_index}"))?;
    if start_content_index > end_content_index
        || end_content_index >= current_hunk.hunk_content.len()
    {
        return Err(eyre!(
            "resolveRegion: Invalid content range, {start_content_index}, {end_content_index}"
        ));
    }

    let mut resolved_diff = FileDiffMetadata {
        hunks: Vec::with_capacity(diff.hunks.len()),
        deletion_lines: Vec::new(),
        addition_lines: Vec::new(),
        split_line_count: 0,
        unified_line_count: 0,
        cache_key: diff.cache_key.as_ref().map(|cache_key| {
            format!(
                "{cache_key}:{}-{hunk_index}:{start_content_index}-{end_content_index}",
                resolution_cache_key_prefix(resolution)
            )
        }),
        ..diff.clone()
    };

    let mut cursor = ResolveCursor {
        next_addition_start: 1,
        next_deletion_start: 1,
        ..ResolveCursor::default()
    };
    let updates_eof_state = hunk_index == diff.hunks.len().saturating_sub(1)
        && end_content_index == current_hunk.hunk_content.len().saturating_sub(1);
    let should_process_collapsed_context = !diff.is_partial;

    for (index, hunk) in diff.hunks.iter().enumerate() {
        process_resolved_collapsed_context(
            diff,
            &mut resolved_diff,
            &mut cursor,
            hunk.deletion_line_index
                .saturating_sub(hunk.collapsed_before),
            hunk.addition_line_index
                .saturating_sub(hunk.collapsed_before),
            hunk.collapsed_before,
            should_process_collapsed_context,
        )?;

        let mut new_hunk = Hunk {
            hunk_content: Vec::new(),
            addition_start: cursor.next_addition_start,
            deletion_start: cursor.next_deletion_start,
            addition_line_index: cursor.next_addition_line_index,
            deletion_line_index: cursor.next_deletion_line_index,
            addition_count: 0,
            deletion_count: 0,
            deletion_lines: 0,
            addition_lines: 0,
            split_line_start: cursor.split_line_count,
            unified_line_start: cursor.unified_line_count,
            split_line_count: 0,
            unified_line_count: 0,
            ..hunk.clone()
        };

        for (content_index, content) in hunk.hunk_content.iter().enumerate() {
            if index != hunk_index
                || content_index < start_content_index
                || content_index > end_content_index
            {
                push_content_lines_to_diff(
                    content,
                    &mut resolved_diff,
                    &diff.deletion_lines,
                    &diff.addition_lines,
                )?;
                let new_content = reindex_hunk_content(
                    content,
                    cursor.next_deletion_line_index,
                    cursor.next_addition_line_index,
                );
                new_hunk.hunk_content.push(new_content.clone());
                advance_resolve_cursor(&new_content, &mut cursor, &mut new_hunk);
            } else if indexes_to_delete.contains(&content_index) {
                new_hunk.hunk_content.push(HunkContent::Context {
                    lines: 0,
                    deletion_line_index: cursor.next_deletion_line_index,
                    addition_line_index: cursor.next_addition_line_index,
                });
            } else if let HunkContent::Context { lines, .. } = content {
                push_content_lines_to_diff(
                    content,
                    &mut resolved_diff,
                    &diff.deletion_lines,
                    &diff.addition_lines,
                )?;
                let new_content = HunkContent::Context {
                    lines: *lines,
                    deletion_line_index: cursor.next_deletion_line_index,
                    addition_line_index: cursor.next_addition_line_index,
                };
                new_hunk.hunk_content.push(new_content.clone());
                advance_resolve_cursor(&new_content, &mut cursor, &mut new_hunk);
            } else if let HunkContent::Change {
                deletions,
                deletion_line_index,
                additions,
                addition_line_index,
            } = *content
            {
                push_resolve_lines_to_diff(
                    resolution,
                    deletions,
                    deletion_line_index,
                    additions,
                    addition_line_index,
                    &mut resolved_diff,
                    &diff.deletion_lines,
                    &diff.addition_lines,
                )?;
                let lines = match resolution {
                    NormalizedDiffResolution::Deletions => deletions,
                    NormalizedDiffResolution::Additions => additions,
                    NormalizedDiffResolution::Both => deletions + additions,
                };
                let new_content = HunkContent::Context {
                    lines,
                    deletion_line_index: cursor.next_deletion_line_index,
                    addition_line_index: cursor.next_addition_line_index,
                };
                new_hunk.hunk_content.push(new_content.clone());
                advance_resolve_cursor(&new_content, &mut cursor, &mut new_hunk);
            }
        }

        if index == hunk_index && updates_eof_state {
            let no_eof_cr = if resolution == NormalizedDiffResolution::Deletions {
                hunk.no_eof_cr_deletions
            } else {
                hunk.no_eof_cr_additions
            };
            new_hunk.no_eof_cr_additions = no_eof_cr;
            new_hunk.no_eof_cr_deletions = no_eof_cr;
        }

        resolved_diff.hunks.push(new_hunk);
    }

    if let Some(final_hunk) = diff.hunks.last().filter(|_| !diff.is_partial) {
        let deletion_start = final_hunk.deletion_line_index + final_hunk.deletion_count;
        let addition_start = final_hunk.addition_line_index + final_hunk.addition_count;
        let line_count = diff
            .deletion_lines
            .len()
            .saturating_sub(deletion_start)
            .min(diff.addition_lines.len().saturating_sub(addition_start));
        push_resolved_collapsed_context_lines(
            &mut resolved_diff,
            &diff.deletion_lines,
            &diff.addition_lines,
            deletion_start,
            addition_start,
            line_count,
        )?;
    }

    resolved_diff.split_line_count = cursor.split_line_count;
    resolved_diff.unified_line_count = cursor.unified_line_count;
    Ok(resolved_diff)
}

#[inline]
pub(super) fn normalize_diff_resolution(
    resolution: DiffHunkResolution,
) -> NormalizedDiffResolution {
    match resolution {
        DiffHunkResolution::Accept
        | DiffHunkResolution::Incoming
        | DiffHunkResolution::Additions => NormalizedDiffResolution::Additions,
        DiffHunkResolution::Reject
        | DiffHunkResolution::Current
        | DiffHunkResolution::Deletions => NormalizedDiffResolution::Deletions,
        DiffHunkResolution::Both => NormalizedDiffResolution::Both,
    }
}

#[inline]
fn normalize_merge_conflict_resolution(
    resolution: MergeConflictResolution,
) -> NormalizedDiffResolution {
    match resolution {
        MergeConflictResolution::Current => NormalizedDiffResolution::Deletions,
        MergeConflictResolution::Incoming => NormalizedDiffResolution::Additions,
        MergeConflictResolution::Both => NormalizedDiffResolution::Both,
    }
}

#[inline]
fn resolution_cache_key_prefix(resolution: NormalizedDiffResolution) -> char {
    match resolution {
        NormalizedDiffResolution::Deletions => 'd',
        NormalizedDiffResolution::Additions => 'a',
        NormalizedDiffResolution::Both => 'b',
    }
}

#[inline]
fn process_resolved_collapsed_context(
    source_diff: &FileDiffMetadata,
    resolved_diff: &mut FileDiffMetadata,
    cursor: &mut ResolveCursor,
    deletion_line_index: usize,
    addition_line_index: usize,
    line_count: usize,
    should_process_content: bool,
) -> color_eyre::Result<()> {
    if line_count == 0 {
        return Ok(());
    }

    if should_process_content {
        push_resolved_collapsed_context_lines(
            resolved_diff,
            &source_diff.deletion_lines,
            &source_diff.addition_lines,
            deletion_line_index,
            addition_line_index,
            line_count,
        )?;
        cursor.next_addition_line_index += line_count;
        cursor.next_deletion_line_index += line_count;
    }

    cursor.next_addition_start += line_count;
    cursor.next_deletion_start += line_count;
    cursor.split_line_count += line_count;
    cursor.unified_line_count += line_count;
    Ok(())
}

#[inline]
fn push_resolved_collapsed_context_lines(
    diff: &mut FileDiffMetadata,
    deletion_lines: &[String],
    addition_lines: &[String],
    deletion_line_index: usize,
    addition_line_index: usize,
    line_count: usize,
) -> color_eyre::Result<()> {
    for index in 0..line_count {
        let deletion_line = deletion_lines
            .get(deletion_line_index + index)
            .ok_or_else(|| eyre!("pushCollapsedContextLines: missing collapsed context line"))?;
        let addition_line = addition_lines
            .get(addition_line_index + index)
            .ok_or_else(|| eyre!("pushCollapsedContextLines: missing collapsed context line"))?;
        diff.deletion_lines.push(deletion_line.clone());
        diff.addition_lines.push(addition_line.clone());
    }
    Ok(())
}

#[inline]
fn push_content_lines_to_diff(
    content: &HunkContent,
    diff: &mut FileDiffMetadata,
    deletion_lines: &[String],
    addition_lines: &[String],
) -> color_eyre::Result<()> {
    match *content {
        HunkContent::Context {
            lines,
            addition_line_index,
            ..
        } => {
            for index in 0..lines {
                let line = addition_lines
                    .get(addition_line_index + index)
                    .ok_or_else(|| eyre!("pushContentLinesToDiff: Context line does not exist"))?;
                diff.deletion_lines.push(line.clone());
                diff.addition_lines.push(line.clone());
            }
        }
        HunkContent::Change {
            deletions,
            deletion_line_index,
            additions,
            addition_line_index,
        } => {
            for index in 0..deletions.max(additions) {
                if index < deletions {
                    let line =
                        deletion_lines
                            .get(deletion_line_index + index)
                            .ok_or_else(|| {
                                eyre!("pushContentLinesToDiff: Deletion line does not exist")
                            })?;
                    diff.deletion_lines.push(line.clone());
                }
                if index < additions {
                    let line =
                        addition_lines
                            .get(addition_line_index + index)
                            .ok_or_else(|| {
                                eyre!("pushContentLinesToDiff: Addition line does not exist")
                            })?;
                    diff.addition_lines.push(line.clone());
                }
            }
        }
    }
    Ok(())
}

#[inline]
fn push_resolve_lines_to_diff(
    resolution: NormalizedDiffResolution,
    deletions: usize,
    deletion_line_index: usize,
    additions: usize,
    addition_line_index: usize,
    diff: &mut FileDiffMetadata,
    deletion_lines: &[String],
    addition_lines: &[String],
) -> color_eyre::Result<()> {
    if matches!(
        resolution,
        NormalizedDiffResolution::Deletions | NormalizedDiffResolution::Both
    ) {
        for index in 0..deletions {
            let line = deletion_lines
                .get(deletion_line_index + index)
                .ok_or_else(|| eyre!("pushResolveLinesToDiff: Deletion line does not exist"))?;
            diff.deletion_lines.push(line.clone());
            diff.addition_lines.push(line.clone());
        }
    }
    if matches!(
        resolution,
        NormalizedDiffResolution::Additions | NormalizedDiffResolution::Both
    ) {
        for index in 0..additions {
            let line = addition_lines
                .get(addition_line_index + index)
                .ok_or_else(|| eyre!("pushResolveLinesToDiff: Addition line does not exist"))?;
            diff.deletion_lines.push(line.clone());
            diff.addition_lines.push(line.clone());
        }
    }
    Ok(())
}

#[inline]
fn reindex_hunk_content(
    content: &HunkContent,
    deletion_line_index: usize,
    addition_line_index: usize,
) -> HunkContent {
    match *content {
        HunkContent::Context { lines, .. } => HunkContent::Context {
            lines,
            deletion_line_index,
            addition_line_index,
        },
        HunkContent::Change {
            deletions,
            additions,
            ..
        } => HunkContent::Change {
            deletions,
            deletion_line_index,
            additions,
            addition_line_index,
        },
    }
}

#[inline]
fn advance_resolve_cursor(content: &HunkContent, cursor: &mut ResolveCursor, hunk: &mut Hunk) {
    match *content {
        HunkContent::Context { lines, .. } => {
            cursor.next_addition_line_index += lines;
            cursor.next_deletion_line_index += lines;
            cursor.next_addition_start += lines;
            cursor.next_deletion_start += lines;
            cursor.split_line_count += lines;
            cursor.unified_line_count += lines;

            hunk.addition_count += lines;
            hunk.deletion_count += lines;
            hunk.split_line_count += lines;
            hunk.unified_line_count += lines;
        }
        HunkContent::Change {
            deletions,
            additions,
            ..
        } => {
            cursor.next_addition_line_index += additions;
            cursor.next_deletion_line_index += deletions;
            cursor.next_addition_start += additions;
            cursor.next_deletion_start += deletions;
            cursor.split_line_count += deletions.max(additions);
            cursor.unified_line_count += deletions + additions;

            hunk.deletion_count += deletions;
            hunk.deletion_lines += deletions;
            hunk.addition_count += additions;
            hunk.addition_lines += additions;
            hunk.split_line_count += deletions.max(additions);
            hunk.unified_line_count += deletions + additions;
        }
    }
}
