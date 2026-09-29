use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Padding, Paragraph, Wrap},
};
use std::ops::Range;

use unicode_width::UnicodeWidthStr;

use crate::app::{ActivePane, App, DiffStatsState, DiffViewMode, ReviewMode, SnackbarVariant};
use crate::git::{
    BranchOperation, BranchSnapshot, DiffLineTotals, Divergence, HeadState, RepoOperation,
    ReviewDiffStats,
};
use crate::sidebar::SidebarSection;

use super::layout::top_right_rect;
use super::pull_request::pull_request_chip_spans;
use super::{
    NOTICE_WIDTH, chip_color, error_color, panel_color, primary_color, success_color,
    surface_color, text_color, text_faint_color, text_subtle_color, warning_color,
};

/// Nerd Font branch glyph, matching the file icons in the sidebar.
pub(super) const BRANCH_ICON: &str = "\u{e0a0}";

/// A clickable control in the footer. Rendering and mouse hit testing both
/// derive these from the same footer model, so a click lands on what is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FooterAction {
    ToggleDiffViewMode,
    ToggleLineWrap,
    ToggleWhitespace,
    ToggleSidebar,
    OpenHelp,
    SwitchPane,
    ToggleStage,
    FindFile,
    SearchDiff,
    OpenBranches,
    /// The current branch's pull request chip.
    OpenPullRequest,
    /// The "new commits" notice while reviewing a pull request.
    ReloadPullRequest,
}

/// A run of right-hand footer text, optionally clickable.
struct FooterSegment {
    spans: Vec<Span<'static>>,
    action: Option<FooterAction>,
}

impl FooterSegment {
    fn plain(text: &'static str) -> Self {
        Self::spans(vec![Span::raw(text)])
    }

    fn spans(spans: Vec<Span<'static>>) -> Self {
        Self {
            spans,
            action: None,
        }
    }
}

/// Footer content for one frame at a given width.
struct FooterModel {
    left: Vec<FooterSegment>,
    right: Vec<FooterSegment>,
}

impl FooterModel {
    /// Left: what is being compared and how big the change is, plus any
    /// transient status. Right: view mode chips and key hints for the focused
    /// pane. Hints are dropped first when the terminal is narrow.
    fn new(app: &App, width: u16) -> Self {
        let mut left = vec![FooterSegment::plain(" ")];
        left.extend(review_target_segments(app));
        left.push(FooterSegment::plain("   "));
        left.push(FooterSegment::spans(change_summary_spans(app)));
        if !app.status_message_is_default()
            && let Some(message) = app.status_message.as_deref()
        {
            left.push(FooterSegment::plain("   "));
            left.push(FooterSegment::spans(vec![Span::styled(
                message.to_string(),
                Style::new().fg(text_color()),
            )]));
        }

        let mut right = vec![
            chip(
                diff_mode_label(app.diff_view_mode),
                FooterAction::ToggleDiffViewMode,
            ),
            FooterSegment::plain(" "),
            chip(
                app.diff_line_wrap_mode.label(),
                FooterAction::ToggleLineWrap,
            ),
        ];
        if app.diff_whitespace_mode == crate::git::WhitespaceMode::Ignore {
            right.push(FooterSegment::plain(" "));
            right.push(chip("ignore ws", FooterAction::ToggleWhitespace));
        }
        if app.sidebar_hidden {
            right.push(FooterSegment::plain(" "));
            right.push(chip("sidebar hidden", FooterAction::ToggleSidebar));
        }

        let budget =
            (width as usize).saturating_sub(segments_width(&left) + segments_width(&right) + 5);
        let hints = key_hint_segments(app, budget);
        if !hints.is_empty() {
            right.push(FooterSegment::plain("   "));
            right.extend(hints);
        }
        Self { left, right }
    }

    /// Column range of each clickable segment. The left group starts at the
    /// area's edge; the right group is right-aligned with one trailing space
    /// of padding.
    fn action_columns(&self, area: Rect) -> Vec<(Range<u16>, FooterAction)> {
        let right_start = area
            .right()
            .saturating_sub(segments_width(&self.right) as u16 + 1);
        let mut regions = Vec::new();
        for (start, segments) in [(area.x, &self.left), (right_start, &self.right)] {
            let mut column = start;
            for segment in segments {
                let width = spans_width(&segment.spans) as u16;
                if let Some(action) = segment.action {
                    regions.push((column..column + width, action));
                }
                column += width;
            }
        }
        regions
    }

