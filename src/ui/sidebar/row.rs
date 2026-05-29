use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::ListItem,
};

use crate::{git, sidebar::SidebarItem};

use super::{
    super::{add_bg_color, border_active_color, border_color, text_color, text_muted_color},
    text::{devicon_for_path, file_label_width, sidebar_status_label, status_gap, truncate_middle},
};

pub(super) fn list_item(item: &SidebarItem, row_width: u16) -> ListItem<'static> {
    match item {
        SidebarItem::Header {
            label,
            depth,
            collapsed,
            matches_search,
            ..
        } => header_item(label, *depth, *collapsed, *matches_search),
        SidebarItem::File {
            file,
            label,
            depth,
            matches_search,
            ..
        } => file_item(file, label, *depth, *matches_search, row_width),
    }
}

fn header_item(
    label: &str,
    depth: usize,
    collapsed: bool,
    matches_search: bool,
) -> ListItem<'static> {
    let indent = "  ".repeat(depth);
    let arrow = if collapsed { "▸ " } else { "▾ " };
    let label_style = if matches_search {
        Style::new().fg(text_color()).add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(text_muted_color())
    };

    ListItem::new(Line::from(vec![
        Span::styled(indent, Style::new().fg(text_muted_color())),
        Span::styled(arrow, Style::new().fg(border_active_color())),
        Span::styled(label.to_string(), label_style),
    ]))
}

fn file_item(
    file: &git::FileEntry,
    label: &str,
    depth: usize,
    matches_search: bool,
    row_width: u16,
) -> ListItem<'static> {
    let indent = "  ".repeat(depth);
    let staged = git::is_file_staged(&file.status);
    let row_style = if staged {
        Style::new().bg(add_bg_color())
    } else {
        Style::new()
    };
    let status_color = git::status_color(&file.status);
    let label_style = Style::new().fg(status_color).add_modifier(
        matches_search
            .then_some(Modifier::BOLD)
            .unwrap_or(Modifier::empty()),
    );
    let (indicator, indicator_style) = devicon_for_path(&file.path)
        .map(|(icon, color)| (format!("{icon} "), Style::new().fg(color)))
        .unwrap_or_else(|| (format!("{} ", file.status), Style::new().fg(status_color)));
    let status = sidebar_status_label(&file.status);
    let label_width = file_label_width(row_width, &indent, &indicator, &status);
    let display_label = truncate_middle(label, label_width);
    let status_gap = status_gap(row_width, &indent, &indicator, &display_label, &status);
    let status_style = if staged {
        Style::new().fg(status_color).add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(status_color)
    };

    ListItem::new(Line::from(vec![
        Span::styled(indent, Style::new().fg(border_color())),
        Span::styled(indicator, indicator_style),
        Span::styled(display_label, label_style),
        Span::raw(status_gap),
        Span::styled(status, status_style),
        Span::raw(" "),
    ]))
    .style(row_style)
}
