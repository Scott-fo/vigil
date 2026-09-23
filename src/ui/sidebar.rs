use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    symbols::scrollbar,
    text::{Line, Span},
    widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};

use crate::app::{ActivePane, App};

use super::{layout::SidebarLayout, rule_color, text_faint_color, text_subtle_color};

mod row;
mod text;

use self::row::{RowContext, RowSelection, row_line};

/// The divider is a thin rule; when the list overflows, a heavier segment of
/// the same rule marks the scroll position.
const DIVIDER_SCROLLBAR: scrollbar::Set = scrollbar::Set {
    track: "│",
    thumb: "┃",
    begin: "│",
    end: "│",
};

pub(super) fn render_sidebar(frame: &mut Frame, app: &mut App, layout: SidebarLayout) {
    render_title(frame, app, layout.title);

    let list = layout.list;
    app.sidebar_viewport_height = list.height as usize;
    let max_scroll = app
        .sidebar_items
        .len()
        .saturating_sub(app.sidebar_viewport_height);
    if app.sidebar_scroll > max_scroll {
        app.sidebar_scroll = max_scroll;
    }
    let visible_start = app.sidebar_scroll.min(max_scroll);
    let visible_end = visible_start
        .saturating_add(app.sidebar_viewport_height)
        .min(app.sidebar_items.len());

    let focused = app.active_pane == ActivePane::Sidebar;
    // Derived from the mouse position each frame, so hover stays on the row
    // under the pointer even after the list scrolls.
    let hovered_row = app
        .mouse_position
        .filter(|position| list.contains(*position))
        .map(|position| visible_start + (position.y - list.y) as usize);
    let row_width = list.width;
    for (offset, index) in (visible_start..visible_end).enumerate() {
        let item = &app.sidebar_items[index];
        let selection = match (index == app.selected_sidebar_row, focused) {
            (false, _) => RowSelection::None,
            (true, true) => RowSelection::Focused,
            (true, false) => RowSelection::Unfocused,
        };
        let context = RowContext {
            width: row_width,
            selection,
            hovered: hovered_row == Some(index),
            review_comment_count: item
                .file()
                .map(|file| app.review_comment_count_for_file(&file.path))
                .unwrap_or_default(),
            viewed: item
                .file()
                .is_some_and(|file| app.is_file_viewed(&file.path)),
        };
        let row_area = Rect::new(list.x, list.y + offset as u16, list.width, 1);
        frame.render_widget(Paragraph::new(row_line(item, context)), row_area);
    }

    render_divider(frame, app, layout, visible_start);
}

fn render_divider(frame: &mut Frame, app: &App, layout: SidebarLayout, visible_start: usize) {
    let divider = layout.divider;
    let rule = Style::new().fg(rule_color());
    for y in divider.top()..divider.bottom() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled("│", rule))),
            Rect::new(divider.x, y, 1, 1),
        );
    }

    let list = layout.list;
    if app.sidebar_items.len() <= list.height as usize {
        return;
    }
    // The thumb spans only the list rows so it tracks the list position.
    let mut scrollbar_state = ScrollbarState::new(app.sidebar_items.len())
        .position(visible_start)
        .viewport_content_length(list.height as usize);
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .symbols(DIVIDER_SCROLLBAR)
            .track_style(rule)
            .thumb_style(Style::new().fg(text_faint_color())),
        Rect::new(divider.x, list.y, 1, list.height),
        &mut scrollbar_state,
    );
}

fn render_title(frame: &mut Frame, app: &App, area: Rect) {
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "  CHANGES",
            Style::new()
                .fg(text_faint_color())
                .add_modifier(Modifier::BOLD),
        ))),
        area,
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!("{} ", app.files.len()),
            Style::new().fg(text_subtle_color()),
        )))
        .alignment(Alignment::Right),
        area,
    );
}