    fn action_at(&self, area: Rect, column: u16) -> Option<FooterAction> {
        self.action_columns(area)
            .into_iter()
            .find_map(|(range, action)| range.contains(&column).then_some(action))
    }
}

/// Footer action under a terminal cell, if any.
pub(super) fn footer_action_at(
    app: &App,
    area: Rect,
    column: u16,
    row: u16,
) -> Option<FooterAction> {
    if row != area.y {
        return None;
    }
    FooterModel::new(app, area.width).action_at(area, column)
}

pub(super) fn render_footer(frame: &mut Frame, app: &App, area: Rect) {
    let model = FooterModel::new(app, area.width);
    let hovered = app
        .mouse_position
        .filter(|position| position.y == area.y)
        .and_then(|position| model.action_at(area, position.x));
    // Hovered controls brighten so they read as clickable.
    let flatten = |segments: Vec<FooterSegment>| -> Vec<Span<'static>> {
        segments
            .into_iter()
            .flat_map(|segment| {
                let hovered = hovered.is_some() && segment.action == hovered;
                segment.spans.into_iter().map(move |span| {
                    if hovered {
                        let style = span.style.fg(text_color());
                        span.style(style)
                    } else {
                        span
                    }
                })
            })
            .collect()
    };
    render_split_line(
        frame,
        area,
        flatten(model.left),
        flatten(model.right),
        surface_color(),
    );
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

/// What is under review. In the working tree this is the repository and its
/// checked-out branch; the branch readout opens the branch panel.
fn review_target_segments(app: &App) -> Vec<FooterSegment> {
    let subtle = Style::new().fg(text_subtle_color());
    let faint = Style::new().fg(text_faint_color());
    match &app.review_mode {
        ReviewMode::WorkingTree => {
            let repo_name = app
                .repo_root
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let branch = match app.branch_snapshot() {
                Some(snapshot) => branch_readout_spans(snapshot),
                None => vec![Span::styled("working tree", faint)],
            };
            let mut segments = vec![
                FooterSegment::spans(vec![
                    Span::styled(repo_name, subtle),
                    Span::styled(" · ", faint),
                ]),
                FooterSegment {
                    spans: branch,
                    action: Some(FooterAction::OpenBranches),
                },
            ];
            if let Some(summary) = app.current_branch_pull_request() {
                segments.push(FooterSegment::plain(" "));
                segments.push(FooterSegment {
                    spans: pull_request_chip_spans(summary),
                    action: Some(FooterAction::OpenPullRequest),
                });
            }
            segments
        }
        mode @ ReviewMode::PullRequest(_) => {
            let mut segments = vec![FooterSegment::spans(compare_target_spans(mode))];
            let pending = app.pending_draft_count();
            if pending > 0 {
                segments.push(FooterSegment::plain(" "));
                segments.push(FooterSegment::spans(vec![Span::styled(
                    format!(" {pending} pending "),
                    Style::new().fg(primary_color()).bg(chip_color()),
                )]));
            }
            if app.pull_request_has_newer_head() {
                segments.push(FooterSegment::plain(" "));
                segments.push(FooterSegment {
                    spans: vec![Span::styled(
                        " new commits · r reload ",
                        Style::new().fg(warning_color()).bg(chip_color()),
                    )],
                    action: Some(FooterAction::ReloadPullRequest),
                });
            }
            segments
        }
        mode => vec![FooterSegment::spans(compare_target_spans(mode))],
    }
}

