use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, Padding, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
    },
};

use crate::{
    app::App,
    git::{DiffSearchLineKind, DiffSearchPreviewLine, DiffSearchResult, DiffSearchSyntaxRange},
};

use super::super::{
    border_active_color, border_color, diff_added_style, diff_context_color, diff_removed_style,
    panel_color, primary_color, selected_list_item_text_color, syntax_style, text_color,
    text_muted_color,
};
use super::frame::render_modal_frame;
use super::list::{
    render_list_error, render_list_frame, render_list_message, render_modal_input,
    visible_list_range,
};

pub(super) fn render_diff_search_modal(frame: &mut Frame, app: &App) {
    let inner = render_modal_frame(frame, 104, 24, "Diff Search");

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(2),
        ])
        .split(inner);

    if app.diff_search_query.is_empty() {
        render_modal_input(
            frame,
            chunks[0],
            "Search changed diff lines...",
            true,
            false,
            None,
        );
    } else {
        render_modal_input(
            frame,
            chunks[0],
            app.diff_search_query.clone(),
            false,
            false,
            None,
        );
    }

    let list_inner = render_list_frame(frame, chunks[1]);
    if let Some(error) = app.diff_search_error.as_ref() {
        render_list_error(frame, list_inner, "Diff search failed", error);
    } else if app.diff_search_loading {
        render_list_message(frame, list_inner, diff_search_loading_message(app));
    } else if app.diff_search_query.trim().is_empty() {
        render_list_message(frame, list_inner, "Type a query to search changed lines.");
    } else if app.diff_search_results.items.is_empty() {
        render_list_message(frame, list_inner, "No matching diff lines.");
    } else {
        render_diff_search_results(frame, list_inner, app);
    }

    let footer = Paragraph::new(Text::from(vec![
        Line::from(Span::styled(
            "j/k select. Enter jumps to line. Esc closes.",
            Style::new().fg(text_muted_color()),
        )),
        Line::from(Span::styled(
            format!("{} matches", app.diff_search_results.total_matched),
            Style::new().fg(diff_context_color()),
        )),
    ]))
    .style(Style::new().bg(panel_color()))
    .block(Block::new().padding(Padding::horizontal(1)));
    frame.render_widget(footer, chunks[2]);
}

fn render_diff_search_results(frame: &mut Frame, area: Rect, app: &App) {
    let selected_index = app
        .diff_search_selected_index
        .min(app.diff_search_results.items.len().saturating_sub(1));
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
        .split(area);

    render_result_list(
        frame,
        panes[0],
        &app.diff_search_results.items,
        selected_index,
    );
    render_hunk_preview(
        frame,
        panes[1],
        app.diff_search_results.items.get(selected_index),
    );
}

fn render_result_list(
    frame: &mut Frame,
    area: Rect,
    results: &[DiffSearchResult],
    selected_index: usize,
) {
    let display_entries = diff_search_display_entries(results);
    let selected_display_index = display_entries
        .iter()
        .position(|entry| matches!(entry, DiffSearchDisplayEntry::Result(index) if *index == selected_index))
        .unwrap_or(0);
    let viewport_height = (area.height as usize).max(1);
    let visible_range = visible_list_range(
        display_entries.len(),
        selected_display_index,
        viewport_height,
    );
    let mut lines = Vec::with_capacity(visible_range.len());
    for display_index in visible_range.clone() {
        match display_entries[display_index] {
            DiffSearchDisplayEntry::Header(index) => {
                lines.push(render_diff_search_file_header(&results[index]));
            }
            DiffSearchDisplayEntry::Result(index) => {
                lines.push(render_diff_search_result(
                    &results[index],
                    index == selected_index,
                ));
            }
        }
    }

    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .style(Style::new().bg(panel_color()))
            .block(Block::new().padding(Padding::horizontal(1))),
        area,
    );

    if display_entries.len() > viewport_height {
        let mut scrollbar_state = ScrollbarState::new(display_entries.len())
            .position(visible_range.start)
            .viewport_content_length(viewport_height);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .thumb_style(Style::new().fg(border_active_color()))
            .track_style(Style::new().fg(border_color()));
        frame.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffSearchDisplayEntry {
    Header(usize),
    Result(usize),
}

fn diff_search_display_entries(results: &[DiffSearchResult]) -> Vec<DiffSearchDisplayEntry> {
    let mut entries = Vec::with_capacity(results.len().saturating_mul(2));
    let mut previous_file: Option<&str> = None;
    for (index, result) in results.iter().enumerate() {
        if previous_file != Some(result.file_path.as_str()) {
            entries.push(DiffSearchDisplayEntry::Header(index));
            previous_file = Some(result.file_path.as_str());
        }
        entries.push(DiffSearchDisplayEntry::Result(index));
    }
    entries
}

fn render_hunk_preview(frame: &mut Frame, area: Rect, result: Option<&DiffSearchResult>) {
    let block = Block::default()
        .borders(Borders::LEFT)
        .border_style(Style::new().fg(border_color()))
        .style(Style::new().bg(panel_color()))
        .padding(Padding::horizontal(1));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Some(result) = result else {
        return;
    };

    let mut lines = vec![
        Line::from(vec![
            Span::styled(result.file_path.clone(), Style::new().fg(text_color())),
            Span::styled(
                format!(
                    "  @@ -{} +{} @@",
                    result.hunk_old_start, result.hunk_new_start
                ),
                Style::new().fg(diff_context_color()),
            ),
        ]),
        Line::default(),
    ];
    lines.extend(
        result
            .preview_lines
            .iter()
            .map(|line| render_preview_line(result, line)),
    );

    let max_lines = inner.height as usize;
    lines.truncate(max_lines);
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .style(Style::new().bg(panel_color()))
            .block(Block::new()),
        inner,
    );
}

