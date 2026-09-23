use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Padding, Paragraph},
};
use unicode_width::UnicodeWidthStr;

use crate::app::{
    ActivePane, App, DiffStatsState, DiffViewMode, RemoteSyncDirection, ReviewMode, SnackbarVariant,
};
use crate::git::{DiffLineTotals, ReviewDiffStats};

use super::layout::top_right_rect;
use super::{
    NOTICE_WIDTH, chip_color, error_color, panel_color, primary_color, success_color,
    surface_color, text_color, text_faint_color, text_subtle_color,
};

/// Bottom bar. Left: what is being compared and how big the change is, plus
/// any transient status. Right: view mode chips and key hints for the focused
/// pane. Hints are dropped first when the terminal is narrow.
pub(super) fn render_footer(frame: &mut Frame, app: &App, area: Rect) {
    let mut left = vec![Span::raw(" ")];
    left.extend(review_target_spans(app));
    left.push(Span::raw("   "));
    left.extend(change_summary_spans(app));
    if !app.shows_review_summary_status()
        && let Some(message) = app.status_message.as_deref()
    {
        left.push(Span::styled("   ", Style::new()));
        left.push(Span::styled(
            message.to_string(),
            Style::new().fg(text_color()),
        ));
    }

    let mut chips = chip(diff_mode_label(app.diff_view_mode));
    chips.push(Span::raw(" "));
    chips.extend(chip(app.diff_line_wrap_mode.label()));
    if app.sidebar_hidden {
        chips.push(Span::raw(" "));
        chips.extend(chip("sidebar hidden"));
    }

    let budget = (area.width as usize).saturating_sub(spans_width(&left) + spans_width(&chips) + 5);
    let mut right = key_hint_spans(app, budget);
    if !right.is_empty() {
        right.insert(0, Span::raw("   "));
    }
    let mut right_group = chips;
    right_group.extend(right);
    render_split_line(frame, area, left, right_group, surface_color());
}

fn render_split_line(
    frame: &mut Frame,
    area: Rect,
    left: Vec<Span<'static>>,
    mut right: Vec<Span<'static>>,
    background: ratatui::style::Color,
) {
    frame.render_widget(Block::new().style(Style::new().bg(background)), area);
    frame.render_widget(Paragraph::new(Line::from(left)), area);
    if !right.is_empty() {
        right.push(Span::raw(" "));
        frame.render_widget(
            Paragraph::new(Line::from(right)).alignment(Alignment::Right),
            area,
        );
    }
}

fn review_target_spans(app: &App) -> Vec<Span<'static>> {
    let subtle = Style::new().fg(text_subtle_color());
    let faint = Style::new().fg(text_faint_color());
    match &app.review_mode {
        ReviewMode::WorkingTree => {
            let repo_name = app
                .repo_root
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            vec![
                Span::styled(repo_name, subtle),
                Span::styled(" · working tree", faint),
            ]
        }
        ReviewMode::CommitCompare(selection) => vec![
            Span::styled("commit ", faint),
            Span::styled(selection.short_hash.clone(), subtle),
            Span::styled("  ", faint),
            Span::styled(selection.subject.clone(), faint),
        ],
        ReviewMode::BranchCompare(selection) => vec![
            Span::styled(selection.source_ref.clone(), subtle),
            Span::styled(" → ", faint),
            Span::styled(selection.destination_ref.clone(), subtle),
        ],
    }
}

fn change_summary_spans(app: &App) -> Vec<Span<'static>> {
    let mut spans = match app.diff_stats_state() {
        DiffStatsState::Ready(stats) => ready_summary_spans(app, stats),
        DiffStatsState::Loading { file_count } => {
            let mut spans = file_count_spans(file_count);
            spans.push(Span::styled(
                "  counting…",
                Style::new().fg(text_faint_color()),
            ));
            spans
        }
        DiffStatsState::Unavailable { file_count } => file_count_spans(file_count),
    };

    let hidden = app.hidden_file_count();
    if hidden > 0 {
        spans.push(Span::styled(
            format!("  {hidden} hidden"),
            Style::new().fg(text_faint_color()),
        ));
    }
    spans
}

fn ready_summary_spans(app: &App, stats: ReviewDiffStats) -> Vec<Span<'static>> {
    if let ReviewMode::WorkingTree = app.review_mode
        && let (Some(tracked), Some(untracked)) = (stats.tracked, stats.untracked)
        && untracked.file_count > 0
    {
        let mut spans = Vec::new();
        if tracked.file_count > 0 {
            spans.extend(scope_spans("tracked", tracked));
            spans.push(Span::styled("  ·  ", Style::new().fg(text_faint_color())));
        }
        spans.extend(scope_spans("untracked", untracked));
        return spans;
    }

    let mut spans = file_count_spans(stats.file_count);
    spans.push(Span::raw("  "));
    spans.extend(line_change_spans(stats.additions, stats.deletions));
    spans
}

