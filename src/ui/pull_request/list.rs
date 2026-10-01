use std::ops::Range;

use ratatui::{
    Frame,
    layout::{Alignment, Position, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    app::{App, PullRequestListStatus, PullRequestListView, QueryInput},
    forge::{ForgeError, PullRequestListFilter, PullRequestSummary},
};

use super::{
    super::{
        chip_color, error_color, hover_color, layout::PullRequestListLayout, primary_color,
        selection_color, status::line_change_spans, surface_color, text_color, text_faint_color,
        text_subtle_color,
    },
    labels::{
        check_rollup_glyph, compact_age, draft_span, faint, freshness_text, now_unix_seconds,
        short_decision_span, subtle, truncate_end,
    },
};

/// Terminals narrower than this drop the author and line-count columns.
const WIDE_LIST_WIDTH: usize = 90;
const AUTHOR_WIDTH: usize = 14;
const DECISION_WIDTH: usize = 9;
const AGE_WIDTH: usize = 4;
const CHANGES_WIDTH: usize = 13;

/// What a click on the pull request list lands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullRequestListTarget {
    Tab(PullRequestListFilter),
    /// A row, by index into the rows shown.
    Row(usize),
}

pub(in crate::ui) fn filter_label(filter: PullRequestListFilter) -> &'static str {
    match filter {
        PullRequestListFilter::NeedsMyReview => "Needs my review",
        PullRequestListFilter::Mine => "Mine",
        PullRequestListFilter::AllOpen => "All open",
    }
}

/// Each tab's columns and label in the tabs row, left to right.
fn tab_regions(
    view: &PullRequestListView<'_>,
    area: Rect,
) -> Vec<(Range<u16>, PullRequestListFilter, String)> {
    let mut column = area.x + 1;
    let mut regions = Vec::new();
    for (filter, count) in &view.tabs {
        let label = match count {
            Some(count) => format!(" {} {count} ", filter_label(*filter)),
            None => format!(" {} ", filter_label(*filter)),
        };
        let width = label.width() as u16;
        regions.push((column..column + width, *filter, label));
        column += width + 1;
    }
    regions
}

/// The target under a terminal cell on the list screen.
pub(in crate::ui) fn list_target_at(
    app: &App,
    area: Rect,
    column: u16,
    row: u16,
) -> Option<PullRequestListTarget> {
    let layout = PullRequestListLayout::new(area);
    let view = app.pull_request_list_view();
    if row == layout.tabs.y {
        return tab_regions(&view, layout.tabs)
            .into_iter()
            .find_map(|(range, filter, _)| {
                range
                    .contains(&column)
                    .then_some(PullRequestListTarget::Tab(filter))
            });
    }
    if !layout.list.contains(Position::new(column, row)) {
        return None;
    }
    let scroll = visible_scroll(&view, layout.list.height as usize);
    let index = scroll + (row - layout.list.y) as usize;
    (index < view.rows.len()).then_some(PullRequestListTarget::Row(index))
}

/// The first row shown: the stored scroll, moved just enough to keep the
/// selection on screen.
fn visible_scroll(view: &PullRequestListView<'_>, height: usize) -> usize {
    if height == 0 {
        return 0;
    }
    let max = view.rows.len().saturating_sub(height);
    let mut scroll = view.scroll.min(max);
    if view.selected < scroll {
        scroll = view.selected;
    } else if view.selected >= scroll + height {
        scroll = view.selected + 1 - height;
    }
    scroll.min(max)
}

pub(in crate::ui) fn render_pull_request_list(frame: &mut Frame, app: &mut App, area: Rect) {
    frame.render_widget(Block::new().style(Style::new().bg(surface_color())), area);
    let layout = PullRequestListLayout::new(area);
    let now = now_unix_seconds();
    let hovered = app
        .mouse_position
        .and_then(|position| list_target_at(app, area, position.x, position.y));
    let view = app.pull_request_list_view();
    let scroll = visible_scroll(&view, layout.list.height as usize);

    render_header(frame, &view, layout.header);
    render_tabs(frame, &view, layout.tabs, hovered);
    render_rows(frame, &view, layout.list, scroll, hovered, now);
    render_footer(frame, &view, layout.footer, now);
    app.set_pull_request_list_scroll(scroll);
}

fn render_header(frame: &mut Frame, view: &PullRequestListView<'_>, area: Rect) {
    let mut left = vec![
        Span::raw(" "),
        Span::styled(
            "PULL REQUESTS",
            Style::new()
                .fg(text_faint_color())
                .add_modifier(Modifier::BOLD),
        ),
    ];
    if let Some(repository) = view.repository {
        left.push(faint("  "));
        left.push(subtle(repository.name_with_owner()));
    }
    frame.render_widget(Paragraph::new(Line::from(left)), area);

    let hints = [
        ("⏎", "open"),
        ("/", "filter"),
        ("tab", "next tab"),
        ("r", "refresh"),
        ("esc", "back"),
    ];
    let mut right = Vec::new();
    for (key, label) in hints {
        right.push(Span::styled(
            key,
            Style::new().fg(text_color()).add_modifier(Modifier::BOLD),
        ));
        right.push(faint(format!(" {label}  ")));
    }
    frame.render_widget(
        Paragraph::new(Line::from(right)).alignment(Alignment::Right),
        area,
    );
}

