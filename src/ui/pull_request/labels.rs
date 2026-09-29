use std::time::{SystemTime, UNIX_EPOCH};

use ratatui::{
    style::{Color, Modifier, Style},
    text::Span,
};
use unicode_width::UnicodeWidthChar;

use crate::forge::{
    CheckRollup, CheckState, PullRequestState, PullRequestSummary, ReviewDecision, ReviewState,
    Timestamp,
};

use super::super::{
    error_color, primary_color, success_color, text_faint_color, text_subtle_color, warning_color,
};

/// A glyph for one check or a check rollup.
pub(in crate::ui) fn check_glyph(state: CheckState) -> Span<'static> {
    let (glyph, color) = match state {
        CheckState::Success => ("✓", success_color()),
        CheckState::Failure => ("✗", error_color()),
        CheckState::Pending => ("●", warning_color()),
        CheckState::Cancelled => ("⊘", text_faint_color()),
        CheckState::Neutral => ("○", text_faint_color()),
        CheckState::Skipped => ("↷", text_faint_color()),
    };
    Span::styled(glyph, Style::new().fg(color))
}

pub(in crate::ui) fn check_state_label(state: CheckState) -> &'static str {
    match state {
        CheckState::Success => "passed",
        CheckState::Failure => "failed",
        CheckState::Pending => "pending",
        CheckState::Cancelled => "cancelled",
        CheckState::Neutral => "neutral",
        CheckState::Skipped => "skipped",
    }
}

pub(in crate::ui) fn check_rollup_glyph(checks: Option<CheckRollup>) -> Option<Span<'static>> {
    checks.map(|rollup| check_glyph(rollup.state))
}

pub(in crate::ui) fn decision_span(decision: ReviewDecision) -> Span<'static> {
    let (label, color) = match decision {
        ReviewDecision::Approved => ("approved", success_color()),
        ReviewDecision::ChangesRequested => ("changes requested", error_color()),
        ReviewDecision::ReviewRequired => ("review required", warning_color()),
    };
    Span::styled(label, Style::new().fg(color))
}

/// `approved` / `changes` / `review`: decisions for narrow columns.
pub(in crate::ui) fn short_decision_span(decision: ReviewDecision) -> Span<'static> {
    let (label, color) = match decision {
        ReviewDecision::Approved => ("approved", success_color()),
        ReviewDecision::ChangesRequested => ("changes", error_color()),
        ReviewDecision::ReviewRequired => ("review", warning_color()),
    };
    Span::styled(label, Style::new().fg(color))
}

pub(in crate::ui) fn review_state_span(state: ReviewState) -> (Span<'static>, Span<'static>) {
    let (glyph, label, color) = match state {
        ReviewState::Approved => ("✓", "approved", success_color()),
        ReviewState::ChangesRequested => ("✗", "changes requested", error_color()),
        ReviewState::Commented => ("●", "commented", text_subtle_color()),
        ReviewState::Dismissed => ("○", "dismissed", text_faint_color()),
        ReviewState::Pending => ("◌", "pending", text_faint_color()),
    };
    (
        Span::styled(glyph, Style::new().fg(color)),
        Span::styled(label, Style::new().fg(color)),
    )
}

/// `merged` / `closed` for pull requests that are no longer open.
pub(in crate::ui) fn closed_state_span(state: PullRequestState) -> Option<Span<'static>> {
    match state {
        PullRequestState::Open => None,
        PullRequestState::Merged => Some(Span::styled("merged", Style::new().fg(primary_color()))),
        PullRequestState::Closed => Some(Span::styled("closed", Style::new().fg(error_color()))),
    }
}

pub(in crate::ui) fn draft_span() -> Span<'static> {
    Span::styled(
        "draft",
        Style::new()
            .fg(text_faint_color())
            .add_modifier(Modifier::ITALIC),
    )
}

