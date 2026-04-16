mod diff;
mod layout;
mod modals;
mod sidebar;
pub mod splash;
mod status;

use ratatui::{
    Frame,
    layout::{Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear},
};

use crate::{
    app::{ActivePane, App, DiffViewMode, PreparedDiffViewport},
    git::{DiffSelectionPane, DiffSelectionPoint},
    theme,
};

use self::{
    diff::render_diff, layout::main_layout, modals::render_modals, sidebar::render_sidebar,
    splash::Splash, status::render_notifications,
};

const NOTICE_WIDTH: u16 = 36;

fn palette() -> theme::ThemePalette {
    theme::active_palette()
}

fn background_color() -> Color {
    palette().background
}

fn panel_color() -> Color {
    palette().background_panel
}

fn element_color() -> Color {
    palette().background_element
}

fn border_color() -> Color {
    palette().border
}

fn border_active_color() -> Color {
    palette().border_active
}

fn text_color() -> Color {
    palette().text
}

fn text_muted_color() -> Color {
    palette().text_muted
}

fn primary_color() -> Color {
    palette().primary
}

fn error_color() -> Color {
    palette().error
}

fn warning_color() -> Color {
    palette().warning
}

fn diff_context_color() -> Color {
    palette().diff_context
}

fn add_bg_color() -> Color {
    palette().diff_added_bg
}

fn remove_bg_color() -> Color {
    palette().diff_removed_bg
}

fn selected_list_item_text_color() -> Color {
    palette().selected_list_item_text
}

pub fn render(frame: &mut Frame, app: &mut App) {
    frame.render_widget(Clear, frame.area());
    frame.render_widget(
        Block::new().style(Style::new().bg(background_color())),
        frame.area(),
    );

    if app.show_splash() {
        frame.render_widget(
            Splash::new(
                app.splash_error(),
                Style::new().fg(text_color()),
                Style::new().fg(text_muted_color()),
            ),
            frame.area(),
        );
    } else {
        let [sidebar_area, diff_area] = main_layout(frame.area(), app.sidebar_hidden);
        if !app.sidebar_hidden {
            render_sidebar(frame, app, sidebar_area);
        }
        render_diff(frame, app, diff_area);
    }

    render_modals(frame, app);
    render_notifications(frame, app);
}

pub fn sidebar_file_at(
    app: &App,
    mouse_column: u16,
    mouse_row: u16,
    terminal_width: u16,
    terminal_height: u16,
) -> Option<String> {
    if app.show_splash() || app.sidebar_hidden {
        return None;
    }

    let sidebar_inner = sidebar_inner_area(app, terminal_width, terminal_height);
    let point = Position::new(mouse_column, mouse_row);

    if !sidebar_inner.contains(point) {
        return None;
    }

    let viewport_height = sidebar_inner.height as usize;
    let max_scroll = app.sidebar_items.len().saturating_sub(viewport_height);
    let visible_start = app.sidebar_scroll.min(max_scroll);
    let relative_row = mouse_row.saturating_sub(sidebar_inner.y) as usize;
    let item_index = visible_start.saturating_add(relative_row);
    let item = app.sidebar_items.get(item_index)?;

    match item {
        crate::sidebar::SidebarItem::File { file, .. } => Some(file.path.clone()),
        crate::sidebar::SidebarItem::Header { .. } => None,
    }
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

    let [sidebar_area, diff_area] = main_layout(
        Rect::new(0, 0, terminal_width, terminal_height),
        app.sidebar_hidden,
    );
    let point = Position::new(mouse_column, mouse_row);

    if !app.sidebar_hidden && sidebar_area.contains(point) {
        Some(ActivePane::Sidebar)
    } else if diff_area.contains(point) {
        Some(ActivePane::Diff)
    } else {
        None
    }
}

pub fn diff_gap_click_at(
    app: &mut App,
    mouse_column: u16,
    mouse_row: u16,
    terminal_width: u16,
    terminal_height: u16,
) -> Option<usize> {
    let (body_area, display_index) = diff_body_hit(
        app,
        mouse_column,
        mouse_row,
        terminal_width,
        terminal_height,
    )?;

    app.diff_view.selected_gap_action(
        app.diff_view_mode,
        body_area.width as usize,
        display_index,
    )?;

    Some(display_index)
}

pub fn diff_selection_point_at(
    app: &mut App,
    mouse_column: u16,
    mouse_row: u16,
    terminal_width: u16,
    terminal_height: u16,
) -> Option<DiffSelectionPoint> {
    let (body_area, display_index) = diff_body_hit(
        app,
        mouse_column,
        mouse_row,
        terminal_width,
        terminal_height,
    )?;
    let relative_column = mouse_column.saturating_sub(body_area.x) as usize;
    app.diff_view.selection_point_at(
        app.diff_view_mode,
        body_area.width as usize,
        display_index,
        relative_column,
    )
}

