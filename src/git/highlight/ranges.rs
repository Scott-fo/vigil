use std::{cmp::Reverse, collections::BinaryHeap};

use streaming_iterator::StreamingIterator;
use tree_sitter::{Query, QueryCursor};

use super::{SyntaxToken, push_syntax_token};

#[derive(Clone, Copy)]
struct QueryHighlightRange {
    start: usize,
    end: usize,
    highlight_name: Option<&'static str>,
    specificity: u8,
}

#[inline]
pub(super) fn query_captures_to_lines(
    query_cursor: &mut QueryCursor,
    query: &Query,
    capture_highlight_names: &[Option<&'static str>],
    root_node: tree_sitter::Node<'_>,
    source: &str,
) -> Option<Vec<Vec<SyntaxToken>>> {
    let mut ranges = Vec::new();
    let mut captures = query_cursor.captures(query, root_node, source.as_bytes());
    while {
        captures.advance();
        captures.get().is_some()
    } {
        let Some((query_match, capture_index)) = captures.get() else {
            continue;
        };
        let Some(query_capture) = query_match.captures.get(*capture_index) else {
            continue;
        };
        let start = query_capture.node.start_byte();
        let end = query_capture.node.end_byte();
        if start >= end || end > source.len() {
            continue;
        }
        let highlight_name = capture_highlight_names
            .get(query_capture.index as usize)
            .copied()
            .flatten();
        let specificity = highlight_name
            .map(|name| name.split('.').count() as u8)
            .unwrap_or(0);
        ranges.push(QueryHighlightRange {
            start,
            end,
            highlight_name,
            specificity,
        });
    }

    if ranges.is_empty() {
        return Some(
            source
                .split('\n')
                .map(|line| {
                    vec![SyntaxToken {
                        start: 0,
                        end: line.len(),
                        highlight_name: None,
                    }]
                })
                .collect(),
        );
    }

    let mut lines = Vec::new();
    let mut current_line = Vec::new();
    let mut active_ranges = Vec::new();
    let mut active_endings = BinaryHeap::new();
    let mut current_offset = 0usize;
    let mut current_line_start = 0usize;
    let mut next_range_index = 0usize;

    while next_range_index < ranges.len() || !active_endings.is_empty() {
        let next_start = ranges
            .get(next_range_index)
            .map(|range| range.start)
            .unwrap_or(usize::MAX);
        let next_end = active_endings
            .peek()
            .map(|ending: &Reverse<(usize, usize)>| ending.0.0)
            .unwrap_or(usize::MAX);
        let next_offset = next_start.min(next_end);

        if current_offset < next_offset {
            let highlight_name = select_active_highlight_name(&active_ranges, &ranges);
            push_highlighted_source_segment(
                &mut lines,
                &mut current_line,
                source,
                current_offset,
                next_offset,
                &mut current_line_start,
                highlight_name,
            );
        }

        if next_end <= next_start {
            while let Some(Reverse((end, range_index))) = active_endings.peek().copied() {
                if end != next_end {
                    break;
                }
                let _ = active_endings.pop();
                if let Some(position) = active_ranges
                    .iter()
                    .position(|active_range_index| *active_range_index == range_index)
                {
                    active_ranges.swap_remove(position);
                }
            }
            current_offset = next_end;
            continue;
        }

        while let Some(range) = ranges.get(next_range_index) {
            if range.start != next_start {
                break;
            }
            active_ranges.push(next_range_index);
            active_endings.push(Reverse((range.end, next_range_index)));
            next_range_index += 1;
        }
        current_offset = next_start;
    }

    if current_offset < source.len() {
        let highlight_name = select_active_highlight_name(&active_ranges, &ranges);
        push_highlighted_source_segment(
            &mut lines,
            &mut current_line,
            source,
            current_offset,
            source.len(),
            &mut current_line_start,
            highlight_name,
        );
    }

    lines.push(current_line);
    Some(lines)
}

#[inline]
fn select_active_highlight_name(
    active_ranges: &[usize],
    ranges: &[QueryHighlightRange],
) -> Option<&'static str> {
    active_ranges
        .iter()
        .copied()
        .max_by_key(|range_index| {
            let range = ranges[*range_index];
            (range.specificity, *range_index)
        })
        .and_then(|range_index| ranges[range_index].highlight_name)
}

#[inline]
fn push_highlighted_source_segment(
    lines: &mut Vec<Vec<SyntaxToken>>,
    current_line: &mut Vec<SyntaxToken>,
    source: &str,
    mut start: usize,
    end: usize,
    current_line_start: &mut usize,
    highlight_name: Option<&'static str>,
) {
    while start < end {
        let segment = &source[start..end];
        if let Some(newline_offset) = segment.find('\n') {
            let line_end = start + newline_offset;
            push_syntax_token(
                current_line,
                start.saturating_sub(*current_line_start),
                line_end.saturating_sub(*current_line_start),
                highlight_name,
            );
            lines.push(std::mem::take(current_line));
            start = line_end + 1;
            *current_line_start = start;
        } else {
            push_syntax_token(
                current_line,
                start.saturating_sub(*current_line_start),
                end.saturating_sub(*current_line_start),
                highlight_name,
            );
            break;
        }
    }
}
