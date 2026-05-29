use ratatui::{
    Frame,
    style::Style,
    text::{Line, Span, Text},
    widgets::Paragraph,
};

use crate::app::App;

use super::super::{panel_color, text_color, text_muted_color, warning_color};
use super::frame::render_danger_modal_frame;

pub(super) fn render_discard_modal(frame: &mut Frame, app: &App) {
    let Some(file) = app.discard_target.as_ref() else {
        return;
    };

    let inner = render_danger_modal_frame(frame, 72, 9, "Discard File Changes?");

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
