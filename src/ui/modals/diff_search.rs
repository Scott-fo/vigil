use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Padding, Paragraph},
};

use crate::{
    app::App,
    git::{DiffSearchLineKind, DiffSearchResult, DiffSearchSyntaxRange},
};

use super::super::{
    diff_added_style, diff_context_color, diff_removed_style, panel_color, primary_color,
    selected_list_item_text_color, syntax_style, text_color, text_muted_color,
};
use super::frame::render_modal_frame;
use super::list::{
    render_list_error, render_list_frame, render_list_message, render_modal_input,
    render_visible_list,
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
        let selected_index = app
            .diff_search_selected_index
            .min(app.diff_search_results.items.len().saturating_sub(1));
        render_visible_list(
            frame,
            list_inner,
            app.diff_search_results.items.len(),
            selected_index,
            |display_index, selected| {
                render_diff_search_result(&app.diff_search_results.items[display_index], selected)
            },
        );
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

fn diff_search_loading_message(app: &App) -> &'static str {
    if app.diff_search_query.trim().is_empty() {
        "Indexing changed diff lines..."
    } else {
        "Searching changed diff lines..."
    }
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

    let marker = match result.kind {
        DiffSearchLineKind::Addition => "+",
        DiffSearchLineKind::Deletion => "-",
        DiffSearchLineKind::Context => " ",
    };
    let line_number = result
        .new_line
        .or(result.old_line)
        .map(|line| line.to_string())
        .unwrap_or_else(|| "-".to_string());

    let mut spans = vec![
        Span::styled(format!("{marker} "), marker_style),
        Span::styled(
            format!("{}:{}  ", result.file_path, line_number),
            base_style.add_modifier(Modifier::BOLD),
        ),
    ];
    spans.extend(line_spans(result, base_style, selected));
    Line::from(spans).style(base_style)
}

fn diff_search_line_style(kind: DiffSearchLineKind) -> Style {
    match kind {
        DiffSearchLineKind::Addition => diff_added_style(),
        DiffSearchLineKind::Deletion => diff_removed_style(),
        DiffSearchLineKind::Context => Style::new().fg(text_color()),
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
