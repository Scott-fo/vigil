use std::collections::VecDeque;

use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::ui;

use super::{
    DIFF_TAB_WIDTH, DiffLineKind, DiffRow, DiffRowSyntax, DiffSelectionPoint, DisplayNavTarget,
    DisplayRowRefs, DisplaySelectionLine, DisplaySelectionSegment, GapExpandDirection,
    MergeConflictMarkerRowType, SyntaxToken,
};

fn resolve_split_target_line(left: Option<&DiffRow>, right: Option<&DiffRow>) -> Option<usize> {
    right
        .and_then(|row| row.new_line)
        .or_else(|| left.and_then(|row| row.old_line))
}

#[derive(Debug, Clone)]
pub(super) struct RenderedDisplayLine {
    pub(super) line: Line<'static>,
    pub(super) selection: DisplaySelectionLine,
}

#[derive(Debug, Clone)]
struct WrappedLineContent {
    spans: Vec<Span<'static>>,
    text: String,
    content_width: usize,
}

pub(super) fn render_unified_code_lines(row: &DiffRow, width: usize) -> Vec<RenderedDisplayLine> {
    let base_style = base_style(row.kind);
    let sign_style = match row.kind {
        DiffLineKind::Context => ui::context_sign_style(),
        DiffLineKind::Added => ui::added_sign_style(),
        DiffLineKind::Removed => ui::removed_sign_style(),
        DiffLineKind::ConflictAction | DiffLineKind::ConflictMarker(_) => ui::diff_hunk_style(),
    };
    let marker = match row.kind {
        DiffLineKind::Context => ' ',
        DiffLineKind::Added => '+',
        DiffLineKind::Removed => '-',
        DiffLineKind::ConflictAction => ' ',
        DiffLineKind::ConflictMarker(_) => '!',
    };
    let unified_line_number = match row.kind {
        DiffLineKind::Added | DiffLineKind::Context => row.new_line,
        DiffLineKind::Removed => row.old_line,
        DiffLineKind::ConflictAction | DiffLineKind::ConflictMarker(_) => None,
    };

    let prefix = vec![
        Span::styled(
            format_line_number(unified_line_number),
            base_style.patch(ui::line_number_style()),
        ),
        Span::styled(format!("{marker} "), sign_style),
    ];
    let continuation_prefix = vec![
        Span::styled(
            " ".repeat(format_line_number(None).width()),
            base_style.patch(ui::line_number_style()),
        ),
        Span::styled("  ", sign_style),
    ];
    let content = render_row_content(row.unified_content(), &row.text, base_style);
    let prefix_width = spans_width(&prefix);
    wrap_prefixed_spans_to_lines(prefix, continuation_prefix, content, width, base_style)
        .into_iter()
        .map(|wrapped| RenderedDisplayLine {
            line: Line::from(wrapped.spans).style(base_style),
            selection: DisplaySelectionLine {
                unified: Some(DisplaySelectionSegment {
                    start_column: prefix_width,
                    content_width: wrapped.content_width,
                    text: wrapped.text,
                }),
                ..DisplaySelectionLine::default()
            },
        })
        .collect()
}

fn render_split_pair_lines(
    left: Option<&DiffRow>,
    right: Option<&DiffRow>,
    side_width: usize,
) -> Vec<RenderedDisplayLine> {
    let left_lines = render_split_side_lines(left, true, side_width);
    let right_lines = render_split_side_lines(right, false, side_width);
    let line_count = left_lines.len().max(right_lines.len());
    let gap = Span::styled("   ", ui::diff_context_style());

    (0..line_count)
        .map(|index| {
            let left_line = left_lines
                .get(index)
                .cloned()
                .unwrap_or_else(|| blank_split_side(side_width));
            let right_line = right_lines
                .get(index)
                .cloned()
                .unwrap_or_else(|| blank_split_side(side_width));
            let mut spans = Vec::new();
            spans.extend(left_line.spans.clone());
            spans.push(gap.clone());
            spans.extend(right_line.spans.clone());
            RenderedDisplayLine {
                line: Line::from(spans),
                selection: DisplaySelectionLine {
                    left: Some(DisplaySelectionSegment {
                        start_column: left_line.start_column,
                        content_width: left_line.content_width,
                        text: left_line.text,
                    }),
                    right: Some(DisplaySelectionSegment {
                        start_column: side_width + 3 + right_line.start_column,
                        content_width: right_line.content_width,
                        text: right_line.text,
                    }),
                    ..DisplaySelectionLine::default()
                },
            }
        })
        .collect()
}

