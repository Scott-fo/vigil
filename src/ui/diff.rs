use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    symbols::scrollbar,
    text::{Line, Span, Text},
    widgets::{Block, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};

use crate::{
    app::{ActivePane, App},
    git,
};

use super::{
    highlight_line, highlight_line_range, layout::DiffLayout, status::line_change_spans,
    surface_color, text_color, text_faint_color, text_subtle_color,
};

/// Thin scrollbar: a faint thumb on an invisible track.
pub(super) const QUIET_SCROLLBAR: scrollbar::Set = scrollbar::Set {
    track: " ",
    thumb: "▐",
    begin: " ",
    end: " ",
};

pub(super) fn render_diff(frame: &mut Frame, app: &mut App, layout: DiffLayout) {
    frame.render_widget(
        Block::new().style(Style::new().bg(surface_color())),
        layout.area,
    );
    render_file_header(frame, app, layout.header);
    render_diff_body(frame, app, layout.body);
}

fn render_file_header(frame: &mut Frame, app: &App, area: Rect) {
    let Some(file) = app.files.get(app.selected_file_index) else {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " No file selected",
                Style::new().fg(text_subtle_color()),
            ))),
            area,
        );
        return;
    };

    let status = git::status_label(&file.status);
    let mut left = vec![Span::raw(" ")];
    left.extend(file_path_spans(file));
    if !status.is_empty() {
        left.push(Span::styled(
            format!("  {status}"),
            Style::new().fg(git::status_color(&file.status)),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(left)), area);

    if let Some(totals) = app.file_line_totals(&file.path) {
        let mut right = line_change_spans(totals.additions, totals.deletions);
        right.push(Span::raw(" "));
        frame.render_widget(
            Paragraph::new(Line::from(right)).alignment(Alignment::Right),
            area,
        );
    }
}

/// The file's path with its directory dimmed and file name bold. Renames use
/// git's compact form, sharing the common directory: `src/{old.rs → new.rs}`.
fn file_path_spans(file: &git::FileEntry) -> Vec<Span<'static>> {
    let dim = Style::new().fg(text_subtle_color());
    let faint = Style::new().fg(text_faint_color());
    let name = Style::new().fg(text_color()).add_modifier(Modifier::BOLD);
    let (directory, file_name) = split_directory(&file.path);

    let Some(original) = file.original_path() else {
        return vec![
            Span::styled(directory.to_string(), dim),
            Span::styled(file_name.to_string(), name),
        ];
    };

    let shared = shared_directory_prefix(original, &file.path);
    let original_rest = &original[shared.len()..];
    let (new_directory, _) = split_directory(&file.path[shared.len()..]);
    let mut spans = Vec::new();
    if !shared.is_empty() {
        spans.push(Span::styled(shared.to_string(), dim));
        spans.push(Span::styled("{", faint));
    }
    spans.push(Span::styled(original_rest.to_string(), dim));
    spans.push(Span::styled(" → ", faint));
    spans.push(Span::styled(new_directory.to_string(), dim));
    spans.push(Span::styled(file_name.to_string(), name));
    if !shared.is_empty() {
        spans.push(Span::styled("}", faint));
    }
    spans
}

/// Splits `a/b/c.rs` into `("a/b/", "c.rs")`.
fn split_directory(path: &str) -> (&str, &str) {
    match path.rfind('/') {
        Some(index) => path.split_at(index + 1),
        None => ("", path),
    }
}

/// Longest leading run of whole directories two paths share, such as `src/`
/// for `src/old.rs` and `src/new.rs`.
fn shared_directory_prefix<'a>(left: &'a str, right: &str) -> &'a str {
    let mut end = 0;
    for ((index, a), b) in left.char_indices().zip(right.chars()) {
        if a != b {
            break;
        }
        if a == '/' {
            end = index + 1;
        }
    }
    &left[..end]
}

