use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Padding, Paragraph},
};

use crate::app::{App, DiffStatsState, RemoteSyncDirection, ReviewMode, SnackbarVariant};
use crate::git::{DiffLineTotals, ReviewDiffStats};

use super::layout::top_right_rect;
use super::{
    NOTICE_WIDTH, error_color, panel_color, primary_color, success_color, text_color,
    text_muted_color,
};

pub(super) fn render_status_line(frame: &mut Frame, app: &App, area: Rect) {
    let line = if app.shows_review_summary_status() {
        review_summary_line(app)
    } else {
        Line::from(Span::styled(
            app.status_message.clone().unwrap_or_default(),
            Style::new().fg(text_muted_color()),
        ))
    };
    let paragraph = Paragraph::new(line)
        .style(Style::new().bg(panel_color()))
        .block(Block::new().padding(Padding::horizontal(1)));
    frame.render_widget(paragraph, area);
}

pub(super) fn sidebar_change_summary(app: &App) -> String {
    match app.diff_stats_state() {
        DiffStatsState::Ready(stats) => format!(
            "{}  +{} -{}",
            app.files.len(),
            stats.additions,
            stats.deletions
        ),
        _ => app.files.len().to_string(),
    }
}

fn review_summary_line(app: &App) -> Line<'static> {
    match app.diff_stats_state() {
        DiffStatsState::Ready(stats) => ready_review_summary_line(app, stats),
        _ => Line::from(Span::styled(
            app.default_status_message(),
            Style::new().fg(text_muted_color()),
        )),
    }
}

fn ready_review_summary_line(app: &App, stats: ReviewDiffStats) -> Line<'static> {
    let mut spans = match &app.review_mode {
        ReviewMode::WorkingTree => working_tree_summary_spans(stats),
        ReviewMode::CommitCompare(selection) => {
            let mut spans = vec![Span::styled(
                format!("commit {}  ", selection.short_hash),
                Style::new().fg(text_muted_color()),
            )];
            spans.extend(file_and_line_spans(stats.file_count, stats));
            spans
        }
        ReviewMode::BranchCompare(selection) => {
            let mut spans = vec![Span::styled(
                format!(
                    "{} -> {}  ",
                    selection.source_ref, selection.destination_ref
                ),
                Style::new().fg(text_muted_color()),
            )];
            spans.extend(file_and_line_spans(stats.file_count, stats));
            spans
        }
    };

    if spans.is_empty() {
        spans.push(Span::styled(
            app.default_status_message(),
            Style::new().fg(text_muted_color()),
        ));
    }

    Line::from(spans)
}

fn working_tree_summary_spans(stats: ReviewDiffStats) -> Vec<Span<'static>> {
    match (stats.tracked, stats.untracked) {
        (Some(tracked), Some(untracked)) if tracked.file_count > 0 && untracked.file_count > 0 => {
            let mut spans = scope_spans("tracked", tracked, true);
            spans.push(Span::styled("  ·  ", Style::new().fg(text_muted_color())));
            spans.extend(scope_spans("untracked", untracked, false));
            spans
        }
        (Some(_), Some(untracked)) if untracked.file_count > 0 => {
            scope_spans("untracked", untracked, false)
        }
        _ => file_and_line_spans(stats.file_count, stats),
    }
}

fn scope_spans(
    label: &'static str,
    scope: DiffLineTotals,
    include_deletions: bool,
) -> Vec<Span<'static>> {
    let noun = match (label, scope.file_count) {
        ("untracked", 1) => "untracked file",
        ("untracked", _) => "untracked files",
        (_, 1) => "tracked file",
        (_, _) => "tracked files",
    };
    let mut spans = vec![Span::styled(
        format!("{} {noun} ", scope.file_count),
        Style::new().fg(text_muted_color()),
    )];
    spans.extend(line_change_spans(
        scope.additions,
        scope.deletions,
        include_deletions || scope.deletions > 0,
    ));
    spans
}

fn file_and_line_spans(file_count: usize, stats: ReviewDiffStats) -> Vec<Span<'static>> {
    let mut spans = vec![Span::styled(
        format!(
            "{} file{}  ",
            file_count,
            if file_count == 1 { "" } else { "s" }
        ),
        Style::new().fg(text_muted_color()),
    )];
    spans.extend(line_change_spans(stats.additions, stats.deletions, true));
    spans
}

fn line_change_spans(
    additions: usize,
    deletions: usize,
    include_deletions: bool,
) -> Vec<Span<'static>> {
    let mut spans = vec![Span::styled(
        format!("+{additions}"),
        Style::new()
            .fg(success_color())
            .add_modifier(Modifier::BOLD),
    )];
    if include_deletions {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            format!("-{deletions}"),
            Style::new().fg(error_color()).add_modifier(Modifier::BOLD),
        ));
    }
    spans
}

pub(super) fn render_notifications(frame: &mut Frame, app: &App) {
    let mut top = frame.area().y + 1;

    if let Some(direction) = app.remote_sync {
        let label = match direction {
            RemoteSyncDirection::Pull => "Pulling from remote...",
            RemoteSyncDirection::Push => "Pushing to remote...",
        };
        let area = top_right_rect(NOTICE_WIDTH, 3, top, frame.area());
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::new().fg(primary_color()))
            .style(Style::new().bg(panel_color()));
        let inner = block.inner(area);
        frame.render_widget(ratatui::widgets::Clear, area);
        frame.render_widget(block, area);
        frame.render_widget(
            Paragraph::new(Text::from(Line::from(Span::styled(
                label,
                Style::new().fg(text_muted_color()),
            ))))
            .style(Style::new().bg(panel_color()))
            .block(Block::new().padding(Padding::horizontal(0))),
            inner,
        );
        top = top.saturating_add(4);
    }

    if let Some(notice) = app.snackbar_notice.as_ref() {
        let area = top_right_rect(NOTICE_WIDTH, 3, top, frame.area());
        let border_color = match notice.variant {
            SnackbarVariant::Info => primary_color(),
            SnackbarVariant::Error => error_color(),
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::new().fg(border_color))
            .style(Style::new().bg(panel_color()));
        let inner = block.inner(area);
        frame.render_widget(ratatui::widgets::Clear, area);
        frame.render_widget(block, area);
        frame.render_widget(
            Paragraph::new(Text::from(Line::from(Span::styled(
                notice.message.clone(),
                Style::new().fg(text_color()),
            ))))
            .style(Style::new().bg(panel_color()))
            .block(Block::new().padding(Padding::horizontal(0))),
            inner,
        );
    }
}
