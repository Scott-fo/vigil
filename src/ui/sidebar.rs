use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, ListState, Scrollbar, ScrollbarOrientation, ScrollbarState},
};
use unicode_width::UnicodeWidthChar;

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

    app.sidebar_viewport_height = inner.height as usize;
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

    let items: Vec<ListItem> = app
        .sidebar_items
        .iter()
        .skip(visible_start)
        .take(visible_end.saturating_sub(visible_start))
        .map(|item| match item {
            SidebarItem::Header {
                label,
                depth,
                collapsed,
                matches_search,
                ..
            } => {
                let indent = "  ".repeat(*depth);
                let arrow = if *collapsed { "▸ " } else { "▾ " };
                let label_style = if *matches_search {
                    Style::new().fg(text_color()).add_modifier(Modifier::BOLD)
                } else {
                    Style::new().fg(text_muted_color())
                };
                ListItem::new(Line::from(vec![
                    Span::styled(indent, Style::new().fg(text_muted_color())),
                    Span::styled(arrow, Style::new().fg(border_active_color())),
                    Span::styled(label.clone(), label_style),
                ]))
            }
            SidebarItem::File {
                file,
                label,
                depth,
                matches_search,
                ..
            } => {
                let indent = "  ".repeat(*depth);
                let staged = git::is_file_staged(&file.status);
                let row_style = if staged {
                    Style::new().bg(add_bg_color())
                } else {
                    Style::new()
                };
                let status_color = git::status_color(&file.status);
                let label_style = Style::new().fg(status_color).add_modifier(
                    (*matches_search)
                        .then_some(Modifier::BOLD)
                        .unwrap_or(Modifier::empty()),
                );
                let (indicator, indicator_style) = devicon_for_path(&file.path)
                    .map(|(icon, color)| (format!("{icon} "), Style::new().fg(color)))
                    .unwrap_or_else(|| {
                        (format!("{} ", file.status), Style::new().fg(status_color))
                    });
                let status = sidebar_status_label(&file.status);
                let label_width =
                    file_label_width(inner.width.saturating_sub(1), &indent, &indicator, &status);
                let display_label = truncate_middle(label, label_width);
                let status_gap = status_gap(
                    inner.width.saturating_sub(1),
                    &indent,
                    &indicator,
                    &display_label,
                    &status,
                );
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
        })
        .collect();

    let item_count = items.len();
    let list = List::new(items)
        .highlight_style(
            Style::new()
                .bg(primary_color())
                .fg(selected_list_item_text_color())
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("");

    let selected_row =
        (app.selected_sidebar_row < app.sidebar_items.len()).then_some(app.selected_sidebar_row);
    let mut list_state = ListState::default();
    list_state.select(selected_row.and_then(|row| {
        row.checked_sub(visible_start)
            .filter(|relative_row| *relative_row < item_count)
    }));
    frame.render_stateful_widget(list, inner, &mut list_state);

    let sidebar_height = inner.height.saturating_sub(1) as usize;
    let mut scrollbar_state = ScrollbarState::new(app.sidebar_items.len())
        .position(visible_start)
        .viewport_content_length(sidebar_height);
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .thumb_style(Style::new().fg(border_active_color()))
        .track_style(Style::new().fg(border_color()));
    frame.render_stateful_widget(scrollbar, inner, &mut scrollbar_state);
}

fn devicon_for_path(path: &str) -> Option<(char, Color)> {
    let icon = devicons::icon_for_file(path, &Some(devicons::Theme::Dark));
    if icon.icon == '*' {
        return None;
    }

    Some((
        icon.icon,
        hex_color(icon.color).unwrap_or_else(text_muted_color),
    ))
}

fn hex_color(value: &str) -> Option<Color> {
    let hex = value.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }

    let red = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let green = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let blue = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Color::Rgb(red, green, blue))
}

fn sidebar_status_label(status: &str) -> String {
    if status == "??" {
        return "A".to_string();
    }

    for marker in ['D', 'A', 'M', 'R', 'C', 'U'] {
        if status.contains(marker) {
            return marker.to_string();
        }
    }

    status.trim().to_string()
}

fn file_label_width(width: u16, indent: &str, indicator: &str, status: &str) -> usize {
    let reserved_width = display_width(indent)
        .saturating_add(display_width(indicator))
        .saturating_add(display_width(status))
        .saturating_add(2);
    (width as usize).saturating_sub(reserved_width).max(1)
}