fn render_diff_body(frame: &mut Frame, app: &mut App, area: Rect) {
    if app.selected_file_is_collapsed_generated() {
        render_generated_placeholder(frame, app, area);
        return;
    }

    let diff_focused = app.active_pane == ActivePane::Diff;
    let mode = app.diff_view_mode;
    let line_wrap = app.diff_line_wrap_mode;
    if app.diff_text_selection.is_none() {
        render_diff_body_windowed(frame, app, area, mode, diff_focused);
        return;
    }

    let Some(viewport) = app.prepare_diff_viewport(mode, area.width as usize, area.height as usize)
    else {
        let paragraph = Paragraph::new(Text::default())
            .style(Style::new().fg(text_color()).bg(surface_color()))
            .scroll((0, 0));
        frame.render_widget(paragraph, area);
        return;
    };
    app.update_diff_viewport(mode, viewport.width, viewport.start, viewport.end);
    let all_lines = augmented_diff_lines(
        app,
        mode,
        line_wrap,
        area.width as usize,
        diff_focused,
        viewport.selected_index,
    );
    let visible_start = viewport.visual_start.min(all_lines.len());
    let visible_end = viewport.visual_end.min(all_lines.len());
    let visible_lines = all_lines[visible_start..visible_end].to_vec();
    let paragraph = Paragraph::new(Text::from(visible_lines))
        .style(Style::new().fg(text_color()).bg(surface_color()))
        .scroll((0, 0));
    frame.render_widget(paragraph, area);

    if viewport.rendered_line_count > area.height as usize {
        let mut scrollbar_state = ScrollbarState::new(viewport.rendered_line_count)
            .position(app.diff_scroll as usize)
            .viewport_content_length(area.height as usize);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .symbols(QUIET_SCROLLBAR)
            .begin_symbol(None)
            .end_symbol(None)
            .thumb_style(Style::new().fg(text_faint_color()));
        frame.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
    }
}

/// `Generated file · +1204 −88 · ⏎ show diff`, in place of a lockfile diff.
fn render_generated_placeholder(frame: &mut Frame, app: &App, area: Rect) {
    if area.height < 2 {
        return;
    }
    let faint = Style::new().fg(text_faint_color());
    let mut spans = vec![Span::styled(
        "   Generated file",
        Style::new().fg(text_subtle_color()),
    )];
    if let Some(totals) = app
        .files
        .get(app.selected_file_index)
        .and_then(|file| app.file_line_totals(&file.path))
    {
        spans.push(Span::styled("  ·  ", faint));
        spans.extend(line_change_spans(totals.additions, totals.deletions));
    }
    spans.extend([
        Span::styled("  ·  ", faint),
        Span::styled(
            "⏎",
            Style::new().fg(text_color()).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" show diff", faint),
    ]);
    let row = Rect::new(area.x, area.y + 1, area.width, 1);
    frame.render_widget(Paragraph::new(Line::from(spans)), row);
}

