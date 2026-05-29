use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Padding, Paragraph},
};

use crate::app::App;

use super::super::{panel_color, text_muted_color};
use super::frame::render_modal_frame;

mod body;
mod hint;
mod status;

use self::{body::render_blame_body, hint::blame_hint, status::render_blame_status};

pub(super) fn render_blame_modal(frame: &mut Frame, app: &mut App) {
    let title = app
        .blame_target
        .as_ref()
        .map(|target| format!("Blame {}:{}", target.file_path, target.line_number))
        .unwrap_or_else(|| "Blame".to_string());
    let inner = render_modal_frame(frame, 86, 20, title);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Min(6),
            Constraint::Length(2),
        ])
        .split(inner);

    render_blame_status(frame, app, chunks[0], chunks[1]);
    render_blame_body(frame, app, chunks[2]);

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            blame_hint(app),
            Style::new().fg(text_muted_color()),
        )))
        .style(Style::new().bg(panel_color()))
        .block(Block::new().padding(Padding::horizontal(1))),
        chunks[3],
    );
}
