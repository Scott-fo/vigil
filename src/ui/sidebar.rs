use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, Scrollbar, ScrollbarOrientation, ScrollbarState},
};

use crate::{
    app::{ActivePane, App},
    git,
    sidebar::SidebarItem,
};

use super::{
    add_bg_color, border_active_color, border_color, bordered_panel, primary_color,
    selected_list_item_text_color, text_color, text_muted_color,
};

pub(super) fn render_sidebar(frame: &mut Frame, app: &mut App, area: Rect) {
    let block = bordered_panel(
        "Changed Files",
        app.active_pane == ActivePane::Sidebar,
        Some(format!("{}", app.files.len())),
    );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let items: Vec<ListItem> = app
        .sidebar_items
        .iter()
        .map(|item| match item {
            SidebarItem::Header {
                label,
                depth,
                collapsed,
                ..
            } => {
                let indent = "  ".repeat(*depth);
                let arrow = if *collapsed { "▸ " } else { "▾ " };
                ListItem::new(Line::from(vec![
                    Span::styled(indent, Style::new().fg(text_muted_color())),
                    Span::styled(arrow, Style::new().fg(border_active_color())),
                    Span::styled(label.clone(), Style::new().fg(text_muted_color())),
                ]))
            }
            SidebarItem::File {
                file, label, depth, ..
            } => {
                let indent = "  ".repeat(*depth);
                let staged = git::is_file_staged(&file.status);
                let row_style = if staged {
                    Style::new().bg(add_bg_color())
                } else {
                    Style::new()
                };
                ListItem::new(Line::from(vec![
                    Span::styled(indent, Style::new().fg(border_color())),
                    Span::styled(
                        format!("{} ", file.status),
                        Style::new().fg(git::status_color(&file.status)),
                    ),
                    Span::styled(
                        label.clone(),
                        if staged {
                            Style::new().fg(text_color())
                        } else {
                            Style::new().fg(text_muted_color())
                        },
                    ),
                ]))
                .style(row_style)
            }
        })
        .collect();

    let list = List::new(items)
        .highlight_style(
            Style::new()
                .bg(primary_color())
                .fg(selected_list_item_text_color())
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("");

    frame.render_stateful_widget(list, inner, &mut app.sidebar_state);

    let sidebar_height = inner.height.saturating_sub(1) as usize;
    let mut scrollbar_state = ScrollbarState::new(app.sidebar_items.len())
        .position(app.sidebar_state.offset())
        .viewport_content_length(sidebar_height);
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .thumb_style(Style::new().fg(border_active_color()))
        .track_style(Style::new().fg(border_color()));
    frame.render_stateful_widget(scrollbar, inner, &mut scrollbar_state);
}