pub(super) fn render_split_hunk_rows(
    rows: &[DiffRow],
    row_index_offset: usize,
    side_width: usize,
) -> Vec<(
    Line<'static>,
    Option<DisplayNavTarget>,
    DisplayRowRefs,
    DisplaySelectionLine,
)> {
    let mut rendered = Vec::with_capacity(rows.len());
    let mut pending_removed: Vec<(usize, &DiffRow)> = Vec::new();
    let mut pending_added: Vec<(usize, &DiffRow)> = Vec::new();

    let flush_pending = |rendered: &mut Vec<(
        Line<'static>,
        Option<DisplayNavTarget>,
        DisplayRowRefs,
        DisplaySelectionLine,
    )>,
                         removed: &mut Vec<(usize, &DiffRow)>,
                         added: &mut Vec<(usize, &DiffRow)>| {
        let row_count = removed.len().max(added.len());
        for index in 0..row_count {
            let left = removed.get(index).copied();
            let right = added.get(index).copied();
            let target_line =
                resolve_split_target_line(left.map(|(_, row)| row), right.map(|(_, row)| row));
            let row_refs = DisplayRowRefs {
                left: left.map(|(row_index, _)| row_index),
                right: right.map(|(row_index, _)| row_index),
            };
            for rendered_line in render_split_pair_lines(
                left.map(|(_, row)| row),
                right.map(|(_, row)| row),
                side_width,
            ) {
                rendered.push((
                    rendered_line.line,
                    target_line.map(DisplayNavTarget::Line),
                    row_refs,
                    rendered_line.selection,
                ));
            }
        }
        removed.clear();
        added.clear();
    };

    for (row_offset, row) in rows.iter().enumerate() {
        let row_index = row_index_offset + row_offset;
        match row.kind {
            DiffLineKind::Removed => pending_removed.push((row_index, row)),
            DiffLineKind::Added => pending_added.push((row_index, row)),
            DiffLineKind::Context => {
                flush_pending(&mut rendered, &mut pending_removed, &mut pending_added);
                let target_line = resolve_split_target_line(Some(row), Some(row));
                let row_refs = DisplayRowRefs {
                    left: Some(row_index),
                    right: Some(row_index),
                };
                for rendered_line in render_split_pair_lines(Some(row), Some(row), side_width) {
                    rendered.push((
                        rendered_line.line,
                        target_line.map(DisplayNavTarget::Line),
                        row_refs,
                        rendered_line.selection,
                    ));
                }
            }
            DiffLineKind::ConflictAction | DiffLineKind::ConflictMarker(_) => {
                flush_pending(&mut rendered, &mut pending_removed, &mut pending_added);
                let row_refs = DisplayRowRefs {
                    left: Some(row_index),
                    right: Some(row_index),
                };
                for rendered_line in render_unified_code_lines(row, side_width * 2 + 3) {
                    rendered.push((
                        rendered_line.line,
                        row.conflict_index.map(DisplayNavTarget::Conflict),
                        row_refs,
                        rendered_line.selection,
                    ));
                }
            }
        }
    }

    flush_pending(&mut rendered, &mut pending_removed, &mut pending_added);
    rendered
}

