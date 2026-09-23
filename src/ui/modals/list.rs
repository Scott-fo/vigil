use std::ops::Range;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span, Text},
    widgets::{Block, Padding, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};

use super::super::{
    diff::QUIET_SCROLLBAR, error_color, primary_color, selection_color, text_faint_color,
    text_subtle_color,
};

const ACCENT_BAR: &str = "▎";

/// Rows of a picker modal: query prompt(s) on top, the list, then a hint
/// footer, separated by single blank rows.
pub(super) struct PickerLayout {
    pub(super) prompt: Rect,
    pub(super) list: Rect,
    pub(super) footer: Rect,
}

impl PickerLayout {
    /// `prompt_rows` is the height of the prompt block (1 per input field,
    /// plus any extra option rows the modal shows above the list).
    pub(super) fn new(area: Rect, prompt_rows: u16) -> Self {
        let [prompt, _, list, _, footer] = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(prompt_rows),
                Constraint::Length(1),
                Constraint::Min(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .areas(area);
        Self {
            prompt,
            list,
            footer,
        }
    }
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

/// Wraps row content with the shared list cursor treatment: a primary accent
/// bar and selection tint on the cursor row, a blank gutter elsewhere. Matches
/// the sidebar so every list in vigil selects the same way.
pub(super) fn list_row(content: Vec<Span<'static>>, selected: bool) -> Line<'static> {
    let gutter = if selected {
        Span::styled(ACCENT_BAR, Style::new().fg(primary_color()))
    } else {
        Span::raw(" ")
    };
    let mut spans = Vec::with_capacity(content.len() + 2);
    spans.push(gutter);
    spans.push(Span::raw(" "));
    spans.extend(content);
    let line = Line::from(spans);
    if selected {
        line.style(Style::new().bg(selection_color()))
    } else {
        line
    }
}

pub(super) fn render_list_message(frame: &mut Frame, area: Rect, message: impl Into<String>) {
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            message.into(),
            Style::new().fg(text_faint_color()),
        )))
        .block(Block::new().padding(Padding::horizontal(2))),
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
                Style::new().fg(text_subtle_color()),
            )),
        ]))
        .block(Block::new().padding(Padding::horizontal(2))),
        area,
    );
}

/// Renders the window of rows around the selection. `row_content` returns the
/// row's spans; the cursor treatment is applied here.
pub(super) fn render_visible_list<F>(
    frame: &mut Frame,
    area: Rect,
    item_count: usize,
    selected_index: usize,
    mut row_content: F,
) -> Range<usize>
where
    F: FnMut(usize, bool) -> Vec<Span<'static>>,
{
    let viewport_height = area.height as usize;
    let selected_index = selected_index.min(item_count.saturating_sub(1));
    let visible_range = visible_list_range(item_count, selected_index, viewport_height);
    let lines = visible_range
        .clone()
        .map(|display_index| {
            let selected = display_index == selected_index;
            list_row(row_content(display_index, selected), selected)
        })
        .collect::<Vec<_>>();

    frame.render_widget(Paragraph::new(Text::from(lines)), area);
    render_quiet_scrollbar(frame, area, item_count, visible_range.start);

    visible_range
}

pub(super) fn render_quiet_scrollbar(
    frame: &mut Frame,
    area: Rect,
    content_length: usize,
    position: usize,
) {
    let viewport_height = area.height as usize;
    if content_length <= viewport_height {
        return;
    }
    let mut scrollbar_state = ScrollbarState::new(content_length)
        .position(position)
        .viewport_content_length(viewport_height);
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .symbols(QUIET_SCROLLBAR)
            .begin_symbol(None)
            .end_symbol(None)
            .thumb_style(Style::new().fg(text_faint_color())),
        area,
        &mut scrollbar_state,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn only_the_cursor_row_gets_the_accent_and_tint() {
        let selected = list_row(vec![Span::raw("main.rs")], true);
        let plain = list_row(vec![Span::raw("main.rs")], false);

        assert_eq!(selected.spans[0].content, ACCENT_BAR);
        assert!(selected.style.bg.is_some());
        assert_eq!(plain.spans[0].content, " ");
        assert!(plain.style.bg.is_none());
        assert_eq!(selected.width(), plain.width());
    }
}
