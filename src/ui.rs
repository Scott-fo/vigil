use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, Clear, List, ListItem, Padding, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Wrap,
    },
};

use crate::{
    app::{ActivePane, App, DiffViewMode},
    git::{self, DiffView},
    sidebar::SidebarItem,
};
use ratatui::layout::Position;

const BACKGROUND: Color = Color::Rgb(36, 39, 58);
const PANEL: Color = Color::Rgb(30, 32, 48);
const ELEMENT: Color = Color::Rgb(24, 25, 38);
const BORDER: Color = Color::Rgb(54, 58, 79);
const BORDER_ACTIVE: Color = Color::Rgb(73, 77, 100);
const TEXT: Color = Color::Rgb(202, 211, 245);
const TEXT_MUTED: Color = Color::Rgb(184, 192, 224);
const GREEN: Color = Color::Rgb(166, 218, 149);
const RED: Color = Color::Rgb(237, 135, 150);
const YELLOW: Color = Color::Rgb(238, 212, 159);
const PEACH: Color = Color::Rgb(245, 169, 127);
const BLUE: Color = Color::Rgb(138, 173, 244);
const MAUVE: Color = Color::Rgb(198, 160, 246);
const SKY: Color = Color::Rgb(145, 215, 227);
const OVERLAY2: Color = Color::Rgb(147, 154, 183);
const ADD_BG: Color = Color::Rgb(41, 52, 43);
const REMOVE_BG: Color = Color::Rgb(58, 42, 49);

pub fn render(frame: &mut Frame, app: &mut App) {
    frame.render_widget(Clear, frame.area());
    frame.render_widget(
        Block::new().style(Style::new().bg(BACKGROUND)),
        frame.area(),
    );

    let layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(38), Constraint::Min(40)])
        .split(frame.area());

    render_sidebar(frame, app, layout[0]);
    render_diff(frame, app, layout[1]);
}

pub fn sidebar_file_at(
    app: &App,
    mouse_column: u16,
    mouse_row: u16,
    terminal_width: u16,
    terminal_height: u16,
) -> Option<String> {
    let terminal_area = Rect::new(0, 0, terminal_width, terminal_height);
    let layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(38), Constraint::Min(40)])
        .split(terminal_area);
    let sidebar_area = layout[0];
    let sidebar_inner = bordered_panel("Changed Files", false, None).inner(sidebar_area);
    let point = Position::new(mouse_column, mouse_row);

    if !sidebar_inner.contains(point) {
        return None;
    }

    let relative_row = mouse_row.saturating_sub(sidebar_inner.y) as usize;
    let item_index = app.sidebar_state.offset().saturating_add(relative_row);
    let item = app.sidebar_items.get(item_index)?;

    match item {
        SidebarItem::File { file, .. } => Some(file.path.clone()),
        SidebarItem::Header { .. } => None,
    }
}