pub fn diff_selection_drag_point_at(
    app: &mut App,
    anchor_pane: DiffSelectionPane,
    mouse_column: u16,
    mouse_row: u16,
    terminal_width: u16,
    terminal_height: u16,
) -> Option<DiffSelectionPoint> {
    let (body_area, display_index) = diff_body_clamped_hit(
        app,
        mouse_column,
        mouse_row,
        terminal_width,
        terminal_height,
    )?;
    let relative_column = mouse_column.saturating_sub(body_area.x) as usize;
    app.diff_view.selection_point_for_pane(
        app.diff_view_mode,
        body_area.width as usize,
        display_index,
        anchor_pane,
        relative_column,
    )
}

pub fn prepare_diff_viewport_for_terminal(
    app: &mut App,
    terminal_width: u16,
    terminal_height: u16,
) -> Option<PreparedDiffViewport> {
    if app.show_splash() {
        return None;
    }

    let [_, diff_area] = main_layout(
        Rect::new(0, 0, terminal_width, terminal_height),
        app.sidebar_hidden,
    );
    let title = app
        .files
        .get(app.selected_file_index)
        .map(|file| file.label.clone())
        .unwrap_or_else(|| "No file selected".to_string());
    let mode_label = app.review_mode_label();
    let block = bordered_panel(
        &title,
        app.active_pane == ActivePane::Diff,
        Some(if mode_label.is_empty() {
            diff_pane_label(app)
        } else {
            format!("{}  {mode_label}", diff_pane_label(app))
        }),
    );
    let inner = block.inner(diff_area);
    let chunks = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            ratatui::layout::Constraint::Min(1),
            ratatui::layout::Constraint::Length(1),
        ])
        .split(inner);
    let body_area = chunks[0];

    app.prepare_diff_viewport(
        app.diff_view_mode,
        body_area.width as usize,
        body_area.height as usize,
    )
}

fn diff_body_hit(
    app: &mut App,
    mouse_column: u16,
    mouse_row: u16,
    terminal_width: u16,
    terminal_height: u16,
) -> Option<(Rect, usize)> {
    let (body_area, viewport) = diff_body_state(app, terminal_width, terminal_height)?;
    let point = Position::new(mouse_column, mouse_row);
    if !body_area.contains(point) {
        return None;
    }

    let display_index = viewport.start + mouse_row.saturating_sub(body_area.y) as usize;
    (display_index < viewport.end).then_some((body_area, display_index))
}

fn diff_body_clamped_hit(
    app: &mut App,
    mouse_column: u16,
    mouse_row: u16,
    terminal_width: u16,
    terminal_height: u16,
) -> Option<(Rect, usize)> {
    let (body_area, viewport) = diff_body_state(app, terminal_width, terminal_height)?;
    if viewport.start >= viewport.end {
        return None;
    }

    let clamped_row = mouse_row.clamp(
        body_area.y,
        body_area
            .y
            .saturating_add(body_area.height.saturating_sub(1)),
    );
    let display_index = (viewport.start + clamped_row.saturating_sub(body_area.y) as usize)
        .min(viewport.end.saturating_sub(1));
    let _ = mouse_column;
    Some((body_area, display_index))
}

fn diff_body_state(
    app: &mut App,
    terminal_width: u16,
    terminal_height: u16,
) -> Option<(Rect, PreparedDiffViewport)> {
    if app.show_splash() {
        return None;
    }

    let [_, diff_area] = main_layout(
        Rect::new(0, 0, terminal_width, terminal_height),
        app.sidebar_hidden,
    );
    let title = app
        .files
        .get(app.selected_file_index)
        .map(|file| file.label.clone())
        .unwrap_or_else(|| "No file selected".to_string());
    let mode_label = app.review_mode_label();
    let block = bordered_panel(
        &title,
        app.active_pane == ActivePane::Diff,
        Some(if mode_label.is_empty() {
            diff_pane_label(app)
        } else {
            format!("{}  {mode_label}", diff_pane_label(app))
        }),
    );
    let inner = block.inner(diff_area);
    let chunks = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            ratatui::layout::Constraint::Min(1),
            ratatui::layout::Constraint::Length(1),
        ])
        .split(inner);
    let body_area = chunks[0];
    let viewport = app.prepare_diff_viewport(
        app.diff_view_mode,
        body_area.width as usize,
        body_area.height as usize,
    )?;
    Some((body_area, viewport))
}

