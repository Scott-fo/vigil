use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Padding, Paragraph},
};

use crate::app::{App, DiffStatsState};
use crate::git::{DiffLineTotals, ReviewDiffStats};

use super::super::{
    error_color, panel_color, success_color, text_color, text_muted_color, warning_color,
};
use super::frame::render_modal_frame;

pub(super) fn render_diff_stats_modal(frame: &mut Frame, app: &App) {
    let inner = render_modal_frame(frame, 58, 16, "Diff Stats (F2)");
    let width = inner.width.saturating_sub(2) as usize;

    let mut lines = match app.diff_stats_state() {
        DiffStatsState::Ready(stats) if stats.has_working_tree_scopes() => {
            working_tree_stat_lines(stats, width)
        }
        DiffStatsState::Ready(stats) => combined_stat_lines(stats, width),
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

fn working_tree_stat_lines(stats: ReviewDiffStats, width: usize) -> Vec<Line<'static>> {
    let tracked = stats.tracked.unwrap_or_default();
    let untracked = stats.untracked.unwrap_or_default();
    vec![
        scope_header_line(width),
        scope_row("Tracked", tracked, width),
        scope_row("Untracked", untracked, width),
        scope_row("Total", stats.totals(), width),
        separator_line(width),
        muted_line("Untracked files count as added lines."),
        muted_line(""),
    ]
}

fn combined_stat_lines(stats: ReviewDiffStats, width: usize) -> Vec<Line<'static>> {
    vec![
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
        muted_line(""),
    ]
}

fn scope_header_line(width: usize) -> Line<'static> {
    let files = pad_left("files", 8);
    let additions = pad_left("+", 8);
    let deletions = pad_left("-", 8);
    let label_width = width
        .saturating_sub(files.len())
        .saturating_sub(additions.len())
        .saturating_sub(deletions.len())
        .max(1);
    Line::from(vec![
        Span::styled(
            format!("{:label_width$}", ""),
            Style::new().fg(text_muted_color()),
        ),
        Span::styled(files, Style::new().fg(text_muted_color())),
        Span::styled(additions, Style::new().fg(text_muted_color())),
        Span::styled(deletions, Style::new().fg(text_muted_color())),
    ])
}

fn scope_row(label: &'static str, scope: DiffLineTotals, width: usize) -> Line<'static> {
    let files = pad_left(&format_count(scope.file_count), 8);
    let additions = pad_left(&format!("+{}", format_count(scope.additions)), 8);
    let deletions = pad_left(&format!("-{}", format_count(scope.deletions)), 8);
    let label_width = width
        .saturating_sub(files.len())
        .saturating_sub(additions.len())
        .saturating_sub(deletions.len())
        .max(label.len() + 1);
    Line::from(vec![
        Span::styled(
            format!("{label:<label_width$}"),
            Style::new().fg(text_muted_color()),
        ),
        Span::styled(files, Style::new().fg(text_color())),
        Span::styled(
            additions,
            Style::new()
                .fg(success_color())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            deletions,
            Style::new().fg(error_color()).add_modifier(Modifier::BOLD),
        ),
    ])
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

fn pad_left(value: &str, width: usize) -> String {
    format!("{value:>width$}")
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