pub(super) fn render_expand_gap_line(
    width: usize,
    _remaining: usize,
    _has_expansion: bool,
    direction: GapExpandDirection,
) -> Line<'static> {
    let hint_style = ui::diff_hunk_style();
    let action_style = ui::diff_hunk_style().add_modifier(Modifier::BOLD);
    let label = match direction {
        GapExpandDirection::Down => "↑↑",
        GapExpandDirection::Up => "↓↓",
    };
    let side_padding = 1;
    let trailing_padding = width
        .saturating_sub(side_padding)
        .saturating_sub(label.width());
    let mut spans = vec![
        Span::styled(" ".repeat(side_padding), hint_style),
        Span::styled(label.to_string(), action_style),
        Span::styled(" ".repeat(trailing_padding), hint_style),
    ];
    spans = fit_spans_to_width(spans, width.max(1), hint_style);
    Line::from(spans).style(ui::diff_hunk_style())
}

pub(super) fn render_expanded_context_lines(
    line_number: usize,
    text: &str,
    highlighted_content: Option<Vec<SyntaxToken>>,
    width: usize,
    split: bool,
) -> Vec<RenderedDisplayLine> {
    let row = DiffRow {
        kind: DiffLineKind::Context,
        old_line: Some(line_number),
        new_line: Some(line_number),
        conflict_index: None,
        text: text.to_string(),
        syntax: DiffRowSyntax {
            left: highlighted_content.clone(),
            right: highlighted_content,
        },
    };
    if split {
        let total_width = width.saturating_sub(1);
        let gutter_width = 3;
        let side_width = total_width.saturating_sub(gutter_width) / 2;
        render_split_pair_lines(Some(&row), Some(&row), side_width)
    } else {
        render_unified_code_lines(&row, width)
    }
}

#[derive(Debug, Clone)]
struct WrappedSideLine {
    spans: Vec<Span<'static>>,
    start_column: usize,
    content_width: usize,
    text: String,
}

fn render_split_side_lines(
    row: Option<&DiffRow>,
    left_side: bool,
    width: usize,
) -> Vec<WrappedSideLine> {
    let Some(row) = row else {
        return vec![blank_split_side(width)];
    };

    let line_number = if left_side {
        row.old_line
    } else {
        row.new_line
    };
    let base_style = base_style(row.kind);
    let prefix = vec![Span::styled(
        format_line_number(line_number),
        base_style.patch(ui::line_number_style()),
    )];
    let continuation_prefix = vec![Span::styled(
        " ".repeat(format_line_number(None).width()),
        base_style.patch(ui::line_number_style()),
    )];
    let content = render_row_content(row.side_content(left_side), &row.text, base_style);
    let prefix_width = spans_width(&prefix);
    wrap_prefixed_spans_to_lines(prefix, continuation_prefix, content, width + 1, base_style)
        .into_iter()
        .map(|wrapped| WrappedSideLine {
            spans: wrapped.spans,
            start_column: prefix_width,
            content_width: wrapped.content_width,
            text: wrapped.text,
        })
        .collect()
}

fn blank_split_side(width: usize) -> WrappedSideLine {
    WrappedSideLine {
        spans: vec![Span::styled(" ".repeat(width), ui::diff_context_style())],
        start_column: 0,
        content_width: width,
        text: String::new(),
    }
}

fn render_row_content(
    syntax_tokens: Option<&[SyntaxToken]>,
    text: &str,
    fallback: Style,
) -> Vec<Span<'static>> {
    let has_tabs = text.as_bytes().contains(&b'\t');

    let raw_spans = match syntax_tokens {
        Some(tokens) if !tokens.is_empty() => tokens
            .iter()
            .map(|token| {
                let style = token
                    .highlight_name
                    .map(|name| ui::syntax_style(name, fallback))
                    .unwrap_or(fallback);
                let content = text
                    .get(token.start..token.end)
                    .map(str::to_string)
                    .unwrap_or_default();
                Span::styled(content, style)
            })
            .collect(),
        _ => vec![Span::styled(text.to_string(), fallback)],
    };

    if has_tabs {
        expand_tabs_in_spans(raw_spans)
    } else {
        raw_spans
    }
}

