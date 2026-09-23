use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use unicode_width::UnicodeWidthStr;

use super::super::{text_color, text_faint_color, text_subtle_color};

/// A key and what it does, e.g. `("⏎", "select")`.
pub(super) type KeyHint = (&'static str, &'static str);

/// One-row modal footer: key hints on the left and optional status text
/// (current selection, preview target) on the right when it fits.
pub(super) fn render_hint_footer(
    frame: &mut Frame,
    area: Rect,
    hints: &[KeyHint],
    status: Option<String>,
) {
    let area = Rect { height: 1, ..area };
    let left = Line::from(hint_spans(hints));
    let left_width = left.width();
    frame.render_widget(Paragraph::new(left), area);

    if let Some(status) = status {
        let room = (area.width as usize).saturating_sub(left_width + 4);
        if room >= 8 {
            let status = truncate_start(&status, room.saturating_sub(1));
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    format!("{status} "),
                    Style::new().fg(text_subtle_color()),
                )))
                .alignment(Alignment::Right),
                area,
            );
        }
    }
}

pub(super) fn hint_spans(hints: &[KeyHint]) -> Vec<Span<'static>> {
    let mut spans = vec![Span::raw(" ")];
    for (index, (key, label)) in hints.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled("   ", Style::new()));
        }
        spans.push(Span::styled(
            *key,
            Style::new().fg(text_color()).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            format!(" {label}"),
            Style::new().fg(text_faint_color()),
        ));
    }
    spans
}

/// Keeps the end of long paths visible: `…/app/diff/stats.rs`.
fn truncate_start(value: &str, max_width: usize) -> String {
    if value.width() <= max_width {
        return value.to_string();
    }
    let mut kept = String::new();
    let mut width = 1;
    for ch in value.chars().rev() {
        let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + ch_width > max_width {
            break;
        }
        width += ch_width;
        kept.insert(0, ch);
    }
    format!("…{kept}")
}

#[cfg(test)]
mod tests {
    use super::truncate_start;

    #[test]
    fn truncate_start_keeps_the_file_name() {
        assert_eq!(truncate_start("src/app/diff/stats.rs", 12), "…ff/stats.rs");
        assert_eq!(truncate_start("short.rs", 12), "short.rs");
    }
}
