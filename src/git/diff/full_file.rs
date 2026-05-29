use std::collections::HashMap;

use super::{ChangeType, FileContents, FileDiffMetadata, Hunk, HunkContent, ParseDiffOptions};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FullDiffOp {
    Equal { old_index: usize, new_index: usize },
    Delete { old_index: usize, new_index: usize },
    Insert { old_index: usize, new_index: usize },
}

#[inline]
pub fn parse_diff_from_file(
    old_file: &FileContents,
    new_file: &FileContents,
    options: ParseDiffOptions,
) -> FileDiffMetadata {
    let deletion_lines = split_file_contents_owned(&old_file.contents);
    let addition_lines = split_file_contents_owned(&new_file.contents);
    let context_lines = if options.context_lines == 0 {
        4
    } else {
        options.context_lines
    };
    let ops = compute_full_diff_ops(&deletion_lines, &addition_lines, options.ignore_whitespace);
    let hunks = build_full_diff_hunks(&ops, context_lines);
    let mut file = FileDiffMetadata {
        name: new_file.name.clone(),
        prev_name: (old_file.name != new_file.name).then(|| old_file.name.clone()),
        new_object_id: None,
        prev_object_id: None,
        mode: None,
        prev_mode: None,
        change_type: if old_file.name != new_file.name {
            if hunks.is_empty() {
                ChangeType::RenamePure
            } else {
                ChangeType::RenameChanged
            }
        } else if old_file.contents.is_empty() && !new_file.contents.is_empty() {
            ChangeType::New
        } else if !old_file.contents.is_empty() && new_file.contents.is_empty() {
            ChangeType::Deleted
        } else {
            ChangeType::Change
        },
        hunks,
        split_line_count: 0,
        unified_line_count: 0,
        is_partial: false,
        deletion_lines,
        addition_lines,
        cache_key: old_file
            .cache_key
            .as_ref()
            .zip(new_file.cache_key.as_ref())
            .map(|(old_key, new_key)| format!("{old_key}:{new_key}")),
    };

    apply_full_diff_no_eof_flags(&mut file);
    finalize_full_file_line_counts(&mut file);
    file
}

#[inline]
pub(super) fn split_file_contents_owned(contents: &str) -> Vec<String> {
    if contents.is_empty() {
        return Vec::new();
    }

    let mut lines = Vec::new();
    let mut start_index = 0usize;
    for (index, byte) in contents.as_bytes().iter().copied().enumerate() {
        if byte == b'\n' {
            lines.push(contents[start_index..=index].to_string());
            start_index = index + 1;
        }
    }
    if start_index < contents.len() {
        lines.push(contents[start_index..].to_string());
    }
    lines
}

#[inline]
pub(super) fn compute_full_diff_ops(
    old_lines: &[String],
    new_lines: &[String],
    ignore_whitespace: bool,
) -> Vec<FullDiffOp> {
    let anchors = unique_ordered_line_anchors(old_lines, new_lines, ignore_whitespace);
    if anchors.is_empty() {
        return compute_myers_full_diff_ops(old_lines, new_lines, 0, 0, ignore_whitespace);
    }

    let mut ops = Vec::with_capacity(old_lines.len() + new_lines.len());
    let mut previous_old_index = 0usize;
    let mut previous_new_index = 0usize;
    for (old_index, new_index) in anchors {
        ops.extend(compute_myers_full_diff_ops(
            &old_lines[previous_old_index..old_index],
            &new_lines[previous_new_index..new_index],
            previous_old_index,
            previous_new_index,
            ignore_whitespace,
        ));
        ops.push(FullDiffOp::Equal {
            old_index,
            new_index,
        });
        previous_old_index = old_index + 1;
        previous_new_index = new_index + 1;
    }
    ops.extend(compute_myers_full_diff_ops(
        &old_lines[previous_old_index..],
        &new_lines[previous_new_index..],
        previous_old_index,
        previous_new_index,
        ignore_whitespace,
    ));
    ops
}

