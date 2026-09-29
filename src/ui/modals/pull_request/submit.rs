use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Padding, Paragraph, Wrap},
};

use crate::{
    app::{REVIEW_EVENTS, SubmitForm, SubmitWarning, event_allowed},
    forge::ReviewEvent,
    ui::{
        error_color, primary_color, text_color, text_faint_color, text_subtle_color, warning_color,
    },
};

use super::super::{frame::render_modal_frame, hints::render_hint_footer};
use super::{short_oid, text::render_text_area};

pub(super) struct SubmitModal<'a> {
    pub(super) form: &'a SubmitForm,
    pub(super) number: u64,
    pub(super) draft_count: usize,
    pub(super) is_author: bool,
    pub(super) warnings: &'a [SubmitWarning],
    pub(super) submitting: bool,
}

pub(super) fn render_submit(frame: &mut Frame, modal: SubmitModal<'_>) {
    let warning_rows = modal.warnings.len() as u16 * 2;
    let inner = render_modal_frame(
        frame,
        84,
        16 + warning_rows,
        format!("Submit review · #{}", modal.number),
    );
    let [verdict, drafts, warnings, label, body, status, _, footer] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Length(warning_rows),
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(inner);
    let padded = Block::new().padding(Padding::horizontal(1));

    let mut spans = Vec::new();
    for (index, event) in REVIEW_EVENTS.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw("   "));
        }
        let allowed = event_allowed(*event, modal.is_author);
        let chosen = *event == modal.form.event();
        let (marker, style) = match (chosen, allowed) {
            (true, _) => (
                "● ",
                Style::new()
                    .fg(primary_color())
                    .add_modifier(Modifier::BOLD),
            ),
            (false, true) => ("○ ", Style::new().fg(text_color())),
            (false, false) => (
                "○ ",
                Style::new()
                    .fg(text_faint_color())
                    .add_modifier(Modifier::CROSSED_OUT),
            ),
        };
        spans.push(Span::styled(
            format!("{marker}{}", event_label(*event)),
            style,
        ));
    }
    if modal.is_author {
        spans.push(Span::styled(
            "   your pull request: comment only",
            Style::new().fg(text_faint_color()),
        ));
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)).block(padded.clone()),
        verdict,
    );

    let draft_line = match modal.draft_count {
        0 => Span::styled(
            "No draft comments. Add a summary, or approve.",
            Style::new().fg(text_subtle_color()),
        ),
        1 => Span::styled("Sends 1 draft comment.", Style::new().fg(text_color())),
        count => Span::styled(
            format!("Sends {count} draft comments."),
            Style::new().fg(text_color()),
        ),
    };
    frame.render_widget(
        Paragraph::new(Line::from(draft_line)).block(padded.clone()),
        drafts,
    );

    let warning_lines = modal
        .warnings
        .iter()
        .map(|warning| {
            Line::from(Span::styled(
                format!("⚠ {}", warning_text(warning)),
                Style::new().fg(warning_color()),
            ))
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(Text::from(warning_lines))
            .wrap(Wrap { trim: false })
            .block(padded.clone()),
        warnings,
    );

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "Summary (optional for approvals and reviews with comments)",
            Style::new().fg(text_faint_color()),
        )))
        .block(padded.clone()),
        label,
    );
    render_text_area(
        frame,
        body,
        modal.form.body(),
        "Leave a summary in Markdown",
        !modal.submitting,
    );

    let status_span = if modal.submitting {
        Some(Span::styled(
            "submitting…",
            Style::new().fg(warning_color()),
        ))
    } else {
        modal
            .form
            .error()
            .map(|error| Span::styled(error.to_string(), Style::new().fg(error_color())))
    };
    if let Some(span) = status_span {
        frame.render_widget(Paragraph::new(Line::from(span)).block(padded), status);
    }

    render_hint_footer(
        frame,
        footer,
        &[
            ("tab", "verdict"),
            ("ctrl-s", "submit"),
            ("ctrl-e", "$EDITOR"),
            ("esc", "cancel"),
        ],
        None,
    );
}

pub(super) fn event_label(event: ReviewEvent) -> &'static str {
    match event {
        ReviewEvent::Comment => "Comment",
        ReviewEvent::Approve => "Approve",
        ReviewEvent::RequestChanges => "Request changes",
    }
}

fn warning_text(warning: &SubmitWarning) -> String {
    match warning {
        SubmitWarning::PendingReviewOnGitHub { comment_count } => format!(
            "You have a pending review on GitHub ({comment_count} comment{}). GitHub refuses a \
             new review until you submit or delete that one in the browser.",
            if *comment_count == 1 { "" } else { "s" }
        ),
        SubmitWarning::NewCommits { reviewed, latest } => format!(
            "New commits since your drafts ({} → {}). This review attaches to {} and its \
             comments may show as outdated; r reloads first.",
            short_oid(reviewed),
            short_oid(latest),
            short_oid(reviewed)
        ),
        SubmitWarning::DraftsLeftBehind(count) => format!(
            "{count} draft{} lost {} lines to new commits and will not be sent; see D.",
            if *count == 1 { "" } else { "s" },
            if *count == 1 { "its" } else { "their" }
        ),
    }
}
