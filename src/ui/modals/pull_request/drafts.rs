use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Padding, Paragraph},
};

use crate::{
    app::{DraftEntry, DraftList},
    ui::{error_color, text_color, text_faint_color, text_subtle_color, warning_color},
};

use super::super::{
    frame::render_modal_frame, hints::render_hint_footer, list::render_visible_list,
};
use super::composer::anchor_label;

pub(super) fn render_drafts(frame: &mut Frame, list: &DraftList, drafts: &[DraftEntry<'_>]) {
    let height = (drafts.len() as u16).min(14) + 7;
    let inner = render_modal_frame(
        frame,
        96,
        height,
        format!("Draft comments · {}", drafts.len()),
    );
    let [rows, _, status, _, footer] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(inner);

    let width = rows.width as usize;
    render_visible_list(frame, rows, drafts.len(), list.selected(), |index, _| {
        let entry = drafts[index];
        let place = format!(
            "{} · {}",
            entry.draft.path,
            anchor_label(&entry.draft.anchor)
        );
        let mut spans = vec![Span::styled(place.clone(), Style::new().fg(text_color()))];
        let mut used = place.chars().count() + 2;
        if !entry.attached {
            spans.push(Span::styled(
                "  needs attention",
                Style::new().fg(warning_color()),
            ));
            used += 17;
        }
        let first_line = entry.draft.body.lines().next().unwrap_or_default();
        let room = width.saturating_sub(used + 4);
        if room > 8 {
            let excerpt = if first_line.chars().count() > room {
                format!("{}…", first_line.chars().take(room - 1).collect::<String>())
            } else {
                first_line.to_string()
            };
            spans.push(Span::styled(
                format!("  {excerpt}"),
                Style::new().fg(text_subtle_color()),
            ));
        }
        spans
    });

    let status_span = if list.confirm_delete() {
        Span::styled(
            "press d again to delete this draft",
            Style::new().fg(error_color()),
        )
    } else if drafts.iter().any(|entry| !entry.attached) {
        Span::styled(
            "Drafts that need attention lost their lines to new commits; edit or delete them.",
            Style::new().fg(text_faint_color()),
        )
    } else {
        Span::styled(
            "Drafts stay on this machine until you submit a review.",
            Style::new().fg(text_faint_color()),
        )
    };
    frame.render_widget(
        Paragraph::new(Line::from(status_span)).block(Block::new().padding(Padding::horizontal(1))),
        status,
    );
    render_hint_footer(
        frame,
        footer,
        &[
            ("j/k", "move"),
            ("⏎", "edit"),
            ("d d", "delete"),
            ("esc", "close"),
        ],
        None,
    );
}