#[inline]
fn compute_myers_full_diff_ops(
    old_lines: &[String],
    new_lines: &[String],
    old_base_index: usize,
    new_base_index: usize,
    ignore_whitespace: bool,
) -> Vec<FullDiffOp> {
    let old_len = old_lines.len();
    let new_len = new_lines.len();
    if old_len == 0 {
        return (0..new_len)
            .map(|new_index| FullDiffOp::Insert {
                old_index: old_base_index,
                new_index: new_base_index + new_index,
            })
            .collect();
    }
    if new_len == 0 {
        return (0..old_len)
            .map(|old_index| FullDiffOp::Delete {
                old_index: old_base_index + old_index,
                new_index: new_base_index,
            })
            .collect();
    }

    let max_distance = old_len + new_len;
    let offset = max_distance as isize;
    let vector_len = max_distance * 2 + 3;
    let mut frontier = vec![-1isize; vector_len];
    frontier[(offset + 1) as usize] = 0;
    let mut trace = Vec::new();

    for distance in 0..=max_distance {
        let mut next_frontier = frontier.clone();
        let distance = distance as isize;
        let mut diagonal = -distance;
        while diagonal <= distance {
            let mut x = if diagonal == -distance {
                frontier[(offset + diagonal + 1) as usize]
            } else if diagonal != distance
                && frontier[(offset + diagonal - 1) as usize]
                    < frontier[(offset + diagonal + 1) as usize]
            {
                frontier[(offset + diagonal + 1) as usize]
            } else {
                frontier[(offset + diagonal - 1) as usize] + 1
            };
            let mut y = x - diagonal;

            while x >= 0
                && y >= 0
                && (x as usize) < old_len
                && (y as usize) < new_len
                && diff_lines_equal(
                    &old_lines[x as usize],
                    &new_lines[y as usize],
                    ignore_whitespace,
                )
            {
                x += 1;
                y += 1;
            }

            next_frontier[(offset + diagonal) as usize] = x;
            if x as usize >= old_len && y as usize >= new_len {
                trace.push(next_frontier);
                return backtrack_full_diff_ops(
                    &trace,
                    old_len,
                    new_len,
                    old_base_index,
                    new_base_index,
                    offset,
                );
            }
            diagonal += 2;
        }

        trace.push(next_frontier.clone());
        frontier = next_frontier;
    }

    Vec::new()
}

#[inline]
fn unique_ordered_line_anchors(
    old_lines: &[String],
    new_lines: &[String],
    ignore_whitespace: bool,
) -> Vec<(usize, usize)> {
    let mut old_occurrences: HashMap<String, (usize, usize)> = HashMap::new();
    for (index, line) in old_lines.iter().enumerate() {
        let entry = old_occurrences
            .entry(diff_line_key(line, ignore_whitespace))
            .or_insert((0, index));
        entry.0 += 1;
        entry.1 = index;
    }

    let mut new_occurrences: HashMap<String, (usize, usize)> = HashMap::new();
    for (index, line) in new_lines.iter().enumerate() {
        let entry = new_occurrences
            .entry(diff_line_key(line, ignore_whitespace))
            .or_insert((0, index));
        entry.0 += 1;
        entry.1 = index;
    }

    let mut candidates = old_occurrences
        .into_iter()
        .filter_map(|(line, (old_count, old_index))| {
            if old_count != 1 {
                return None;
            }
            let (new_count, new_index) = new_occurrences.get(&line).copied()?;
            (new_count == 1).then_some((old_index, new_index))
        })
        .collect::<Vec<_>>();
    candidates.sort_unstable_by_key(|(old_index, new_index)| (*old_index, *new_index));
    longest_increasing_new_index_subsequence(&candidates)
}

#[inline]
fn diff_line_key(line: &str, ignore_whitespace: bool) -> String {
    if ignore_whitespace {
        line.trim().to_string()
    } else {
        line.to_string()
    }
}

