use ratatui::layout::{Position, Rect};

use crate::{
    app::{ActivePane, App},
    sidebar::SidebarItem,
};

use super::super::layout::ScreenLayout;

pub fn sidebar_file_at(
    app: &App,
    mouse_column: u16,
    mouse_row: u16,
    terminal_width: u16,
    terminal_height: u16,
) -> Option<String> {
    let item_index = sidebar_item_index_at(
        app,
        mouse_column,
        mouse_row,
        terminal_width,
        terminal_height,
    )?;
    let item = app.sidebar_items.get(item_index)?;

    match item {
        SidebarItem::File { file, .. } => Some(file.path.clone()),
        SidebarItem::Header { .. } => None,
    }
}

pub fn sidebar_item_index_at(
    app: &App,
    mouse_column: u16,
    mouse_row: u16,
    terminal_width: u16,
    terminal_height: u16,
) -> Option<usize> {
    if app.show_splash() || app.sidebar_hidden {
        return None;
    }

    let sidebar = ScreenLayout::new(
        Rect::new(0, 0, terminal_width, terminal_height),
        app.sidebar_hidden,
    )
    .sidebar?;
    let sidebar_inner = sidebar.list;
    let point = Position::new(mouse_column, mouse_row);

    if !sidebar_inner.contains(point) {
        return None;
    }

    let viewport_height = sidebar_inner.height as usize;
    let max_scroll = app.sidebar_items.len().saturating_sub(viewport_height);
    let visible_start = app.sidebar_scroll.min(max_scroll);
    let relative_row = mouse_row.saturating_sub(sidebar_inner.y) as usize;
    let item_index = visible_start.saturating_add(relative_row);
    app.sidebar_items.get(item_index)?;
    Some(item_index)
}

pub fn hovered_pane_at(
    app: &App,
    mouse_column: u16,
    mouse_row: u16,
    terminal_width: u16,
    terminal_height: u16,
) -> Option<ActivePane> {
    if app.show_splash() {
        return None;
    }

    let layout = ScreenLayout::new(
        Rect::new(0, 0, terminal_width, terminal_height),
        app.sidebar_hidden,
    );
    let point = Position::new(mouse_column, mouse_row);

    if layout
        .sidebar
        .is_some_and(|sidebar| sidebar.area.contains(point))
    {
        Some(ActivePane::Sidebar)
    } else if layout.diff.area.contains(point) {
        Some(ActivePane::Diff)
    } else {
        None
    }
}
