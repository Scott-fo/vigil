use std::collections::HashMap;

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::{
    DiffHunkBlock, DiffHunkGap, DiffLineKind, DiffRow, DiffRowSyntax, FileDiffMetadata,
    HunkContent, MergeConflictDiffAction, MergeConflictLabels, MergeConflictMarkerRow,
    MergeConflictMarkerRowType, line_without_ending, parse_patch_files, process_file,
};

const CONFLICT_SIDE_LABEL_WIDTH: usize = 32;

#[inline]
pub(super) fn build_diff_rows(
    diff: &str,
    _filetype: Option<&'static str>,
) -> (Vec<DiffRow>, Vec<DiffHunkBlock>, Vec<DiffHunkGap>) {
    let parsed = parse_patch_files(diff, None, false).unwrap_or_default();
    let mut rows = Vec::new();
    let mut hunks = Vec::new();

    for patch in parsed {
        for file in patch.files {
            append_file_diff_rows(&file, &mut rows, &mut hunks);
        }
    }
    if rows.is_empty() && diff.trim_start().starts_with("@@ -") {
        let synthetic = format!("--- a/file\n+++ b/file\n{diff}");
        if let Ok(Some(file)) = process_file(&synthetic, None, Some(false), false) {
            append_file_diff_rows(&file, &mut rows, &mut hunks);
        }
    }

    let gaps = diff_gaps_from_hunks(&hunks);

    (rows, hunks, gaps)
}

pub(super) fn diff_gaps_from_hunks(hunks: &[DiffHunkBlock]) -> Vec<DiffHunkGap> {
    let mut gaps = Vec::new();
    for (gap_index, pair) in hunks.windows(2).enumerate() {
        let previous = &pair[0];
        let next = &pair[1];
        let new_start = previous.new_start.saturating_add(previous.new_count);
        let new_count = next.new_start.saturating_sub(new_start);
        if new_count == 0 {
            continue;
        }
        gaps.push(DiffHunkGap {
            gap_index,
            new_start,
            new_count,
        });
    }
    gaps
}

fn append_file_diff_rows(
    file: &FileDiffMetadata,
    rows: &mut Vec<DiffRow>,
    hunks: &mut Vec<DiffHunkBlock>,
) {
    append_file_diff_rows_with_conflicts(
        file,
        &[],
        &[],
        &MergeConflictLabels::default(),
        rows,
        hunks,
    );
}

pub(super) fn append_file_diff_rows_with_conflicts(
    file: &FileDiffMetadata,
    actions: &[Option<MergeConflictDiffAction>],
    marker_rows: &[MergeConflictMarkerRow],
    labels: &MergeConflictLabels,
    rows: &mut Vec<DiffRow>,
    hunks: &mut Vec<DiffHunkBlock>,
) {
    for hunk in &file.hunks {
        let hunk_index = hunks.len();
        let chrome = build_conflict_chrome_rows(actions, marker_rows, labels, hunk_index);
        let row_start = rows.len();
        let mut old_line = hunk.deletion_start;
        let mut new_line = hunk.addition_start;

        for (content_index, content) in hunk.hunk_content.iter().enumerate() {
            append_chrome_rows(rows, chrome.before.get(&content_index));
            let conflict_index =
                conflict_index_for_hunk_content(actions, hunk_index, content_index);
            match content {
                HunkContent::Context {
                    lines,
                    addition_line_index,
                    ..
                } => {
                    for offset in 0..*lines {
                        let text = file
                            .addition_lines
                            .get(addition_line_index + offset)
                            .map(|line| line_without_ending(line))
                            .unwrap_or("");
                        rows.push(render_diff_row(
                            Some(old_line),
                            Some(new_line),
                            text,
                            DiffLineKind::Context,
                            conflict_index,
                        ));
                        old_line += 1;
                        new_line += 1;
                    }
                }
                HunkContent::Change {
                    deletions,
                    deletion_line_index,
                    additions,
                    addition_line_index,
                } => {
                    for offset in 0..*deletions {
                        let text = file
                            .deletion_lines
                            .get(deletion_line_index + offset)
                            .map(|line| line_without_ending(line))
                            .unwrap_or("");
                        rows.push(render_diff_row(
                            Some(old_line),
                            None,
                            text,
                            DiffLineKind::Removed,
                            conflict_index,
                        ));
                        old_line += 1;
                    }
                    append_chrome_rows(rows, chrome.between_change_sides.get(&content_index));
                    for offset in 0..*additions {
                        let text = file
                            .addition_lines
                            .get(addition_line_index + offset)
                            .map(|line| line_without_ending(line))
                            .unwrap_or("");
                        rows.push(render_diff_row(
                            None,
                            Some(new_line),
                            text,
                            DiffLineKind::Added,
                            conflict_index,
                        ));
                        new_line += 1;
                    }
                }
            }
            append_chrome_rows(rows, chrome.after.get(&content_index));
        }

        hunks.push(DiffHunkBlock {
            new_start: hunk.addition_start,
            new_count: hunk.addition_count,
            row_start,
            row_end: rows.len(),
        });
    }
}