#[inline]
fn longest_increasing_new_index_subsequence(candidates: &[(usize, usize)]) -> Vec<(usize, usize)> {
    if candidates.is_empty() {
        return Vec::new();
    }

    let mut tails: Vec<usize> = Vec::new();
    let mut previous: Vec<Option<usize>> = vec![None; candidates.len()];
    for (candidate_index, &(_, new_index)) in candidates.iter().enumerate() {
        let position = tails
            .binary_search_by(|&tail_index| candidates[tail_index].1.cmp(&new_index))
            .unwrap_or_else(|position| position);
        if position > 0 {
            previous[candidate_index] = Some(tails[position - 1]);
        }
        if position == tails.len() {
            tails.push(candidate_index);
        } else {
            tails[position] = candidate_index;
        }
    }

    let mut result = Vec::with_capacity(tails.len());
    let mut current = tails.last().copied();
    while let Some(index) = current {
        result.push(candidates[index]);
        current = previous[index];
    }
    result.reverse();
    result
}

#[inline]
fn backtrack_full_diff_ops(
    trace: &[Vec<isize>],
    old_len: usize,
    new_len: usize,
    old_base_index: usize,
    new_base_index: usize,
    offset: isize,
) -> Vec<FullDiffOp> {
    let mut ops = Vec::with_capacity(old_len + new_len);
    let mut x = old_len as isize;
    let mut y = new_len as isize;

    for distance in (1..trace.len()).rev() {
        let previous_frontier = &trace[distance - 1];
        let diagonal = x - y;
        let distance = distance as isize;
        let previous_diagonal = if diagonal == -distance
            || (diagonal != distance
                && previous_frontier[(offset + diagonal - 1) as usize]
                    < previous_frontier[(offset + diagonal + 1) as usize])
        {
            diagonal + 1
        } else {
            diagonal - 1
        };

        let previous_x = previous_frontier[(offset + previous_diagonal) as usize];
        let previous_y = previous_x - previous_diagonal;

        while x > previous_x && y > previous_y {
            x -= 1;
            y -= 1;
            ops.push(FullDiffOp::Equal {
                old_index: old_base_index + x as usize,
                new_index: new_base_index + y as usize,
            });
        }

        if x == previous_x {
            y -= 1;
            ops.push(FullDiffOp::Insert {
                old_index: old_base_index + x as usize,
                new_index: new_base_index + y as usize,
            });
        } else {
            x -= 1;
            ops.push(FullDiffOp::Delete {
                old_index: old_base_index + x as usize,
                new_index: new_base_index + y as usize,
            });
        }
    }

    while x > 0 && y > 0 {
        x -= 1;
        y -= 1;
        ops.push(FullDiffOp::Equal {
            old_index: old_base_index + x as usize,
            new_index: new_base_index + y as usize,
        });
    }
    while x > 0 {
        x -= 1;
        ops.push(FullDiffOp::Delete {
            old_index: old_base_index + x as usize,
            new_index: new_base_index,
        });
    }
    while y > 0 {
        y -= 1;
        ops.push(FullDiffOp::Insert {
            old_index: old_base_index,
            new_index: new_base_index + y as usize,
        });
    }

    ops.reverse();
    ops
}

#[inline]
fn diff_lines_equal(old_line: &str, new_line: &str, ignore_whitespace: bool) -> bool {
    if ignore_whitespace {
        old_line.trim() == new_line.trim()
    } else {
        old_line == new_line
    }
}

#[inline]
fn build_full_diff_hunks(ops: &[FullDiffOp], context_lines: usize) -> Vec<Hunk> {
    let mut changed_ranges = Vec::new();
    let mut index = 0usize;
    while index < ops.len() {
        if matches!(ops[index], FullDiffOp::Equal { .. }) {
            index += 1;
            continue;
        }
        let start = index;
        while index < ops.len() && !matches!(ops[index], FullDiffOp::Equal { .. }) {
            index += 1;
        }
        changed_ranges.push((start, index));
    }

    if changed_ranges.is_empty() {
        return Vec::new();
    }

    let mut expanded_ranges: Vec<(usize, usize)> = Vec::new();
    for (start, end) in changed_ranges {
        let expanded_start = start.saturating_sub(context_lines);
        let expanded_end = (end + context_lines).min(ops.len());
        if let Some((_, previous_end)) = expanded_ranges.last_mut() {
            if expanded_start <= *previous_end {
                *previous_end = (*previous_end).max(expanded_end);
                continue;
            }
        }
        expanded_ranges.push((expanded_start, expanded_end));
    }

    expanded_ranges
        .into_iter()
        .map(|(start, end)| build_full_diff_hunk(&ops[start..end]))
        .collect()
}

