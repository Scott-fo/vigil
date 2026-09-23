use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Padding, Paragraph},
};

use crate::app::App;

use super::super::{error_color, text_faint_color};
use super::frame::render_modal_frame;
use super::hints::render_hint_footer;
use super::prompt::{Prompt, render_prompt};

pub(super) fn render_commit_modal(frame: &mut Frame, app: &App) {
    let inner = render_modal_frame(frame, 72, 8, "Commit staged changes");
    let [label, prompt, _, error, footer] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .areas(inner);

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "Message",
            Style::new().fg(text_faint_color()),
        )))
        .block(Block::new().padding(Padding::horizontal(1))),
        label,
    );
    render_prompt(
        frame,
        prompt,
        Prompt::new(&app.commit_message, "Describe the change"),
    );
    if let Some(message) = app.commit_error.as_deref() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                message.to_string(),
                Style::new().fg(error_color()),
            )))
            .block(Block::new().padding(Padding::horizontal(1))),
            error,
        );
    }
    render_hint_footer(frame, footer, &[("⏎", "commit"), ("esc", "cancel")], None);
}
