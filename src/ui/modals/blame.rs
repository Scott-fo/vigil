use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
};

use crate::app::App;

use super::frame::render_modal_frame;
use super::hints::render_hint_footer;

mod body;
mod hint;
mod status;

use self::{body::render_blame_body, hint::blame_hints, status::render_blame_status};

pub(super) fn render_blame_modal(frame: &mut Frame, app: &mut App) {
    let title = app
        .blame_target
        .as_ref()
        .map(|target| format!("Blame {}:{}", target.file_path, target.line_number))
        .unwrap_or_else(|| "Blame".to_string());
    let inner = render_modal_frame(frame, 86, 18, title);

    let [subject, meta, _, body, _, footer] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(inner);

    render_blame_status(frame, app, subject, meta);
    render_blame_body(frame, app, body);

    let (hints, status) = blame_hints(app);
    render_hint_footer(frame, footer, hints, status.map(str::to_string));
}