#[inline]
fn build_full_diff_hunk(ops: &[FullDiffOp]) -> Hunk {
    let old_start_index = ops.iter().find_map(full_diff_old_index).unwrap_or(0);
    let new_start_index = ops.iter().find_map(full_diff_new_index).unwrap_or(0);
    let mut hunk = Hunk {
        collapsed_before: 0,
        split_line_count: 0,
        split_line_start: 0,
        unified_line_count: 0,
        unified_line_start: 0,
        addition_count: 0,
        addition_start: full_diff_start_line_number(new_start_index, ops, true),
        addition_lines: 0,
        addition_line_index: new_start_index,
        deletion_count: 0,
        deletion_start: full_diff_start_line_number(old_start_index, ops, false),
        deletion_lines: 0,
        deletion_line_index: old_start_index,
        hunk_content: Vec::new(),
        hunk_context: None,
        hunk_specs: String::new(),
        no_eof_cr_additions: false,
        no_eof_cr_deletions: false,
    };

    let mut current_content_index = None;
    for op in ops {
        match *op {
            FullDiffOp::Equal {
                old_index,
                new_index,
            } => {
                let index = ensure_context_group(
                    &mut hunk.hunk_content,
                    &mut current_content_index,
                    old_index,
                    new_index,
                );
                if let HunkContent::Context { lines, .. } = &mut hunk.hunk_content[index] {
                    *lines += 1;
                }
                hunk.addition_count += 1;
                hunk.deletion_count += 1;
            }
            FullDiffOp::Delete {
                old_index,
                new_index,
            } => {
                let index = ensure_change_group(
                    &mut hunk.hunk_content,
                    &mut current_content_index,
                    old_index,
                    new_index,
                );
                if let HunkContent::Change { deletions, .. } = &mut hunk.hunk_content[index] {
                    *deletions += 1;
                }
                hunk.deletion_count += 1;
                hunk.deletion_lines += 1;
            }
            FullDiffOp::Insert {
                old_index,
                new_index,
            } => {
                let index = ensure_change_group(
                    &mut hunk.hunk_content,
                    &mut current_content_index,
                    old_index,
                    new_index,
                );
                if let HunkContent::Change { additions, .. } = &mut hunk.hunk_content[index] {
                    *additions += 1;
                }
                hunk.addition_count += 1;
                hunk.addition_lines += 1;
            }
        }
    }

    hunk.hunk_specs = format!(
        "@@ -{},{} +{},{} @@",
        hunk.deletion_start, hunk.deletion_count, hunk.addition_start, hunk.addition_count
    );
    for content in &hunk.hunk_content {
        match content {
            HunkContent::Context { lines, .. } => {
                hunk.split_line_count += *lines;
                hunk.unified_line_count += *lines;
            }
            HunkContent::Change {
                additions,
                deletions,
                ..
            } => {
                hunk.split_line_count += (*additions).max(*deletions);
                hunk.unified_line_count += *additions + *deletions;
            }
        }
    }
    hunk
}

#[inline]
fn ensure_change_group(
    content: &mut Vec<HunkContent>,
    current_content_index: &mut Option<usize>,
    deletion_line_index: usize,
    addition_line_index: usize,
) -> usize {
    if let Some(index) = *current_content_index {
        if matches!(content.get(index), Some(HunkContent::Change { .. })) {
            return index;
        }
    }

    content.push(HunkContent::Change {
        deletions: 0,
        deletion_line_index,
        additions: 0,
        addition_line_index,
    });
    let index = content.len() - 1;
    *current_content_index = Some(index);
    index
}