fn diff_search_loading_message(app: &App) -> &'static str {
    if app.diff_search_query.trim().is_empty() {
        "Indexing changed diff lines..."
    } else {
        "Searching changed diff lines..."
    }
}

fn render_diff_search_file_header(result: &DiffSearchResult) -> Line<'static> {
    let (file_name, parent) = file_name_and_parent(&result.file_path);
    let mut spans = vec![Span::styled("  ", Style::new().fg(text_muted_color()))];
    if let Some((icon, color)) = devicon_for_path(&result.file_path) {
        spans.push(Span::styled(format!("{icon} "), Style::new().fg(color)));
    }
    spans.push(Span::styled(
        file_name,
        Style::new().fg(text_color()).add_modifier(Modifier::BOLD),
    ));
    if let Some(parent) = parent {
        spans.push(Span::styled(
            format!("  {parent}"),
            Style::new().fg(text_muted_color()),
        ));
    }
    Line::from(spans).style(Style::new().bg(panel_color()))
}

fn render_diff_search_result(result: &DiffSearchResult, selected: bool) -> Line<'static> {
    let base_style = if selected {
        Style::new()
            .bg(primary_color())
            .fg(selected_list_item_text_color())
    } else {
        diff_search_line_style(result.kind)
    };
    let marker_style = if selected {
        base_style.add_modifier(Modifier::BOLD)
    } else {
        match result.kind {
            DiffSearchLineKind::Addition => diff_added_style(),
            DiffSearchLineKind::Deletion => diff_removed_style(),
            DiffSearchLineKind::Context => Style::new().fg(diff_context_color()),
        }
    };

    let mut match_line = vec![
        Span::styled("  ", base_style),
        Span::styled(
            format!("{}  ", line_range_label(result)),
            muted_style_for_selection(selected),
        ),
        Span::styled(format!("{} ", diff_marker(result.kind)), marker_style),
    ];
    match_line.extend(line_spans(result, base_style, selected));

    Line::from(match_line).style(base_style)
}

fn diff_search_line_style(kind: DiffSearchLineKind) -> Style {
    match kind {
        DiffSearchLineKind::Addition => diff_added_style(),
        DiffSearchLineKind::Deletion => diff_removed_style(),
        DiffSearchLineKind::Context => Style::new().fg(text_color()),
    }
}

fn render_preview_line(
    result: &DiffSearchResult,
    preview: &DiffSearchPreviewLine,
) -> Line<'static> {
    let base_style = diff_search_line_style(preview.kind);
    let line_number = preview
        .new_line
        .or(preview.old_line)
        .map(|line| line.to_string())
        .unwrap_or_else(|| "-".to_string());
    let mut spans = vec![
        Span::styled(
            format!("{line_number:>4} "),
            base_style.patch(line_number_style()),
        ),
        Span::styled(
            format!("{} ", diff_marker(preview.kind)),
            diff_marker_style(preview.kind),
        ),
    ];

    if preview.is_match {
        spans.extend(line_spans(result, base_style, false));
    } else {
        spans.push(Span::styled(preview.line.clone(), base_style));
    }

    Line::from(spans).style(base_style)
}