fn render_tabs(
    frame: &mut Frame,
    view: &PullRequestListView<'_>,
    area: Rect,
    hovered: Option<PullRequestListTarget>,
) {
    for (range, filter, label) in tab_regions(view, area) {
        if range.start >= area.right() {
            break;
        }
        let style = if filter == view.filter {
            Style::new()
                .fg(text_color())
                .bg(chip_color())
                .add_modifier(Modifier::BOLD)
        } else if hovered == Some(PullRequestListTarget::Tab(filter)) {
            Style::new().fg(text_color())
        } else {
            Style::new().fg(text_subtle_color())
        };
        let width = (range.end.min(area.right()) - range.start).max(1);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(label, style))),
            Rect::new(range.start, area.y, width, 1),
        );
    }
}

fn render_rows(
    frame: &mut Frame,
    view: &PullRequestListView<'_>,
    area: Rect,
    scroll: usize,
    hovered: Option<PullRequestListTarget>,
    now: i64,
) {
    if let Some(message) = empty_message(view) {
        frame.render_widget(
            Paragraph::new(Line::from(vec![Span::raw("   "), message])),
            Rect::new(area.x, area.y, area.width, 1.min(area.height)),
        );
        return;
    }
    let number_width = view
        .rows
        .iter()
        .map(|summary| summary.number.to_string().len() + 1)
        .max()
        .unwrap_or(2);
    let author_width = view
        .rows
        .iter()
        .map(|summary| summary.author.width())
        .max()
        .unwrap_or(0)
        .min(AUTHOR_WIDTH);
    for (index, summary) in view
        .rows
        .iter()
        .copied()
        .enumerate()
        .skip(scroll)
        .take(area.height as usize)
    {
        let selected = index == view.selected;
        let columns = RowColumns {
            width: area.width as usize,
            number: number_width,
            author: author_width,
        };
        let mut line = row_line(summary, columns, selected, now);
        if selected {
            line = line.style(Style::new().bg(selection_color()));
        } else if hovered == Some(PullRequestListTarget::Row(index)) {
            line = line.style(Style::new().bg(hover_color()));
        }
        let y = area.y + (index - scroll) as u16;
        frame.render_widget(Paragraph::new(line), Rect::new(area.x, y, area.width, 1));
    }
}

/// Why no rows show, when none do.
fn empty_message(view: &PullRequestListView<'_>) -> Option<Span<'static>> {
    let message = match view.status {
        PullRequestListStatus::Connecting => faint("Connecting to GitHub…"),
        PullRequestListStatus::Unavailable(Some(error)) => {
            Span::styled(unavailable_message(error), Style::new().fg(error_color()))
        }
        PullRequestListStatus::Unavailable(None) => faint("Pull requests are unavailable here."),
        PullRequestListStatus::Loading => faint("Loading pull requests…"),
        PullRequestListStatus::Failed(error) => Span::styled(
            format!("Could not load pull requests: {error}"),
            Style::new().fg(error_color()),
        ),
        PullRequestListStatus::Ready if !view.rows.is_empty() => return None,
        PullRequestListStatus::Ready if !view.query.trim().is_empty() => {
            let number = view.query.trim().trim_start_matches('#');
            if number.parse::<u64>().is_ok() {
                faint(format!(
                    "No listed pull request matches · ⏎ opens #{number}"
                ))
            } else {
                faint(format!("No pull request matches “{}”", view.query.trim()))
            }
        }
        PullRequestListStatus::Ready => faint(match view.filter {
            PullRequestListFilter::NeedsMyReview => "No open pull requests need your review.",
            PullRequestListFilter::Mine => "You have no open pull requests.",
            PullRequestListFilter::AllOpen => "No open pull requests.",
        }),
    };
    Some(message)
}

fn unavailable_message(error: &ForgeError) -> String {
    match error {
        ForgeError::NotGitHubRepository { .. } => {
            "This repository has no GitHub remote.".to_string()
        }
        other => format!("Pull requests are unavailable: {other}"),
    }
}

#[derive(Debug, Clone, Copy)]
struct RowColumns {
    width: usize,
    number: usize,
    author: usize,
}

/// ` #17  draft Title ……  author  ✓ approved  3h  +12 −3`
fn row_line(
    summary: &PullRequestSummary,
    columns: RowColumns,
    selected: bool,
    now: i64,
) -> Line<'static> {
    let wide = columns.width >= WIDE_LIST_WIDTH;
    let number = format!("#{}", summary.number);
    let mut left = vec![
        if selected {
            Span::styled("▎", Style::new().fg(primary_color()))
        } else {
            Span::raw(" ")
        },
        subtle(format!("{number:>width$}  ", width = columns.number)),
    ];

    let mut right = Vec::new();
    if wide {
        right.push(subtle(format!(
            "{:<width$}  ",
            truncate_end(&summary.author, columns.author),
            width = columns.author
        )));
    }
    right.push(check_rollup_glyph(summary.checks).unwrap_or_else(|| Span::raw(" ")));
    right.push(Span::raw(" "));
    let decision = summary
        .review_decision
        .map(short_decision_span)
        .unwrap_or_else(|| Span::raw(""));
    let decision_pad = DECISION_WIDTH.saturating_sub(decision.content.width());
    right.push(decision);
    right.push(Span::raw(" ".repeat(decision_pad)));
    right.push(faint(format!(
        "{:>width$}",
        compact_age(&summary.updated_at, now),
        width = AGE_WIDTH
    )));
    if wide {
        let changes = line_change_spans(summary.additions as usize, summary.deletions as usize);
        let changes_width = changes
            .iter()
            .map(|span| span.content.width())
            .sum::<usize>();
        right.push(Span::raw(
            " ".repeat(CHANGES_WIDTH.saturating_sub(changes_width)),
        ));
        right.extend(changes);
    }
    right.push(Span::raw(" "));

    let used = spans_width(&left) + spans_width(&right);
    let draft = summary.is_draft.then(draft_span);
    let draft_width = draft.as_ref().map_or(0, |span| span.content.width() + 1);
    let title_width = columns.width.saturating_sub(used + draft_width + 2);
    let title = truncate_end(&summary.title, title_width);
    let gap = columns
        .width
        .saturating_sub(used + draft_width + title.width())
        .max(1);
    if let Some(draft) = draft {
        left.push(draft);
        left.push(Span::raw(" "));
    }
    let title_style = if selected {
        Style::new().fg(text_color()).add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(text_color())
    };
    left.push(Span::styled(title, title_style));
    left.push(Span::raw(" ".repeat(gap)));
    left.extend(right);
    Line::from(left)
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

fn spans_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|span| span.content.width()).sum()
}