fn sidebar_inner_area(app: &App, terminal_width: u16, terminal_height: u16) -> Rect {
    let [sidebar_area, _] = main_layout(
        Rect::new(0, 0, terminal_width, terminal_height),
        app.sidebar_hidden,
    );
    bordered_panel("Changed Files", false, None).inner(sidebar_area)
}

fn diff_pane_label(app: &App) -> String {
    if app.sidebar_hidden {
        return format!(
            "{}  diff  sidebar hidden",
            diff_mode_label(app.diff_view_mode)
        );
    }

    match app.active_pane {
        ActivePane::Sidebar => format!("{}  sidebar", diff_mode_label(app.diff_view_mode)),
        ActivePane::Diff => format!("{}  diff", diff_mode_label(app.diff_view_mode)),
    }
}

fn bordered_panel(title: &str, active: bool, right_title: Option<String>) -> Block<'static> {
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(if active {
            border_active_color()
        } else {
            border_color()
        }))
        .style(Style::new().bg(panel_color()))
        .title(Line::from(Span::styled(
            format!(" {} ", title),
            Style::new().fg(text_color()).add_modifier(Modifier::BOLD),
        )));

    if let Some(right_title) = right_title {
        block = block.title_bottom(
            Line::from(Span::styled(
                format!(" {} ", right_title),
                Style::new().fg(text_muted_color()),
            ))
            .right_aligned(),
        );
    }

    block
}

fn diff_mode_label(mode: DiffViewMode) -> &'static str {
    match mode {
        DiffViewMode::Unified => "unified",
        DiffViewMode::Split => "split",
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::git::FileEntry;

    fn build_test_app() -> App {
        let mut app = App::new_for_benchmarks(PathBuf::from("/tmp/vigil-ui-tests"));
        app.files.push(FileEntry {
            status: "M ".to_string(),
            path: "src/main.rs".to_string(),
            label: "main.rs".to_string(),
            filetype: Some("rust"),
        });
        app
    }

    #[test]
    fn hovered_pane_uses_full_width_diff_when_sidebar_is_hidden() {
        let mut app = build_test_app();
        app.sidebar_hidden = true;

        assert_eq!(hovered_pane_at(&app, 2, 2, 120, 40), Some(ActivePane::Diff));
    }

    #[test]
    fn sidebar_hit_testing_is_disabled_when_sidebar_is_hidden() {
        let mut app = build_test_app();
        app.sidebar_hidden = true;

        assert_eq!(sidebar_file_at(&app, 2, 2, 120, 40), None);
    }
}

