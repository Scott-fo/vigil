use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use unicode_width::UnicodeWidthStr;

use super::super::{chip_color, primary_color, text_color, text_faint_color, text_subtle_color};

/// A single-row text field: optional label, `›` marker, then the query or a
/// placeholder. Drawn on a chip-tinted row instead of a bordered box.
pub(super) struct Prompt<'a> {
    pub(super) query: &'a str,
    pub(super) placeholder: &'a str,
    pub(super) active: bool,
    /// Left-aligned label such as `Source`; padded to `label_width`.
    pub(super) label: Option<(&'a str, usize)>,
}

impl<'a> Prompt<'a> {
    pub(super) fn new(query: &'a str, placeholder: &'a str) -> Self {
        Self {
            query,
            placeholder,
            active: true,
            label: None,
        }
    }

    pub(super) fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    pub(super) fn label(mut self, label: &'a str, width: usize) -> Self {
        self.label = Some((label, width));
        self
    }
}

pub(super) fn render_prompt(frame: &mut Frame, area: Rect, prompt: Prompt<'_>) {
    let mut spans = vec![Span::raw(" ")];
    if let Some((label, width)) = prompt.label {
        let label_style = if prompt.active {
            Style::new().fg(text_color()).add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(text_faint_color())
        };
        let padding = width.saturating_sub(label.width());
        spans.push(Span::styled(
            format!("{label}{} ", " ".repeat(padding)),
            label_style,
        ));
    }

    let marker_style = if prompt.active {
        Style::new()
            .fg(primary_color())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(text_faint_color())
    };
    spans.push(Span::styled("› ", marker_style));

    if prompt.query.is_empty() {
        if prompt.active {
            spans.push(caret());
        }
        spans.push(Span::styled(
            prompt.placeholder.to_string(),
            Style::new().fg(if prompt.active {
                text_faint_color()
            } else {
                text_subtle_color()
            }),
        ));
    } else {
        spans.push(Span::styled(
            prompt.query.to_string(),
            Style::new().fg(text_color()),
        ));
        if prompt.active {
            spans.push(caret());
        }
    }

    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::new().bg(chip_color())),
        Rect { height: 1, ..area },
    );
}

fn caret() -> Span<'static> {
    Span::styled("▏", Style::new().fg(primary_color()))
}
