use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Padding, Paragraph},
};
use unicode_width::UnicodeWidthStr;

use crate::app::{ActivePane, App};

use super::super::{primary_color, text_color, text_faint_color, text_subtle_color};
use super::frame::render_modal_frame;

struct Section {
    title: &'static str,
    keys: Vec<(&'static str, &'static str)>,
}

const COLUMN_GAP: u16 = 4;

pub(super) fn render_help_modal(frame: &mut Frame, app: &App) {
    let [left, right] = sections(app);
    let left_lines = section_lines(&left);
    let right_lines = section_lines(&right);
    let left_width = lines_width(&left_lines);
    let right_width = lines_width(&right_lines);

    // Two columns plus frame border (2), frame inset (1 each side), and
    // extra help padding (1 each side).
    let width = (left_width + right_width) as u16 + COLUMN_GAP + 6;
    let body_height = left_lines.len().max(right_lines.len()) as u16;
    // Body, blank line, footer hint, frame border, and the row under the title.
    let height = body_height + 5;
    let inner = render_modal_frame(frame, width, height, "Keyboard shortcuts");

    let inner = Block::new().padding(Padding::horizontal(1)).inner(inner);
    let [body, _, footer] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(inner);
    let [left_area, _, right_area] = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(left_width as u16),
            Constraint::Length(COLUMN_GAP),
            Constraint::Min(1),
        ])
        .areas(body);

    frame.render_widget(Paragraph::new(Text::from(left_lines)), left_area);
    frame.render_widget(Paragraph::new(Text::from(right_lines)), right_area);

    let pane_hint = match app.active_pane {
        ActivePane::Sidebar if !app.sidebar_hidden => "sidebar focused",
        ActivePane::Diff => "diff focused",
        ActivePane::Sidebar => "sidebar hidden",
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(pane_hint, Style::new().fg(text_faint_color())),
            Span::styled("  ·  ", Style::new().fg(text_faint_color())),
            Span::styled(
                "esc",
                Style::new().fg(text_color()).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" close", Style::new().fg(text_faint_color())),
        ])),
        footer,
    );
}

fn sections(app: &App) -> [Vec<Section>; 2] {
    let mut global = vec![
        ("?", "toggle help"),
        ("tab", "switch sidebar / diff focus"),
        ("Ctrl-B", "toggle left sidebar"),
        ("F2", "open diff stats"),
        ("ff / fg / fx", "find file / search diff / hide suffix"),
        ("v", "toggle unified / split diff"),
        ("z", "toggle line wrap"),
        ("r", "refresh"),
        ("R", "run Codex review"),
        ("E", "edit Codex review context"),
        ("S", "open Codex review summary"),
        ("g", "open commit search"),
        ("b", "open branch compare"),
        ("m", "merge compared branches"),
        ("w", "open worktree picker"),
        ("t", "open theme picker"),
        ("Ctrl-L", "reset compare mode"),
        ("p / P", "pull / push"),
        ("q", "quit"),
    ];
    if app.can_initialize_git_repo() {
        global.insert(7, ("i", "git init when splash is shown"));
    }

    [
        vec![Section {
            title: "Global",
            keys: global,
        }],
        vec![
            Section {
                title: "Navigation",
                keys: vec![
                    ("j / k", "move selection"),
                    ("Ctrl-D / Ctrl-U", "page diff"),
                    ("mouse wheel", "scroll hovered pane"),
                    ("drag in diff", "select code text"),
                ],
            },
            Section {
                title: "Actions",
                keys: vec![
                    ("enter / o / e", "open in editor"),
                    ("enter on gap", "expand hidden lines"),
                    ("click gap row", "↓ top row, ↑ bottom row"),
                    ("Ctrl-C", "copy selection, or quit"),
                    ("space", "stage / unstage file"),
                    ("A", "toggle stage all files"),
                    ("1 / 2 / 3", "resolve conflict: shown sides / both"),
                    ("d", "discard selected file"),
                    ("c", "commit staged changes"),
                ],
            },
        ],
    ]
}

fn section_lines(sections: &[Section]) -> Vec<Line<'static>> {
    let key_width = sections
        .iter()
        .flat_map(|section| section.keys.iter())
        .map(|(key, _)| key.width())
        .max()
        .unwrap_or(0);

    let mut lines = Vec::new();
    for (index, section) in sections.iter().enumerate() {
        if index > 0 {
            lines.push(Line::default());
        }
        lines.push(Line::from(Span::styled(
            section.title.to_uppercase(),
            Style::new()
                .fg(text_faint_color())
                .add_modifier(Modifier::BOLD),
        )));
        for (key, description) in &section.keys {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{key:<key_width$}  "),
                    Style::new()
                        .fg(primary_color())
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    description.to_string(),
                    Style::new().fg(text_subtle_color()),
                ),
            ]));
        }
    }
    lines
}

fn lines_width(lines: &[Line<'_>]) -> usize {
    lines.iter().map(Line::width).max().unwrap_or(0)
}
