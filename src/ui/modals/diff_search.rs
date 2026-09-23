use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::Paragraph,
};

use crate::{
    app::App,
    git::{DiffSearchLineKind, DiffSearchResult, DiffSearchSyntaxRange},
};

use super::super::{
    error_color, primary_color, selected_list_item_text_color, success_color, syntax_style,
    text_color, text_faint_color, text_subtle_color,
};
use super::frame::render_modal_frame;
use super::hints::render_hint_footer;
use super::list::{
    PickerLayout, list_row, render_list_error, render_list_message, render_quiet_scrollbar,
    visible_list_range,
};
use super::prompt::{Prompt, render_prompt};

pub(super) fn render_diff_search_modal(frame: &mut Frame, app: &App) {
    let inner = render_modal_frame(frame, 104, 24, "Search diff");
    let layout = PickerLayout::new(inner, 1);

    render_prompt(
        frame,
        layout.prompt,
        Prompt::new(&app.diff_search_query, "Search changed diff lines"),
    );

    let list = layout.list;
    if let Some(error) = app.diff_search_error.as_ref() {
        render_list_error(frame, list, "Diff search failed", error);
    } else if app.diff_search_loading {
        render_list_message(frame, list, diff_search_loading_message(app));
    } else if app.diff_search_query.trim().is_empty() {
        render_list_message(frame, list, "Type a query to search changed lines.");
    } else if app.diff_search_results.items.is_empty() && app.diff_search_is_indexing_partial() {
        render_list_message(frame, list, app.diff_search_partial_loading_message());
    } else if app.diff_search_results.items.is_empty() {
        render_list_message(frame, list, "No matching diff lines.");
    } else {
        render_diff_search_results(frame, list, app);
    }

    render_hint_footer(
        frame,
        layout.footer,
        &[
            ("tab", "mode"),
            ("j/k", "select"),
            ("⏎", "jump"),
            ("esc", "close"),
        ],
        Some(format!("{} search", app.diff_search_mode.label())),
    );
}

fn render_diff_search_results(frame: &mut Frame, area: Rect, app: &App) {
    let selected_index = app
        .diff_search_selected_index
        .min(app.diff_search_results.items.len().saturating_sub(1));
    render_result_list(frame, area, &app.diff_search_results.items, selected_index);
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

    frame.render_widget(Paragraph::new(Text::from(lines)), area);
    render_quiet_scrollbar(frame, area, display_entries.len(), visible_range.start);
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

fn diff_search_loading_message(app: &App) -> &'static str {
    if app.diff_search_is_indexing_partial() && !app.diff_search_query.trim().is_empty() {
        return app.diff_search_partial_loading_message();
    }
    if app.diff_search_query.trim().is_empty() {
        "Indexing changed diff lines..."
    } else {
        "Searching changed diff lines..."
    }
}

fn render_diff_search_file_header(result: &DiffSearchResult) -> Line<'static> {
    let (file_name, parent) = file_name_and_parent(&result.file_path);
    let mut spans = vec![Span::raw("  ")];
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
            Style::new().fg(text_subtle_color()),
        ));
    }
    Line::from(spans)
}

/// Result rows indent under their file header. The +/- marker carries the
/// change kind; rows stay on the modal background so the cursor tint and
/// match highlights read clearly.
fn render_diff_search_result(result: &DiffSearchResult, selected: bool) -> Line<'static> {
    let base_style = Style::new().fg(text_color());
    let marker_style = match result.kind {
        DiffSearchLineKind::Addition => Style::new().fg(success_color()),
        DiffSearchLineKind::Deletion => Style::new().fg(error_color()),
        DiffSearchLineKind::Context => Style::new().fg(text_faint_color()),
    }
    .add_modifier(Modifier::BOLD);

    let mut spans = vec![
        Span::raw("  "),
        Span::styled(
            format!("{:>9}  ", line_range_label(result)),
            Style::new().fg(text_faint_color()),
        ),
        Span::styled(format!("{} ", diff_marker(result.kind)), marker_style),
    ];
    spans.extend(line_spans(result, base_style));
    list_row(spans, selected)
}

fn diff_marker(kind: DiffSearchLineKind) -> &'static str {
    match kind {
        DiffSearchLineKind::Addition => "+",
        DiffSearchLineKind::Deletion => "-",
        DiffSearchLineKind::Context => " ",
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

fn line_spans(result: &DiffSearchResult, base_style: Style) -> Vec<Span<'static>> {
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
        let style = diff_search_segment_style(base_style, syntax_name, matched);
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
        hex_color(icon.color).unwrap_or_else(text_subtle_color),
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

fn diff_search_segment_style(base_style: Style, syntax_name: Option<&str>, matched: bool) -> Style {
    let mut style = syntax_name
        .map(|name| syntax_style(name, base_style))
        .unwrap_or(base_style);

    if matched {
        style = style
            .fg(selected_list_item_text_color())
            .bg(primary_color())
            .add_modifier(Modifier::BOLD);
    }

    style
}