fn truncate_middle(value: &str, max_width: usize) -> String {
    if display_width(value) <= max_width {
        return value.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    if max_width == 1 {
        return "…".to_string();
    }

    let left_width = max_width.saturating_sub(1) / 2;
    let right_width = max_width.saturating_sub(1).saturating_sub(left_width);
    format!(
        "{}…{}",
        take_width_prefix(value, left_width),
        take_width_suffix(value, right_width)
    )
}

fn take_width_prefix(value: &str, max_width: usize) -> String {
    let mut width = 0usize;
    let mut result = String::new();
    for char in value.chars() {
        let char_width = char.width().unwrap_or(0);
        if width.saturating_add(char_width) > max_width {
            break;
        }
        width = width.saturating_add(char_width);
        result.push(char);
    }
    result
}

fn take_width_suffix(value: &str, max_width: usize) -> String {
    let mut width = 0usize;
    let mut chars = Vec::new();
    for char in value.chars().rev() {
        let char_width = char.width().unwrap_or(0);
        if width.saturating_add(char_width) > max_width {
            break;
        }
        width = width.saturating_add(char_width);
        chars.push(char);
    }
    chars.into_iter().rev().collect()
}

fn status_gap(width: u16, indent: &str, indicator: &str, label: &str, status: &str) -> String {
    if status.is_empty() {
        return String::new();
    }

    let occupied_width = display_width(indent)
        .saturating_add(display_width(indicator))
        .saturating_add(display_width(label))
        .saturating_add(display_width(status))
        .saturating_add(1);
    let row_width = width as usize;
    let gap_width = row_width.saturating_sub(occupied_width).max(1);
    " ".repeat(gap_width)
}

fn display_width(value: &str) -> usize {
    value
        .chars()
        .map(|char| char.width().unwrap_or(0))
        .sum::<usize>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn devicon_resolves_known_types_and_skips_default_icon() {
        let tsx = devicon_for_path("src/components/Button.tsx").expect("tsx should have a devicon");
        assert_ne!(tsx.0, '*');

        assert!(devicon_for_path("src/components/Button.unknown-vigil-type").is_none());
    }

    #[test]
    fn parses_devicon_hex_colors() {
        assert_eq!(hex_color("#1354BF"), Some(Color::Rgb(19, 84, 191)));
        assert_eq!(hex_color("1354BF"), None);
        assert_eq!(hex_color("#1354"), None);
    }

    #[test]
    fn sidebar_status_label_normalizes_git_status_columns() {
        assert_eq!(sidebar_status_label(" M"), "M");
        assert_eq!(sidebar_status_label("M "), "M");
        assert_eq!(sidebar_status_label("MM"), "M");
        assert_eq!(sidebar_status_label("A "), "A");
        assert_eq!(sidebar_status_label("??"), "A");
        assert_eq!(sidebar_status_label(" D"), "D");
    }

    #[test]
    fn truncate_middle_keeps_extension_visible() {
        assert_eq!(
            truncate_middle("JavaScriptSyntaxHighlighter.tsx", 18),
            "JavaScri…ghter.tsx"
        );
        assert_eq!(truncate_middle("short.ts", 18), "short.ts");
        assert_eq!(truncate_middle("short.ts", 1), "…");
    }

    #[test]
    fn status_gap_keeps_status_visible() {
        let (tsx_icon, _) =
            devicon_for_path("file.tsx").expect("tsx should have a devicon for width tests");
        let indicator = format!("{tsx_icon} ");

        assert!(!status_gap(12, "", &indicator, "file.tsx", "M").is_empty());
        assert_eq!(status_gap(4, "", &indicator, "file.tsx", "M"), " ");
        assert_eq!(status_gap(12, "", &indicator, "file.tsx", ""), "");
    }

    #[test]
    fn file_label_width_reserves_status_gap_and_trailing_space() {
        let (tsx_icon, _) =
            devicon_for_path("file.tsx").expect("tsx should have a devicon for width tests");
        let indicator = format!("{tsx_icon} ");
        let label_width = file_label_width(24, "", &indicator, "M");
        let label = truncate_middle("JavaScriptSyntaxHighlighter.tsx", label_width);
        let gap = status_gap(24, "", &indicator, &label, "M");
        let rendered_width = display_width(&indicator)
            + display_width(&label)
            + display_width(&gap)
            + display_width("M")
            + 1;

        assert!(display_width(&label) <= label_width);
        assert_eq!(rendered_width, 24);
    }
}