pub(super) fn expand_tabs_in_spans(spans: Vec<Span<'static>>) -> Vec<Span<'static>> {
    let mut expanded = Vec::with_capacity(spans.len());
    let mut visual_column = 0usize;

    for span in spans {
        let style = span.style;
        let mut chunk = String::new();

        for ch in span.content.chars() {
            if ch == '\t' {
                if !chunk.is_empty() {
                    expanded.push(Span::styled(std::mem::take(&mut chunk), style));
                }

                let tab_width = tab_display_width(visual_column);
                if tab_width > 0 {
                    expanded.push(Span::styled(" ".repeat(tab_width), style));
                    visual_column += tab_width;
                }
                continue;
            }

            let Some(ch_width) = UnicodeWidthChar::width(ch) else {
                continue;
            };
            if ch_width == 0 {
                continue;
            }

            chunk.push(ch);
            visual_column += ch_width;
        }

        if !chunk.is_empty() {
            expanded.push(Span::styled(chunk, style));
        }
    }

    expanded
}

fn tab_display_width(visual_column: usize) -> usize {
    let offset = visual_column % DIFF_TAB_WIDTH;
    if offset == 0 {
        DIFF_TAB_WIDTH
    } else {
        DIFF_TAB_WIDTH - offset
    }
}

fn fit_spans_to_width(
    spans: Vec<Span<'static>>,
    width: usize,
    pad_style: Style,
) -> Vec<Span<'static>> {
    if width == 0 {
        return Vec::new();
    }

    let mut fitted = Vec::new();
    let mut current_width = 0usize;

    for span in spans {
        if current_width >= width {
            break;
        }

        let content = span.content.as_ref();
        let remaining = width.saturating_sub(current_width);
        let content_width = UnicodeWidthStr::width(content);

        if content_width <= remaining {
            current_width += content_width;
            fitted.push(span);
            continue;
        }

        let truncated = truncate_to_width(content, remaining);
        current_width += UnicodeWidthStr::width(truncated.as_str());
        fitted.push(Span::styled(truncated, span.style));
        break;
    }

    if current_width < width {
        fitted.push(Span::styled(" ".repeat(width - current_width), pad_style));
    }

    fitted
}

fn wrap_prefixed_spans_to_lines(
    prefix: Vec<Span<'static>>,
    continuation_prefix: Vec<Span<'static>>,
    content: Vec<Span<'static>>,
    width: usize,
    pad_style: Style,
) -> Vec<WrappedLineContent> {
    let target_width = width.saturating_sub(1);
    let prefix_width = spans_width(&prefix);
    let continuation_prefix_width = spans_width(&continuation_prefix);
    let content_width = target_width.saturating_sub(prefix_width);
    let continuation_content_width = target_width.saturating_sub(continuation_prefix_width);

    if target_width == 0 || content_width == 0 || continuation_content_width == 0 {
        let mut spans = prefix;
        spans.extend(content);
        return wrap_spans_to_width(spans, target_width.max(1), pad_style)
            .into_iter()
            .map(|line| WrappedLineContent {
                text: line_text(&line),
                content_width: spans_width(&line),
                spans: line,
            })
            .collect();
    }

    let wrapped_content = wrap_spans_to_width(content, content_width, pad_style);
    wrapped_content
        .into_iter()
        .enumerate()
        .map(|(index, line_content)| {
            let text = line_text(&line_content);
            let mut spans = if index == 0 {
                prefix.clone()
            } else {
                continuation_prefix.clone()
            };
            spans.extend(line_content);
            WrappedLineContent {
                text,
                content_width: if index == 0 {
                    content_width
                } else {
                    continuation_content_width
                },
                spans,
            }
        })
        .collect()
}

