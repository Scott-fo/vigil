use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, Clear, List, ListItem, Padding, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState,
    },
};

use crate::{
    app::{ActivePane, App, DiffViewMode, RemoteSyncDirection, SnackbarVariant},
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
const NOTICE_WIDTH: u16 = 36;

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

    if app.commit_modal_open {
        render_commit_modal(frame, app);
    }

    if app.discard_target.is_some() {
        render_discard_modal(frame, app);
    }

    render_notifications(frame, app);
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

    render_diff_body(
        frame,
        &mut app.diff_scroll,
        &mut app.diff_view,
        app.diff_view_mode,
        chunks[0],
    );
    render_status_line(frame, app, chunks[1]);
}

fn render_diff_body(
    frame: &mut Frame,
    diff_scroll: &mut u16,
    diff_view: &mut DiffView,
    mode: DiffViewMode,
    area: Rect,
) {
    let rendered_lines = diff_view.rendered_lines(mode, area.width as usize);
    let viewport_height = area.height as usize;
    let max_scroll = rendered_lines
        .len()
        .saturating_sub(viewport_height)
        .min(u16::MAX as usize) as u16;
    if *diff_scroll > max_scroll {
        *diff_scroll = max_scroll;
    }

    let visible_start = (*diff_scroll as usize).min(max_scroll as usize);
    let visible_end = (visible_start + viewport_height).min(rendered_lines.len());
    let paragraph = Paragraph::new(Text::from(rendered_lines[visible_start..visible_end].to_vec()))
        .style(Style::new().fg(TEXT).bg(PANEL))
        .scroll((0, 0));
    frame.render_widget(paragraph, area);

    if rendered_lines.len() > viewport_height {
        let mut scrollbar_state = ScrollbarState::new(rendered_lines.len())
            .position(*diff_scroll as usize)
            .viewport_content_length(viewport_height);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .thumb_style(Style::new().fg(BORDER_ACTIVE))
            .track_style(Style::new().fg(BORDER));
        frame.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
    }
}

