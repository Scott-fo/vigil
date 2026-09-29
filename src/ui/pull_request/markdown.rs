use std::ops::Range;

use ratatui::{
    style::Style,
    text::{Line, Span},
};
use unicode_width::UnicodeWidthChar;

use crate::git;

use super::super::{style::syntax_style, text_color};

/// Markdown text as highlighted lines, word-wrapped to `width` columns and
/// indented by `indent`. Highlighting is line by line, like markdown diffs.
pub(in crate::ui) fn markdown_lines(text: &str, width: usize, indent: &str) -> Vec<Line<'static>> {
    let base = Style::new().fg(text_color());
    let indent_width = indent.chars().filter_map(|ch| ch.width()).sum::<usize>();
    let text_width = width.saturating_sub(indent_width).max(8);
    let mut lines = Vec::new();
    for source in text.trim_end().lines() {
        let source = source.trim_end_matches('\r');
        let tokens = git::highlight_markdown_line(source);
        for row in wrap_ranges(source, text_width) {
            let mut spans = vec![Span::raw(indent.to_string())];
            spans.extend(styled_row(source, row, &tokens, base));
            lines.push(Line::from(spans));
        }
    }
    lines
}

fn styled_row(
    source: &str,
    row: Range<usize>,
    tokens: &[git::SyntaxToken],
    base: Style,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut position = row.start;
    for token in tokens {
        let start = token.start.max(row.start);
        let end = token.end.min(row.end);
        if start >= end {
            continue;
        }
        if start > position {
            spans.push(Span::styled(source[position..start].to_string(), base));
        }
        let style = token
            .highlight_name
            .map_or(base, |name| syntax_style(name, base));
        spans.push(Span::styled(source[start..end].to_string(), style));
        position = end;
    }
    if position < row.end {
        spans.push(Span::styled(source[position..row.end].to_string(), base));
    }
    spans
}

/// An open wrapped row: byte range and display width so far.
struct Row {
    start: usize,
    end: usize,
    width: usize,
    has_word: bool,
}

/// Byte ranges of `text` per wrapped row: greedy word wrap that keeps leading
/// indentation on the first row and splits words wider than a row. An empty
/// line is one empty row.
fn wrap_ranges(text: &str, width: usize) -> Vec<Range<usize>> {
    let width = width.max(1);
    let mut rows = Vec::new();
    let leading = text.len() - text.trim_start().len();
    let leading_width = text_width(&text[..leading]);
    let mut row = (leading > 0 && leading_width < width).then_some(Row {
        start: 0,
        end: leading,
        width: leading_width,
        has_word: false,
    });

    for (start, word) in words(text) {
        let end = start + word.len();
        let word_width = text_width(word);
        if let Some(open) = row.as_mut() {
            let gap = text_width(&text[open.end..start]);
            if open.width + gap + word_width <= width {
                open.end = end;
                open.width += gap + word_width;
                open.has_word = true;
                continue;
            }
            // The word starts a new row; an indentation-only row is dropped.
            if open.has_word {
                rows.push(open.start..open.end);
            }
        }

        let mut chunk_start = start;
        let mut chunk_width = 0;
        for (offset, ch) in word.char_indices() {
            let ch_width = ch.width().unwrap_or(0);
            if chunk_width > 0 && chunk_width + ch_width > width {
                rows.push(chunk_start..start + offset);
                chunk_start = start + offset;
                chunk_width = 0;
            }
            chunk_width += ch_width;
        }
        row = Some(Row {
            start: chunk_start,
            end,
            width: chunk_width,
            has_word: true,
        });
    }
    if let Some(open) = row {
        rows.push(open.start..open.end);
    }
    if rows.is_empty() {
        rows.push(0..0);
    }
    rows
}

fn text_width(text: &str) -> usize {
    text.chars().filter_map(|ch| ch.width()).sum()
}

/// `(byte offset, word)` for each run of non-whitespace.
fn words(text: &str) -> impl Iterator<Item = (usize, &str)> {
    text.split_whitespace().map(move |word| {
        let offset = word.as_ptr() as usize - text.as_ptr() as usize;
        (offset, word)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(text: &str, width: usize) -> Vec<&str> {
        wrap_ranges(text, width)
            .into_iter()
            .map(|range| &text[range])
            .collect()
    }

    #[test]
    fn wraps_words_and_keeps_indentation() {
        assert_eq!(rows("one two three", 7), vec!["one two", "three"]);
        assert_eq!(rows("  - item text", 8), vec!["  - item", "text"]);
        assert_eq!(rows("", 10), vec![""]);
        assert_eq!(rows("abcdefgh ij", 3), vec!["abc", "def", "gh", "ij"]);
    }

    #[test]
    fn markdown_rows_keep_every_character() {
        let text = "## Summary of **bold** changes";
        let joined: String = markdown_lines(text, 12, "")
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(joined, text);
    }
}
