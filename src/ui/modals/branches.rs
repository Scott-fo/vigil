use std::time::{SystemTime, UNIX_EPOCH};

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Padding, Paragraph, Wrap},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    app::{App, BranchPanelMode, BranchPanelRow, BranchPanelView, BranchSection},
    git::{BranchEntry, BranchLocation, BranchSnapshot, Divergence, HeadState},
};

use super::super::status::{
    BRANCH_ICON, divergence_spans, operation_progress_label, repo_operation_label,
};
use super::super::{
    chip_color, error_color, primary_color, text_color, text_faint_color, text_subtle_color,
    warning_color,
};
use super::frame::render_modal_frame;
use super::hints::{KeyHint, render_hint_footer};
use super::list::{
    list_row, render_list_error, render_list_message, render_quiet_scrollbar, visible_list_range,
};
use super::prompt::{Prompt, render_prompt};

const PANEL_WIDTH: u16 = 108;
/// Frame, head line, prompt, footer, and the blank rows between them.
const PANEL_CHROME_ROWS: u16 = 9;
const NAME_WIDTH: usize = 30;
const DIVERGENCE_WIDTH: usize = 9;
const HASH_WIDTH: usize = 9;
const AGE_WIDTH: usize = 5;
/// Error text may take this many rows under the head line before it is cut.
const MAX_MESSAGE_ROWS: u16 = 4;

pub(super) fn render_branch_panel(frame: &mut Frame, app: &mut App) {
    let Some(view) = app.branch_panel_view() else {
        return;
    };

    let message = status_lines(&view);
    let message_rows = wrapped_rows(&message, PANEL_WIDTH - 6).clamp(1, MAX_MESSAGE_ROWS);
    // Sized for every branch plus section headings so the panel keeps its
    // height while the filter narrows the list.
    let list_rows = view
        .snapshot
        .map_or(1, |snapshot| snapshot.branches.len() as u16 + 2);
    let height = (list_rows + message_rows + PANEL_CHROME_ROWS).clamp(14, 34);
    let inner = render_modal_frame(frame, PANEL_WIDTH, height, "Branches");
    let [head, status, _, prompt, _, list, _, footer] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(message_rows),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(inner);

    if let Some(snapshot) = view.snapshot {
        render_head_line(frame, head, snapshot);
    }
    frame.render_widget(
        Paragraph::new(Text::from(message))
            .wrap(Wrap { trim: true })
            .block(Block::new().padding(Padding::horizontal(1))),
        status,
    );
    render_mode_prompt(frame, prompt, view.panel.mode(), view.panel.query());
    render_branch_list(frame, list, &view);
    render_hint_footer(frame, footer, mode_hints(view.panel.mode()), None);
}

/// ` main → origin/main ↑2 ↓1 · merging` on the left, fetch age on the right.
fn render_head_line(frame: &mut Frame, area: Rect, snapshot: &BranchSnapshot) {
    let faint = Style::new().fg(text_faint_color());
    let strong = Style::new().fg(text_color()).add_modifier(Modifier::BOLD);
    let mut spans = vec![
        Span::raw(" "),
        Span::styled(format!("{BRANCH_ICON} "), Style::new().fg(primary_color())),
    ];
    match &snapshot.head {
        HeadState::Branch(name) | HeadState::Unborn(name) => {
            spans.push(Span::styled(name.clone(), strong));
        }
        HeadState::Detached { short_hash } => {
            spans.push(Span::styled("detached at ", faint));
            spans.push(Span::styled(short_hash.clone(), strong));
        }
    }
    if let Some(upstream) = snapshot
        .head_branch()
        .and_then(|branch| branch.upstream.as_ref())
    {
        spans.push(Span::styled("  →  ", faint));
        spans.push(Span::styled(
            upstream.name.clone(),
            Style::new().fg(text_subtle_color()),
        ));
        spans.extend(divergence_spans(upstream.divergence));
    }
    if let Some(operation) = snapshot.operation {
        spans.push(Span::styled("  ·  ", faint));
        spans.push(Span::styled(
            repo_operation_label(operation),
            Style::new()
                .fg(warning_color())
                .add_modifier(Modifier::BOLD),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);

    let fetched = match (snapshot.remotes.is_empty(), snapshot.last_fetch) {
        (true, _) => "no remotes".to_string(),
        (false, None) => "never fetched".to_string(),
        (false, Some(time)) => {
            let elapsed = SystemTime::now()
                .duration_since(time)
                .map(|elapsed| elapsed.as_secs() as i64)
                .unwrap_or_default();
            match relative_age(elapsed).as_str() {
                "now" => "fetched just now".to_string(),
                age => format!("fetched {age} ago"),
            }
        }
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(format!("{fetched} "), faint)))
            .alignment(Alignment::Right),
        area,
    );
}