#[derive(Debug, Default)]
struct ConflictChromeRows {
    before: HashMap<usize, Vec<DiffRow>>,
    between_change_sides: HashMap<usize, Vec<DiffRow>>,
    after: HashMap<usize, Vec<DiffRow>>,
}

fn build_conflict_chrome_rows(
    actions: &[Option<MergeConflictDiffAction>],
    _marker_rows: &[MergeConflictMarkerRow],
    labels: &MergeConflictLabels,
    hunk_index: usize,
) -> ConflictChromeRows {
    let mut chrome = ConflictChromeRows::default();
    for action in actions.iter().flatten() {
        if action.conflict_data.hunk_index != hunk_index {
            continue;
        }

        push_chrome_row(
            &mut chrome.before,
            action.conflict_data.start_content_index,
            render_conflict_action_row(action.conflict_index, labels),
        );
        push_chrome_row(
            &mut chrome.before,
            action.conflict_data.start_content_index,
            render_conflict_marker_row(
                action.conflict_index,
                MergeConflictMarkerRowType::MarkerStart,
                action.marker_lines.start.as_str(),
                labels,
            ),
        );

        if let (Some(base_content_index), Some(base_marker)) = (
            action.conflict_data.base_content_index,
            action.marker_lines.base.as_deref(),
        ) {
            push_chrome_row(
                &mut chrome.before,
                base_content_index,
                render_conflict_marker_row(
                    action.conflict_index,
                    MergeConflictMarkerRowType::MarkerBase,
                    base_marker,
                    labels,
                ),
            );
            push_chrome_row(
                &mut chrome.after,
                base_content_index,
                render_conflict_marker_row(
                    action.conflict_index,
                    MergeConflictMarkerRowType::MarkerSeparator,
                    action.marker_lines.separator.as_str(),
                    labels,
                ),
            );
        } else {
            let separator_content_index = action
                .conflict_data
                .current_content_index
                .unwrap_or(action.conflict_data.start_content_index);
            push_chrome_row(
                &mut chrome.between_change_sides,
                separator_content_index,
                render_conflict_marker_row(
                    action.conflict_index,
                    MergeConflictMarkerRowType::MarkerSeparator,
                    action.marker_lines.separator.as_str(),
                    labels,
                ),
            );
        }

        push_chrome_row(
            &mut chrome.after,
            action.conflict_data.end_marker_content_index,
            render_conflict_marker_row(
                action.conflict_index,
                MergeConflictMarkerRowType::MarkerEnd,
                action.marker_lines.end.as_str(),
                labels,
            ),
        );
    }

    chrome
}

fn push_chrome_row(map: &mut HashMap<usize, Vec<DiffRow>>, content_index: usize, row: DiffRow) {
    map.entry(content_index).or_default().push(row);
}