/// `#17 ✓ approved`: number, check rollup, and review verdict, plus a draft
/// or merged/closed marker. The compact form the footer shows for the
/// current branch.
pub(in crate::ui) fn pull_request_chip_spans(summary: &PullRequestSummary) -> Vec<Span<'static>> {
    let mut spans = vec![Span::styled(
        format!("#{}", summary.number),
        Style::new().fg(text_subtle_color()),
    )];
    if let Some(state) = closed_state_span(summary.state) {
        spans.push(Span::raw(" "));
        spans.push(state);
        return spans;
    }
    if summary.is_draft {
        spans.push(Span::raw(" "));
        spans.push(draft_span());
    }
    if let Some(glyph) = check_rollup_glyph(summary.checks) {
        spans.push(Span::raw(" "));
        spans.push(glyph);
    }
    if let Some(decision) = summary.review_decision {
        spans.push(Span::raw(" "));
        spans.push(decision_span(decision));
    }
    spans
}

pub(in crate::ui) fn now_unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
}

/// `now`, `5m`, `3h`, `2d`, `3w`, `4mo`, or `2y` since `timestamp`.
pub(in crate::ui) fn compact_age(timestamp: &Timestamp, now: i64) -> String {
    let Some(then) = timestamp.unix_seconds() else {
        return String::new();
    };
    let seconds = (now - then).max(0);
    let minutes = seconds / 60;
    let hours = minutes / 60;
    let days = hours / 24;
    match () {
        _ if minutes < 1 => "now".to_string(),
        _ if hours < 1 => format!("{minutes}m"),
        _ if days < 1 => format!("{hours}h"),
        _ if days < 14 => format!("{days}d"),
        _ if days < 60 => format!("{}w", days / 7),
        _ if days < 365 => format!("{}mo", days / 30),
        _ => format!("{}y", days / 365),
    }
}

/// `3h ago`, or `just now`.
pub(in crate::ui) fn relative_time(timestamp: &Timestamp, now: i64) -> String {
    match compact_age(timestamp, now).as_str() {
        "" => String::new(),
        "now" => "just now".to_string(),
        age => format!("{age} ago"),
    }
}

/// `text`, cut to `max_width` columns with a trailing `…` when longer.
pub(in crate::ui) fn truncate_end(text: &str, max_width: usize) -> String {
    let mut width = 0;
    let mut result = String::new();
    let total: usize = text.chars().map(|ch| ch.width().unwrap_or(0)).sum();
    if total <= max_width {
        return text.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    for ch in text.chars() {
        let ch_width = ch.width().unwrap_or(0);
        if width + ch_width + 1 > max_width {
            break;
        }
        width += ch_width;
        result.push(ch);
    }
    result.push('…');
    result
}

pub(in crate::ui) fn faint(text: impl Into<String>) -> Span<'static> {
    Span::styled(text.into(), Style::new().fg(text_faint_color()))
}

pub(in crate::ui) fn subtle(text: impl Into<String>) -> Span<'static> {
    Span::styled(text.into(), Style::new().fg(text_subtle_color()))
}

pub(in crate::ui) fn colored(text: impl Into<String>, color: Color) -> Span<'static> {
    Span::styled(text.into(), Style::new().fg(color))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ages_read_compactly() {
        let at = Timestamp::new("2026-09-01T00:00:00Z");
        let then = at.unix_seconds().unwrap();

        assert_eq!(compact_age(&at, then + 30), "now");
        assert_eq!(compact_age(&at, then + 5 * 60), "5m");
        assert_eq!(compact_age(&at, then + 3 * 3600), "3h");
        assert_eq!(compact_age(&at, then + 2 * 86_400), "2d");
        assert_eq!(compact_age(&at, then + 21 * 86_400), "3w");
        assert_eq!(compact_age(&at, then + 400 * 86_400), "1y");
        assert_eq!(relative_time(&at, then + 3 * 3600), "3h ago");
    }

    #[test]
    fn truncation_keeps_width_and_marks_the_cut() {
        assert_eq!(truncate_end("short", 10), "short");
        assert_eq!(truncate_end("a longer title", 8), "a longe…");
    }
}