/// The rows under the head: a running operation, the last failure, or a
/// plain-language sync summary for the checked-out branch.
fn status_lines(view: &BranchPanelView<'_>) -> Vec<Line<'static>> {
    let styled = |text: &str, style: Style| {
        text.lines()
            .map(|line| Line::from(Span::styled(line.to_string(), style)))
            .collect()
    };
    if let Some(operation) = view.operation {
        return styled(
            &operation_progress_label(operation),
            Style::new()
                .fg(warning_color())
                .add_modifier(Modifier::BOLD),
        );
    }
    if let Some(error) = view.panel.error() {
        return styled(error, Style::new().fg(error_color()));
    }
    let summary = view.snapshot.map(sync_summary).unwrap_or_default();
    styled(&summary, Style::new().fg(text_subtle_color()))
}

fn sync_summary(snapshot: &BranchSnapshot) -> String {
    match &snapshot.head {
        HeadState::Detached { .. } => {
            return "Not on a branch. Switch to one to pull or push.".to_string();
        }
        HeadState::Unborn(_) => return "No commits yet.".to_string(),
        HeadState::Branch(_) => {}
    }

    let Some(upstream) = snapshot
        .head_branch()
        .and_then(|branch| branch.upstream.as_ref())
    else {
        return if snapshot.remotes.is_empty() {
            "Local only. Add a remote to publish it.".to_string()
        } else {
            "Not published yet. P pushes it and sets its upstream.".to_string()
        };
    };

    match upstream.divergence {
        Divergence::Gone => format!(
            "{} no longer exists on the remote. P publishes it again.",
            upstream.name
        ),
        Divergence::Tracking {
            ahead: 0,
            behind: 0,
        } => format!("Up to date with {}.", upstream.name),
        Divergence::Tracking { ahead, behind: 0 } => {
            format!("{} to push to {}.", commits(ahead), upstream.name)
        }
        Divergence::Tracking { ahead: 0, behind } => {
            format!("{} to pull from {}.", commits(behind), upstream.name)
        }
        Divergence::Tracking { ahead, behind } => format!(
            "Diverged from {}: {} to push, {} to pull.",
            upstream.name,
            commits(ahead),
            behind
        ),
    }
}

fn commits(count: usize) -> String {
    format!("{count} commit{}", if count == 1 { "" } else { "s" })
}

fn wrapped_rows(lines: &[Line<'_>], width: u16) -> u16 {
    let width = width.max(1) as usize;
    lines
        .iter()
        .map(|line| line.width().max(1).div_ceil(width) as u16)
        .sum()
}

fn render_mode_prompt(frame: &mut Frame, area: Rect, mode: &BranchPanelMode, query: &str) {
    match mode {
        BranchPanelMode::Browse => {
            render_prompt(frame, area, Prompt::new(query, "/ to filter").active(false));
        }
        BranchPanelMode::Filter => {
            render_prompt(frame, area, Prompt::new(query, "Filter branches"));
        }
        BranchPanelMode::Create { start_point, name } => {
            let label = format!("New branch from {start_point}");
            render_prompt(
                frame,
                area,
                Prompt::new(name, "name").label(&label, label.width()),
            );
        }
        BranchPanelMode::Rename { branch, name } => {
            let label = format!("Rename {branch} to");
            render_prompt(
                frame,
                area,
                Prompt::new(name, "name").label(&label, label.width()),
            );
        }
        BranchPanelMode::Delete { branch, force } => {
            let danger = Style::new().fg(error_color());
            let strong = danger.add_modifier(Modifier::BOLD);
            let spans = if *force {
                vec![
                    Span::raw(" "),
                    Span::styled(branch.clone(), strong),
                    Span::styled(
                        " has commits that are not merged. Force delete loses them?",
                        danger,
                    ),
                ]
            } else {
                vec![
                    Span::styled(" Delete ", danger),
                    Span::styled(branch.clone(), strong),
                    Span::styled("?", danger),
                ]
            };
            frame.render_widget(
                Paragraph::new(Line::from(spans)).style(Style::new().bg(chip_color())),
                area,
            );
        }
    }
}

fn render_branch_list(frame: &mut Frame, area: Rect, view: &BranchPanelView<'_>) {
    let Some(snapshot) = view.snapshot else {
        match view.load_error {
            Some(error) => render_list_error(frame, area, "Unable to load branches.", error),
            None => render_list_message(frame, area, "Loading branches…"),
        }
        return;
    };
    if view.rows.is_empty() {
        let message = if view.panel.query().is_empty() {
            "No branches yet. Commit to create one."
        } else {
            "No matching branches."
        };
        render_list_message(frame, area, message);
        return;
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|now| now.as_secs() as i64)
        .unwrap_or_default();
    let selected = view.selected_row.unwrap_or(0);
    let range = visible_list_range(view.rows.len(), selected, area.height as usize);
    let content_width = (area.width as usize).saturating_sub(3);
    let lines = view.rows[range.clone()]
        .iter()
        .enumerate()
        .map(|(offset, row)| match row {
            BranchPanelRow::Section(section) => section_line(*section),
            BranchPanelRow::Branch(index) => {
                let is_selected = range.start + offset == selected;
                let branch = &snapshot.branches[*index];
                list_row(
                    branch_row(branch, is_selected, content_width, now),
                    is_selected,
                )
            }
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(Text::from(lines)), area);
    render_quiet_scrollbar(frame, area, view.rows.len(), range.start);
}

fn section_line(section: BranchSection) -> Line<'static> {
    let title = match section {
        BranchSection::Local => "LOCAL",
        BranchSection::Remote => "REMOTE",
    };
    Line::from(Span::styled(
        format!("  {title}"),
        Style::new()
            .fg(text_faint_color())
            .add_modifier(Modifier::BOLD),
    ))
}

/// `● main      ↑2 ↓1   abc1234  Subject…   2d`
fn branch_row(branch: &BranchEntry, selected: bool, width: usize, now: i64) -> Vec<Span<'static>> {
    let faint = Style::new().fg(text_faint_color());
    let subtle = Style::new().fg(text_subtle_color());
    let mut name_style = Style::new().fg(text_color());
    if selected || branch.is_head {
        name_style = name_style.add_modifier(Modifier::BOLD);
    }

    let mut spans = vec![if branch.is_head {
        Span::styled("● ", Style::new().fg(primary_color()))
    } else {
        Span::raw("  ")
    }];

    let name = truncate_end(&branch.name, NAME_WIDTH);
    match &branch.location {
        BranchLocation::Remote { remote } if name.starts_with(remote.as_str()) => {
            let (remote_part, rest) = name.split_at(remote.len());
            spans.push(Span::styled(remote_part.to_string(), faint));
            spans.push(Span::styled(rest.to_string(), name_style));
        }
        _ => spans.push(Span::styled(name.clone(), name_style)),
    }
    spans.push(Span::raw(pad(name.width(), NAME_WIDTH + 2)));

    let divergence = branch
        .upstream
        .as_ref()
        .map(|upstream| divergence_spans(upstream.divergence))
        .unwrap_or_default();
    let divergence_width = divergence
        .iter()
        .map(|span| span.content.width())
        .sum::<usize>();
    spans.extend(divergence);
    spans.push(Span::raw(pad(divergence_width, DIVERGENCE_WIDTH)));

    spans.push(Span::styled(
        format!("{:<HASH_WIDTH$}", branch.tip.short_hash),
        faint,
    ));

    let age = relative_age(now - branch.tip.committed_at);
    let subject_width =
        width.saturating_sub(2 + NAME_WIDTH + 2 + DIVERGENCE_WIDTH + HASH_WIDTH + AGE_WIDTH + 1);
    let subject = truncate_end(&branch.tip.subject, subject_width);
    spans.push(Span::styled(
        subject.clone(),
        if selected { subtle } else { faint },
    ));
    spans.push(Span::raw(pad(subject.width(), subject_width + 1)));
    spans.push(Span::styled(format!("{age:>AGE_WIDTH$}"), faint));
    spans
}

fn pad(used: usize, width: usize) -> String {
    " ".repeat(width.saturating_sub(used).max(1))
}

fn truncate_end(value: &str, max_width: usize) -> String {
    if value.width() <= max_width {
        return value.to_string();
    }
    let mut kept = String::new();
    let mut width = 1;
    for ch in value.chars() {
        let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + ch_width > max_width {
            break;
        }
        width += ch_width;
        kept.push(ch);
    }
    format!("{kept}…")
}

/// Compact age of a commit or fetch: `now`, `5m`, `3h`, `2d`, `3w`, `4mo`, `2y`.
fn relative_age(seconds: i64) -> String {
    const MINUTE: i64 = 60;
    const HOUR: i64 = 60 * MINUTE;
    const DAY: i64 = 24 * HOUR;
    let seconds = seconds.max(0);
    match seconds {
        s if s < MINUTE => "now".to_string(),
        s if s < HOUR => format!("{}m", s / MINUTE),
        s if s < DAY => format!("{}h", s / HOUR),
        s if s < 7 * DAY => format!("{}d", s / DAY),
        s if s < 30 * DAY => format!("{}w", s / (7 * DAY)),
        s if s < 365 * DAY => format!("{}mo", s / (30 * DAY)),
        s => format!("{}y", s / (365 * DAY)),
    }
}

fn mode_hints(mode: &BranchPanelMode) -> &'static [KeyHint] {
    match mode {
        BranchPanelMode::Browse => &[
            ("⏎", "switch"),
            ("n", "new"),
            ("R", "rename"),
            ("d", "delete"),
            ("/", "filter"),
            ("f", "fetch"),
            ("p", "pull"),
            ("P", "push"),
            ("esc", "close"),
        ],
        BranchPanelMode::Filter => &[("↑↓", "move"), ("⏎", "switch"), ("esc", "done")],
        BranchPanelMode::Create { .. } => &[("⏎", "create & switch"), ("esc", "cancel")],
        BranchPanelMode::Rename { .. } => &[("⏎", "rename"), ("esc", "cancel")],
        BranchPanelMode::Delete { force: false, .. } => &[("⏎", "delete"), ("esc", "cancel")],
        BranchPanelMode::Delete { force: true, .. } => &[("⏎", "force delete"), ("esc", "cancel")],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_age_picks_the_largest_whole_unit() {
        assert_eq!(relative_age(-5), "now");
        assert_eq!(relative_age(59), "now");
        assert_eq!(relative_age(5 * 60), "5m");
        assert_eq!(relative_age(3 * 3600 + 59), "3h");
        assert_eq!(relative_age(2 * 86_400), "2d");
        assert_eq!(relative_age(15 * 86_400), "2w");
        assert_eq!(relative_age(120 * 86_400), "4mo");
        assert_eq!(relative_age(800 * 86_400), "2y");
    }

    #[test]
    fn long_names_truncate_with_an_ellipsis() {
        assert_eq!(truncate_end("feature/very-long-name", 10), "feature/v…");
        assert_eq!(truncate_end("main", 10), "main");
    }
}