fn append_chrome_rows(rows: &mut Vec<DiffRow>, chrome_rows: Option<&Vec<DiffRow>>) {
    if let Some(chrome_rows) = chrome_rows {
        rows.extend(chrome_rows.iter().cloned());
    }
}

fn render_conflict_action_row(conflict_index: usize, labels: &MergeConflictLabels) -> DiffRow {
    let current = truncate_conflict_side_label(&labels.current);
    let incoming = truncate_conflict_side_label(&labels.incoming);
    render_diff_row(
        None,
        None,
        &format!("1 Accept {current} | 2 Accept {incoming} | 3 Accept both"),
        DiffLineKind::ConflictAction,
        Some(conflict_index),
    )
}

fn render_conflict_marker_row(
    conflict_index: usize,
    row_type: MergeConflictMarkerRowType,
    line: &str,
    labels: &MergeConflictLabels,
) -> DiffRow {
    let side_label = match row_type {
        MergeConflictMarkerRowType::MarkerStart => Some(labels.current.as_str()),
        MergeConflictMarkerRowType::MarkerBase => Some("Base"),
        MergeConflictMarkerRowType::MarkerSeparator => Some(labels.incoming.as_str()),
        MergeConflictMarkerRowType::MarkerEnd => None,
    };
    let line = line_without_ending(line);
    let text = match side_label {
        Some(label) => format!("{line} ({})", truncate_conflict_side_label(label)),
        None => line.to_string(),
    };
    render_diff_row(
        None,
        None,
        &text,
        DiffLineKind::ConflictMarker(row_type),
        Some(conflict_index),
    )
}

fn conflict_index_for_hunk_content(
    actions: &[Option<MergeConflictDiffAction>],
    hunk_index: usize,
    content_index: usize,
) -> Option<usize> {
    actions.iter().flatten().find_map(|action| {
        (action.conflict_data.hunk_index == hunk_index
            && content_index >= action.conflict_data.start_content_index
            && content_index <= action.conflict_data.end_content_index)
            .then_some(action.conflict_index)
    })
}

fn truncate_conflict_side_label(label: &str) -> String {
    truncate_middle(label.trim(), CONFLICT_SIDE_LABEL_WIDTH)
}

fn truncate_middle(value: &str, max_width: usize) -> String {
    if UnicodeWidthStr::width(value) <= max_width {
        return value.to_string();
    }
    if max_width <= 3 {
        return ".".repeat(max_width);
    }

    let marker = "...";
    let side_width = (max_width - marker.len()) / 2;
    let tail_width = max_width - marker.len() - side_width;
    let head = take_width(value, side_width);
    let tail = take_width_reversed(value, tail_width);
    format!("{head}{marker}{tail}")
}

fn take_width(value: &str, max_width: usize) -> String {
    let mut width = 0;
    let mut result = String::new();
    for ch in value.chars() {
        let Some(ch_width) = UnicodeWidthChar::width(ch) else {
            continue;
        };
        if width + ch_width > max_width {
            break;
        }
        result.push(ch);
        width += ch_width;
    }
    result
}

fn take_width_reversed(value: &str, max_width: usize) -> String {
    let mut width = 0;
    let mut result = Vec::new();
    for ch in value.chars().rev() {
        let Some(ch_width) = UnicodeWidthChar::width(ch) else {
            continue;
        };
        if width + ch_width > max_width {
            break;
        }
        result.push(ch);
        width += ch_width;
    }
    result.into_iter().rev().collect()
}

#[inline]
fn render_diff_row(
    old_line: Option<usize>,
    new_line: Option<usize>,
    content: &str,
    kind: DiffLineKind,
    conflict_index: Option<usize>,
) -> DiffRow {
    DiffRow {
        kind,
        old_line,
        new_line,
        conflict_index,
        text: content.to_string(),
        syntax: DiffRowSyntax::default(),
    }
}
