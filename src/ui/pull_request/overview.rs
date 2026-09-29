use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};

use crate::{
    app::{App, DraftEntry, PullRequestOverview},
    forge::{
        Check, CheckState, MergeMethod, MergeStateStatus, Mergeability, PullRequest,
        PullRequestState, RequestedReviewer,
    },
    review::{DisplayThread, ThreadStatus, UnplacedReason},
};

use super::{
    super::{
        diff::QUIET_SCROLLBAR, error_color, primary_color, status::line_change_spans,
        success_color, text_color, text_faint_color, text_subtle_color, warning_color,
    },
    labels::{
        check_glyph, check_state_label, closed_state_span, colored, decision_span, draft_span,
        faint, now_unix_seconds, relative_time, review_state_span, subtle,
    },
    markdown::markdown_lines,
};

const INDENT: &str = "  ";
const QUOTE: &str = "  │ ";

/// The overview header row: `Overview · #17` on the left, `+12 −3` right.
pub(in crate::ui) fn render_overview_header(frame: &mut Frame, app: &App, area: Rect) {
    let Some(overview) = app.pull_request_overview() else {
        return;
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                "Overview",
                Style::new().fg(text_color()).add_modifier(Modifier::BOLD),
            ),
            faint(format!("  #{}", overview.summary.number)),
        ])),
        area,
    );
    let mut right = line_change_spans(
        overview.summary.additions as usize,
        overview.summary.deletions as usize,
    );
    right.push(Span::raw(" "));
    frame.render_widget(
        Paragraph::new(Line::from(right)).alignment(Alignment::Right),
        area,
    );
}

/// The scrollable overview page in the diff pane body.
pub(in crate::ui) fn render_overview_body(frame: &mut Frame, app: &mut App, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let Some(overview) = app.pull_request_overview() else {
        return;
    };
    let lines = overview_lines(&overview, area.width as usize, now_unix_seconds());
    let height = area.height as usize;
    let max_scroll = lines.len().saturating_sub(height);
    let scroll = overview.scroll.min(max_scroll);
    let visible = lines
        .iter()
        .skip(scroll)
        .take(height)
        .cloned()
        .collect::<Vec<_>>();
    let total = lines.len();
    app.set_pull_request_overview_scroll(scroll);

    frame.render_widget(Paragraph::new(Text::from(visible)), area);
    if total > height {
        let mut state = ScrollbarState::new(total)
            .position(scroll)
            .viewport_content_length(height);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .symbols(QUIET_SCROLLBAR)
                .begin_symbol(None)
                .end_symbol(None)
                .thumb_style(Style::new().fg(text_faint_color())),
            area,
            &mut state,
        );
    }
}

/// Every row of the overview page at `width` columns.
pub(in crate::ui) fn overview_lines(
    overview: &PullRequestOverview<'_>,
    width: usize,
    now: i64,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let summary = overview.summary;
    let title_style = Style::new().fg(text_color()).add_modifier(Modifier::BOLD);
    lines.push(Line::default());
    for row in crate::review::wrap_comment_text(&summary.title, width.saturating_sub(4).max(8)) {
        lines.push(Line::from(vec![
            Span::raw(INDENT),
            Span::styled(row, title_style),
        ]));
    }

    let mut meta = vec![Span::raw(INDENT), subtle(format!("#{}", summary.number))];
    let state = closed_state_span(summary.state)
        .unwrap_or_else(|| Span::styled("open", Style::new().fg(success_color())));
    meta.extend([faint(" · "), state]);
    if summary.is_draft {
        meta.extend([faint(" · "), draft_span()]);
    }
    meta.extend([
        faint(" · "),
        Span::styled(summary.author.clone(), Style::new().fg(text_color())),
        faint(" · "),
        subtle(summary.base_ref_name.clone()),
        faint(" ← "),
        subtle(summary.head_ref_name.clone()),
    ]);
    if summary.is_cross_repository {
        meta.push(faint(" (fork)"));
    }
    lines.push(Line::from(meta));
    let mut totals = vec![
        Span::raw(INDENT),
        faint(format!(
            "{} file{} · ",
            summary.changed_files,
            if summary.changed_files == 1 { "" } else { "s" }
        )),
    ];
    totals.extend(line_change_spans(
        summary.additions as usize,
        summary.deletions as usize,
    ));
    if let Some(detail) = overview.detail {
        totals.push(faint(format!(
            " · {} commit{} · opened {}",
            detail.commit_count,
            if detail.commit_count == 1 { "" } else { "s" },
            relative_time(&detail.created_at, now)
        )));
    }
    lines.push(Line::from(totals));
    lines.push(action_hints_line());
    push_drafts(&mut lines, &overview.drafts, width);

    let Some(detail) = overview.detail else {
        lines.push(Line::default());
        let message = match overview.detail_error {
            Some(error) => Line::from(vec![
                Span::raw(INDENT),
                colored(format!("could not load details: {error}"), error_color()),
            ]),
            None => Line::from(vec![Span::raw(INDENT), faint("Loading details…")]),
        };
        lines.push(message);
        push_unplaced_threads(&mut lines, &overview.unplaced_threads, width, now);
        return lines;
    };

    push_reviews(&mut lines, detail, now);
    push_checks(&mut lines, detail, width);
    push_merge(&mut lines, detail);
    push_description(&mut lines, detail, width);
    push_conversation(&mut lines, detail, width, now);
    push_unplaced_threads(&mut lines, &overview.unplaced_threads, width, now);
    lines.push(Line::default());
    lines
}

