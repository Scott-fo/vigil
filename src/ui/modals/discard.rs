use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::app::App;

use super::super::layout::centered_rect;
use super::super::{error_color, panel_color, text_color, text_muted_color, warning_color};

pub(super) fn render_discard_modal(frame: &mut Frame, app: &App) {
    let Some(file) = app.discard_target.as_ref() else {
        return;
    };

    let area = centered_rect(72, 9, frame.area());
    frame.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(error_color()))
        .style(Style::new().bg(panel_color()))
        .title(Line::from(Span::styled(
            " Discard File Changes? ",
            Style::new().fg(error_color()).add_modifier(Modifier::BOLD),
        )));
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);

    let text = vec![
        Line::from(Span::styled(
            "This will remove all local changes in:",
            Style::new().fg(text_color()),
        )),
        Line::default(),
        Line::from(Span::styled(
            file.label.clone(),
            Style::new().fg(warning_color()),
        )),
        Line::default(),
        Line::from(Span::styled(
            "Enter confirms discard. Esc cancels.",
            Style::new().fg(text_muted_color()),
        )),
    ];
    let paragraph = Paragraph::new(Text::from(text)).style(Style::new().bg(panel_color()));
    frame.render_widget(paragraph, inner);
}
