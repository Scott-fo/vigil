use std::ops::Range;

use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, Padding, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
    },
};

use super::super::{
    border_active_color, border_color, element_color, error_color, panel_color, primary_color,
    text_color, text_muted_color,
};

pub(super) fn render_modal_input(
    frame: &mut Frame,
    area: Rect,
    display: impl Into<String>,
    muted: bool,
    active: bool,
    title: Option<&str>,
) {
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(if active {
            primary_color()
        } else {
            border_color()
        }))
        .padding(Padding::horizontal(1));
    if let Some(title) = title {
        block = block.title(title.to_string());
    }

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            display.into(),
            Style::new().fg(if muted {
                text_muted_color()
            } else {
                text_color()
            }),
        )))
        .style(Style::new().bg(element_color()))
        .block(block),
        area,
    );
}

pub(super) fn visible_list_range(
    item_count: usize,
    selected_index: usize,
    viewport_height: usize,
) -> Range<usize> {
    if item_count == 0 || viewport_height == 0 {
        return 0..0;
    }

    let selected_index = selected_index.min(item_count - 1);
    let max_scroll = item_count.saturating_sub(viewport_height);
    let start = selected_index
        .saturating_sub(viewport_height.saturating_sub(1))
        .min(max_scroll);
    let end = start.saturating_add(viewport_height).min(item_count);
    start..end
}

pub(super) fn render_list_frame(frame: &mut Frame, area: Rect) -> Rect {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(border_color()))
        .style(Style::new().bg(panel_color()));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    inner
}

pub(super) fn render_list_message(frame: &mut Frame, area: Rect, message: impl Into<String>) {
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            message.into(),
            Style::new().fg(text_muted_color()),
        )))
        .style(Style::new().bg(panel_color()))
        .block(Block::new().padding(Padding::horizontal(1))),
        area,
    );
}

pub(super) fn render_list_error(frame: &mut Frame, area: Rect, title: &str, error: &str) {
    frame.render_widget(
        Paragraph::new(Text::from(vec![
            Line::from(Span::styled(
                title.to_string(),
                Style::new().fg(error_color()),
            )),
            Line::default(),
            Line::from(Span::styled(
                error.to_string(),
                Style::new().fg(text_muted_color()),
            )),
        ]))
        .style(Style::new().bg(panel_color()))
        .block(Block::new().padding(Padding::horizontal(1))),
        area,
    );
}

pub(super) fn render_visible_list<F>(
    frame: &mut Frame,
    area: Rect,
    item_count: usize,
    selected_index: usize,
    mut line_for_index: F,
) -> Range<usize>
where
    F: FnMut(usize, bool) -> Line<'static>,
{
    let viewport_height = area.height as usize;
    let selected_index = selected_index.min(item_count.saturating_sub(1));
    let visible_range = visible_list_range(item_count, selected_index, viewport_height);
    let lines = visible_range
        .clone()
        .map(|display_index| line_for_index(display_index, display_index == selected_index))
        .collect::<Vec<_>>();

    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .style(Style::new().bg(panel_color()))
            .block(Block::new().padding(Padding::horizontal(1))),
        area,
    );

    if item_count > viewport_height {
        let mut scrollbar_state = ScrollbarState::new(item_count)
            .position(visible_range.start)
            .viewport_content_length(viewport_height);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .thumb_style(Style::new().fg(border_active_color()))
            .track_style(Style::new().fg(border_color()));
        frame.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
    }

    visible_range
}

#[cfg(test)]
mod tests {
    use super::visible_list_range;

    #[test]
    fn keeps_selected_row_visible_at_bottom_edge() {
        assert_eq!(visible_list_range(10, 5, 4), 2..6);
    }

    #[test]
    fn clamps_to_last_full_window_near_end() {
        assert_eq!(visible_list_range(10, 9, 4), 6..10);
    }

    #[test]
    fn handles_empty_or_zero_height_lists() {
        assert_eq!(visible_list_range(0, 0, 4), 0..0);
        assert_eq!(visible_list_range(10, 3, 0), 0..0);
    }
}
