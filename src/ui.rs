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
    app::{ActivePane, App, BranchCompareField, DiffViewMode, RemoteSyncDirection, SnackbarVariant},
    git::{self, DiffView},
    sidebar::SidebarItem,
    splash::Splash,
    theme,
};
use ratatui::layout::Position;
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
        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(38), Constraint::Min(40)])
            .split(frame.area());

        render_sidebar(frame, app, layout[0]);
        render_diff(frame, app, layout[1]);
    }

    if app.commit_modal_open {
        render_commit_modal(frame, app);
    }

    if app.discard_target.is_some() {
        render_discard_modal(frame, app);
    }

    if app.theme_modal_open {
        render_theme_modal(frame, app);
    }

    if app.commit_search_modal_open {
        render_commit_search_modal(frame, app);
    }

    if app.branch_compare_modal_open {
        render_branch_compare_modal(frame, app);
    }

    if app.blame_modal_open {
        render_blame_modal(frame, app);
    }

    if app.help_modal_open {
        render_help_modal(frame, app);
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
    if app.show_splash() {
        return None;
    }

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

    let terminal_area = Rect::new(0, 0, terminal_width, terminal_height);
    let layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(38), Constraint::Min(40)])
        .split(terminal_area);
    let diff_area = layout[1];
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
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
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
                    Span::styled(indent, Style::new().fg(text_muted_color())),
                    Span::styled(arrow, Style::new().fg(border_active_color())),
                    Span::styled(label.clone(), Style::new().fg(text_muted_color())),
                ]))
            }
            SidebarItem::File {
                file, label, depth, ..
            } => {
                let indent = "  ".repeat(*depth);
                let staged = git::is_file_staged(&file.status);
                let row_style = if staged {
                    Style::new().bg(add_bg_color())
                } else {
                    Style::new()
                };
                ListItem::new(Line::from(vec![
                    Span::styled(indent, Style::new().fg(border_color())),
                    Span::styled(
                        format!("{} ", file.status),
                        Style::new().fg(git::status_color(&file.status)),
                    ),
                    Span::styled(
                        label.clone(),
                        if staged {
                            Style::new().fg(text_color())
                        } else {
                            Style::new().fg(text_muted_color())
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
                .bg(primary_color())
                .fg(selected_list_item_text_color())
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("");

    frame.render_stateful_widget(list, inner, &mut app.sidebar_state);

    let sidebar_height = inner.height.saturating_sub(1) as usize;
    let mut scrollbar_state = ScrollbarState::new(app.sidebar_items.len())
        .position(app.sidebar_state.offset())
        .viewport_content_length(sidebar_height);
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .thumb_style(Style::new().fg(border_active_color()))
        .track_style(Style::new().fg(border_color()));
    frame.render_stateful_widget(scrollbar, inner, &mut scrollbar_state);
}

fn render_diff(frame: &mut Frame, app: &mut App, area: Rect) {
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
        app.selected_diff_line_index,
        app.active_pane == ActivePane::Diff,
        app.diff_view_mode,
        chunks[0],
    );
    render_status_line(frame, app, chunks[1]);
}

fn render_diff_body(
    frame: &mut Frame,
    diff_scroll: &mut u16,
    diff_view: &mut DiffView,
    selected_diff_line_index: usize,
    diff_focused: bool,
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

    let selected_index = selected_diff_line_index.min(rendered_lines.len().saturating_sub(1));
    if diff_focused {
        if selected_index < *diff_scroll as usize {
            *diff_scroll = selected_index.min(max_scroll as usize) as u16;
        } else {
            let visible_end = (*diff_scroll as usize).saturating_add(viewport_height);
            if viewport_height > 0 && selected_index >= visible_end {
                *diff_scroll = selected_index
                    .saturating_add(1)
                    .saturating_sub(viewport_height)
                    .min(max_scroll as usize) as u16;
            }
        }
    }

    let visible_start = (*diff_scroll as usize).min(max_scroll as usize);
    let visible_end = (visible_start + viewport_height).min(rendered_lines.len());
    let visible_lines = rendered_lines[visible_start..visible_end]
        .iter()
        .enumerate()
        .map(|(offset, line)| {
            let display_index = visible_start + offset;
            if diff_focused && display_index == selected_index {
                highlight_line(line)
            } else {
                line.clone()
            }
        })
        .collect::<Vec<_>>();
    let paragraph = Paragraph::new(Text::from(visible_lines))
        .style(Style::new().fg(text_color()).bg(panel_color()))
        .scroll((0, 0));
    frame.render_widget(paragraph, area);

    if rendered_lines.len() > viewport_height {
        let mut scrollbar_state = ScrollbarState::new(rendered_lines.len())
            .position(*diff_scroll as usize)
            .viewport_content_length(viewport_height);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .thumb_style(Style::new().fg(border_active_color()))
            .track_style(Style::new().fg(border_color()));
        frame.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
    }
}

fn render_status_line(frame: &mut Frame, app: &App, area: Rect) {
    let status = app.status_message.clone().unwrap_or_else(|| {
        format!(
            "{} changed file{}",
            app.files.len(),
            if app.files.len() == 1 { "" } else { "s" }
        )
    });
    let line = Paragraph::new(Line::from(Span::styled(
        status,
        Style::new().fg(text_muted_color()),
    )))
    .style(Style::new().bg(panel_color()))
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
            .border_style(Style::new().fg(primary_color()))
            .style(Style::new().bg(panel_color()));
        let inner = block.inner(area);
        frame.render_widget(Clear, area);
        frame.render_widget(block, area);
        frame.render_widget(
            Paragraph::new(Text::from(Line::from(Span::styled(
                label,
                Style::new().fg(text_muted_color()),
            ))))
            .style(Style::new().bg(panel_color()))
            .block(Block::new().padding(Padding::horizontal(0))),
            inner,
        );
        top = top.saturating_add(4);
    }

    if let Some(notice) = app.snackbar_notice.as_ref() {
        let area = top_right_rect(NOTICE_WIDTH, 3, top, frame.area());
        let border_color = match notice.variant {
            SnackbarVariant::Info => primary_color(),
            SnackbarVariant::Error => error_color(),
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::new().fg(border_color))
            .style(Style::new().bg(panel_color()));
        let inner = block.inner(area);
        frame.render_widget(Clear, area);
        frame.render_widget(block, area);
        frame.render_widget(
            Paragraph::new(Text::from(Line::from(Span::styled(
                notice.message.clone(),
                Style::new().fg(text_color()),
            ))))
            .style(Style::new().bg(panel_color()))
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
        .border_style(Style::new().fg(border_active_color()))
        .style(Style::new().bg(panel_color()))
        .title(Line::from(Span::styled(
            " Commit Staged Changes ",
            Style::new().fg(text_color()).add_modifier(Modifier::BOLD),
        )));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let message_label = Line::from(Span::styled("Message:", Style::new().fg(text_color())));
    let input_line = Line::from(Span::styled(
        if app.commit_message.is_empty() {
            "Enter commit message..."
        } else {
            app.commit_message.as_str()
        },
        if app.commit_message.is_empty() {
            Style::new().fg(text_muted_color()).bg(element_color())
        } else {
            Style::new().fg(text_color()).bg(element_color())
        },
    ));
    let hint_or_error = Line::from(Span::styled(
        app.commit_error
            .as_deref()
            .unwrap_or("Enter commits. Esc closes without committing."),
        if app.commit_error.is_some() {
            Style::new().fg(error_color())
        } else {
            Style::new().fg(text_muted_color())
        },
    ));

    let content = vec![
        message_label,
        Line::default(),
        input_line,
        Line::default(),
        hint_or_error,
    ];
    let paragraph = Paragraph::new(Text::from(content))
        .style(Style::new().bg(panel_color()))
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
        .border_style(Style::new().fg(error_color()))
        .style(Style::new().bg(panel_color()))
        .title(Line::from(Span::styled(
            " Discard File Changes? ",
            Style::new().fg(error_color()).add_modifier(Modifier::BOLD),
        )));
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);

    let text = vec![
        Line::from(Span::styled(
            "This will remove all local changes in:",
            Style::new().fg(text_color()),
        )),
        Line::default(),
        Line::from(Span::styled(
            file.label.clone(),
            Style::new().fg(warning_color()),
        )),
        Line::default(),
        Line::from(Span::styled(
            "Enter confirms discard. Esc cancels.",
            Style::new().fg(text_muted_color()),
        )),
    ];
    let paragraph = Paragraph::new(Text::from(text)).style(Style::new().bg(panel_color()));
    frame.render_widget(paragraph, inner);
}

fn render_help_modal(frame: &mut Frame, app: &App) {
    let area = centered_rect(76, 20, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(border_active_color()))
        .style(Style::new().bg(panel_color()))
        .title(Line::from(Span::styled(
            " Help ",
            Style::new().fg(text_color()).add_modifier(Modifier::BOLD),
        )));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let pane_hint = match app.active_pane {
        ActivePane::Sidebar => "Sidebar focused",
        ActivePane::Diff => "Diff focused",
    };

    let mut lines = vec![
        Line::from(Span::styled(
            "Global",
            Style::new().fg(text_color()).add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled(
                "?  ",
                Style::new().fg(primary_color()).add_modifier(Modifier::BOLD),
            ),
            Span::styled("toggle help", Style::new().fg(text_muted_color())),
        ]),
        Line::from(vec![
            Span::styled(
                "tab  ",
                Style::new().fg(primary_color()).add_modifier(Modifier::BOLD),
            ),
            Span::styled("switch sidebar / diff focus", Style::new().fg(text_muted_color())),
        ]),
        Line::from(vec![
            Span::styled(
                "v  ",
                Style::new().fg(primary_color()).add_modifier(Modifier::BOLD),
            ),
            Span::styled("toggle unified / split diff", Style::new().fg(text_muted_color())),
        ]),
        Line::from(vec![
            Span::styled(
                "r  ",
                Style::new().fg(primary_color()).add_modifier(Modifier::BOLD),
            ),
            Span::styled("refresh", Style::new().fg(text_muted_color())),
        ]),
        Line::from(vec![
            Span::styled(
                "g  ",
                Style::new().fg(primary_color()).add_modifier(Modifier::BOLD),
            ),
            Span::styled("open commit search", Style::new().fg(text_muted_color())),
        ]),
        Line::from(vec![
            Span::styled(
                "b  ",
                Style::new().fg(primary_color()).add_modifier(Modifier::BOLD),
            ),
            Span::styled("open branch compare", Style::new().fg(text_muted_color())),
        ]),
        Line::from(vec![
            Span::styled(
                "t  ",
                Style::new().fg(primary_color()).add_modifier(Modifier::BOLD),
            ),
            Span::styled("open theme picker", Style::new().fg(text_muted_color())),
        ]),
        Line::from(vec![
            Span::styled(
                "Ctrl-L  ",
                Style::new().fg(primary_color()).add_modifier(Modifier::BOLD),
            ),
            Span::styled("reset compare mode", Style::new().fg(text_muted_color())),
        ]),
        Line::from(vec![
            Span::styled(
                "q  ",
                Style::new().fg(primary_color()).add_modifier(Modifier::BOLD),
            ),
            Span::styled("quit", Style::new().fg(text_muted_color())),
        ]),
        Line::default(),
        Line::from(Span::styled(
            "Navigation",
            Style::new().fg(text_color()).add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled(
                "j / k  ",
                Style::new().fg(primary_color()).add_modifier(Modifier::BOLD),
            ),
            Span::styled("move selection", Style::new().fg(text_muted_color())),
        ]),
        Line::from(vec![
            Span::styled(
                "Ctrl-D / Ctrl-U  ",
                Style::new().fg(primary_color()).add_modifier(Modifier::BOLD),
            ),
            Span::styled("page diff", Style::new().fg(text_muted_color())),
        ]),
        Line::from(vec![
            Span::styled(
                "mouse wheel  ",
                Style::new().fg(primary_color()).add_modifier(Modifier::BOLD),
            ),
            Span::styled("scroll diff", Style::new().fg(text_muted_color())),
        ]),
        Line::default(),
        Line::from(Span::styled(
            "Actions",
            Style::new().fg(text_color()).add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled(
                "enter / o / e  ",
                Style::new().fg(primary_color()).add_modifier(Modifier::BOLD),
            ),
            Span::styled("open in editor", Style::new().fg(text_muted_color())),
        ]),
        Line::from(vec![
            Span::styled(
                "enter on gap  ",
                Style::new().fg(primary_color()).add_modifier(Modifier::BOLD),
            ),
            Span::styled("expand selected gap row", Style::new().fg(text_muted_color())),
        ]),
        Line::from(vec![
            Span::styled(
                "click gap rows  ",
                Style::new().fg(primary_color()).add_modifier(Modifier::BOLD),
            ),
            Span::styled("top row expands up, bottom row expands down", Style::new().fg(text_muted_color())),
        ]),
        Line::from(vec![
            Span::styled(
                "space  ",
                Style::new().fg(primary_color()).add_modifier(Modifier::BOLD),
            ),
            Span::styled("stage / unstage selected file", Style::new().fg(text_muted_color())),
        ]),
        Line::from(vec![
            Span::styled(
                "d  ",
                Style::new().fg(primary_color()).add_modifier(Modifier::BOLD),
            ),
            Span::styled("discard selected file", Style::new().fg(text_muted_color())),
        ]),
        Line::from(vec![
            Span::styled(
                "c  ",
                Style::new().fg(primary_color()).add_modifier(Modifier::BOLD),
            ),
            Span::styled("commit staged changes", Style::new().fg(text_muted_color())),
        ]),
        Line::from(vec![
            Span::styled(
                "p / P  ",
                Style::new().fg(primary_color()).add_modifier(Modifier::BOLD),
            ),
            Span::styled("pull / push", Style::new().fg(text_muted_color())),
        ]),
        Line::default(),
        Line::from(Span::styled(
            format!("{pane_hint}. Esc closes help."),
            Style::new().fg(text_muted_color()),
        )),
    ];

    if app.can_initialize_git_repo() {
        lines.insert(
            8,
            Line::from(vec![
                Span::styled(
                    "i  ",
                    Style::new().fg(primary_color()).add_modifier(Modifier::BOLD),
                ),
                Span::styled("git init when splash is shown", Style::new().fg(text_muted_color())),
            ]),
        );
    }

    let paragraph = Paragraph::new(Text::from(lines))
        .style(Style::new().bg(panel_color()))
        .block(Block::new().padding(Padding::horizontal(1)));
    frame.render_widget(paragraph, inner);
}

fn render_blame_modal(frame: &mut Frame, app: &mut App) {
    let area = centered_rect(86, 20, frame.area());
    frame.render_widget(Clear, area);

    let title = app
        .blame_target
        .as_ref()
        .map(|target| format!(" Blame {}:{} ", target.file_path, target.line_number))
        .unwrap_or_else(|| " Blame ".to_string());
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(border_active_color()))
        .style(Style::new().bg(panel_color()))
        .title(Line::from(Span::styled(
            title,
            Style::new().fg(text_color()).add_modifier(Modifier::BOLD),
        )));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Min(6),
            Constraint::Length(2),
        ])
        .split(inner);

    if app.blame_loading {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "Loading blamed commit...",
                Style::new().fg(text_muted_color()),
            )))
            .style(Style::new().bg(panel_color()))
            .block(Block::new().padding(Padding::horizontal(1))),
            chunks[0],
        );
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "Waiting for git blame and commit metadata...",
                Style::new().fg(diff_context_color()),
            )))
            .style(Style::new().bg(panel_color()))
            .block(Block::new().padding(Padding::horizontal(1))),
            chunks[1],
        );
    } else if let Some(error) = app.blame_error.as_ref() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "Unable to load blame details.",
                Style::new().fg(error_color()),
            )))
            .style(Style::new().bg(panel_color()))
            .block(Block::new().padding(Padding::horizontal(1))),
            chunks[0],
        );
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                error.clone(),
                Style::new().fg(text_muted_color()),
            )))
            .style(Style::new().bg(panel_color()))
            .block(Block::new().padding(Padding::horizontal(1))),
            chunks[1],
        );
    } else if let Some(details) = app.blame_details.as_ref() {
        let hash_color = if details.is_uncommitted {
            warning_color()
        } else {
            primary_color()
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                details.subject.clone(),
                Style::new().fg(text_color()).add_modifier(Modifier::BOLD),
            )))
            .style(Style::new().bg(panel_color()))
            .block(Block::new().padding(Padding::horizontal(1))),
            chunks[0],
        );
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(details.short_hash.clone(), Style::new().fg(hash_color)),
                Span::styled(
                    if details.date.is_empty() {
                        format!("  {}", details.author)
                    } else {
                        format!("  {}  {}", details.author, details.date)
                    },
                    Style::new().fg(text_muted_color()),
                ),
            ]))
            .style(Style::new().bg(panel_color()))
            .block(Block::new().padding(Padding::horizontal(1))),
            chunks[1],
        );
        let content_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::new().fg(border_color()))
            .style(Style::new().bg(panel_color()));
        let content_inner = content_block.inner(chunks[2]);
        frame.render_widget(content_block, chunks[2]);

        let body_lines = details
            .description
            .lines()
            .map(|line| {
                Line::from(Span::styled(
                    if line.is_empty() {
                        " ".to_string()
                    } else {
                        line.to_string()
                    },
                    Style::new().fg(text_color()),
                ))
            })
            .collect::<Vec<_>>();
        let viewport_height = content_inner.height as usize;
        let max_scroll = body_lines.len().saturating_sub(viewport_height);
        if app.blame_scroll as usize > max_scroll {
            app.blame_scroll = max_scroll as u16;
        }
        let visible_start = app.blame_scroll as usize;
        let visible_end = (visible_start + viewport_height).min(body_lines.len());
        let visible_lines = if body_lines.is_empty() {
            vec![Line::from(Span::styled(
                "No commit description.",
                Style::new().fg(text_muted_color()),
            ))]
        } else {
            body_lines[visible_start..visible_end].to_vec()
        };
        frame.render_widget(
            Paragraph::new(Text::from(visible_lines))
                .style(Style::new().bg(panel_color()))
                .block(Block::new().padding(Padding::horizontal(1))),
            content_inner,
        );

        if body_lines.len() > viewport_height {
            let mut scrollbar_state = ScrollbarState::new(body_lines.len())
                .position(visible_start)
                .viewport_content_length(viewport_height);
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .thumb_style(Style::new().fg(border_active_color()))
                .track_style(Style::new().fg(border_color()));
            frame.render_stateful_widget(scrollbar, content_inner, &mut scrollbar_state);
        }
    }

    let hint = if app.blame_loading {
        "Esc closes."
    } else if app
        .blame_details
        .as_ref()
        .and_then(|details| details.compare_selection.as_ref())
        .is_some()
    {
        "Enter or o opens commit compare. j/k scroll. Esc closes."
    } else {
        "No commit compare available for this line. j/k scroll. Esc closes."
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            hint,
            Style::new().fg(text_muted_color()),
        )))
        .style(Style::new().bg(panel_color()))
        .block(Block::new().padding(Padding::horizontal(1))),
        chunks[3],
    );
}

