use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::Text,
    widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};

use crate::app::{ActivePane, App};

use super::{
    border_active_color, border_color, bordered_panel, diff_mode_label, highlight_line,
    highlight_line_range, panel_color, text_color,
};

use crate::ui::status::render_status_line;

pub(super) fn render_diff(frame: &mut Frame, app: &mut App, area: Rect) {
    let title = app
        .files
        .get(app.selected_file_index)
        .map(|file| file.label.clone())
        .unwrap_or_else(|| "No file selected".to_string());
    let mode_label = app.review_mode_label();
    let right_title = match app.active_pane {
        ActivePane::Sidebar => format!("{}  sidebar", diff_mode_label(app.diff_view_mode)),
        ActivePane::Diff => format!("{}  diff", diff_mode_label(app.diff_view_mode)),
    };
    let block = bordered_panel(
        &title,
        app.active_pane == ActivePane::Diff,
        Some(if mode_label.is_empty() {
            right_title
        } else {
            format!("{right_title}  {mode_label}")
        }),
    );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    render_diff_body(frame, app, chunks[0]);
    render_status_line(frame, app, chunks[1]);
}

fn render_diff_body(frame: &mut Frame, app: &mut App, area: Rect) {
    let diff_focused = app.active_pane == ActivePane::Diff;
    let mode = app.diff_view_mode;
    let Some(viewport) = app.prepare_diff_viewport(mode, area.width as usize, area.height as usize)
    else {
        let paragraph = Paragraph::new(Text::default())
            .style(Style::new().fg(text_color()).bg(panel_color()))
            .scroll((0, 0));
        frame.render_widget(paragraph, area);
        return;
    };
    app.update_diff_viewport(mode, viewport.width, viewport.start, viewport.end);
    let visible_lines = {
        let rendered_lines = app.diff_view.rendered_lines(mode, area.width as usize)
            [viewport.start..viewport.end]
            .to_vec();
        rendered_lines
            .iter()
            .enumerate()
            .map(|(offset, line)| {
                let display_index = viewport.start + offset;
                let mut rendered_line = line.clone();
                if let Some(selection) = app.diff_text_selection
                    && let Some((start, end)) = app.diff_view.selection_columns(
                        mode,
                        area.width as usize,
                        selection.anchor,
                        selection.head,
                        display_index,
                    )
                {
                    rendered_line = highlight_line_range(&rendered_line, start, end);
                }
                if diff_focused && display_index == viewport.selected_index {
                    highlight_line(&rendered_line)
                } else {
                    rendered_line
                }
            })
            .collect::<Vec<_>>()
    };
    let paragraph = Paragraph::new(Text::from(visible_lines))
        .style(Style::new().fg(text_color()).bg(panel_color()))
        .scroll((0, 0));
    frame.render_widget(paragraph, area);

    if viewport.rendered_line_count > area.height as usize {
        let mut scrollbar_state = ScrollbarState::new(viewport.rendered_line_count)
            .position(app.diff_scroll as usize)
            .viewport_content_length(area.height as usize);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .thumb_style(Style::new().fg(border_active_color()))
            .track_style(Style::new().fg(border_color()));
        frame.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
    }
}