fn compare_target_spans(mode: &ReviewMode) -> Vec<Span<'static>> {
    let subtle = Style::new().fg(text_subtle_color());
    let faint = Style::new().fg(text_faint_color());
    match mode {
        ReviewMode::WorkingTree => Vec::new(),
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
        ReviewMode::PullRequest(selection) => vec![
            Span::styled(format!("#{}", selection.number), subtle),
            Span::styled("  ", faint),
            Span::styled(selection.base_ref_name.clone(), subtle),
            Span::styled(" ← ", faint),
            Span::styled(selection.head_ref_name.clone(), subtle),
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

    let viewed = app.viewed_file_count();
    if viewed > 0 {
        spans.push(Span::styled(
            format!("  {viewed}/{} viewed", app.files.len()),
            Style::new().fg(text_faint_color()),
        ));
    }

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

fn chip(label: &str, action: FooterAction) -> FooterSegment {
    FooterSegment {
        spans: vec![Span::styled(
            format!(" {label} "),
            Style::new().fg(text_subtle_color()).bg(chip_color()),
        )],
        action: Some(action),
    }
}

fn diff_mode_label(mode: DiffViewMode) -> &'static str {
    match mode {
        DiffViewMode::Unified => "unified",
        DiffViewMode::Split => "split",
    }
}

/// Key hints for the focused pane, dropping the least important ones first
/// when the footer is narrow. Hints for simple commands are clickable.
fn key_hint_segments(app: &App, budget: usize) -> Vec<FooterSegment> {
    let view_toggle = match app.diff_view_mode {
        DiffViewMode::Unified => "split",
        DiffViewMode::Split => "unified",
    };
    let mut hints: Vec<(&str, &str, Option<FooterAction>)> = vec![
        ("?", "help", Some(FooterAction::OpenHelp)),
        ("j/k", "move", None),
    ];
    match app.active_pane {
        ActivePane::Sidebar if !app.sidebar_hidden => {
            hints.push(("tab", "diff", Some(FooterAction::SwitchPane)));
            if let ReviewMode::WorkingTree = app.review_mode {
                let action = match app.selected_file_section() {
                    Some(SidebarSection::Staged) => "unstage",
                    Some(SidebarSection::Unstaged) | None => "stage",
                };
                hints.push(("space", action, Some(FooterAction::ToggleStage)));
            }
            hints.extend([
                ("x", "viewed", None),
                ("ff", "find file", Some(FooterAction::FindFile)),
                ("fg", "search", Some(FooterAction::SearchDiff)),
                ("v", view_toggle, Some(FooterAction::ToggleDiffViewMode)),
            ]);
        }
        _ if matches!(app.review_mode, ReviewMode::PullRequest(_)) => hints.extend([
            ("tab", "files", Some(FooterAction::SwitchPane)),
            ("c", "comment", None),
            ("[ ]", "change", None),
            ("x", "viewed", None),
            ("fg", "search", Some(FooterAction::SearchDiff)),
            ("v", view_toggle, Some(FooterAction::ToggleDiffViewMode)),
        ]),
        _ => hints.extend([
            ("tab", "files", Some(FooterAction::SwitchPane)),
            ("[ ]", "change", None),
            ("x", "viewed", None),
            ("⏎", "open", None),
            ("fg", "search", Some(FooterAction::SearchDiff)),
            ("v", view_toggle, Some(FooterAction::ToggleDiffViewMode)),
            ("z", "wrap", Some(FooterAction::ToggleLineWrap)),
        ]),
    }

    let mut kept = Vec::new();
    let mut width = 0usize;
    for hint in hints {
        let (key, label, _) = hint;
        let hint_width = key.width() + 1 + label.width() + 2;
        if width + hint_width > budget {
            break;
        }
        width += hint_width;
        kept.push(hint);
    }

    // Help stays rightmost so it is always in the same place.
    let mut segments = Vec::new();
    for (index, (key, label, action)) in kept.into_iter().rev().enumerate() {
        if index > 0 {
            segments.push(FooterSegment::plain("  "));
        }
        segments.push(FooterSegment {
            spans: vec![
                Span::styled(
                    key.to_string(),
                    Style::new().fg(text_color()).add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!(" {label}"), Style::new().fg(text_faint_color())),
            ],
            action,
        });
    }
    segments
}

fn segments_width(segments: &[FooterSegment]) -> usize {
    segments
        .iter()
        .map(|segment| spans_width(&segment.spans))
        .sum()
}

fn spans_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|span| span.content.width()).sum()
}

/// `main ↑2 ↓1 · merging`: the checked-out branch, how it differs from its
/// upstream, and any operation git is in the middle of.
pub(super) fn branch_readout_spans(snapshot: &BranchSnapshot) -> Vec<Span<'static>> {
    let subtle = Style::new().fg(text_subtle_color());
    let faint = Style::new().fg(text_faint_color());
    let mut spans = vec![Span::styled(format!("{BRANCH_ICON} "), faint)];
    match &snapshot.head {
        HeadState::Branch(name) => spans.push(Span::styled(name.clone(), subtle)),
        HeadState::Unborn(name) => {
            spans.push(Span::styled(name.clone(), subtle));
            spans.push(Span::styled(" (no commits)", faint));
        }
        HeadState::Detached { short_hash } => {
            spans.push(Span::styled("detached ", faint));
            spans.push(Span::styled(short_hash.clone(), subtle));
        }
    }
    if let Some(upstream) = snapshot
        .head_branch()
        .and_then(|branch| branch.upstream.as_ref())
    {
        spans.extend(divergence_spans(upstream.divergence));
    }
    if let Some(operation) = snapshot.operation {
        spans.push(Span::styled(" · ", faint));
        spans.push(Span::styled(
            repo_operation_label(operation),
            Style::new().fg(warning_color()),
        ));
    }
    spans
}

