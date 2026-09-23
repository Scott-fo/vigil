use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Padding, Paragraph, Wrap},
};

use crate::app::App;

use super::super::{error_color, text_color, text_faint_color, text_subtle_color, warning_color};
use super::frame::render_danger_modal_frame;
use super::hints::render_hint_footer;

pub(super) fn render_branch_merge_modal(frame: &mut Frame, app: &App) {
    let Some(target) = app.branch_merge_target.as_ref() else {
        return;
    };

    let inner = render_danger_modal_frame(frame, 78, 11, "Prepare branch merge");
    let [body, footer] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .areas(inner);

    let strong = Style::new().fg(text_color()).add_modifier(Modifier::BOLD);
    let mut lines = vec![
        Line::from(vec![
            Span::styled(target.source_ref.clone(), strong),
            Span::styled("  →  ", Style::new().fg(text_faint_color())),
            Span::styled(target.destination_ref.clone(), strong),
        ]),
        Line::default(),
        Line::from(Span::styled(
            "Switches to the destination branch, then merges without committing.",
            Style::new().fg(text_subtle_color()),
        )),
        Line::from(Span::styled(
            "Clean merges stay staged for review. Conflicts open in the working tree.",
            Style::new().fg(text_subtle_color()),
        )),
    ];
    if app.branch_merge_loading {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            "Merging…",
            Style::new()
                .fg(warning_color())
                .add_modifier(Modifier::BOLD),
        )));
    } else if let Some(error) = app.branch_merge_error.as_ref() {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            error.clone(),
            Style::new().fg(error_color()),
        )));
    }

    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .block(Block::new().padding(Padding::horizontal(1))),
        body,
    );
    render_hint_footer(frame, footer, &[("⏎", "merge"), ("esc", "cancel")], None);
}