/// `c comment · S submit review · D drafts`.
fn action_hints_line() -> Line<'static> {
    let key = Style::new().fg(text_color()).add_modifier(Modifier::BOLD);
    let mut spans = vec![Span::raw(INDENT)];
    for (index, (keys, label)) in [("c", "comment"), ("S", "submit review"), ("D", "drafts")]
        .into_iter()
        .enumerate()
    {
        if index > 0 {
            spans.push(faint(" · "));
        }
        spans.push(Span::styled(keys, key));
        spans.push(faint(format!(" {label}")));
    }
    Line::from(spans)
}

/// The reviewer's drafts, with the ones that lost their lines flagged.
fn push_drafts(lines: &mut Vec<Line<'static>>, drafts: &[DraftEntry<'_>], width: usize) {
    if drafts.is_empty() {
        return;
    }
    let pending = drafts.iter().filter(|entry| entry.attached).count();
    let mut extra = vec![colored(format!("{pending} pending"), primary_color())];
    let stale = drafts.len() - pending;
    if stale > 0 {
        extra.push(faint(" · "));
        extra.push(colored(format!("{stale} need attention"), warning_color()));
    }
    extra.push(faint(" · S submits · D edits"));
    section(lines, "Your drafts", extra);
    for entry in drafts {
        let draft = entry.draft;
        let end = draft.anchor.end.position;
        let line = match draft.anchor.start_line().filter(|start| *start != end.line) {
            Some(start) => format!("lines {start}–{}", end.line),
            None => format!("line {}", end.line),
        };
        let mut spans = vec![
            Span::raw(INDENT),
            Span::styled(draft.path.clone(), Style::new().fg(text_color())),
            faint(format!(" · {line} · ")),
        ];
        if entry.attached {
            spans.push(colored("◌ pending", primary_color()));
        } else {
            spans.push(colored(
                "⚠ its lines changed in new commits; edit or delete it",
                warning_color(),
            ));
        }
        lines.push(Line::from(spans));
        for row in crate::review::wrap_comment_text(&draft.body, width.saturating_sub(6).max(8))
            .into_iter()
            .take(3)
        {
            lines.push(Line::from(vec![Span::raw(QUOTE), subtle(row)]));
        }
    }
}

fn section(lines: &mut Vec<Line<'static>>, title: &str, extra: Vec<Span<'static>>) {
    lines.push(Line::default());
    let mut spans = vec![
        Span::raw(INDENT),
        Span::styled(
            title.to_uppercase(),
            Style::new()
                .fg(text_faint_color())
                .add_modifier(Modifier::BOLD),
        ),
    ];
    if !extra.is_empty() {
        spans.push(Span::raw("  "));
        spans.extend(extra);
    }
    lines.push(Line::from(spans));
}

