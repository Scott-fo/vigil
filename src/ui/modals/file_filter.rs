use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::Style,
    text::{Line, Span, Text},
    widgets::{Block, Padding, Paragraph},
};

use crate::app::App;

use super::super::{text_color, text_faint_color, text_subtle_color};
use super::frame::render_modal_frame;
use super::hints::render_hint_footer;
use super::prompt::{Prompt, render_prompt};

pub(super) fn render_file_filter_modal(frame: &mut Frame, app: &App) {
    let inner = render_modal_frame(frame, 78, 10, "Hide files");

    let [intro, _, prompt, _, preview, footer] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .areas(inner);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "Hide paths ending with these suffixes",
                Style::new().fg(text_color()),
            ),
            Span::styled(
                "  space or comma separated",
                Style::new().fg(text_faint_color()),
            ),
        ]))
        .block(Block::new().padding(Padding::horizontal(1))),
        intro,
    );

    render_prompt(
        frame,
        prompt,
        Prompt::new(&app.file_filter_query, "test.ts spec.ts"),
    );

    frame.render_widget(
        Paragraph::new(Text::from(Line::from(Span::styled(
            app.file_filter_preview_message(),
            Style::new().fg(text_subtle_color()),
        ))))
        .block(Block::new().padding(Padding::horizontal(1))),
        preview,
    );

    render_hint_footer(frame, footer, &[("⏎", "apply"), ("esc", "cancel")], None);
}
