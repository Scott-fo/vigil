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
    app::{ActivePane, App, DiffViewMode},
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
        let [sidebar_area, diff_area] = main_layout(frame.area());
        render_sidebar(frame, app, sidebar_area);
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
    if app.show_splash() {
        return None;
    }

    let [sidebar_area, _] = main_layout(Rect::new(0, 0, terminal_width, terminal_height));
    let sidebar_inner = bordered_panel("Changed Files", false, None).inner(sidebar_area);
    let point = Position::new(mouse_column, mouse_row);

    if !sidebar_inner.contains(point) {
        return None;
    }

    let relative_row = mouse_row.saturating_sub(sidebar_inner.y) as usize;
    let item_index = app.sidebar_state.offset().saturating_add(relative_row);
    let item = app.sidebar_items.get(item_index)?;

    match item {
        crate::sidebar::SidebarItem::File { file, .. } => Some(file.path.clone()),
        crate::sidebar::SidebarItem::Header { .. } => None,
    }
}

pub fn diff_gap_click_at(
    app: &mut App,
    mouse_column: u16,
    mouse_row: u16,
    terminal_width: u16,
    terminal_height: u16,
) -> Option<usize> {
    if app.show_splash() {
        return None;
    }

    let [_, diff_area] = main_layout(Rect::new(0, 0, terminal_width, terminal_height));
    let title = app
        .files
        .get(app.selected_file_index)
        .map(|file| file.label.clone())
        .unwrap_or_else(|| "No file selected".to_string());
    let mode_label = app.review_mode_label();
    let right_title = match app.active_pane {
        ActivePane::Sidebar => format!("{}  sidebar", diff_mode_label(app.diff_view_mode)),
        ActivePane::Diff => format!("{}  diff", diff_mode_label(app.diff_view_mode)),
    };
    let block = bordered_panel(
        &title,
        app.active_pane == ActivePane::Diff,
        Some(if mode_label.is_empty() {
            right_title
        } else {
            format!("{right_title}  {mode_label}")
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
    let point = Position::new(mouse_column, mouse_row);
    if !body_area.contains(point) {
        return None;
    }

    let rendered_lines = app
        .diff_view
        .rendered_lines(app.diff_view_mode, body_area.width as usize);
    let viewport_height = body_area.height as usize;
    let max_scroll = rendered_lines
        .len()
        .saturating_sub(viewport_height)
        .min(u16::MAX as usize) as u16;
    let visible_start = (app.diff_scroll as usize).min(max_scroll as usize);
    let display_index = visible_start + mouse_row.saturating_sub(body_area.y) as usize;
    if display_index >= rendered_lines.len() {
        return None;
    }

    app.diff_view
        .selected_gap_action(app.diff_view_mode, display_index)?;

    Some(display_index)
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
        | "method.call" => {
            Style::new().fg(palette.syntax_function)
        }
        "variable"
        | "variable.builtin"
        | "variable.parameter"
        | "variable.member"
        | "property"
        | "property.definition"
        | "parameter"
        | "label"
        | "module"
        | "module.builtin"
        | "namespace"
        | "constant"
        | "constant.builtin" => {
            Style::new().fg(palette.syntax_variable)
        }
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
        "type"
        | "type.builtin"
        | "type.definition"
        | "type.qualifier"
        | "attribute"
        | "attribute.builtin"
        | "tag.attribute"
        | "markup.heading"
        | "markup.heading.1"
        | "markup.heading.2"
        | "markup.heading.3"
        | "markup.heading.4"
        | "markup.heading.5"
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
        | "embedded" => {
            Style::new().fg(text_color())
        }
        "property.builtin" | "tag" | "tag.builtin" | "tag.error" => {
            Style::new().fg(palette.syntax_function)
        }
        _ => fallback,
    };
    fallback.patch(style)
}