fn push_reviews(lines: &mut Vec<Line<'static>>, detail: &PullRequest, now: i64) {
    let verdict = detail
        .summary
        .review_decision
        .map(decision_span)
        .into_iter()
        .collect();
    section(lines, "Reviews", verdict);
    let name_width = detail
        .latest_reviews
        .iter()
        .map(|review| review.author.chars().count())
        .chain(
            detail
                .requested_reviewers
                .iter()
                .map(|reviewer| requested_reviewer_name(reviewer).chars().count()),
        )
        .max()
        .unwrap_or(0)
        .min(24);
    if detail.latest_reviews.is_empty() && detail.requested_reviewers.is_empty() {
        lines.push(Line::from(vec![Span::raw(INDENT), faint("No reviews yet")]));
    }
    for review in &detail.latest_reviews {
        let (glyph, label) = review_state_span(review.state);
        let mut spans = vec![
            Span::raw(INDENT),
            glyph,
            Span::raw(" "),
            Span::styled(
                format!("{:<name_width$}", review.author),
                Style::new().fg(text_color()),
            ),
            Span::raw("  "),
            label,
        ];
        if let Some(submitted) = &review.submitted_at {
            spans.push(faint(format!("  {}", relative_time(submitted, now))));
        }
        lines.push(Line::from(spans));
    }
    for reviewer in &detail.requested_reviewers {
        lines.push(Line::from(vec![
            Span::raw(INDENT),
            faint("○ "),
            Span::styled(
                format!("{:<name_width$}", requested_reviewer_name(reviewer)),
                Style::new().fg(text_subtle_color()),
            ),
            faint("  review requested"),
        ]));
    }
}

fn requested_reviewer_name(reviewer: &RequestedReviewer) -> String {
    match reviewer {
        RequestedReviewer::User { login } => login.clone(),
        RequestedReviewer::Team { slug, .. } => format!("@{slug}"),
    }
}

/// Failures first, then pending, then everything else, each by name.
fn check_order(check: &Check) -> u8 {
    match check.state {
        CheckState::Failure => 0,
        CheckState::Pending => 1,
        CheckState::Cancelled => 2,
        CheckState::Success => 3,
        CheckState::Neutral => 4,
        CheckState::Skipped => 5,
    }
}

fn push_checks(lines: &mut Vec<Line<'static>>, detail: &PullRequest, width: usize) {
    let mut tally = Vec::new();
    if let Some(rollup) = detail.summary.checks {
        for state in [
            CheckState::Failure,
            CheckState::Pending,
            CheckState::Success,
            CheckState::Cancelled,
            CheckState::Skipped,
            CheckState::Neutral,
        ] {
            let count = rollup.counts.get(state);
            if count == 0 {
                continue;
            }
            if !tally.is_empty() {
                tally.push(Span::raw("  "));
            }
            tally.push(check_glyph(state));
            tally.push(subtle(format!(" {count} {}", check_state_label(state))));
        }
    }
    section(lines, "Checks", tally);
    if detail.checks.is_empty() {
        lines.push(Line::from(vec![Span::raw(INDENT), faint("No checks")]));
        return;
    }
    let mut checks = detail.checks.iter().collect::<Vec<_>>();
    checks.sort_by(|a, b| {
        check_order(a)
            .cmp(&check_order(b))
            .then_with(|| a.name.cmp(&b.name))
    });
    for check in checks {
        let name = match &check.workflow {
            Some(workflow) => format!("{workflow} / {}", check.name),
            None => check.name.clone(),
        };
        let mut spans = vec![
            Span::raw(INDENT),
            check_glyph(check.state),
            Span::raw(" "),
            Span::styled(name.clone(), Style::new().fg(text_color())),
        ];
        let mut used = INDENT.len() + 2 + name.chars().count();
        if check.is_required {
            spans.push(faint("  required"));
            used += 10;
        }
        if let Some(summary) = check.summary.as_deref().filter(|text| !text.is_empty()) {
            let room = width.saturating_sub(used + 5);
            if room > 8 {
                let text = summary.lines().next().unwrap_or_default();
                spans.push(faint(format!(
                    "  — {}",
                    super::labels::truncate_end(text, room)
                )));
            }
        }
        lines.push(Line::from(spans));
    }
}

fn push_merge(lines: &mut Vec<Line<'static>>, detail: &PullRequest) {
    section(lines, "Merge", Vec::new());
    let mut spans = vec![Span::raw(INDENT)];
    match detail.summary.state {
        PullRequestState::Merged => spans.push(colored("✓ merged", primary_color())),
        PullRequestState::Closed => spans.push(colored("✗ closed without merging", error_color())),
        PullRequestState::Open => {
            spans.push(match detail.mergeable {
                Mergeability::Mergeable => colored("✓ no conflicts", success_color()),
                Mergeability::Conflicting => {
                    colored("✗ conflicts with the base branch", error_color())
                }
                Mergeability::Unknown => colored(
                    "◌ mergeability unknown · GitHub is computing it",
                    text_faint_color(),
                ),
            });
            // Both unknown says the same thing twice.
            if !(detail.mergeable == Mergeability::Unknown
                && detail.merge_state == MergeStateStatus::Unknown)
            {
                spans.push(faint(" · "));
                spans.push(merge_state_span(detail.merge_state));
            }
        }
    }
    lines.push(Line::from(spans));
    if let Some(auto_merge) = &detail.auto_merge {
        let mut text = format!(
            "auto-merge ({}) enabled",
            merge_method_label(auto_merge.method)
        );
        if let Some(by) = &auto_merge.enabled_by {
            text.push_str(&format!(" by {by}"));
        }
        lines.push(Line::from(vec![
            Span::raw(INDENT),
            colored(text, primary_color()),
        ]));
    }
}

