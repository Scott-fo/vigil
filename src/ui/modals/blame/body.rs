use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, Padding, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
    },
};

use crate::app::App;

use super::super::super::{
    border_active_color, border_color, panel_color, text_color, text_muted_color,
};

pub(super) fn render_blame_body(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(details) = app.blame_details.as_ref() else {
        return;
    };

    let content_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(border_color()))
        .style(Style::new().bg(panel_color()));
    let content_inner = content_block.inner(area);
    frame.render_widget(content_block, area);

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
    let viewport_height = content_inner.height as usize;
    let max_scroll = body_lines.len().saturating_sub(viewport_height);
    if app.blame_scroll as usize > max_scroll {
        app.blame_scroll = max_scroll as u16;
    }
    let visible_start = app.blame_scroll as usize;
    let visible_end = (visible_start + viewport_height).min(body_lines.len());
    let visible_lines = if body_lines.is_empty() {
        vec![Line::from(Span::styled(
            "No commit description.",
            Style::new().fg(text_muted_color()),
        ))]
    } else {
        body_lines[visible_start..visible_end].to_vec()
    };
    frame.render_widget(
        Paragraph::new(Text::from(visible_lines))
            .style(Style::new().bg(panel_color()))
            .block(Block::new().padding(Padding::horizontal(1))),
        content_inner,
    );

    if body_lines.len() > viewport_height {
        let mut scrollbar_state = ScrollbarState::new(body_lines.len())
            .position(visible_start)
            .viewport_content_length(viewport_height);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .thumb_style(Style::new().fg(border_active_color()))
            .track_style(Style::new().fg(border_color()));
        frame.render_stateful_widget(scrollbar, content_inner, &mut scrollbar_state);
    }
}