#[inline]
fn ensure_context_group(
    content: &mut Vec<HunkContent>,
    current_content_index: &mut Option<usize>,
    deletion_line_index: usize,
    addition_line_index: usize,
) -> usize {
    if let Some(index) = *current_content_index {
        if matches!(content.get(index), Some(HunkContent::Context { .. })) {
            return index;
        }
    }

    content.push(HunkContent::Context {
        lines: 0,
        addition_line_index,
        deletion_line_index,
    });
    let index = content.len() - 1;
    *current_content_index = Some(index);
    index
}

#[inline]
fn full_diff_old_index(op: &FullDiffOp) -> Option<usize> {
    match *op {
        FullDiffOp::Equal { old_index, .. } | FullDiffOp::Delete { old_index, .. } => {
            Some(old_index)
        }
        FullDiffOp::Insert { .. } => None,
    }
}

#[inline]
fn full_diff_new_index(op: &FullDiffOp) -> Option<usize> {
    match *op {
        FullDiffOp::Equal { new_index, .. } | FullDiffOp::Insert { new_index, .. } => {
            Some(new_index)
        }
        FullDiffOp::Delete { .. } => None,
    }
}

#[inline]
fn full_diff_start_line_number(
    start_index: usize,
    ops: &[FullDiffOp],
    addition_side: bool,
) -> usize {
    let side_has_lines = ops.iter().any(|op| {
        if addition_side {
            matches!(op, FullDiffOp::Equal { .. } | FullDiffOp::Insert { .. })
        } else {
            matches!(op, FullDiffOp::Equal { .. } | FullDiffOp::Delete { .. })
        }
    });
    if side_has_lines {
        start_index + 1
    } else {
        start_index
    }
}

#[inline]
fn finalize_full_file_line_counts(file: &mut FileDiffMetadata) {
    let mut last_hunk_end = 0usize;
    for hunk in &mut file.hunks {
        hunk.collapsed_before = hunk
            .addition_start
            .saturating_sub(1)
            .saturating_sub(last_hunk_end);
        hunk.split_line_start = file.split_line_count + hunk.collapsed_before;
        hunk.unified_line_start = file.unified_line_count + hunk.collapsed_before;
        file.split_line_count += hunk.collapsed_before + hunk.split_line_count;
        file.unified_line_count += hunk.collapsed_before + hunk.unified_line_count;
        last_hunk_end = hunk
            .addition_start
            .saturating_add(hunk.addition_count)
            .saturating_sub(1);
    }

    if let Some(last_hunk) = file.hunks.last() {
        if !file.addition_lines.is_empty() && !file.deletion_lines.is_empty() {
            let last_hunk_end = last_hunk
                .addition_start
                .saturating_add(last_hunk.addition_count)
                .saturating_sub(1);
            let collapsed_after = file.addition_lines.len().saturating_sub(last_hunk_end);
            file.split_line_count += collapsed_after;
            file.unified_line_count += collapsed_after;
        }
    }
}

#[inline]
fn apply_full_diff_no_eof_flags(file: &mut FileDiffMetadata) {
    let deletion_has_no_eof_cr = file
        .deletion_lines
        .last()
        .is_some_and(|line| !line.ends_with('\n'));
    let addition_has_no_eof_cr = file
        .addition_lines
        .last()
        .is_some_and(|line| !line.ends_with('\n'));

    for hunk in &mut file.hunks {
        if deletion_has_no_eof_cr
            && hunk.deletion_line_index + hunk.deletion_count == file.deletion_lines.len()
        {
            hunk.no_eof_cr_deletions = true;
        }
        if addition_has_no_eof_cr
            && hunk.addition_line_index + hunk.addition_count == file.addition_lines.len()
        {
            hunk.no_eof_cr_additions = true;
        }
    }
}