fn render_status_line(frame: &mut Frame, app: &App, area: Rect) {
    let footer = app
        .status_message
        .clone()
        .unwrap_or_else(|| {
            "q quit  tab switch panes  enter open  space stage  d discard  c commit  p pull  P push  r refresh  v view"
                .to_string()
        });
    let line = Paragraph::new(Line::from(vec![
        Span::styled("q", Style::new().fg(BLUE).add_modifier(Modifier::BOLD)),
        Span::styled(" quit  ", Style::new().fg(TEXT_MUTED)),
        Span::styled("tab", Style::new().fg(BLUE).add_modifier(Modifier::BOLD)),
        Span::styled(" switch panes  ", Style::new().fg(TEXT_MUTED)),
        Span::styled("space", Style::new().fg(BLUE).add_modifier(Modifier::BOLD)),
        Span::styled(" stage  ", Style::new().fg(TEXT_MUTED)),
        Span::styled("enter", Style::new().fg(BLUE).add_modifier(Modifier::BOLD)),
        Span::styled(" open  ", Style::new().fg(TEXT_MUTED)),
        Span::styled("d", Style::new().fg(BLUE).add_modifier(Modifier::BOLD)),
        Span::styled(" discard  ", Style::new().fg(TEXT_MUTED)),
        Span::styled("c", Style::new().fg(BLUE).add_modifier(Modifier::BOLD)),
        Span::styled(" commit  ", Style::new().fg(TEXT_MUTED)),
        Span::styled("p", Style::new().fg(BLUE).add_modifier(Modifier::BOLD)),
        Span::styled(" pull  ", Style::new().fg(TEXT_MUTED)),
        Span::styled("P", Style::new().fg(BLUE).add_modifier(Modifier::BOLD)),
        Span::styled(" push  ", Style::new().fg(TEXT_MUTED)),
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

fn render_notifications(frame: &mut Frame, app: &App) {
    let mut top = frame.area().y + 1;

    if let Some(direction) = app.remote_sync {
        let label = match direction {
            RemoteSyncDirection::Pull => "Pulling from remote...",
            RemoteSyncDirection::Push => "Pushing to remote...",
        };
        let area = top_right_rect(NOTICE_WIDTH, 3, top, frame.area());
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::new().fg(BLUE))
            .style(Style::new().bg(PANEL));
        let inner = block.inner(area);
        frame.render_widget(Clear, area);
        frame.render_widget(block, area);
        frame.render_widget(
            Paragraph::new(Text::from(Line::from(Span::styled(
                label,
                Style::new().fg(TEXT_MUTED),
            ))))
            .style(Style::new().bg(PANEL))
            .block(Block::new().padding(Padding::horizontal(0))),
            inner,
        );
        top = top.saturating_add(4);
    }

    if let Some(notice) = app.snackbar_notice.as_ref() {
        let area = top_right_rect(NOTICE_WIDTH, 3, top, frame.area());
        let border_color = match notice.variant {
            SnackbarVariant::Info => BLUE,
            SnackbarVariant::Error => RED,
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::new().fg(border_color))
            .style(Style::new().bg(PANEL));
        let inner = block.inner(area);
        frame.render_widget(Clear, area);
        frame.render_widget(block, area);
        frame.render_widget(
            Paragraph::new(Text::from(Line::from(Span::styled(
                notice.message.clone(),
                Style::new().fg(TEXT),
            ))))
            .style(Style::new().bg(PANEL))
            .block(Block::new().padding(Padding::horizontal(0))),
            inner,
        );
    }
}

fn render_commit_modal(frame: &mut Frame, app: &App) {
    let area = centered_rect(72, 9, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(BORDER_ACTIVE))
        .style(Style::new().bg(PANEL))
        .title(Line::from(Span::styled(
            " Commit Staged Changes ",
            Style::new().fg(TEXT).add_modifier(Modifier::BOLD),
        )));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let message_label = Line::from(Span::styled(
        "Message:",
        Style::new().fg(TEXT),
    ));
    let input_line = Line::from(Span::styled(
        if app.commit_message.is_empty() {
            "Enter commit message..."
        } else {
            app.commit_message.as_str()
        },
        if app.commit_message.is_empty() {
            Style::new().fg(TEXT_MUTED).bg(ELEMENT)
        } else {
            Style::new().fg(TEXT).bg(ELEMENT)
        },
    ));
    let hint_or_error = Line::from(Span::styled(
        app.commit_error
            .as_deref()
            .unwrap_or("Enter commits. Esc closes without committing."),
        if app.commit_error.is_some() {
            Style::new().fg(RED)
        } else {
            Style::new().fg(TEXT_MUTED)
        },
    ));

    let content = vec![message_label, Line::default(), input_line, Line::default(), hint_or_error];
    let paragraph = Paragraph::new(Text::from(content))
        .style(Style::new().bg(PANEL))
        .block(Block::new().padding(Padding::horizontal(1)));
    frame.render_widget(paragraph, inner);
}

fn render_discard_modal(frame: &mut Frame, app: &App) {
    let Some(file) = app.discard_target.as_ref() else {
        return;
    };

    let area = centered_rect(72, 9, frame.area());
    frame.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(RED))
        .style(Style::new().bg(PANEL))
        .title(Line::from(Span::styled(
            " Discard File Changes? ",
            Style::new().fg(RED).add_modifier(Modifier::BOLD),
        )));
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);

    let text = vec![
        Line::from(Span::styled(
            "This will remove all local changes in:",
            Style::new().fg(TEXT),
        )),
        Line::default(),
        Line::from(Span::styled(file.label.clone(), Style::new().fg(YELLOW))),
        Line::default(),
        Line::from(Span::styled(
            "Enter confirms discard. Esc cancels.",
            Style::new().fg(TEXT_MUTED),
        )),
    ];
    let paragraph = Paragraph::new(Text::from(text)).style(Style::new().bg(PANEL));
    frame.render_widget(paragraph, inner);
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let popup_width = width.min(area.width.saturating_sub(2)).max(1);
    let popup_height = height.min(area.height.saturating_sub(2)).max(1);
    Rect::new(
        area.x + area.width.saturating_sub(popup_width) / 2,
        area.y + area.height.saturating_sub(popup_height) / 2,
        popup_width,
        popup_height,
    )
}

fn top_right_rect(width: u16, height: u16, top: u16, area: Rect) -> Rect {
    let popup_width = width.min(area.width.saturating_sub(2)).max(1);
    let popup_height = height.min(area.height.saturating_sub(2)).max(1);
    Rect::new(
        area.x + area.width.saturating_sub(popup_width).saturating_sub(1),
        top.min(area.y + area.height.saturating_sub(popup_height)),
        popup_width,
        popup_height,
    )
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