fn render_footer(frame: &mut Frame, view: &PullRequestListView<'_>, area: Rect, now: i64) {
    let mut left = vec![Span::raw(" ")];
    if view.query_input == QueryInput::Editing || !view.query.is_empty() {
        left.push(Span::styled(
            "/",
            Style::new()
                .fg(primary_color())
                .add_modifier(Modifier::BOLD),
        ));
        left.push(Span::styled(
            view.query.to_string(),
            Style::new().fg(text_color()),
        ));
        if view.query_input == QueryInput::Editing {
            left.push(Span::styled("▏", Style::new().fg(primary_color())));
        }
        left.push(faint("   "));
    }
    if let Some(number) = view.opening {
        left.push(Span::styled(
            format!("Opening #{number}…"),
            Style::new().fg(primary_color()),
        ));
        left.push(faint("   "));
    }
    if let Some((shown, total)) = view.truncated {
        left.push(faint(format!(
            "Showing the {shown} most recently updated of {total} · refine with /"
        )));
        left.push(faint("   "));
    }
    if let Some(saved_at) = view.saved_at {
        left.push(faint(capitalize(&freshness_text(
            saved_at,
            view.refreshing && view.stale_error.is_none(),
            now,
        ))));
        left.push(faint("   "));
    }
    if let Some(error) = view.stale_error {
        left.push(Span::styled(
            format!("Refresh failed: {error}"),
            Style::new().fg(error_color()),
        ));
    } else if view.refreshing && view.saved_at.is_none() {
        left.push(faint("Refreshing…"));
    }
    frame.render_widget(Block::new().style(Style::new().bg(surface_color())), area);
    frame.render_widget(Paragraph::new(Line::from(left)), area);
    let count = format!("{} shown ", view.rows.len());
    frame.render_widget(
        Paragraph::new(Line::from(faint(count))).alignment(Alignment::Right),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::pull_request_fixtures as fixtures;

    fn text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn rows_fill_the_width_and_keep_their_columns() {
        let mut summary = fixtures::summary(17);
        summary.is_draft = true;
        let now = summary.updated_at.unix_seconds().unwrap() + 3 * 3600;
        for width in [60, 120] {
            let line = row_line(
                &summary,
                RowColumns {
                    width,
                    number: 3,
                    author: 8,
                },
                false,
                now,
            );
            assert_eq!(line.width(), width, "{:?}", text(&line));
            let rendered = text(&line);
            assert!(rendered.contains("#17"));
            assert!(rendered.contains("draft Remove Codex"));
            assert!(rendered.contains("✓ approved"));
            assert!(rendered.contains("3h"));
            assert_eq!(rendered.contains("Scott-fo"), width >= WIDE_LIST_WIDTH);
            assert_eq!(rendered.contains("+211 −3143"), width >= WIDE_LIST_WIDTH);
        }
    }
}