fn line_number_style() -> Style {
    Style::new().fg(diff_context_color())
}

fn diff_marker(kind: DiffSearchLineKind) -> &'static str {
    match kind {
        DiffSearchLineKind::Addition => "+",
        DiffSearchLineKind::Deletion => "-",
        DiffSearchLineKind::Context => " ",
    }
}

fn diff_marker_style(kind: DiffSearchLineKind) -> Style {
    match kind {
        DiffSearchLineKind::Addition => diff_added_style(),
        DiffSearchLineKind::Deletion => diff_removed_style(),
        DiffSearchLineKind::Context => Style::new().fg(diff_context_color()),
    }
}

fn file_name_and_parent(path: &str) -> (String, Option<String>) {
    match path.rsplit_once('/') {
        Some((parent, name)) if !name.is_empty() => (name.to_string(), Some(parent.to_string())),
        _ => (path.to_string(), None),
    }
}

fn line_range_label(result: &DiffSearchResult) -> String {
    let line_number = result.new_line.or(result.old_line).unwrap_or_default();
    let Some(range) = result.match_ranges.first() else {
        return line_number.to_string();
    };
    let start = result.line[..range.start].chars().count() + 1;
    let end = result.line[..range.end].chars().count().max(start);
    format!("{line_number}:{start}-{end}")
}

fn muted_style_for_selection(selected: bool) -> Style {
    if selected {
        Style::new()
            .fg(selected_list_item_text_color())
            .bg(primary_color())
    } else {
        Style::new().fg(text_muted_color())
    }
}

fn line_spans(result: &DiffSearchResult, base_style: Style, selected: bool) -> Vec<Span<'static>> {
    if result.line.is_empty() {
        return vec![Span::styled(String::new(), base_style)];
    }

    let mut boundaries = vec![0, result.line.len()];
    push_range_boundaries(
        &result.line,
        result
            .match_ranges
            .iter()
            .map(|range| range.start..range.end),
        &mut boundaries,
    );
    push_range_boundaries(
        &result.line,
        result
            .syntax_ranges
            .iter()
            .map(|range| range.start..range.end),
        &mut boundaries,
    );
    boundaries.sort_unstable();
    boundaries.dedup();

    let mut spans = Vec::new();
    for window in boundaries.windows(2) {
        let start = window[0];
        let end = window[1];
        if start >= end {
            continue;
        }

        let syntax_name = syntax_name_for_segment(&result.syntax_ranges, start, end);
        let matched = result
            .match_ranges
            .iter()
            .any(|range| range.start < end && start < range.end);
        let style = diff_search_segment_style(base_style, syntax_name, matched, selected);
        spans.push(Span::styled(result.line[start..end].to_string(), style));
    }

    if spans.is_empty() {
        spans.push(Span::styled(result.line.clone(), base_style));
    }
    spans
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

fn push_range_boundaries<I>(line: &str, ranges: I, boundaries: &mut Vec<usize>)
where
    I: IntoIterator<Item = std::ops::Range<usize>>,
{
    for range in ranges {
        if range.start >= range.end || range.end > line.len() {
            continue;
        }
        if !line.is_char_boundary(range.start) || !line.is_char_boundary(range.end) {
            continue;
        }
        boundaries.push(range.start);
        boundaries.push(range.end);
    }
}

fn syntax_name_for_segment(
    ranges: &[DiffSearchSyntaxRange],
    start: usize,
    end: usize,
) -> Option<&'static str> {
    ranges
        .iter()
        .find(|range| range.start <= start && end <= range.end)
        .and_then(|range| range.highlight_name)
}

fn diff_search_segment_style(
    base_style: Style,
    syntax_name: Option<&str>,
    matched: bool,
    selected: bool,
) -> Style {
    let mut style = syntax_name
        .map(|name| syntax_style(name, base_style))
        .unwrap_or(base_style);

    if selected {
        style = style.bg(primary_color());
    }

    if matched {
        style = style
            .fg(selected_list_item_text_color())
            .bg(primary_color())
            .add_modifier(Modifier::BOLD);
    }

    style
}