/// ` ↑2 ↓1` for commits to push and pull; nothing when in sync.
pub(super) fn divergence_spans(divergence: Divergence) -> Vec<Span<'static>> {
    match divergence {
        Divergence::Tracking { ahead, behind } => {
            let mut spans = Vec::new();
            if ahead > 0 {
                spans.push(Span::styled(
                    format!(" ↑{ahead}"),
                    Style::new().fg(primary_color()),
                ));
            }
            if behind > 0 {
                spans.push(Span::styled(
                    format!(" ↓{behind}"),
                    Style::new().fg(warning_color()),
                ));
            }
            spans
        }
        Divergence::Gone => vec![Span::styled(
            " upstream gone",
            Style::new().fg(text_faint_color()),
        )],
    }
}

pub(super) fn repo_operation_label(operation: RepoOperation) -> &'static str {
    match operation {
        RepoOperation::Merge => "merging",
        RepoOperation::Rebase => "rebasing",
        RepoOperation::CherryPick => "cherry-picking",
        RepoOperation::Revert => "reverting",
        RepoOperation::Bisect => "bisecting",
    }
}

/// Present-tense description of a running operation.
pub(super) fn operation_progress_label(operation: &BranchOperation) -> String {
    match operation {
        BranchOperation::Fetch => "Fetching…".to_string(),
        BranchOperation::Pull => "Pulling…".to_string(),
        BranchOperation::Push => "Pushing…".to_string(),
        BranchOperation::Switch { branch } => format!("Switching to {branch}…"),
        BranchOperation::Track { local_name, .. } => format!("Checking out {local_name}…"),
        BranchOperation::Create { name, .. } => format!("Creating {name}…"),
        BranchOperation::Rename { from, .. } => format!("Renaming {from}…"),
        BranchOperation::Delete { branch, .. } => format!("Deleting {branch}…"),
    }
}

pub(super) fn render_notifications(frame: &mut Frame, app: &App) {
    let mut top = frame.area().y + 1;

    // The branch panel shows its own progress.
    if let Some(operation) = app.branch_operation()
        && !app.branch_panel_open()
    {
        let height = render_notice(
            frame,
            top,
            &operation_progress_label(operation),
            primary_color(),
            Style::new().fg(text_subtle_color()),
        );
        top = top.saturating_add(height + 1);
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

/// Draws a notice in the top-right corner and returns its height. Long
/// messages, such as git errors, widen the notice and then wrap.
fn render_notice(
    frame: &mut Frame,
    top: u16,
    message: &str,
    accent: ratatui::style::Color,
    text_style: Style,
) -> u16 {
    let (width, height) = notice_size(message);
    let area = top_right_rect(width, height, top, frame.area());
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(accent))
        .padding(Padding::horizontal(1))
        .style(Style::new().bg(panel_color()));
    frame.render_widget(ratatui::widgets::Clear, area);
    frame.render_widget(
        Paragraph::new(Text::from(
            message
                .lines()
                .map(|line| Line::from(Span::styled(line.to_string(), text_style)))
                .collect::<Vec<_>>(),
        ))
        .wrap(Wrap { trim: true })
        .block(block),
        area,
    );
    area.height
}

const NOTICE_MAX_WIDTH: u16 = 64;
const NOTICE_MAX_TEXT_ROWS: u16 = 8;

/// Outer size of a notice: border and padding take four columns and two rows.
fn notice_size(message: &str) -> (u16, u16) {
    let longest = message
        .lines()
        .map(UnicodeWidthStr::width)
        .max()
        .unwrap_or(0) as u16;
    let width = (longest + 4).clamp(NOTICE_WIDTH, NOTICE_MAX_WIDTH);
    let text_width = (width - 4).max(1) as usize;
    let rows = message
        .lines()
        .map(|line| line.width().max(1).div_ceil(text_width) as u16)
        .sum::<u16>()
        .clamp(1, NOTICE_MAX_TEXT_ROWS);
    (width, rows + 2)
}