fn render_theme_modal(frame: &mut Frame, app: &mut App) {
    let area = centered_rect(76, 22, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(border_active_color()))
        .style(Style::new().bg(panel_color()))
        .title(Line::from(Span::styled(
            " Theme Picker ",
            Style::new().fg(text_color()).add_modifier(Modifier::BOLD),
        )));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(2),
            Constraint::Min(6),
            Constraint::Length(2),
        ])
        .split(inner);

    let query_display = if app.theme_modal_query.is_empty() {
        Span::styled("Search themes...", Style::new().fg(text_muted_color()))
    } else {
        Span::styled(app.theme_modal_query.clone(), Style::new().fg(text_color()))
    };
    let query = Paragraph::new(Line::from(query_display))
        .style(Style::new().bg(element_color()))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::new().fg(border_color()))
                .padding(Padding::horizontal(1)),
        );
    frame.render_widget(query, chunks[0]);

    let mode_line = Paragraph::new(Line::from(vec![
        Span::styled(
            "mode  ",
            Style::new().fg(primary_color()).add_modifier(Modifier::BOLD),
        ),
        Span::styled(app.theme_mode.as_str(), Style::new().fg(text_color())),
        Span::styled("  m toggles light/dark preview", Style::new().fg(text_muted_color())),
    ]))
    .style(Style::new().bg(panel_color()))
    .block(Block::new().padding(Padding::horizontal(1)));
    frame.render_widget(mode_line, chunks[1]);

    let filtered_theme_names = app.filtered_theme_names();
    let list_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(border_color()))
        .style(Style::new().bg(panel_color()));
    let list_inner = list_block.inner(chunks[2]);
    frame.render_widget(list_block, chunks[2]);

    if filtered_theme_names.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "No matching themes.",
                Style::new().fg(text_muted_color()),
            )))
            .style(Style::new().bg(panel_color()))
            .block(Block::new().padding(Padding::horizontal(1))),
            list_inner,
        );
    } else {
        let viewport_height = list_inner.height as usize;
        let selected_index = app
            .theme_modal_selected_index
            .min(filtered_theme_names.len().saturating_sub(1));
        let max_scroll = filtered_theme_names.len().saturating_sub(viewport_height);
        let visible_start = selected_index
            .saturating_sub(viewport_height.saturating_sub(1))
            .min(max_scroll);
        let visible_end = (visible_start + viewport_height).min(filtered_theme_names.len());

        let lines = filtered_theme_names[visible_start..visible_end]
            .iter()
            .enumerate()
            .map(|(offset, theme_name)| {
                let display_index = visible_start + offset;
                let selected = display_index == selected_index;
                let style = if selected {
                    Style::new()
                        .bg(primary_color())
                        .fg(selected_list_item_text_color())
                } else {
                    Style::new().fg(text_color())
                };
                Line::from(Span::styled((*theme_name).to_string(), style)).style(style)
            })
            .collect::<Vec<_>>();

        frame.render_widget(
            Paragraph::new(Text::from(lines))
                .style(Style::new().bg(panel_color()))
                .block(Block::new().padding(Padding::horizontal(1))),
            list_inner,
        );

        if filtered_theme_names.len() > viewport_height {
            let mut scrollbar_state = ScrollbarState::new(filtered_theme_names.len())
                .position(visible_start)
                .viewport_content_length(viewport_height);
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .thumb_style(Style::new().fg(border_active_color()))
                .track_style(Style::new().fg(border_color()));
            frame.render_stateful_widget(scrollbar, list_inner, &mut scrollbar_state);
        }
    }

    let footer = Paragraph::new(Text::from(vec![
        Line::from(Span::styled(
            "Type to filter. j/k move. Enter saves. Esc restores previous theme.",
            Style::new().fg(text_muted_color()),
        )),
        Line::from(Span::styled(
            format!("previewing {} ({})", app.theme_name, app.theme_mode.as_str()),
            Style::new().fg(diff_context_color()),
        )),
    ]))
    .style(Style::new().bg(panel_color()))
    .block(Block::new().padding(Padding::horizontal(1)));
    frame.render_widget(footer, chunks[3]);
}

