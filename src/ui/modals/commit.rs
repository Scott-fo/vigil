use ratatui::{
    Frame,
    style::Style,
    text::{Line, Span, Text},
    widgets::{Block, Padding, Paragraph},
};

use crate::app::App;

use super::super::{element_color, error_color, panel_color, text_color, text_muted_color};
use super::frame::render_modal_frame;

pub(super) fn render_commit_modal(frame: &mut Frame, app: &App) {
    let inner = render_modal_frame(frame, 72, 9, "Commit Staged Changes");

    let message_label = Line::from(Span::styled("Message:", Style::new().fg(text_color())));
    let input_line = Line::from(Span::styled(
        if app.commit_message.is_empty() {
            "Enter commit message..."
        } else {
            app.commit_message.as_str()
        },
        if app.commit_message.is_empty() {
            Style::new().fg(text_muted_color()).bg(element_color())
        } else {
            Style::new().fg(text_color()).bg(element_color())
        },
    ));
    let hint_or_error = Line::from(Span::styled(
        app.commit_error
            .as_deref()
            .unwrap_or("Enter commits. Esc closes without committing."),
        if app.commit_error.is_some() {
            Style::new().fg(error_color())
        } else {
            Style::new().fg(text_muted_color())
        },
    ));

    let content = vec![
        message_label,
        Line::default(),
        input_line,
        Line::default(),
        hint_or_error,
    ];
    let paragraph = Paragraph::new(Text::from(content))
        .style(Style::new().bg(panel_color()))
        .block(Block::new().padding(Padding::horizontal(1)));
    frame.render_widget(paragraph, inner);
}
