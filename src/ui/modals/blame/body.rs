use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span, Text},
    widgets::{Block, Padding, Paragraph},
};

use crate::app::App;

use super::super::super::{text_color, text_faint_color};
use super::super::list::render_quiet_scrollbar;

pub(super) fn render_blame_body(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(details) = app.blame_details.as_ref() else {
        return;
    };

    let body_lines = details
        .description
        .lines()
        .map(|line| {
            Line::from(Span::styled(
                if line.is_empty() {
                    " ".to_string()
                } else {
                    line.to_string()
                },
                Style::new().fg(text_color()),
            ))
        })
        .collect::<Vec<_>>();
    let viewport_height = area.height as usize;
    let max_scroll = body_lines.len().saturating_sub(viewport_height);
    if app.blame_scroll as usize > max_scroll {
        app.blame_scroll = max_scroll as u16;
    }
    let visible_start = app.blame_scroll as usize;
    let visible_end = (visible_start + viewport_height).min(body_lines.len());
    let visible_lines = if body_lines.is_empty() {
        vec![Line::from(Span::styled(
            "No commit description.",
            Style::new().fg(text_faint_color()),
        ))]
    } else {
        body_lines[visible_start..visible_end].to_vec()
    };
    frame.render_widget(
        Paragraph::new(Text::from(visible_lines))
            .block(Block::new().padding(Padding::horizontal(1))),
        area,
    );
    render_quiet_scrollbar(frame, area, body_lines.len(), visible_start);
}
