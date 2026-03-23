use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Padding, Paragraph},
};

use crate::app::App;

use super::super::layout::centered_rect;
use super::super::{
    border_active_color, element_color, error_color, panel_color, text_color, text_muted_color,
};

pub(super) fn render_commit_modal(frame: &mut Frame, app: &App) {
    let area = centered_rect(72, 9, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(border_active_color()))
        .style(Style::new().bg(panel_color()))
        .title(Line::from(Span::styled(
            " Commit Staged Changes ",
            Style::new().fg(text_color()).add_modifier(Modifier::BOLD),
        )));
    let inner = block.inner(area);
    frame.render_widget(block, area);

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
