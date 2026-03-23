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
    panel_color, text_color,
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
    let viewport_height = area.height as usize;
    let (visible_start, visible_end, rendered_line_count, visible_lines) = {
        let rendered_lines = app.diff_view.rendered_lines(mode, area.width as usize);
        let max_scroll = rendered_lines
            .len()
            .saturating_sub(viewport_height)
            .min(u16::MAX as usize) as u16;
        if app.diff_scroll > max_scroll {
            app.diff_scroll = max_scroll;
        }

        let selected_index = app
            .selected_diff_line_index
            .min(rendered_lines.len().saturating_sub(1));
        if diff_focused {
            if selected_index < app.diff_scroll as usize {
                app.diff_scroll = selected_index.min(max_scroll as usize) as u16;
            } else {
                let visible_end = (app.diff_scroll as usize).saturating_add(viewport_height);
                if viewport_height > 0 && selected_index >= visible_end {
                    app.diff_scroll = selected_index
                        .saturating_add(1)
                        .saturating_sub(viewport_height)
                        .min(max_scroll as usize) as u16;
                }
            }
        }

        let visible_start = (app.diff_scroll as usize).min(max_scroll as usize);
        let visible_end = (visible_start + viewport_height).min(rendered_lines.len());
        let visible_lines = rendered_lines[visible_start..visible_end]
            .iter()
            .enumerate()
            .map(|(offset, line)| {
                let display_index = visible_start + offset;
                if diff_focused && display_index == selected_index {
                    highlight_line(line)
                } else {
                    line.clone()
                }
            })
            .collect::<Vec<_>>();

        (
            visible_start,
            visible_end,
            rendered_lines.len(),
            visible_lines,
        )
    };
    app.update_diff_viewport(mode, area.width as usize, visible_start, visible_end);
    let paragraph = Paragraph::new(Text::from(visible_lines))
        .style(Style::new().fg(text_color()).bg(panel_color()))
        .scroll((0, 0));
    frame.render_widget(paragraph, area);

    if rendered_line_count > viewport_height {
        let mut scrollbar_state = ScrollbarState::new(rendered_line_count)
            .position(app.diff_scroll as usize)
            .viewport_content_length(viewport_height);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .thumb_style(Style::new().fg(border_active_color()))
            .track_style(Style::new().fg(border_color()));
        frame.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
    }
}
