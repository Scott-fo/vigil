use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Padding, Paragraph},
};

use crate::app::{ActivePane, App};

use super::super::layout::centered_rect;
use super::super::{border_active_color, panel_color, primary_color, text_color, text_muted_color};

pub(super) fn render_help_modal(frame: &mut Frame, app: &App) {
    let area = centered_rect(76, 22, frame.area());
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
        ActivePane::Sidebar if !app.sidebar_hidden => "Sidebar focused",
        ActivePane::Diff => "Diff focused",
        ActivePane::Sidebar => "Sidebar hidden",
    };

    let mut lines = vec![
        Line::from(Span::styled(
            "Global",
            Style::new().fg(text_color()).add_modifier(Modifier::BOLD),
        )),
        key_line("?", "toggle help"),
        key_line("tab", "switch sidebar / diff focus"),
        key_line("Ctrl-B", "toggle left sidebar"),
        key_line("p", "open file search"),
        key_line("v", "toggle unified / split diff"),
        key_line("r", "refresh"),
        key_line("g", "open commit search"),
        key_line("b", "open branch compare"),
        key_line("t", "open theme picker"),
        key_line("Ctrl-L", "reset compare mode"),
        key_line("Ctrl-P / P", "pull / push"),
        key_line("q", "quit"),
        Line::default(),
        Line::from(Span::styled(
            "Navigation",
            Style::new().fg(text_color()).add_modifier(Modifier::BOLD),
        )),
        key_line("j / k", "move selection"),
        key_line("Ctrl-D / Ctrl-U", "page diff"),
        key_line("mouse wheel", "scroll hovered pane"),
        key_line("drag in diff", "select code text"),
        Line::default(),
        Line::from(Span::styled(
            "Actions",
            Style::new().fg(text_color()).add_modifier(Modifier::BOLD),
        )),
        key_line("enter / o / e", "open in editor"),
        key_line("enter on gap", "expand selected gap row"),
        key_line(
            "click gap rows",
            "top row expands up, bottom row expands down",
        ),
        key_line(
            "Ctrl-C",
            "copy selected diff text or quit if nothing is selected",
        ),
        key_line("space", "stage / unstage selected file"),
        key_line("A", "toggle stage all files"),
        key_line("d", "discard selected file"),
        key_line("c", "commit staged changes"),
        Line::default(),
        Line::from(Span::styled(
            format!("{pane_hint}. Esc closes help."),
            Style::new().fg(text_muted_color()),
        )),
    ];

    if app.can_initialize_git_repo() {
        lines.insert(8, key_line("i", "git init when splash is shown"));
    }

    let paragraph = Paragraph::new(Text::from(lines))
        .style(Style::new().bg(panel_color()))
        .block(Block::new().padding(Padding::horizontal(1)));
    frame.render_widget(paragraph, inner);
}

fn key_line(key: &str, description: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{key}  "),
            Style::new()
                .fg(primary_color())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(description.to_string(), Style::new().fg(text_muted_color())),
    ])
}