fn render_commit_search_modal(frame: &mut Frame, app: &mut App) {
    let area = centered_rect(92, 22, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(border_active_color()))
        .style(Style::new().bg(panel_color()))
        .title(Line::from(Span::styled(
            " Commit Search ",
            Style::new().fg(text_color()).add_modifier(Modifier::BOLD),
        )));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(6),
            Constraint::Length(2),
        ])
        .split(inner);

    let query_display = if app.commit_search_query.is_empty() {
        Span::styled("Search by hash or subject...", Style::new().fg(text_muted_color()))
    } else {
        Span::styled(app.commit_search_query.clone(), Style::new().fg(text_color()))
    };
    let query = Paragraph::new(Line::from(query_display))
        .style(Style::new().bg(element_color()))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::new().fg(border_color()))
                .padding(Padding::horizontal(1)),
        );
    frame.render_widget(query, chunks[0]);

    let filtered_indices = app.filtered_commit_search_indices();
    let list_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(border_color()))
        .style(Style::new().bg(panel_color()));
    let list_inner = list_block.inner(chunks[1]);
    frame.render_widget(list_block, chunks[1]);

    if app.commit_search_loading {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "Loading commits...",
                Style::new().fg(text_muted_color()),
            )))
            .style(Style::new().bg(panel_color()))
            .block(Block::new().padding(Padding::horizontal(1))),
            list_inner,
        );
    } else if let Some(error) = app.commit_search_error.as_ref() {
        frame.render_widget(
            Paragraph::new(Text::from(vec![
                Line::from(Span::styled(
                    "Unable to load commits.",
                    Style::new().fg(error_color()),
                )),
                Line::default(),
                Line::from(Span::styled(error.clone(), Style::new().fg(text_muted_color()))),
            ]))
            .style(Style::new().bg(panel_color()))
            .block(Block::new().padding(Padding::horizontal(1))),
            list_inner,
        );
    } else if filtered_indices.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "No matching commits.",
                Style::new().fg(text_muted_color()),
            )))
            .style(Style::new().bg(panel_color()))
            .block(Block::new().padding(Padding::horizontal(1))),
            list_inner,
        );
    } else {
        let viewport_height = list_inner.height as usize;
        let selected_index = app
            .commit_search_selected_index
            .min(filtered_indices.len().saturating_sub(1));
        let max_scroll = filtered_indices.len().saturating_sub(viewport_height);
        let visible_start = selected_index
            .saturating_sub(viewport_height.saturating_sub(1))
            .min(max_scroll);
        let visible_end = (visible_start + viewport_height).min(filtered_indices.len());

        let lines = filtered_indices[visible_start..visible_end]
            .iter()
            .enumerate()
            .map(|(offset, entry_index)| {
                let display_index = visible_start + offset;
                let selected = display_index == selected_index;
                let commit = &app.commit_search_entries[*entry_index];
                let base_style = if selected {
                    Style::new()
                        .bg(primary_color())
                        .fg(selected_list_item_text_color())
                } else {
                    Style::new().fg(text_color())
                };
                let hash_style = if selected {
                    Style::new()
                        .bg(primary_color())
                        .fg(selected_list_item_text_color())
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::new().fg(primary_color()).add_modifier(Modifier::BOLD)
                };

                Line::from(vec![
                    Span::styled(format!("{:<10}", commit.short_hash), hash_style),
                    Span::styled(" ", base_style),
                    Span::styled(commit.subject.clone(), base_style),
                ])
                .style(base_style)
            })
            .collect::<Vec<_>>();

        frame.render_widget(
            Paragraph::new(Text::from(lines))
                .style(Style::new().bg(panel_color()))
                .block(Block::new().padding(Padding::horizontal(1))),
            list_inner,
        );

        if filtered_indices.len() > viewport_height {
            let mut scrollbar_state = ScrollbarState::new(filtered_indices.len())
                .position(visible_start)
                .viewport_content_length(viewport_height);
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .thumb_style(Style::new().fg(border_active_color()))
                .track_style(Style::new().fg(border_color()));
            frame.render_stateful_widget(scrollbar, list_inner, &mut scrollbar_state);
        }
    }

    let selected_label = filtered_indices
        .get(app.commit_search_selected_index)
        .and_then(|index| app.commit_search_entries.get(*index))
        .map(|commit| format!("selected {}", commit.short_hash))
        .unwrap_or_else(|| "no selection".to_string());
    let footer = Paragraph::new(Text::from(vec![
        Line::from(Span::styled(
            "Type to filter. j/k move. Enter selects. Esc closes.",
            Style::new().fg(text_muted_color()),
        )),
        Line::from(Span::styled(selected_label, Style::new().fg(diff_context_color()))),
    ]))
    .style(Style::new().bg(panel_color()))
    .block(Block::new().padding(Padding::horizontal(1)));
    frame.render_widget(footer, chunks[2]);
}

