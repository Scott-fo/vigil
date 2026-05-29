use color_eyre::eyre::eyre;

use super::{
    DiffIterationOptions, DiffLine, DiffLineMetadata, DiffLineType, DiffStyle, ExpandedHunks,
    FileDiffMetadata, FileIterationOptions, FileLine, Hunk, HunkContent,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ExpandedRegionResult {
    pub(super) from_start: usize,
    pub(super) from_end: usize,
    pub(super) range_size: usize,
    pub(super) collapsed_lines: usize,
}

pub fn iterate_over_file<'a, F>(lines: &'a [String], options: FileIterationOptions, mut callback: F)
where
    F: FnMut(FileLine<'a>) -> bool,
{
    if lines.is_empty() {
        return;
    }

    let starting_line = options.starting_line.min(lines.len());
    let requested_total = options.total_lines.unwrap_or(usize::MAX);
    let len = starting_line
        .saturating_add(requested_total)
        .min(lines.len());
    let last_line_index = match lines.last().map(String::as_str) {
        Some("" | "\n" | "\r\n" | "\r") => lines.len().saturating_sub(2),
        Some(_) => lines.len() - 1,
        None => return,
    };

    for line_index in starting_line..len {
        let is_last_line = line_index == last_line_index;
        if callback(FileLine {
            line_index,
            line_number: line_index + 1,
            content: &lines[line_index],
            is_last_line,
        }) || is_last_line
        {
            break;
        }
    }
}

pub fn collect_diff_lines(
    diff: &FileDiffMetadata,
    options: DiffIterationOptions<'_>,
) -> color_eyre::Result<Vec<DiffLine>> {
    let mut lines = Vec::new();
    collect_all_diff_lines(diff, options, &mut lines)?;

    if options.starting_line == 0 && options.total_lines.is_none() {
        return Ok(lines);
    }

    let start = options.starting_line.min(lines.len());
    let end = options
        .total_lines
        .map(|total| start.saturating_add(total).min(lines.len()))
        .unwrap_or(lines.len());
    Ok(lines[start..end].to_vec())
}

pub fn iterate_over_diff<F>(
    diff: &FileDiffMetadata,
    options: DiffIterationOptions<'_>,
    mut callback: F,
) -> color_eyre::Result<()>
where
    F: FnMut(DiffLine) -> bool,
{
    for line in collect_diff_lines(diff, options)? {
        if callback(line) {
            break;
        }
    }
    Ok(())
}

fn collect_all_diff_lines(
    diff: &FileDiffMetadata,
    options: DiffIterationOptions<'_>,
    lines: &mut Vec<DiffLine>,
) -> color_eyre::Result<()> {
    let Some(final_hunk_index) = diff.hunks.len().checked_sub(1) else {
        return Ok(());
    };

    for (hunk_index, hunk) in diff.hunks.iter().enumerate() {
        let leading_region = get_expanded_region(
            diff.is_partial,
            hunk.collapsed_before,
            options.expanded_hunks,
            hunk_index,
            options.collapsed_context_threshold,
        );
        let trailing_region = if hunk_index == final_hunk_index && has_final_collapsed_hunk(diff) {
            Some(get_expanded_region(
                diff.is_partial,
                get_trailing_range_size(diff, hunk)?,
                options.expanded_hunks,
                diff.hunks.len(),
                options.collapsed_context_threshold,
            ))
        } else {
            None
        };
        let mut pending_collapsed = leading_region.collapsed_lines;

        emit_expanded_region_start(lines, hunk_index, hunk, &leading_region, options.diff_style);
        emit_expanded_region_end(
            lines,
            hunk_index,
            hunk,
            &leading_region,
            options.diff_style,
            &mut pending_collapsed,
        );

        let last_content_index = hunk.hunk_content.len().saturating_sub(1);
        for (content_index, content) in hunk.hunk_content.iter().enumerate() {
            let is_last_content = content_index == last_content_index;
            let collapsed_before = take_pending_collapsed(&mut pending_collapsed);
            let collapsed_after = if is_last_content {
                trailing_region
                    .filter(|region| region.from_start + region.from_end == 0)
                    .map(|region| region.collapsed_lines)
                    .unwrap_or(0)
            } else {
                0
            };

            match *content {
                HunkContent::Context {
                    lines: context_lines,
                    addition_line_index,
                    deletion_line_index,
                } => emit_context_diff_lines(
                    lines,
                    hunk_index,
                    hunk,
                    options.diff_style,
                    context_lines,
                    addition_line_index,
                    deletion_line_index,
                    is_last_content,
                    collapsed_before,
                    collapsed_after,
                ),
                HunkContent::Change {
                    deletions,
                    deletion_line_index,
                    additions,
                    addition_line_index,
                } => emit_change_diff_lines(
                    lines,
                    hunk_index,
                    hunk,
                    options.diff_style,
                    deletions,
                    deletion_line_index,
                    additions,
                    addition_line_index,
                    is_last_content,
                    collapsed_before,
                    collapsed_after,
                ),
            }
        }

        if let Some(trailing_region) = trailing_region {
            emit_trailing_expanded_region(lines, diff, hunk, &trailing_region, options.diff_style)?;
        }
    }

    Ok(())
}

#[inline]
fn emit_expanded_region_start(
    lines: &mut Vec<DiffLine>,
    hunk_index: usize,
    hunk: &Hunk,
    region: &ExpandedRegionResult,
    diff_style: DiffStyle,
) {
    if region.from_start == 0 {
        return;
    }

    let unified_line_index = hunk.unified_line_start.saturating_sub(region.range_size);
    let split_line_index = hunk.split_line_start.saturating_sub(region.range_size);
    let deletion_line_index = hunk.deletion_line_index.saturating_sub(region.range_size);
    let addition_line_index = hunk.addition_line_index.saturating_sub(region.range_size);
    let deletion_line_number = hunk.deletion_start.saturating_sub(region.range_size);
    let addition_line_number = hunk.addition_start.saturating_sub(region.range_size);

    for index in 0..region.from_start {
        push_context_expanded_line(
            lines,
            hunk_index,
            true,
            0,
            0,
            diff_style,
            LinePairIndices {
                unified_line_index: unified_line_index + index,
                split_line_index: split_line_index + index,
                deletion_line_index: deletion_line_index + index,
                addition_line_index: addition_line_index + index,
                deletion_line_number: deletion_line_number + index,
                addition_line_number: addition_line_number + index,
            },
        );
    }
}

#[inline]
fn emit_expanded_region_end(
    lines: &mut Vec<DiffLine>,
    hunk_index: usize,
    hunk: &Hunk,
    region: &ExpandedRegionResult,
    diff_style: DiffStyle,
    pending_collapsed: &mut usize,
) {
    if region.from_end == 0 {
        return;
    }

    let unified_line_index = hunk.unified_line_start.saturating_sub(region.from_end);
    let split_line_index = hunk.split_line_start.saturating_sub(region.from_end);
    let deletion_line_index = hunk.deletion_line_index.saturating_sub(region.from_end);
    let addition_line_index = hunk.addition_line_index.saturating_sub(region.from_end);
    let deletion_line_number = hunk.deletion_start.saturating_sub(region.from_end);
    let addition_line_number = hunk.addition_start.saturating_sub(region.from_end);

    for index in 0..region.from_end {
        push_context_expanded_line(
            lines,
            hunk_index,
            true,
            if index == 0 {
                take_pending_collapsed(pending_collapsed)
            } else {
                0
            },
            0,
            diff_style,
            LinePairIndices {
                unified_line_index: unified_line_index + index,
                split_line_index: split_line_index + index,
                deletion_line_index: deletion_line_index + index,
                addition_line_index: addition_line_index + index,
                deletion_line_number: deletion_line_number + index,
                addition_line_number: addition_line_number + index,
            },
        );
    }
}

#[inline]
fn emit_context_diff_lines(
    lines: &mut Vec<DiffLine>,
    hunk_index: usize,
    hunk: &Hunk,
    diff_style: DiffStyle,
    context_lines: usize,
    addition_line_index: usize,
    deletion_line_index: usize,
    is_last_content: bool,
    collapsed_before: usize,
    collapsed_after: usize,
) {
    let unified_offset =
        unified_offset_before_content(hunk, addition_line_index, deletion_line_index);
    let split_offset = split_offset_before_content(hunk, addition_line_index, deletion_line_index);
    for index in 0..context_lines {
        let is_last_line = is_last_content && index == context_lines.saturating_sub(1);
        push_context_line(
            lines,
            hunk_index,
            collapsed_before_for_index(collapsed_before, index),
            collapsed_after_for_index(collapsed_after, index, context_lines),
            diff_style,
            LinePairIndices {
                unified_line_index: hunk.unified_line_start + unified_offset + index,
                split_line_index: hunk.split_line_start + split_offset + index,
                deletion_line_index: deletion_line_index + index,
                addition_line_index: addition_line_index + index,
                deletion_line_number: hunk.deletion_start + deletion_line_index
                    - hunk.deletion_line_index
                    + index,
                addition_line_number: hunk.addition_start + addition_line_index
                    - hunk.addition_line_index
                    + index,
            },
            is_last_line && hunk.no_eof_cr_deletions,
            is_last_line && hunk.no_eof_cr_additions,
        );
    }
}

#[inline]
fn emit_change_diff_lines(
    lines: &mut Vec<DiffLine>,
    hunk_index: usize,
    hunk: &Hunk,
    diff_style: DiffStyle,
    deletions: usize,
    deletion_line_index: usize,
    additions: usize,
    addition_line_index: usize,
    is_last_content: bool,
    collapsed_before: usize,
    collapsed_after: usize,
) {
    let split_count = deletions.max(additions);
    let unified_start = hunk.unified_line_start
        + unified_offset_before_content(hunk, addition_line_index, deletion_line_index);
    let split_start = hunk.split_line_start
        + split_offset_before_content(hunk, addition_line_index, deletion_line_index);
    let deletion_line_number = hunk.deletion_start + deletion_line_index - hunk.deletion_line_index;
    let addition_line_number = hunk.addition_start + addition_line_index - hunk.addition_line_index;

    if diff_style == DiffStyle::Unified {
        for index in 0..deletions {
            lines.push(DiffLine {
                hunk_index,
                has_hunk: true,
                collapsed_before: collapsed_before_for_index(collapsed_before, index),
                collapsed_after: if additions == 0 {
                    collapsed_after_for_index(collapsed_after, index, deletions)
                } else {
                    0
                },
                line_type: DiffLineType::Change,
                deletion_line: Some(DiffLineMetadata {
                    unified_line_index: unified_start + index,
                    split_line_index: split_start + index,
                    line_index: deletion_line_index + index,
                    line_number: deletion_line_number + index,
                    no_eof_cr: is_last_content
                        && index == deletions.saturating_sub(1)
                        && hunk.no_eof_cr_deletions,
                }),
                addition_line: None,
            });
        }
        for index in 0..additions {
            lines.push(DiffLine {
                hunk_index,
                has_hunk: true,
                collapsed_before: collapsed_before_for_index(collapsed_before, deletions + index),
                collapsed_after: collapsed_after_for_index(collapsed_after, index, additions),
                line_type: DiffLineType::Change,
                deletion_line: None,
                addition_line: Some(DiffLineMetadata {
                    unified_line_index: unified_start + deletions + index,
                    split_line_index: split_start + index,
                    line_index: addition_line_index + index,
                    line_number: addition_line_number + index,
                    no_eof_cr: is_last_content
                        && index == additions.saturating_sub(1)
                        && hunk.no_eof_cr_additions,
                }),
            });
        }
        return;
    }

    for index in 0..split_count {
        let deletion_line = (index < deletions).then(|| DiffLineMetadata {
            unified_line_index: unified_start + index,
            split_line_index: split_start + index,
            line_index: deletion_line_index + index,
            line_number: deletion_line_number + index,
            no_eof_cr: is_last_content
                && index == split_count.saturating_sub(1)
                && hunk.no_eof_cr_deletions,
        });
        let addition_line = (index < additions).then(|| DiffLineMetadata {
            unified_line_index: unified_start + deletions + index,
            split_line_index: split_start + index,
            line_index: addition_line_index + index,
            line_number: addition_line_number + index,
            no_eof_cr: is_last_content
                && index == split_count.saturating_sub(1)
                && hunk.no_eof_cr_additions,
        });
        lines.push(DiffLine {
            hunk_index,
            has_hunk: true,
            collapsed_before: collapsed_before_for_index(collapsed_before, index),
            collapsed_after: collapsed_after_for_index(collapsed_after, index, split_count),
            line_type: DiffLineType::Change,
            deletion_line,
            addition_line,
        });
    }
}

#[inline]
fn emit_trailing_expanded_region(
    lines: &mut Vec<DiffLine>,
    diff: &FileDiffMetadata,
    hunk: &Hunk,
    region: &ExpandedRegionResult,
    diff_style: DiffStyle,
) -> color_eyre::Result<()> {
    let len = region.from_start + region.from_end;
    if len == 0 {
        return Ok(());
    }

    let deletion_start = hunk.deletion_line_index + hunk.deletion_count;
    let addition_start = hunk.addition_line_index + hunk.addition_count;
    if deletion_start + len > diff.deletion_lines.len()
        || addition_start + len > diff.addition_lines.len()
    {
        return Err(eyre!(
            "iterateOverDiff: trailing context out of bounds for {}",
            diff.name
        ));
    }

    for index in 0..len {
        push_context_expanded_line(
            lines,
            diff.hunks.len(),
            false,
            0,
            if index == len - 1 {
                region.collapsed_lines
            } else {
                0
            },
            diff_style,
            LinePairIndices {
                unified_line_index: hunk.unified_line_start + hunk.unified_line_count + index,
                split_line_index: hunk.split_line_start + hunk.split_line_count + index,
                deletion_line_index: deletion_start + index,
                addition_line_index: addition_start + index,
                deletion_line_number: hunk.deletion_start + hunk.deletion_count + index,
                addition_line_number: hunk.addition_start + hunk.addition_count + index,
            },
        );
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct LinePairIndices {
    unified_line_index: usize,
    split_line_index: usize,
    deletion_line_index: usize,
    addition_line_index: usize,
    deletion_line_number: usize,
    addition_line_number: usize,
}

fn push_context_line(
    lines: &mut Vec<DiffLine>,
    hunk_index: usize,
    collapsed_before: usize,
    collapsed_after: usize,
    diff_style: DiffStyle,
    indices: LinePairIndices,
    no_eof_cr_deletions: bool,
    no_eof_cr_additions: bool,
) {
    push_paired_line(
        lines,
        hunk_index,
        true,
        DiffLineType::Context,
        collapsed_before,
        collapsed_after,
        diff_style,
        indices,
        no_eof_cr_deletions,
        no_eof_cr_additions,
    );
}

fn push_context_expanded_line(
    lines: &mut Vec<DiffLine>,
    hunk_index: usize,
    has_hunk: bool,
    collapsed_before: usize,
    collapsed_after: usize,
    diff_style: DiffStyle,
    indices: LinePairIndices,
) {
    push_paired_line(
        lines,
        hunk_index,
        has_hunk,
        DiffLineType::ContextExpanded,
        collapsed_before,
        collapsed_after,
        diff_style,
        indices,
        false,
        false,
    );
}

fn push_paired_line(
    lines: &mut Vec<DiffLine>,
    hunk_index: usize,
    has_hunk: bool,
    line_type: DiffLineType,
    collapsed_before: usize,
    collapsed_after: usize,
    _diff_style: DiffStyle,
    indices: LinePairIndices,
    no_eof_cr_deletions: bool,
    no_eof_cr_additions: bool,
) {
    lines.push(DiffLine {
        hunk_index,
        has_hunk,
        collapsed_before,
        collapsed_after,
        line_type,
        deletion_line: Some(DiffLineMetadata {
            unified_line_index: indices.unified_line_index,
            split_line_index: indices.split_line_index,
            line_index: indices.deletion_line_index,
            line_number: indices.deletion_line_number,
            no_eof_cr: no_eof_cr_deletions,
        }),
        addition_line: Some(DiffLineMetadata {
            unified_line_index: indices.unified_line_index,
            split_line_index: indices.split_line_index,
            line_index: indices.addition_line_index,
            line_number: indices.addition_line_number,
            no_eof_cr: no_eof_cr_additions,
        }),
    });
}

#[inline]
pub(super) fn get_expanded_region(
    is_partial: bool,
    range_size: usize,
    expanded_hunks: Option<ExpandedHunks<'_>>,
    hunk_index: usize,
    collapsed_context_threshold: usize,
) -> ExpandedRegionResult {
    if range_size == 0 || is_partial {
        return ExpandedRegionResult {
            from_start: 0,
            from_end: 0,
            range_size,
            collapsed_lines: range_size,
        };
    }

    if expanded_hunks == Some(ExpandedHunks::All) || range_size <= collapsed_context_threshold {
        return ExpandedRegionResult {
            from_start: range_size,
            from_end: 0,
            range_size,
            collapsed_lines: 0,
        };
    }

    let region = match expanded_hunks {
        Some(ExpandedHunks::Regions(regions)) => regions.get(&hunk_index).copied(),
        _ => None,
    };
    let from_start = region
        .map(|region| region.from_start.min(range_size))
        .unwrap_or(0);
    let from_end = region
        .map(|region| region.from_end.min(range_size))
        .unwrap_or(0);
    let expanded_count = from_start + from_end;
    if expanded_count >= range_size {
        return ExpandedRegionResult {
            from_start: range_size,
            from_end: 0,
            range_size,
            collapsed_lines: 0,
        };
    }

    ExpandedRegionResult {
        from_start,
        from_end,
        range_size,
        collapsed_lines: range_size - expanded_count,
    }
}

#[inline]
pub(super) fn has_final_collapsed_hunk(diff: &FileDiffMetadata) -> bool {
    let Some(last_hunk) = diff.hunks.last() else {
        return false;
    };
    if diff.is_partial || diff.addition_lines.is_empty() || diff.deletion_lines.is_empty() {
        return false;
    }
    last_hunk.addition_line_index + last_hunk.addition_count < diff.addition_lines.len()
        || last_hunk.deletion_line_index + last_hunk.deletion_count < diff.deletion_lines.len()
}

#[inline]
pub(super) fn no_newline_metadata_line_counts(hunk: &Hunk) -> (usize, usize) {
    if !hunk.no_eof_cr_additions && !hunk.no_eof_cr_deletions {
        return (0, 0);
    }

    let Some(last_content) = hunk.hunk_content.last() else {
        return (0, 0);
    };

    match *last_content {
        HunkContent::Context { lines, .. } => {
            let metadata_rows = usize::from(lines > 0);
            (metadata_rows, metadata_rows)
        }
        HunkContent::Change {
            deletions,
            additions,
            ..
        } => {
            let unified = usize::from(deletions > 0 && hunk.no_eof_cr_deletions)
                + usize::from(additions > 0 && hunk.no_eof_cr_additions);
            let split = usize::from(
                (deletions > 0 && hunk.no_eof_cr_deletions)
                    || (additions > 0 && hunk.no_eof_cr_additions),
            );
            (split, unified)
        }
    }
}

#[inline]
pub(super) fn get_trailing_range_size(
    diff: &FileDiffMetadata,
    hunk: &Hunk,
) -> color_eyre::Result<usize> {
    let addition_remaining = diff
        .addition_lines
        .len()
        .saturating_sub(hunk.addition_line_index + hunk.addition_count);
    let deletion_remaining = diff
        .deletion_lines
        .len()
        .saturating_sub(hunk.deletion_line_index + hunk.deletion_count);
    if addition_remaining != deletion_remaining {
        return Err(eyre!(
            "iterateOverDiff: trailing context mismatch (additions={}, deletions={}) for {}",
            addition_remaining,
            deletion_remaining,
            diff.name
        ));
    }
    Ok(addition_remaining.min(deletion_remaining))
}

fn take_pending_collapsed(value: &mut usize) -> usize {
    let pending = *value;
    *value = 0;
    pending
}

fn collapsed_before_for_index(collapsed_before: usize, index: usize) -> usize {
    if index == 0 { collapsed_before } else { 0 }
}

fn collapsed_after_for_index(collapsed_after: usize, index: usize, len: usize) -> usize {
    if len > 0 && index == len - 1 {
        collapsed_after
    } else {
        0
    }
}

fn unified_offset_before_content(
    hunk: &Hunk,
    addition_line_index: usize,
    deletion_line_index: usize,
) -> usize {
    let mut offset = 0usize;
    for content in &hunk.hunk_content {
        if hunk_content_starts_at(content, addition_line_index, deletion_line_index) {
            break;
        }
        match *content {
            HunkContent::Context { lines, .. } => offset += lines,
            HunkContent::Change {
                additions,
                deletions,
                ..
            } => offset += additions + deletions,
        }
    }
    offset
}

fn split_offset_before_content(
    hunk: &Hunk,
    addition_line_index: usize,
    deletion_line_index: usize,
) -> usize {
    let mut offset = 0usize;
    for content in &hunk.hunk_content {
        if hunk_content_starts_at(content, addition_line_index, deletion_line_index) {
            break;
        }
        match *content {
            HunkContent::Context { lines, .. } => offset += lines,
            HunkContent::Change {
                additions,
                deletions,
                ..
            } => offset += additions.max(deletions),
        }
    }
    offset
}

fn hunk_content_starts_at(
    content: &HunkContent,
    addition_line_index: usize,
    deletion_line_index: usize,
) -> bool {
    match *content {
        HunkContent::Context {
            addition_line_index: content_addition_line_index,
            deletion_line_index: content_deletion_line_index,
            ..
        }
        | HunkContent::Change {
            addition_line_index: content_addition_line_index,
            deletion_line_index: content_deletion_line_index,
            ..
        } => {
            content_addition_line_index == addition_line_index
                && content_deletion_line_index == deletion_line_index
        }
    }
}