fn highlight_line(line: &Line<'static>) -> Line<'static> {
    Line::from(
        line.spans
            .iter()
            .cloned()
            .map(|span| Span::styled(span.content, span.style.add_modifier(Modifier::REVERSED)))
            .collect::<Vec<_>>(),
    )
    .style(line.style.add_modifier(Modifier::REVERSED))
}

fn highlight_line_range(line: &Line<'static>, start: usize, end: usize) -> Line<'static> {
    if start >= end {
        return line.clone();
    }

    let mut highlighted = Vec::new();
    let mut column = 0usize;

    for span in &line.spans {
        let content = span.content.as_ref();
        let span_width = unicode_width::UnicodeWidthStr::width(content);
        if span_width == 0 {
            highlighted.push(span.clone());
            continue;
        }

        let span_start = column;
        let span_end = column + span_width;
        column = span_end;

        if span_end <= start || span_start >= end {
            highlighted.push(span.clone());
            continue;
        }

        let highlight_start = start.saturating_sub(span_start).min(span_width);
        let highlight_end = end.saturating_sub(span_start).min(span_width);

        let prefix = slice_text_by_width(content, 0, highlight_start);
        if !prefix.is_empty() {
            highlighted.push(Span::styled(prefix, span.style));
        }

        let selected = slice_text_by_width(content, highlight_start, highlight_end);
        if !selected.is_empty() {
            highlighted.push(Span::styled(
                selected,
                span.style.add_modifier(Modifier::REVERSED),
            ));
        }

        let suffix = slice_text_by_width(content, highlight_end, span_width);
        if !suffix.is_empty() {
            highlighted.push(Span::styled(suffix, span.style));
        }
    }

    Line::from(highlighted).style(line.style)
}

fn slice_text_by_width(content: &str, start: usize, end: usize) -> String {
    let mut result = String::new();
    let mut used = 0usize;

    for ch in content.chars() {
        let Some(ch_width) = unicode_width::UnicodeWidthChar::width(ch) else {
            continue;
        };
        if ch_width == 0 {
            continue;
        }

        let next_width = used + ch_width;
        if next_width <= start {
            used = next_width;
            continue;
        }
        if used >= end {
            break;
        }

        result.push(ch);
        used = next_width;
    }

    result
}

pub fn diff_meta_style() -> Style {
    Style::new().fg(diff_context_color())
}

pub fn diff_hunk_style() -> Style {
    Style::new()
        .fg(palette().diff_hunk_header)
        .add_modifier(Modifier::BOLD)
}

pub fn diff_context_style() -> Style {
    Style::new().fg(text_color())
}

pub fn diff_added_style() -> Style {
    Style::new().fg(text_color()).bg(add_bg_color())
}

pub fn diff_removed_style() -> Style {
    Style::new().fg(text_color()).bg(remove_bg_color())
}

pub fn line_number_style() -> Style {
    Style::new().fg(palette().diff_line_number)
}

pub fn added_sign_style() -> Style {
    Style::new()
        .fg(palette().diff_highlight_added)
        .bg(add_bg_color())
        .add_modifier(Modifier::BOLD)
}

pub fn removed_sign_style() -> Style {
    Style::new()
        .fg(palette().diff_highlight_removed)
        .bg(remove_bg_color())
        .add_modifier(Modifier::BOLD)
}

pub fn context_sign_style() -> Style {
    Style::new().fg(diff_context_color())
}

pub fn syntax_style(name: &str, fallback: Style) -> Style {
    let palette = palette();
    let style = match name {
        "comment" | "comment.documentation" => Style::new().fg(palette.syntax_comment),
        "markup.quote" => Style::new().fg(palette.syntax_comment),
        "keyword"
        | "keyword.conditional"
        | "keyword.conditional.ternary"
        | "keyword.coroutine"
        | "keyword.debug"
        | "keyword.directive"
        | "keyword.exception"
        | "keyword.function"
        | "keyword.import"
        | "keyword.modifier"
        | "keyword.operator"
        | "keyword.repeat"
        | "keyword.return"
        | "keyword.type"
        | "conditional"
        | "exception"
        | "repeat" => Style::new()
            .fg(palette.syntax_keyword)
            .add_modifier(Modifier::BOLD),
        "function"
        | "function.builtin"
        | "function.call"
        | "function.method"
        | "function.method.call"
        | "function.method.builtin"
        | "function.macro"
        | "function.special"
        | "constructor"
        | "constructor.builtin"
        | "method"
        | "method.call" => Style::new().fg(palette.syntax_function),
        "label"
        | "module"
        | "module.builtin"
        | "namespace"
        | "variable.parameter"
        | "property"
        | "property.definition"
        | "parameter"
        | "field" => Style::new().fg(palette.syntax_variable),
        "constant" | "constant.builtin" => Style::new().fg(palette.syntax_number),
        "variable" | "variable.member" => Style::new(),
        "variable.builtin" => Style::new().fg(palette.syntax_variable),
        "string"
        | "character"
        | "character.special"
        | "markup.link.url"
        | "markup.raw"
        | "markup.raw.block"
        | "string.escape"
        | "string.regexp"
        | "string.special"
        | "string.special.url"
        | "string.special.key"
        | "string.special.path"
        | "string.special.regex"
        | "string.special.symbol"
        | "string.special.uri" => Style::new().fg(palette.syntax_string),
        "number" | "number.float" | "boolean" => Style::new().fg(palette.syntax_number),
        "type" | "type.builtin" | "type.definition" | "type.qualifier" | "attribute"
        | "attribute.builtin" | "tag.attribute" | "markup.heading" | "markup.heading.1"
        | "markup.heading.2" | "markup.heading.3" | "markup.heading.4" | "markup.heading.5"
        | "markup.heading.6" => Style::new().fg(palette.syntax_type),
        "markup.link" | "markup.link.label" => Style::new().fg(palette.syntax_function),
        "markup.list" | "markup.list.checked" | "markup.list.unchecked" => {
            Style::new().fg(palette.syntax_keyword)
        }
        "operator" | "delimiter" => Style::new().fg(palette.syntax_operator),
        "punctuation"
        | "punctuation.delimiter"
        | "punctuation.bracket"
        | "punctuation.special"
        | "tag.delimiter"
        | "embedded" => Style::new().fg(text_color()),
        "property.builtin" | "tag" | "tag.builtin" | "tag.error" => {
            Style::new().fg(palette.syntax_function)
        }
        _ => fallback,
    };
    fallback.patch(style)
}
