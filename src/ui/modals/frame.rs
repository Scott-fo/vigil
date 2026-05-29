use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear},
};

use super::super::layout::centered_rect;
use super::super::{border_active_color, error_color, panel_color, text_color};

pub(super) fn render_modal_frame(
    frame: &mut Frame,
    width: u16,
    height: u16,
    title: impl AsRef<str>,
) -> Rect {
    render_modal_frame_with_colors(
        frame,
        width,
        height,
        title,
        border_active_color(),
        text_color(),
    )
}

pub(super) fn render_danger_modal_frame(
    frame: &mut Frame,
    width: u16,
    height: u16,
    title: impl AsRef<str>,
) -> Rect {
    render_modal_frame_with_colors(frame, width, height, title, error_color(), error_color())
}

fn render_modal_frame_with_colors(
    frame: &mut Frame,
    width: u16,
    height: u16,
    title: impl AsRef<str>,
    border_color: Color,
    title_color: Color,
) -> Rect {
    let area = centered_rect(width, height, frame.area());
    frame.render_widget(Clear, area);

    let title = format!(" {} ", title.as_ref());
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(border_color))
        .style(Style::new().bg(panel_color()))
        .title(Line::from(Span::styled(
            title,
            Style::new().fg(title_color).add_modifier(Modifier::BOLD),
        )));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    inner
}