fn render_diff_body_windowed(
    frame: &mut Frame,
    app: &mut App,
    area: Rect,
    mode: crate::app::DiffViewMode,
    diff_focused: bool,
) {
    if area.width == 0 || area.height == 0 {
        let paragraph = Paragraph::new(Text::default())
            .style(Style::new().fg(text_color()).bg(surface_color()))
            .scroll((0, 0));
        frame.render_widget(paragraph, area);
        return;
    }

    let width = area.width as usize;
    let height = area.height as usize;
    let estimated_line_count = app
        .diff_view
        .estimated_display_line_count(mode, app.diff_line_wrap_mode);
    let max_start = estimated_line_count.saturating_sub(height);
    let selected_index = app
        .selected_diff_line_index
        .min(estimated_line_count.saturating_sub(1));
    if diff_focused && selected_index < app.diff_scroll as usize {
        app.diff_scroll = selected_index.min(u16::MAX as usize) as u16;
    } else if diff_focused {
        let visible_end = (app.diff_scroll as usize).saturating_add(height);
        if selected_index >= visible_end {
            app.diff_scroll = selected_index
                .saturating_add(1)
                .saturating_sub(height)
                .min(u16::MAX as usize) as u16;
        }
    }

    let visual_start = (app.diff_scroll as usize).min(max_start);
    let visual_end = visual_start.saturating_add(height);
    app.diff_scroll = visual_start.min(u16::MAX as usize) as u16;
    app.update_diff_viewport(mode, width, visual_start, visual_end);

    let mut visible_lines = app.diff_view.rendered_lines_window(
        mode,
        width,
        app.diff_line_wrap_mode,
        visual_start,
        visual_end,
    );
    if diff_focused && selected_index >= visual_start && selected_index < visual_end {
        let offset = selected_index - visual_start;
        if let Some(line) = visible_lines.get_mut(offset) {
            *line = highlight_line(line);
        }
    }
    let paragraph = Paragraph::new(Text::from(visible_lines))
        .style(Style::new().fg(text_color()).bg(surface_color()))
        .scroll((0, 0));
    frame.render_widget(paragraph, area);

    if estimated_line_count > height {
        let mut scrollbar_state = ScrollbarState::new(estimated_line_count)
            .position(app.diff_scroll as usize)
            .viewport_content_length(height);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .symbols(QUIET_SCROLLBAR)
            .begin_symbol(None)
            .end_symbol(None)
            .thumb_style(Style::new().fg(text_faint_color()));
        frame.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
    }
}

fn augmented_diff_lines(
    app: &mut App,
    mode: crate::app::DiffViewMode,
    line_wrap: crate::app::DiffLineWrapMode,
    width: usize,
    diff_focused: bool,
    selected_index: usize,
) -> Vec<Line<'static>> {
    let rendered_lines = app
        .diff_view
        .rendered_lines(mode, width, line_wrap)
        .to_vec();
    let mut lines = Vec::with_capacity(rendered_lines.len());

    for (display_index, line) in rendered_lines.into_iter().enumerate() {
        let mut rendered_line = line;
        if let Some(selection) = app.diff_text_selection
            && let Some((start, end)) = app.diff_view.selection_columns(
                mode,
                width,
                line_wrap,
                selection.anchor,
                selection.head,
                display_index,
            )
        {
            rendered_line = highlight_line_range(&rendered_line, start, end);
        }
        if diff_focused && display_index == selected_index {
            rendered_line = highlight_line(&rendered_line);
        }

        lines.push(rendered_line);
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, label: &str) -> git::FileEntry {
        git::FileEntry {
            status: "R ".to_string(),
            path: path.to_string(),
            label: label.to_string(),
            filetype: None,
        }
    }

    fn text(spans: &[Span<'_>]) -> String {
        spans.iter().map(|span| span.content.as_ref()).collect()
    }

    #[test]
    fn header_shows_plain_path_for_unrenamed_files() {
        let spans = file_path_spans(&entry("src/app/mod.rs", "src/app/mod.rs"));

        assert_eq!(text(&spans), "src/app/mod.rs");
        assert_eq!(
            spans.last().map(|span| span.content.as_ref()),
            Some("mod.rs")
        );
    }

    #[test]
    fn header_shows_renames_with_shared_directory_in_braces() {
        let spans = file_path_spans(&entry("src/git/new.rs", "src/git/old.rs -> src/git/new.rs"));
        assert_eq!(text(&spans), "src/git/{old.rs → new.rs}");

        let moved = file_path_spans(&entry("docs/notes.md", "notes.md -> docs/notes.md"));
        assert_eq!(text(&moved), "notes.md → docs/notes.md");
    }

    #[test]
    fn shared_prefix_only_counts_whole_directories() {
        assert_eq!(
            shared_directory_prefix("src/app/a.rs", "src/apple/a.rs"),
            "src/"
        );
        assert_eq!(shared_directory_prefix("a.rs", "b.rs"), "");
    }
}
