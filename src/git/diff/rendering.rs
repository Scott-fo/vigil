use std::{collections::VecDeque, ops::Range};

use ratatui::{
    style::Style,
    text::{Line, Span},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{app::DiffLineWrapMode, ui};

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
    /// True for the second and later rows of a soft-wrapped line, so copied
    /// text can rejoin them without a line break.
    continues_previous: bool,
}

/// A row to draw plus the byte ranges of its text to emphasize as changed.
#[derive(Debug, Clone, Copy)]
pub(super) struct RenderRow<'a> {
    pub(super) row: &'a DiffRow,
    pub(super) emphasis: &'a [Range<usize>],
}

impl<'a> RenderRow<'a> {
    /// A row drawn without intra-line emphasis, such as context or chrome.
    pub(super) fn plain(row: &'a DiffRow) -> Self {
        Self { row, emphasis: &[] }
    }
}

pub(super) fn render_unified_code_lines(
    input: RenderRow<'_>,
    width: usize,
    line_wrap: DiffLineWrapMode,
) -> Vec<RenderedDisplayLine> {
    let RenderRow { row, emphasis } = input;
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
        Span::styled(
            format!("{WRAP_MARKER} "),
            base_style.patch(ui::diff_wrap_marker_style()),
        ),
    ];
    let content = render_row_content(row.unified_content(), row, emphasis, base_style);
    let prefix_width = spans_width(&prefix);
    prefixed_spans_to_lines(
        prefix,
        continuation_prefix,
        content,
        width,
        base_style,
        line_wrap,
    )
    .into_iter()
    .map(|wrapped| RenderedDisplayLine {
        line: Line::from(wrapped.spans).style(base_style),
        selection: DisplaySelectionLine {
            unified: Some(DisplaySelectionSegment {
                start_column: prefix_width,
                content_width: wrapped.content_width,
                text: wrapped.text,
                continues_previous: wrapped.continues_previous,
            }),
            ..DisplaySelectionLine::default()
        },
    })
    .collect()
}

pub(super) fn render_split_pair_lines(
    left: Option<RenderRow<'_>>,
    right: Option<RenderRow<'_>>,
    side_width: usize,
    line_wrap: DiffLineWrapMode,
) -> Vec<RenderedDisplayLine> {
    let left_lines = render_split_side_lines(left, true, side_width, line_wrap);
    let right_lines = render_split_side_lines(right, false, side_width, line_wrap);
    let line_count = left_lines.len().max(right_lines.len());
    let gap = Span::styled(" │ ", ui::line_number_style());

    (0..line_count)
        .map(|index| {
            // A side with fewer wrapped rows than the other is padded with
            // blank rows that belong to its last line.
            let left_line = left_lines
                .get(index)
                .cloned()
                .unwrap_or_else(|| blank_split_filler(side_width));
            let right_line = right_lines
                .get(index)
                .cloned()
                .unwrap_or_else(|| blank_split_filler(side_width));
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
                        continues_previous: left_line.continues_previous,
                    }),
                    right: Some(DisplaySelectionSegment {
                        start_column: side_width + 3 + right_line.start_column,
                        content_width: right_line.content_width,
                        text: right_line.text,
                        continues_previous: right_line.continues_previous,
                    }),
                    ..DisplaySelectionLine::default()
                },
            }
        })
        .collect()
}

