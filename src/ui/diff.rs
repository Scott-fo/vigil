use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    symbols::scrollbar,
    text::{Line, Span, Text},
    widgets::{Block, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{
    app::{ActivePane, App},
    git,
    review::{ReviewDisplayComment, ReviewSeverity},
};

use super::{
    error_color, highlight_line, highlight_line_range, layout::DiffLayout,
    status::line_change_spans, surface_color, text_color, text_faint_color, text_subtle_color,
    warning_color,
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

    let directory = file
        .path
        .strip_suffix(file.label.as_str())
        .unwrap_or_default()
        .to_string();
    let status = git::status_label(&file.status);
    let mut left = vec![
        Span::raw(" "),
        Span::styled(directory, Style::new().fg(text_subtle_color())),
        Span::styled(
            file.label.clone(),
            Style::new().fg(text_color()).add_modifier(Modifier::BOLD),
        ),
    ];
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

fn render_diff_body(frame: &mut Frame, app: &mut App, area: Rect) {
    if app.selected_file_is_collapsed_generated() {
        render_generated_placeholder(frame, app, area);
        return;
    }

    let diff_focused = app.active_pane == ActivePane::Diff;
    let mode = app.diff_view_mode;
    let line_wrap = app.diff_line_wrap_mode;
    if app.diff_text_selection.is_none() && app.review_report.is_none() {
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
        let comments = app.review_comments_for_display_index(display_index, width);
        lines.extend(
            comments
                .iter()
                .flat_map(|comment| render_review_comment(comment, width)),
        );
    }

    lines
}

fn render_review_comment(comment: &ReviewDisplayComment, width: usize) -> Vec<Line<'static>> {
    let style = match comment.severity {
        ReviewSeverity::Critical | ReviewSeverity::High => Style::new().fg(error_color()),
        ReviewSeverity::Medium => Style::new().fg(warning_color()),
        ReviewSeverity::Low | ReviewSeverity::Info => Style::new().fg(text_subtle_color()),
    };
    let heading_prefix = "  ╭─ ";
    let body_prefix = "  │  ";
    let end_prefix = "  ╰─";
    let text_width = width
        .saturating_sub(UnicodeWidthStr::width(body_prefix))
        .saturating_sub(2)
        .max(16);
    let mut lines = Vec::new();

    let heading = format!("{} · {}", severity_label(comment.severity), comment.title);
    for (index, segment) in wrap_text(&heading, text_width).into_iter().enumerate() {
        if index == 0 {
            lines.push(Line::from(vec![
                Span::styled(
                    heading_prefix.to_string(),
                    Style::new().fg(text_subtle_color()),
                ),
                Span::styled(segment, style.add_modifier(Modifier::BOLD)),
            ]));
        } else {
            lines.push(comment_line(body_prefix, segment, style));
        }
    }

    for segment in wrap_text(&comment.body, text_width) {
        lines.push(comment_line(
            body_prefix,
            segment,
            Style::new().fg(text_subtle_color()),
        ));
    }
    lines.push(Line::from(Span::styled(
        end_prefix.to_string(),
        Style::new().fg(text_subtle_color()),
    )));

    lines
}

fn comment_line(prefix: &str, text: String, style: Style) -> Line<'static> {
    Line::from(vec![
        Span::styled(prefix.to_string(), Style::new().fg(text_subtle_color())),
        Span::styled(text, style),
    ])
}

fn severity_label(severity: ReviewSeverity) -> &'static str {
    match severity {
        ReviewSeverity::Critical => "critical",
        ReviewSeverity::High => "high",
        ReviewSeverity::Medium => "medium",
        ReviewSeverity::Low => "low",
        ReviewSeverity::Info => "info",
    }
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0;

    for word in text.split_whitespace() {
        let word_width = UnicodeWidthStr::width(word);
        if word_width > width {
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
                current_width = 0;
            }
            push_wrapped_word(word, width, &mut lines);
            continue;
        }

        if current.is_empty() {
            current.push_str(word);
            current_width = word_width;
        } else if current_width + 1 + word_width <= width {
            current.push(' ');
            current.push_str(word);
            current_width += 1 + word_width;
        } else {
            lines.push(std::mem::take(&mut current));
            current.push_str(word);
            current_width = word_width;
        }
    }

    if !current.is_empty() {
        lines.push(current);
    }

    if lines.is_empty() {
        lines.push(String::new());
    }

    lines
}

fn push_wrapped_word(word: &str, width: usize, lines: &mut Vec<String>) {
    let mut current = String::new();
    let mut current_width = 0;

    for ch in word.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if current_width > 0 && current_width + ch_width > width {
            lines.push(std::mem::take(&mut current));
            current_width = 0;
        }
        current.push(ch);
        current_width += ch_width;
    }

    if !current.is_empty() {
        lines.push(current);
    }
}
