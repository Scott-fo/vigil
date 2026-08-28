use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Padding, Paragraph},
};

use crate::app::App;

use super::super::{panel_color, text_color, text_muted_color};
use super::frame::render_modal_frame;
use super::list::render_modal_input;

pub(super) fn render_file_filter_modal(frame: &mut Frame, app: &App) {
    let inner = render_modal_frame(frame, 78, 12, "Hide Files");

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Min(3),
        ])
        .split(inner);

    let header = Paragraph::new(Text::from(vec![Line::from(Span::styled(
        "Hide files whose paths end with these suffixes.",
        Style::new().fg(text_color()).add_modifier(Modifier::BOLD),
    ))]))
    .style(Style::new().bg(panel_color()))
    .block(Block::new().padding(Padding::horizontal(1)));
    frame.render_widget(header, chunks[0]);

    if app.file_filter_query.is_empty() {
        render_modal_input(frame, chunks[1], "test.ts spec.ts", true, true, None);
    } else {
        render_modal_input(
            frame,
            chunks[1],
            app.file_filter_query.clone(),
            false,
            true,
            None,
        );
    }

    let footer = Paragraph::new(Text::from(vec![
        Line::from(Span::styled(
            app.file_filter_preview_message(),
            Style::new().fg(text_muted_color()),
        )),
        Line::from(Span::styled(
            "Space or comma separated. Enter applies. Esc cancels.",
            Style::new().fg(text_muted_color()),
        )),
    ]))
    .style(Style::new().bg(panel_color()))
    .block(Block::new().padding(Padding::horizontal(1)));
    frame.render_widget(footer, chunks[2]);
}