fn line_text(spans: &[Span<'static>]) -> String {
    spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>()
        .trim_end_matches(' ')
        .to_string()
}

fn wrap_spans_to_width(
    spans: Vec<Span<'static>>,
    width: usize,
    pad_style: Style,
) -> Vec<Vec<Span<'static>>> {
    if width == 0 {
        return vec![Vec::new()];
    }

    let mut queue = VecDeque::from(spans);
    let mut wrapped = Vec::new();

    while !queue.is_empty() {
        let mut line = Vec::new();
        let mut current_width = 0usize;

        while current_width < width {
            let Some(span) = queue.pop_front() else {
                break;
            };

            let content = span.content.into_owned();
            let remaining = width.saturating_sub(current_width);
            let content_width = UnicodeWidthStr::width(content.as_str());

            if content_width <= remaining {
                current_width += content_width;
                line.push(Span::styled(content, span.style));
                continue;
            }

            let (head, tail) = split_string_at_width(&content, remaining);
            if !head.is_empty() {
                current_width += UnicodeWidthStr::width(head.as_str());
                line.push(Span::styled(head, span.style));
            }
            if !tail.is_empty() {
                queue.push_front(Span::styled(tail, span.style));
            }
            break;
        }

        if current_width < width {
            line.push(Span::styled(" ".repeat(width - current_width), pad_style));
        }
        wrapped.push(line);
    }

    if wrapped.is_empty() {
        wrapped.push(vec![Span::styled(" ".repeat(width), pad_style)]);
    }

    wrapped
}

fn spans_width(spans: &[Span<'static>]) -> usize {
    spans
        .iter()
        .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
        .sum()
}

pub(super) fn normalize_selection_points(
    anchor: DiffSelectionPoint,
    head: DiffSelectionPoint,
) -> (DiffSelectionPoint, DiffSelectionPoint) {
    if (anchor.display_index, anchor.column) <= (head.display_index, head.column) {
        (anchor, head)
    } else {
        (head, anchor)
    }
}

fn split_string_at_width(content: &str, width: usize) -> (String, String) {
    let mut head = String::new();
    let mut used = 0usize;
    let mut split_at = content.len();

    for (index, ch) in content.char_indices() {
        let Some(ch_width) = UnicodeWidthChar::width(ch) else {
            continue;
        };
        if ch_width == 0 {
            continue;
        }
        if used + ch_width > width {
            split_at = index;
            return (head, content[split_at..].to_string());
        }
        used += ch_width;
        head.push(ch);
        split_at = index + ch.len_utf8();
    }

    (head, content[split_at..].to_string())
}

pub(super) fn slice_string_by_width(content: &str, start: usize, end: usize) -> String {
    let mut result = String::new();
    let mut used = 0usize;

    for ch in content.chars() {
        let Some(ch_width) = UnicodeWidthChar::width(ch) else {
            continue;
        };
        if ch_width == 0 {
            continue;
        }

        let next_width = used + ch_width;
        if next_width <= start {
            used = next_width;
            continue;
        }
        if used >= end {
            break;
        }

        result.push(ch);
        used = next_width;
    }

    result
}

fn truncate_to_width(content: &str, width: usize) -> String {
    let mut result = String::new();
    let mut used = 0usize;
    for ch in content.chars() {
        let ch_width = UnicodeWidthStr::width(ch.encode_utf8(&mut [0; 4]));
        if used + ch_width > width {
            break;
        }
        used += ch_width;
        result.push(ch);
    }
    result
}

fn format_line_number(line: Option<usize>) -> String {
    format!(
        "{:>4} ",
        line.map_or(String::new(), |line| line.to_string())
    )
}

fn base_style(kind: DiffLineKind) -> Style {
    match kind {
        DiffLineKind::Context => ui::diff_context_style(),
        DiffLineKind::Added => ui::diff_added_style(),
        DiffLineKind::Removed => ui::diff_removed_style(),
        DiffLineKind::ConflictAction => ui::diff_hunk_style(),
        DiffLineKind::ConflictMarker(MergeConflictMarkerRowType::MarkerStart)
        | DiffLineKind::ConflictMarker(MergeConflictMarkerRowType::MarkerBase) => {
            ui::diff_removed_style()
        }
        DiffLineKind::ConflictMarker(MergeConflictMarkerRowType::MarkerSeparator)
        | DiffLineKind::ConflictMarker(MergeConflictMarkerRowType::MarkerEnd) => {
            ui::diff_added_style()
        }
    }
}