pub(super) fn render_split_hunk_rows<'a>(
    rows: &'a [DiffRow],
    row_index_offset: usize,
    side_width: usize,
    line_wrap: DiffLineWrapMode,
    emphasis_for: impl Fn(usize) -> &'a [Range<usize>],
) -> Vec<(
    Line<'static>,
    Option<DisplayNavTarget>,
    DisplayRowRefs,
    DisplaySelectionLine,
)> {
    let mut rendered = Vec::with_capacity(rows.len());
    let mut pending_removed: Vec<(usize, RenderRow<'a>)> = Vec::new();
    let mut pending_added: Vec<(usize, RenderRow<'a>)> = Vec::new();

    let flush_pending = |rendered: &mut Vec<(
        Line<'static>,
        Option<DisplayNavTarget>,
        DisplayRowRefs,
        DisplaySelectionLine,
    )>,
                         removed: &mut Vec<(usize, RenderRow<'a>)>,
                         added: &mut Vec<(usize, RenderRow<'a>)>| {
        let row_count = removed.len().max(added.len());
        for index in 0..row_count {
            let left = removed.get(index).copied();
            let right = added.get(index).copied();
            let target_line = resolve_split_target_line(
                left.map(|(_, render)| render.row),
                right.map(|(_, render)| render.row),
            );
            let row_refs = DisplayRowRefs {
                left: left.map(|(row_index, _)| row_index),
                right: right.map(|(row_index, _)| row_index),
            };
            for rendered_line in render_split_pair_lines(
                left.map(|(_, render)| render),
                right.map(|(_, render)| render),
                side_width,
                line_wrap,
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
        let with_emphasis = || RenderRow {
            row,
            emphasis: emphasis_for(row_index),
        };
        match row.kind {
            DiffLineKind::Removed => pending_removed.push((row_index, with_emphasis())),
            DiffLineKind::Added => pending_added.push((row_index, with_emphasis())),
            DiffLineKind::Context => {
                flush_pending(&mut rendered, &mut pending_removed, &mut pending_added);
                let target_line = resolve_split_target_line(Some(row), Some(row));
                let row_refs = DisplayRowRefs {
                    left: Some(row_index),
                    right: Some(row_index),
                };
                let plain = RenderRow::plain(row);
                for rendered_line in
                    render_split_pair_lines(Some(plain), Some(plain), side_width, line_wrap)
                {
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
                for rendered_line in
                    render_unified_code_lines(RenderRow::plain(row), side_width * 2 + 3, line_wrap)
                {
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

/// Minimum dotted-rule cells kept after the scope label so the band still
/// reads as a divider when the context is long.
const GAP_RULE_MIN_WIDTH: usize = 4;

/// One row of the collapsed-context band between hunks. Each gap renders as a
/// pair of rows: the top row reveals lines below the previous hunk and the
/// bottom row reveals lines above the next hunk. The top row also names the
/// scope the next hunk starts in (git's `@@ … @@ <context>`), truncated so the
/// dotted rule keeps a few cells.
pub(super) fn render_expand_gap_line(
    width: usize,
    remaining: usize,
    direction: GapExpandDirection,
    context: Option<&str>,
) -> Line<'static> {
    let band_style = ui::diff_gap_style();
    let arrow = match direction {
        GapExpandDirection::Up => "↓",
        GapExpandDirection::Down => "↑",
    };
    let gutter_width = format_line_number(None).width();
    let mut spans = vec![
        Span::styled(" ".repeat(gutter_width), band_style),
        Span::styled(arrow.to_string(), ui::diff_gap_action_style()),
        Span::styled(" ", band_style),
    ];
    if let GapExpandDirection::Up = direction {
        spans.push(Span::styled(
            format!(
                "{remaining} unchanged line{} ",
                if remaining == 1 { "" } else { "s" }
            ),
            band_style,
        ));
        if let Some(context) = context {
            let separator = "· ";
            // Reserve the space after the label, the rule, and the band's
            // final blank column.
            let budget = width
                .saturating_sub(spans_width(&spans) + separator.width() + 2 + GAP_RULE_MIN_WIDTH);
            if budget > 1 {
                spans.push(Span::styled(separator, ui::diff_gap_rule_style()));
                spans.push(Span::styled(
                    format!("{} ", truncate_with_ellipsis(context, budget)),
                    ui::diff_gap_context_style(),
                ));
            }
        }
        let used = spans_width(&spans);
        spans.push(Span::styled(
            "┄".repeat(width.saturating_sub(used + 1)),
            ui::diff_gap_rule_style(),
        ));
    }
    let used = spans_width(&spans);
    spans.push(Span::styled(
        " ".repeat(width.saturating_sub(used)),
        band_style,
    ));
    spans = fit_spans_to_width(spans, width.max(1), band_style);
    Line::from(spans).style(band_style)
}

fn truncate_with_ellipsis(content: &str, max_width: usize) -> String {
    if content.width() <= max_width {
        return content.to_string();
    }
    format!(
        "{}…",
        truncate_to_width(content, max_width.saturating_sub(1))
    )
}

pub(super) fn render_expanded_context_lines(
    line_number: usize,
    text: &str,
    highlighted_content: Option<Vec<SyntaxToken>>,
    width: usize,
    split: bool,
    line_wrap: DiffLineWrapMode,
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
    let row = RenderRow::plain(&row);
    if split {
        let total_width = width.saturating_sub(1);
        let gutter_width = 3;
        let side_width = total_width.saturating_sub(gutter_width) / 2;
        render_split_pair_lines(Some(row), Some(row), side_width, line_wrap)
    } else {
        render_unified_code_lines(row, width, line_wrap)
    }
}

#[derive(Debug, Clone)]
struct WrappedSideLine {
    spans: Vec<Span<'static>>,
    start_column: usize,
    content_width: usize,
    text: String,
    continues_previous: bool,
}

fn render_split_side_lines(
    input: Option<RenderRow<'_>>,
    left_side: bool,
    width: usize,
    line_wrap: DiffLineWrapMode,
) -> Vec<WrappedSideLine> {
    let Some(RenderRow { row, emphasis }) = input else {
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
    let gutter_width = format_line_number(None).width();
    let continuation_prefix = vec![
        Span::styled(
            " ".repeat(gutter_width.saturating_sub(2)),
            base_style.patch(ui::line_number_style()),
        ),
        Span::styled(
            format!("{WRAP_MARKER} "),
            base_style.patch(ui::diff_wrap_marker_style()),
        ),
    ];
    let content = render_row_content(row.side_content(left_side), row, emphasis, base_style);
    let prefix_width = spans_width(&prefix);
    prefixed_spans_to_lines(
        prefix,
        continuation_prefix,
        content,
        width + 1,
        base_style,
        line_wrap,
    )
    .into_iter()
    .map(|wrapped| WrappedSideLine {
        spans: wrapped.spans,
        start_column: prefix_width,
        content_width: wrapped.content_width,
        text: wrapped.text,
        continues_previous: wrapped.continues_previous,
    })
    .collect()
}

fn blank_split_side(width: usize) -> WrappedSideLine {
    WrappedSideLine {
        spans: vec![Span::styled(" ".repeat(width), ui::diff_context_style())],
        start_column: 0,
        content_width: width,
        text: String::new(),
        continues_previous: false,
    }
}

fn blank_split_filler(width: usize) -> WrappedSideLine {
    WrappedSideLine {
        continues_previous: true,
        ..blank_split_side(width)
    }
}

fn render_row_content(
    syntax_tokens: Option<&[SyntaxToken]>,
    row: &DiffRow,
    emphasis: &[Range<usize>],
    fallback: Style,
) -> Vec<Span<'static>> {
    let text = row.text.as_str();
    let has_tabs = text.as_bytes().contains(&b'\t');

    let token_style = |token: &SyntaxToken| {
        token
            .highlight_name
            .map(|name| ui::syntax_style(name, fallback))
            .unwrap_or(fallback)
    };
    let emphasis_style = match row.kind {
        DiffLineKind::Added if !emphasis.is_empty() => Some(ui::diff_added_emphasis_style()),
        DiffLineKind::Removed if !emphasis.is_empty() => Some(ui::diff_removed_emphasis_style()),
        _ => None,
    };

    let raw_spans = match (syntax_tokens, emphasis_style) {
        (Some(tokens), None) if !tokens.is_empty() => tokens
            .iter()
            .map(|token| {
                let content = text
                    .get(token.start..token.end)
                    .map(str::to_string)
                    .unwrap_or_default();
                Span::styled(content, token_style(token))
            })
            .collect(),
        (Some(tokens), Some(emphasis_style)) if !tokens.is_empty() => {
            let segments: Vec<(Range<usize>, Style)> = tokens
                .iter()
                .filter(|token| text.get(token.start..token.end).is_some())
                .map(|token| (token.start..token.end, token_style(token)))
                .collect();
            emphasized_spans(text, &segments, emphasis, emphasis_style)
        }
        (_, Some(emphasis_style)) => {
            emphasized_spans(text, &[(0..text.len(), fallback)], emphasis, emphasis_style)
        }
        _ => vec![Span::styled(text.to_string(), fallback)],
    };

    if has_tabs {
        expand_tabs_in_spans(raw_spans)
    } else {
        raw_spans
    }
}

/// Splits styled segments at emphasis boundaries and patches the emphasis
/// background onto the changed parts, keeping each segment's syntax color.
fn emphasized_spans(
    text: &str,
    segments: &[(Range<usize>, Style)],
    emphasis: &[Range<usize>],
    emphasis_style: Style,
) -> Vec<Span<'static>> {
    let mut spans = Vec::with_capacity(segments.len() + emphasis.len() * 2);
    for (range, style) in segments {
        let mut cursor = range.start;
        for emphasized in emphasis {
            let start = emphasized.start.max(cursor);
            let end = emphasized.end.min(range.end);
            if start >= end {
                continue;
            }
            if cursor < start
                && let Some(plain) = text.get(cursor..start)
            {
                spans.push(Span::styled(plain.to_string(), *style));
            }
            if let Some(changed) = text.get(start..end) {
                spans.push(Span::styled(
                    changed.to_string(),
                    style.patch(emphasis_style),
                ));
            }
            cursor = end;
        }
        if cursor < range.end
            && let Some(plain) = text.get(cursor..range.end)
        {
            spans.push(Span::styled(plain.to_string(), *style));
        }
    }
    spans
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

fn prefixed_spans_to_lines(
    prefix: Vec<Span<'static>>,
    continuation_prefix: Vec<Span<'static>>,
    content: Vec<Span<'static>>,
    width: usize,
    pad_style: Style,
    line_wrap: DiffLineWrapMode,
) -> Vec<WrappedLineContent> {
    if line_wrap.is_wrapped() {
        return wrap_prefixed_spans_to_lines(
            prefix,
            continuation_prefix,
            content,
            width,
            pad_style,
        );
    }

    vec![truncate_prefixed_spans_to_line(
        prefix, content, width, pad_style,
    )]
}

fn truncate_prefixed_spans_to_line(
    prefix: Vec<Span<'static>>,
    content: Vec<Span<'static>>,
    width: usize,
    pad_style: Style,
) -> WrappedLineContent {
    let target_width = width.saturating_sub(1);
    if target_width == 0 {
        return WrappedLineContent {
            spans: Vec::new(),
            text: String::new(),
            content_width: 0,
            continues_previous: false,
        };
    }

    let prefix_width = spans_width(&prefix);
    if prefix_width >= target_width {
        return WrappedLineContent {
            spans: fit_spans_to_width(prefix, target_width, pad_style),
            text: String::new(),
            content_width: 0,
            continues_previous: false,
        };
    }

    let content_width = target_width.saturating_sub(prefix_width);
    let content = fit_spans_to_width(content, content_width, pad_style);
    let text = line_text(&content);
    let mut spans = prefix;
    spans.extend(content);

    WrappedLineContent {
        spans,
        text,
        content_width,
        continues_previous: false,
    }
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
                continues_previous: false,
            })
            .collect();
    }

    // Continuation rows share the first row's content width, since both
    // prefixes are gutter-wide.
    let row_width = content_width.min(continuation_content_width);
    let rows = wrap_spans_at_words(content, row_width);
    let last_index = rows.len().saturating_sub(1);
    rows.into_iter()
        .enumerate()
        .map(|(index, row)| {
            let mut spans = if index == 0 {
                prefix.clone()
            } else {
                continuation_prefix.clone()
            };
            spans.extend(row.spans);
            if row.width < row_width {
                spans.push(Span::styled(" ".repeat(row_width - row.width), pad_style));
            }
            // Only the final row drops trailing spaces; earlier rows keep them
            // so copying a wrapped line reproduces the original text.
            let mut text = row.text;
            if index == last_index {
                let trimmed_len = text.trim_end_matches(' ').len();
                text.truncate(trimmed_len);
            }
            WrappedLineContent {
                text,
                content_width: if index == 0 {
                    content_width
                } else {
                    continuation_content_width
                },
                spans,
                continues_previous: index > 0,
            }
        })
        .collect()
}

/// Gutter glyph marking the second and later rows of a soft-wrapped line.
const WRAP_MARKER: &str = "↪";

struct WrappedRow {
    spans: Vec<Span<'static>>,
    /// The row's source text, without padding.
    text: String,
    /// Display width of `spans`.
    width: usize,
}

/// Soft-wraps styled content into rows at most `width` cells wide, breaking
/// after whitespace or punctuation when possible so identifiers stay whole.
fn wrap_spans_at_words(spans: Vec<Span<'static>>, width: usize) -> Vec<WrappedRow> {
    let mut text = String::with_capacity(spans.iter().map(|span| span.content.len()).sum());
    let mut total_width = 0usize;
    for span in &spans {
        text.push_str(&span.content);
        total_width += UnicodeWidthStr::width(span.content.as_ref());
    }
    // Most diff lines fit; keep their spans untouched.
    if total_width <= width {
        return vec![WrappedRow {
            spans,
            text,
            width: total_width,
        }];
    }

    let row_ends = word_wrap_row_ends(&text, width);
    let mut rows = Vec::with_capacity(row_ends.len());
    let mut row_start = 0usize;
    for &(row_end, row_width) in &row_ends {
        rows.push(WrappedRow {
            spans: Vec::new(),
            text: text[row_start..row_end].to_string(),
            width: row_width,
        });
        row_start = row_end;
    }
    let row_ends = row_ends
        .into_iter()
        .map(|(row_end, _)| row_end)
        .collect::<Vec<_>>();

    // Cut each span at the row boundaries that fall inside it.
    let mut row_index = 0usize;
    let mut span_start = 0usize;
    for span in spans {
        let span_end = span_start + span.content.len();
        while row_ends[row_index] <= span_start && row_index + 1 < row_ends.len() {
            row_index += 1;
        }
        // Spans that sit inside one row move over without copying.
        if span_end <= row_ends[row_index] {
            rows[row_index].spans.push(span);
            span_start = span_end;
            continue;
        }

        let content = span.content.as_ref();
        let mut cursor = span_start;
        while cursor < span_end {
            while row_ends[row_index] <= cursor && row_index + 1 < row_ends.len() {
                row_index += 1;
            }
            let piece_end = row_ends[row_index].min(span_end);
            rows[row_index].spans.push(Span::styled(
                content[cursor - span_start..piece_end - span_start].to_string(),
                span.style,
            ));
            cursor = piece_end;
        }
        span_start = span_end;
    }

    rows
}

/// Byte offset where each soft-wrapped row of `text` ends, with the row's
/// display width; the last entry ends at `text.len()`. Rows break after the
/// last whitespace or punctuation that fits, like an editor's soft wrap, and
/// hard-break only a run with no break point. A break never leaves a row of
/// nothing but indentation.
fn word_wrap_row_ends(text: &str, width: usize) -> Vec<(usize, usize)> {
    let width = width.max(1);
    let mut row_ends = Vec::new();
    let mut row_width = 0usize;
    // Width of the whitespace the current row starts with; a break inside it
    // would produce a blank row.
    let mut leading_blank = 0usize;
    let mut row_has_text = false;
    // Byte offset and row width just after the most recent break opportunity.
    let mut break_after: Option<(usize, usize)> = None;

    for (index, ch) in text.char_indices() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        while ch_width > 0 && row_width > 0 && row_width + ch_width > width {
            let (cut_end, cut_width) = break_after
                .filter(|&(_, cut_width)| cut_width > leading_blank)
                .unwrap_or((index, row_width));
            row_ends.push((cut_end, cut_width));
            row_width -= cut_width;
            // Anything carried to the next row follows the last break point,
            // so it holds no whitespace.
            leading_blank = 0;
            row_has_text = row_width > 0;
            break_after = None;
        }
        row_width += ch_width;
        if !row_has_text {
            if ch.is_whitespace() {
                leading_blank += ch_width;
            } else {
                row_has_text = ch_width > 0;
            }
        }
        if is_word_break(ch) {
            break_after = Some((index + ch.len_utf8(), row_width));
        }
    }
    row_ends.push((text.len(), row_width));
    row_ends
}

/// Characters a soft wrap may follow: whitespace and punctuation, but not `_`,
/// which is part of identifiers.
fn is_word_break(ch: char) -> bool {
    ch.is_whitespace() || (ch.is_ascii_punctuation() && ch != '_')
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
