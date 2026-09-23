use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Padding, Paragraph},
};

use crate::app::App;

use super::super::{text_color, text_subtle_color};
use super::frame::render_danger_modal_frame;
use super::hints::render_hint_footer;

pub(super) fn render_discard_modal(frame: &mut Frame, app: &App) {
    let Some(file) = app.discard_target.as_ref() else {
        return;
    };

    let inner = render_danger_modal_frame(frame, 72, 8, "Discard changes?");
    let [body, footer] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .areas(inner);

    let directory = file
        .path
        .strip_suffix(file.label.as_str())
        .unwrap_or_default()
        .to_string();
    frame.render_widget(
        Paragraph::new(Text::from(vec![
            Line::from(Span::styled(
                "All local changes in this file will be lost.",
                Style::new().fg(text_subtle_color()),
            )),
            Line::default(),
            Line::from(vec![
                Span::styled(directory, Style::new().fg(text_subtle_color())),
                Span::styled(
                    file.label.clone(),
                    Style::new().fg(text_color()).add_modifier(Modifier::BOLD),
                ),
            ]),
        ]))
        .block(Block::new().padding(Padding::horizontal(1))),
        body,
    );
    render_hint_footer(frame, footer, &[("⏎", "discard"), ("esc", "cancel")], None);
}