fn render_branch_compare_modal(frame: &mut Frame, app: &mut App) {
    let area = centered_rect(84, 23, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(border_active_color()))
        .style(Style::new().bg(panel_color()))
        .title(Line::from(Span::styled(
            " Branch Compare ",
            Style::new().fg(text_color()).add_modifier(Modifier::BOLD),
        )));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(6),
            Constraint::Length(2),
        ])
        .split(inner);

    let source_active = app.branch_compare_active_field == BranchCompareField::Source;
    let source_display = if app.branch_compare_source_query.is_empty() {
        app.branch_compare_source_ref
            .clone()
            .unwrap_or_else(|| "Source ref".to_string())
    } else {
        app.branch_compare_source_query.clone()
    };
    let source = Paragraph::new(Line::from(Span::styled(
        source_display,
        if app.branch_compare_source_query.is_empty() {
            Style::new().fg(text_muted_color())
        } else {
            Style::new().fg(text_color())
        },
    )))
    .style(Style::new().bg(element_color()))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::new().fg(if source_active {
                primary_color()
            } else {
                border_color()
            }))
            .title(" Source ")
            .padding(Padding::horizontal(1)),
    );
    frame.render_widget(source, chunks[0]);

    let destination_active = app.branch_compare_active_field == BranchCompareField::Destination;
    let destination_display = if app.branch_compare_destination_query.is_empty() {
        app.branch_compare_destination_ref
            .clone()
            .unwrap_or_else(|| "Destination ref".to_string())
    } else {
        app.branch_compare_destination_query.clone()
    };
    let destination = Paragraph::new(Line::from(Span::styled(
        destination_display,
        if app.branch_compare_destination_query.is_empty() {
            Style::new().fg(text_muted_color())
        } else {
            Style::new().fg(text_color())
        },
    )))
    .style(Style::new().bg(element_color()))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::new().fg(if destination_active {
                primary_color()
            } else {
                border_color()
            }))
            .title(" Destination ")
            .padding(Padding::horizontal(1)),
    );
    frame.render_widget(destination, chunks[1]);

    let filtered_refs = app.filtered_branch_compare_refs();
    let list_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(border_color()))
        .style(Style::new().bg(panel_color()));
    let list_inner = list_block.inner(chunks[2]);
    frame.render_widget(list_block, chunks[2]);

    if app.branch_compare_loading {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "Loading refs...",
                Style::new().fg(text_muted_color()),
            )))
            .style(Style::new().bg(panel_color()))
            .block(Block::new().padding(Padding::horizontal(1))),
            list_inner,
        );
    } else if let Some(error) = app.branch_compare_error.as_ref() {
        frame.render_widget(
            Paragraph::new(Text::from(vec![
                Line::from(Span::styled(
                    "Unable to load refs.",
                    Style::new().fg(error_color()),
                )),
                Line::default(),
                Line::from(Span::styled(error.clone(), Style::new().fg(text_muted_color()))),
            ]))
            .style(Style::new().bg(panel_color()))
            .block(Block::new().padding(Padding::horizontal(1))),
            list_inner,
        );
    } else if filtered_refs.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "No matching refs.",
                Style::new().fg(text_muted_color()),
            )))
            .style(Style::new().bg(panel_color()))
            .block(Block::new().padding(Padding::horizontal(1))),
            list_inner,
        );
    } else {
        let viewport_height = list_inner.height as usize;
        let selected_index = match app.branch_compare_active_field {
            BranchCompareField::Source => app.branch_compare_selected_source_index,
            BranchCompareField::Destination => app.branch_compare_selected_destination_index,
        }
        .min(filtered_refs.len().saturating_sub(1));
        let max_scroll = filtered_refs.len().saturating_sub(viewport_height);
        let visible_start = selected_index
            .saturating_sub(viewport_height.saturating_sub(1))
            .min(max_scroll);
        let visible_end = (visible_start + viewport_height).min(filtered_refs.len());

        let lines = filtered_refs[visible_start..visible_end]
            .iter()
            .enumerate()
            .map(|(offset, ref_name)| {
                let display_index = visible_start + offset;
                let selected = display_index == selected_index;
                let style = if selected {
                    Style::new()
                        .bg(primary_color())
                        .fg(selected_list_item_text_color())
                } else {
                    Style::new().fg(text_color())
                };
                Line::from(Span::styled(ref_name.clone(), style)).style(style)
            })
            .collect::<Vec<_>>();

        frame.render_widget(
            Paragraph::new(Text::from(lines))
                .style(Style::new().bg(panel_color()))
                .block(Block::new().padding(Padding::horizontal(1))),
            list_inner,
        );

        if filtered_refs.len() > viewport_height {
            let mut scrollbar_state = ScrollbarState::new(filtered_refs.len())
                .position(visible_start)
                .viewport_content_length(viewport_height);
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .thumb_style(Style::new().fg(border_active_color()))
                .track_style(Style::new().fg(border_color()));
            frame.render_stateful_widget(scrollbar, list_inner, &mut scrollbar_state);
        }
    }

    let footer = Paragraph::new(Text::from(vec![
        Line::from(Span::styled(
            "Tab switches field. Type to filter. j/k move. Enter compares. Esc closes.",
            Style::new().fg(text_muted_color()),
        )),
        Line::from(Span::styled(
            format!(
                "source: {}  destination: {}",
                app.branch_compare_source_ref.as_deref().unwrap_or("none"),
                app.branch_compare_destination_ref.as_deref().unwrap_or("none")
            ),
            Style::new().fg(diff_context_color()),
        )),
    ]))
    .style(Style::new().bg(panel_color()))
    .block(Block::new().padding(Padding::horizontal(1)));
    frame.render_widget(footer, chunks[3]);
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
        .border_style(Style::new().fg(if active { border_active_color() } else { border_color() }))
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
        "keyword" => Style::new()
            .fg(palette.syntax_keyword)
            .add_modifier(Modifier::BOLD),
        "function" | "function.builtin" | "constructor" | "constructor.builtin" => {
            Style::new().fg(palette.syntax_function)
        }
        "variable" | "variable.builtin" | "variable.parameter" | "variable.member" | "property" => {
            Style::new().fg(palette.syntax_variable)
        }
        "string" | "string.escape" | "string.special" => Style::new().fg(palette.syntax_string),
        "number" | "boolean" => Style::new().fg(palette.syntax_number),
        "type" | "type.builtin" | "attribute" => Style::new().fg(palette.syntax_type),
        "operator" => Style::new().fg(palette.syntax_operator),
        "punctuation" | "punctuation.delimiter" | "punctuation.bracket" => Style::new().fg(text_color()),
        "property.builtin" | "tag" => Style::new().fg(palette.syntax_function),
        _ => fallback,
    };
    fallback.patch(style)
}