fn render_sidebar(frame: &mut Frame, app: &mut App, area: Rect) {
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
                    Span::styled(indent, Style::new().fg(TEXT_MUTED)),
                    Span::styled(arrow, Style::new().fg(BORDER_ACTIVE)),
                    Span::styled(label.clone(), Style::new().fg(TEXT_MUTED)),
                ]))
            }
            SidebarItem::File {
                file, label, depth, ..
            } => {
                let indent = "  ".repeat(*depth);
                let staged = git::is_file_staged(&file.status);
                let row_style = if staged {
                    Style::new().bg(ADD_BG)
                } else {
                    Style::new()
                };
                ListItem::new(Line::from(vec![
                    Span::styled(indent, Style::new().fg(BORDER)),
                    Span::styled(
                        format!("{} ", file.status),
                        Style::new().fg(git::status_color(&file.status)),
                    ),
                    Span::styled(
                        label.clone(),
                        if staged {
                            Style::new().fg(TEXT)
                        } else {
                            Style::new().fg(TEXT_MUTED)
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
                .bg(ELEMENT)
                .fg(TEXT)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("");

    frame.render_stateful_widget(list, inner, &mut app.sidebar_state);

    let sidebar_height = inner.height.saturating_sub(1) as usize;
    let mut scrollbar_state = ScrollbarState::new(app.sidebar_items.len())
        .position(app.sidebar_state.offset())
        .viewport_content_length(sidebar_height);
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .thumb_style(Style::new().fg(BORDER_ACTIVE))
        .track_style(Style::new().fg(BORDER));
    frame.render_stateful_widget(scrollbar, inner, &mut scrollbar_state);
}

fn render_diff(frame: &mut Frame, app: &mut App, area: Rect) {
    let title = app
        .files
        .get(app.selected_file_index)
        .map(|file| file.label.clone())
        .unwrap_or_else(|| "No file selected".to_string());
    let block = bordered_panel(
        &title,
        app.active_pane == ActivePane::Diff,
        Some(match app.active_pane {
            ActivePane::Sidebar => format!("{}  sidebar", diff_mode_label(app.diff_view_mode)),
            ActivePane::Diff => format!("{}  diff", diff_mode_label(app.diff_view_mode)),
        }),
    );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    let diff_view = app.diff_view.clone();
    let scroll = app.diff_scroll;
    render_diff_body(
        frame,
        &mut app.diff_scroll,
        &diff_view,
        scroll,
        app.diff_view_mode,
        chunks[0],
    );
    render_status_line(frame, app, chunks[1]);
}

fn render_diff_body(
    frame: &mut Frame,
    diff_scroll: &mut u16,
    diff_view: &DiffView,
    scroll: u16,
    mode: DiffViewMode,
    area: Rect,
) {
    let rendered_lines = diff_view.render_lines(mode, area.width as usize);
    let max_scroll = rendered_lines
        .len()
        .saturating_sub(1)
        .min(u16::MAX as usize) as u16;
    if *diff_scroll > max_scroll {
        *diff_scroll = max_scroll;
    }

    let paragraph = Paragraph::new(Text::from(rendered_lines.clone()))
        .style(Style::new().fg(TEXT).bg(PANEL))
        .scroll(((*diff_scroll).min(scroll), 0))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);

    let viewport_height = area.height as usize;
    let mut scrollbar_state = ScrollbarState::new(rendered_lines.len())
        .position(*diff_scroll as usize)
        .viewport_content_length(viewport_height);
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .thumb_style(Style::new().fg(BORDER_ACTIVE))
        .track_style(Style::new().fg(BORDER));
    frame.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
}

fn render_status_line(frame: &mut Frame, app: &App, area: Rect) {
    let footer = app
        .status_message
        .clone()
        .unwrap_or_else(|| "q quit  tab switch panes  r refresh  v view".to_string());
    let line = Paragraph::new(Line::from(vec![
        Span::styled("q", Style::new().fg(BLUE).add_modifier(Modifier::BOLD)),
        Span::styled(" quit  ", Style::new().fg(TEXT_MUTED)),
        Span::styled("tab", Style::new().fg(BLUE).add_modifier(Modifier::BOLD)),
        Span::styled(" switch panes  ", Style::new().fg(TEXT_MUTED)),
        Span::styled("r", Style::new().fg(BLUE).add_modifier(Modifier::BOLD)),
        Span::styled(" refresh  ", Style::new().fg(TEXT_MUTED)),
        Span::styled("v", Style::new().fg(BLUE).add_modifier(Modifier::BOLD)),
        Span::styled(
            format!(" {}  {}", diff_mode_label(app.diff_view_mode), footer),
            Style::new().fg(TEXT_MUTED),
        ),
    ]))
    .style(Style::new().bg(PANEL))
    .block(Block::new().padding(Padding::horizontal(1)));
    frame.render_widget(line, area);
}

fn bordered_panel(title: &str, active: bool, right_title: Option<String>) -> Block<'static> {
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(if active { BORDER_ACTIVE } else { BORDER }))
        .style(Style::new().bg(PANEL))
        .title(Line::from(Span::styled(
            format!(" {} ", title),
            Style::new().fg(TEXT).add_modifier(Modifier::BOLD),
        )));

    if let Some(right_title) = right_title {
        block = block.title_bottom(
            Line::from(Span::styled(
                format!(" {} ", right_title),
                Style::new().fg(TEXT_MUTED),
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

pub fn diff_meta_style() -> Style {
    Style::new().fg(OVERLAY2)
}

pub fn diff_hunk_style() -> Style {
    Style::new().fg(PEACH).add_modifier(Modifier::BOLD)
}

pub fn diff_context_style() -> Style {
    Style::new().fg(TEXT)
}

pub fn diff_added_style() -> Style {
    Style::new().fg(TEXT).bg(ADD_BG)
}

pub fn diff_removed_style() -> Style {
    Style::new().fg(TEXT).bg(REMOVE_BG)
}

pub fn line_number_style() -> Style {
    Style::new().fg(BORDER_ACTIVE)
}

pub fn added_sign_style() -> Style {
    Style::new()
        .fg(GREEN)
        .bg(ADD_BG)
        .add_modifier(Modifier::BOLD)
}

pub fn removed_sign_style() -> Style {
    Style::new()
        .fg(RED)
        .bg(REMOVE_BG)
        .add_modifier(Modifier::BOLD)
}

pub fn context_sign_style() -> Style {
    Style::new().fg(OVERLAY2)
}

pub fn syntax_style(name: &str, fallback: Style) -> Style {
    let style = match name {
        "comment" | "comment.documentation" => Style::new().fg(OVERLAY2),
        "keyword" => Style::new().fg(MAUVE).add_modifier(Modifier::BOLD),
        "function" | "function.builtin" | "constructor" | "constructor.builtin" => {
            Style::new().fg(BLUE)
        }
        "variable" | "variable.builtin" | "variable.parameter" | "variable.member" | "property" => {
            Style::new().fg(RED)
        }
        "string" | "string.escape" | "string.special" => Style::new().fg(GREEN),
        "number" | "boolean" => Style::new().fg(PEACH),
        "type" | "type.builtin" | "attribute" => Style::new().fg(YELLOW),
        "operator" => Style::new().fg(SKY),
        "punctuation" | "punctuation.delimiter" | "punctuation.bracket" => Style::new().fg(TEXT),
        "property.builtin" | "tag" => Style::new().fg(BLUE),
        _ => fallback,
    };
    fallback.patch(style)
}
