use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Padding, Paragraph},
};

use crate::{
    app::{Composer, ComposerStatus, ComposerTarget},
    forge::{DiffPosition, DiffSide},
    review::DraftAnchor,
    ui::{error_color, text_color, text_faint_color, warning_color},
};

use super::super::{frame::render_modal_frame, hints::render_hint_footer};
use super::text::render_text_area;

pub(super) fn render_composer(frame: &mut Frame, composer: &Composer, posting: bool) {
    let target = composer.target();
    let (title, place) = match target {
        ComposerTarget::NewDraft { path, anchor } => ("New comment", place(path, anchor)),
        ComposerTarget::EditDraft { path, anchor, .. } => ("Edit draft", place(path, anchor)),
        ComposerTarget::Reply { path, line, .. } => (
            "Reply",
            match line {
                Some(line) => format!("{path}:{line}"),
                None => path.clone(),
            },
        ),
        ComposerTarget::Conversation => ("Comment", "on the conversation".to_string()),
    };
    let inner = render_modal_frame(frame, 84, 18, title);
    let [header, _, body, status, _, footer] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(inner);

    let mut header_spans = vec![Span::styled(
        place,
        Style::new().fg(text_color()).add_modifier(Modifier::BOLD),
    )];
    header_spans.push(Span::styled(
        if target.posts_immediately() {
            "  ·  posts to GitHub now"
        } else {
            "  ·  draft, sent with your review"
        },
        Style::new().fg(text_faint_color()),
    ));
    frame.render_widget(
        Paragraph::new(Line::from(header_spans))
            .block(Block::new().padding(Padding::horizontal(1))),
        header,
    );

    render_text_area(
        frame,
        body,
        composer.text(),
        "Write Markdown. ⏎ starts a new line.",
        !posting,
    );

    let status_line = if posting {
        Some(Span::styled("posting…", Style::new().fg(warning_color())))
    } else if let Some(error) = composer.error() {
        Some(Span::styled(
            error.to_string(),
            Style::new().fg(error_color()),
        ))
    } else if composer.status() == ComposerStatus::ConfirmDiscard {
        Some(Span::styled(
            "unsaved text · esc again to discard it",
            Style::new().fg(warning_color()),
        ))
    } else {
        None
    };
    if let Some(span) = status_line {
        frame.render_widget(
            Paragraph::new(Line::from(span)).block(Block::new().padding(Padding::horizontal(1))),
            status,
        );
    }

    let save = match target {
        ComposerTarget::NewDraft { .. } | ComposerTarget::EditDraft { .. } => "save draft",
        ComposerTarget::Reply { .. } => "post reply",
        ComposerTarget::Conversation => "post comment",
    };
    let mut hints = vec![("ctrl-s", save), ("ctrl-e", "$EDITOR")];
    if composer.can_suggest() {
        hints.push(("ctrl-g", "suggestion"));
    }
    hints.push(("esc", "cancel"));
    render_hint_footer(
        frame,
        footer,
        &hints,
        Some(format!("{} lines", composer.text().lines().len()))
            .filter(|_| composer.text().lines().len() > 1),
    );
}

fn place(path: &str, anchor: &DraftAnchor) -> String {
    format!("{path} · {}", anchor_label(anchor))
}

/// `line 12`, `old line 7`, or `lines 10–14`.
pub(super) fn anchor_label(anchor: &DraftAnchor) -> String {
    let end = anchor.end.position;
    match &anchor.start {
        Some(start) if start.position != end => format!(
            "lines {}–{}",
            position_label(start.position),
            position_label(end)
        ),
        _ => format!("line {}", position_label(end)),
    }
}

fn position_label(position: DiffPosition) -> String {
    match position.side {
        DiffSide::Left => format!("old {}", position.line),
        DiffSide::Right => position.line.to_string(),
    }
}