fn scope_spans(label: &'static str, scope: DiffLineTotals) -> Vec<Span<'static>> {
    let mut spans = vec![Span::styled(
        format!("{} {label}  ", scope.file_count),
        Style::new().fg(text_subtle_color()),
    )];
    spans.extend(line_change_spans(scope.additions, scope.deletions));
    spans
}

fn file_count_spans(file_count: usize) -> Vec<Span<'static>> {
    vec![Span::styled(
        format!(
            "{file_count} file{}",
            if file_count == 1 { "" } else { "s" }
        ),
        Style::new().fg(text_faint_color()),
    )]
}

/// `+12 −3` in the theme's success and error colors. Zero-sided counts are
/// dropped so pure additions read as `+12`.
pub(super) fn line_change_spans(additions: usize, deletions: usize) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    if additions > 0 || deletions == 0 {
        spans.push(Span::styled(
            format!("+{additions}"),
            Style::new().fg(success_color()),
        ));
    }
    if deletions > 0 {
        if !spans.is_empty() {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(
            format!("−{deletions}"),
            Style::new().fg(error_color()),
        ));
    }
    spans
}

fn chip(label: &str) -> Vec<Span<'static>> {
    vec![Span::styled(
        format!(" {label} "),
        Style::new().fg(text_subtle_color()).bg(chip_color()),
    )]
}

fn diff_mode_label(mode: DiffViewMode) -> &'static str {
    match mode {
        DiffViewMode::Unified => "unified",
        DiffViewMode::Split => "split",
    }
}

/// Key hints for the focused pane, dropping the least important ones first
/// when the footer is narrow.
fn key_hint_spans(app: &App, budget: usize) -> Vec<Span<'static>> {
    let view_toggle = match app.diff_view_mode {
        DiffViewMode::Unified => "split",
        DiffViewMode::Split => "unified",
    };
    let mut hints: Vec<(&str, &str)> = vec![("?", "help"), ("j/k", "move")];
    match app.active_pane {
        ActivePane::Sidebar if !app.sidebar_hidden => {
            hints.push(("tab", "diff"));
            if let ReviewMode::WorkingTree = app.review_mode {
                hints.push(("space", "stage"));
            }
            hints.extend([("ff", "find file"), ("fg", "search"), ("v", view_toggle)]);
        }
        _ => hints.extend([
            ("tab", "files"),
            ("⏎", "open"),
            ("fg", "search"),
            ("v", view_toggle),
            ("z", "wrap"),
        ]),
    }

    let mut kept: Vec<(&str, &str)> = Vec::new();
    let mut width = 0usize;
    for (key, label) in hints {
        let hint_width = key.width() + 1 + label.width() + 2;
        if width + hint_width > budget {
            break;
        }
        width += hint_width;
        kept.push((key, label));
    }

    // Help stays rightmost so it is always in the same place.
    let mut spans = Vec::new();
    for (key, label) in kept.into_iter().rev() {
        spans.push(Span::styled(
            key.to_string(),
            Style::new().fg(text_color()).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            format!(" {label}  "),
            Style::new().fg(text_faint_color()),
        ));
    }
    if let Some(last) = spans.last_mut() {
        let trimmed = last.content.trim_end().to_string();
        last.content = trimmed.into();
    }
    spans
}

fn spans_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|span| span.content.width()).sum()
}

pub(super) fn render_notifications(frame: &mut Frame, app: &App) {
    let mut top = frame.area().y + 1;

    if let Some(direction) = app.remote_sync {
        let label = match direction {
            RemoteSyncDirection::Pull => "Pulling from remote…",
            RemoteSyncDirection::Push => "Pushing to remote…",
        };
        render_notice(
            frame,
            top,
            label,
            primary_color(),
            Style::new().fg(text_subtle_color()),
        );
        top = top.saturating_add(4);
    }

    if let Some(notice) = app.snackbar_notice.as_ref() {
        let accent = match notice.variant {
            SnackbarVariant::Info => primary_color(),
            SnackbarVariant::Error => error_color(),
        };
        render_notice(
            frame,
            top,
            &notice.message,
            accent,
            Style::new().fg(text_color()),
        );
    }
}

fn render_notice(
    frame: &mut Frame,
    top: u16,
    message: &str,
    accent: ratatui::style::Color,
    text_style: Style,
) {
    let area = top_right_rect(NOTICE_WIDTH, 3, top, frame.area());
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(accent))
        .padding(Padding::horizontal(1))
        .style(Style::new().bg(panel_color()));
    frame.render_widget(ratatui::widgets::Clear, area);
    frame.render_widget(
        Paragraph::new(Text::from(Line::from(Span::styled(
            message.to_string(),
            text_style,
        ))))
        .block(block),
        area,
    );
}
