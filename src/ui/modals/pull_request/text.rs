use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span, Text},
    widgets::Paragraph,
};
use unicode_width::UnicodeWidthChar;

use crate::{
    app::TextArea,
    ui::{chip_color, primary_color, text_color, text_faint_color},
};

const CARET: &str = "▏";

/// A multi-line text field on a chip-tinted block: lines wrap at the field's
/// width, a caret marks the cursor when `active`, and the view scrolls to
/// keep the cursor visible.
pub(super) fn render_text_area(
    frame: &mut Frame,
    area: Rect,
    text: &TextArea,
    placeholder: &str,
    active: bool,
) {
    if area.width < 3 || area.height == 0 {
        return;
    }
    let width = area.width as usize - 2;
    let text_style = Style::new().fg(text_color());
    let caret = Span::styled(CARET, Style::new().fg(primary_color()));

    let mut rows: Vec<Line<'static>> = Vec::new();
    let mut cursor_row = 0;
    if text.lines().len() == 1 && text.lines()[0].is_empty() {
        let mut spans = vec![Span::raw(" ")];
        if active {
            spans.push(caret.clone());
        }
        spans.push(Span::styled(
            placeholder.to_string(),
            Style::new().fg(text_faint_color()),
        ));
        rows.push(Line::from(spans));
    } else {
        let (cursor_line, cursor_column) = text.cursor();
        for (line_index, line) in text.lines().iter().enumerate() {
            let chunks = wrap_chars(line, width);
            let mut consumed = 0;
            for (chunk_index, chunk) in chunks.iter().enumerate() {
                let chunk_len = chunk.chars().count();
                let last = chunk_index + 1 == chunks.len();
                let cursor_here = active
                    && line_index == cursor_line
                    && cursor_column >= consumed
                    && (cursor_column < consumed + chunk_len || last);
                let mut spans = vec![Span::raw(" ")];
                if cursor_here {
                    let split = cursor_column - consumed;
                    let (before, after): (String, String) = (
                        chunk.chars().take(split).collect(),
                        chunk.chars().skip(split).collect(),
                    );
                    spans.push(Span::styled(before, text_style));
                    spans.push(caret.clone());
                    spans.push(Span::styled(after, text_style));
                    cursor_row = rows.len();
                } else {
                    spans.push(Span::styled(chunk.clone(), text_style));
                }
                rows.push(Line::from(spans));
                consumed += chunk_len;
            }
        }
    }

    let height = area.height as usize;
    let scroll = (cursor_row + 1).saturating_sub(height);
    let visible = rows
        .into_iter()
        .skip(scroll)
        .take(height)
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(Text::from(visible)).style(Style::new().bg(chip_color())),
        area,
    );
}

/// Splits `line` into pieces at most `width` columns wide, keeping at least
/// one (possibly empty) piece.
fn wrap_chars(line: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut chunks = vec![String::new()];
    let mut used = 0;
    for ch in line.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + ch_width > width && used > 0 {
            chunks.push(String::new());
            used = 0;
        }
        chunks.last_mut().expect("never empty").push(ch);
        used += ch_width;
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::wrap_chars;

    #[test]
    fn long_lines_wrap_by_width() {
        assert_eq!(wrap_chars("abcdef", 4), vec!["abcd", "ef"]);
        assert_eq!(wrap_chars("", 4), vec![""]);
    }
}
