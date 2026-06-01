use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Padding, Paragraph},
};

use crate::app::{App, DiffStatsState};

use super::super::{
    error_color, panel_color, success_color, text_color, text_muted_color, warning_color,
};
use super::frame::render_modal_frame;

pub(super) fn render_diff_stats_modal(frame: &mut Frame, app: &App) {
    let inner = render_modal_frame(frame, 58, 13, "Diff Stats (F2)");
    let width = inner.width.saturating_sub(2) as usize;

    let mut lines = match app.diff_stats_state() {
        DiffStatsState::Ready(stats) => vec![
            stat_line(
                "Files",
                stats.file_count,
                width,
                Style::new().fg(text_color()),
            ),
            separator_line(width),
            stat_line(
                "Additions",
                stats.additions,
                width,
                Style::new()
                    .fg(success_color())
                    .add_modifier(Modifier::BOLD),
            ),
            separator_line(width),
            stat_line(
                "Deletions",
                stats.deletions,
                width,
                Style::new().fg(error_color()).add_modifier(Modifier::BOLD),
            ),
            separator_line(width),
            stat_line(
                "Lines",
                stats.lines,
                width,
                Style::new().fg(text_muted_color()),
            ),
        ],
        DiffStatsState::Loading { file_count } => vec![
            stat_line("Files", file_count, width, Style::new().fg(text_color())),
            separator_line(width),
            Line::from(Span::styled(
                "Diff metrics are loading in the background...",
                Style::new().fg(warning_color()),
            )),
        ],
        DiffStatsState::Unavailable { file_count } => vec![
            stat_line("Files", file_count, width, Style::new().fg(text_color())),
            separator_line(width),
            Line::from(Span::styled(
                "No parsed diff metrics are available yet.",
                Style::new().fg(text_muted_color()),
            )),
        ],
    };
    lines.push(muted_line("Esc, Enter, q, or F2 closes."));

    let paragraph = Paragraph::new(Text::from(lines))
        .style(Style::new().bg(panel_color()))
        .block(Block::new().padding(Padding::horizontal(1)));
    frame.render_widget(paragraph, inner);
}

fn stat_line(label: &'static str, value: usize, width: usize, value_style: Style) -> Line<'static> {
    let value = format_count(value);
    let spacing = width
        .saturating_sub(label.len())
        .saturating_sub(value.len())
        .max(1);
    Line::from(vec![
        Span::styled(label, Style::new().fg(text_muted_color())),
        Span::raw(" ".repeat(spacing)),
        Span::styled(value, value_style),
    ])
}

fn separator_line(width: usize) -> Line<'static> {
    Line::from(Span::styled(
        "-".repeat(width.max(1)),
        Style::new().fg(text_muted_color()),
    ))
}

fn muted_line(text: &'static str) -> Line<'static> {
    Line::from(Span::styled(text, Style::new().fg(text_muted_color())))
}

fn format_count(value: usize) -> String {
    let digits = value.to_string();
    let mut formatted = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            formatted.push(',');
        }
        formatted.push(ch);
    }
    formatted.chars().rev().collect()
}