fn merge_state_span(state: MergeStateStatus) -> Span<'static> {
    match state {
        MergeStateStatus::Clean => colored("ready to merge", success_color()),
        MergeStateStatus::HasHooks => colored("ready to merge (with hooks)", success_color()),
        MergeStateStatus::Unstable => {
            colored("mergeable · non-required checks failing", warning_color())
        }
        MergeStateStatus::Blocked => colored("blocked by branch protection", warning_color()),
        MergeStateStatus::Behind => colored("behind the base branch", warning_color()),
        MergeStateStatus::Dirty => colored("merge conflicts", error_color()),
        MergeStateStatus::Draft => colored("draft", text_faint_color()),
        MergeStateStatus::Unknown => faint("merge state unknown · GitHub is computing it"),
    }
}

fn merge_method_label(method: MergeMethod) -> &'static str {
    match method {
        MergeMethod::Merge => "merge commit",
        MergeMethod::Squash => "squash",
        MergeMethod::Rebase => "rebase",
    }
}

fn push_description(lines: &mut Vec<Line<'static>>, detail: &PullRequest, width: usize) {
    section(lines, "Description", Vec::new());
    if detail.body.trim().is_empty() {
        lines.push(Line::from(vec![Span::raw(INDENT), faint("No description")]));
        return;
    }
    lines.extend(markdown_lines(
        &detail.body,
        width.saturating_sub(2),
        INDENT,
    ));
}

fn push_conversation(lines: &mut Vec<Line<'static>>, detail: &PullRequest, width: usize, now: i64) {
    if detail.conversation.is_empty() {
        return;
    }
    section(
        lines,
        "Conversation",
        vec![faint(detail.conversation.len().to_string())],
    );
    for (index, comment) in detail.conversation.iter().enumerate() {
        if index > 0 {
            lines.push(Line::default());
        }
        lines.push(author_line(&comment.author, &comment.created_at, now));
        lines.extend(markdown_lines(
            &comment.body,
            width.saturating_sub(2),
            QUOTE,
        ));
    }
}

fn author_line(author: &str, at: &crate::forge::Timestamp, now: i64) -> Line<'static> {
    Line::from(vec![
        Span::raw(INDENT),
        Span::styled(
            author.to_string(),
            Style::new().fg(text_color()).add_modifier(Modifier::BOLD),
        ),
        faint(format!(" · {}", relative_time(at, now))),
    ])
}

fn push_unplaced_threads(
    lines: &mut Vec<Line<'static>>,
    threads: &[(UnplacedReason, &DisplayThread)],
    width: usize,
    now: i64,
) {
    if threads.is_empty() {
        return;
    }
    section(
        lines,
        "Threads outside the diff",
        vec![faint(threads.len().to_string())],
    );
    for (index, (reason, thread)) in threads.iter().enumerate() {
        if index > 0 {
            lines.push(Line::default());
        }
        let place = match reason {
            UnplacedReason::FileLevel => "file comment".to_string(),
            UnplacedReason::Outdated {
                original_line: Some(line),
            } => format!("outdated · was line {line}"),
            UnplacedReason::Outdated {
                original_line: None,
            } => "outdated".to_string(),
            UnplacedReason::NotInDiff => "file not in the diff".to_string(),
        };
        let status = match &thread.status {
            ThreadStatus::Unresolved => colored("● unresolved", warning_color()),
            ThreadStatus::Resolved { .. } => colored("✓ resolved", success_color()),
        };
        lines.push(Line::from(vec![
            Span::raw(INDENT),
            Span::styled(thread.path.clone(), Style::new().fg(text_color())),
            faint(format!(" · {place} · ")),
            status,
        ]));
        if thread.status != ThreadStatus::Unresolved {
            continue;
        }
        for comment in &thread.comments {
            lines.push(author_line(&comment.author, &comment.created_at, now));
            lines.extend(markdown_lines(
                &comment.body,
                width.saturating_sub(2),
                QUOTE,
            ));
        }
    }
}
