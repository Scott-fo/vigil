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
use super::list::render_quiet_scrollbar;

struct Section {
    title: &'static str,
    keys: Vec<(&'static str, &'static str)>,
}

const COLUMN_GAP: u16 = 3;
/// Frame border (2), the row under the title, a blank row, and the footer.
const HELP_CHROME_ROWS: u16 = 5;

/// Two columns of shortcuts that scroll together. On a terminal too short
/// for every row, `j`/`k` and page keys scroll; the footer says where the
/// view is.
pub(super) fn render_help_modal(frame: &mut Frame, app: &mut App) {
    let [left, right] = sections(app);
    let left_lines = section_lines(&left);
    let right_lines = section_lines(&right);
    let left_width = lines_width(&left_lines);
    let right_width = lines_width(&right_lines);
    let content_rows = left_lines.len().max(right_lines.len());

    // Two columns plus frame border (2), frame inset (1 each side),
    // extra help padding (1 each side), and a column for the scrollbar.
    let width = (left_width + right_width) as u16 + COLUMN_GAP + 7;
    let max_height = frame
        .area()
        .height
        .saturating_sub(2)
        .max(HELP_CHROME_ROWS + 1);
    let height = (content_rows as u16 + HELP_CHROME_ROWS).min(max_height);
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
    let visible_rows = body.height as usize;
    let max_scroll = content_rows.saturating_sub(visible_rows);
    let scroll = app.help_scroll.min(max_scroll);
    app.help_scroll = scroll;

    let [left_area, _, right_area, scrollbar] = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(left_width as u16),
            Constraint::Length(COLUMN_GAP),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .areas(body);
    let window = |lines: Vec<Line<'static>>| {
        Text::from(
            lines
                .into_iter()
                .skip(scroll)
                .take(visible_rows)
                .collect::<Vec<_>>(),
        )
    };
    frame.render_widget(Paragraph::new(window(left_lines)), left_area);
    frame.render_widget(Paragraph::new(window(right_lines)), right_area);
    render_quiet_scrollbar(frame, scrollbar, content_rows, scroll);

    let pane_hint = match app.active_pane {
        ActivePane::Sidebar if !app.sidebar_hidden => "sidebar focused",
        ActivePane::Diff => "diff focused",
        ActivePane::Sidebar => "sidebar hidden",
    };
    let key = Style::new().fg(text_color()).add_modifier(Modifier::BOLD);
    let faint = Style::new().fg(text_faint_color());
    let mut spans = vec![Span::styled(pane_hint, faint)];
    if max_scroll > 0 {
        spans.extend([
            Span::styled("  ·  ", faint),
            Span::styled("j/k", key),
            Span::styled(
                format!(
                    " scroll {}–{} of {content_rows}",
                    scroll + 1,
                    (scroll + visible_rows).min(content_rows)
                ),
                faint,
            ),
        ]);
    }
    spans.extend([
        Span::styled("  ·  ", faint),
        Span::styled("esc", key),
        Span::styled(" close", faint),
    ]);
    frame.render_widget(Paragraph::new(Line::from(spans)), footer);
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
        ("W", "ignore / show whitespace changes"),
        ("r", "refresh"),
        ("g", "open commit search"),
        ("b", "open branch compare"),
        ("m", "merge compared branches"),
        ("B", "branches: switch, create, sync"),
        ("w", "open worktree picker"),
        ("t", "open theme picker"),
        ("Ctrl-L", "back to working tree"),
        ("p / P", "pull / push current branch"),
        ("q", "quit"),
    ];
    if app.can_initialize_git_repo() {
        global.insert(7, ("i", "git init when splash is shown"));
    }

    [
        vec![
            Section {
                title: "Global",
                keys: global,
            },
            Section {
                title: "Pull requests",
                keys: vec![
                    ("L", "pull request list"),
                    ("O", "review this branch's pull request"),
                    ("r", "refetch the pull request under review"),
                    ("y", "copy the pull request's branch name"),
                    ("K", "check out the pull request's branch"),
                    ("] on overview", "step into the first file"),
                    ("Ctrl-d/u", "scroll the overview"),
                    ("Ctrl-L", "leave pull request review"),
                    ("esc", "back to the list it came from"),
                    ("list: tab, 1-3", "switch tab"),
                    ("list: /#17", "filter, or open any number"),
                    ("list: ⏎ / esc", "open / back"),
                ],
            },
        ],
        vec![
            Section {
                title: "Navigation",
                keys: vec![
                    ("j / k", "move selection"),
                    ("] / [", "next / previous change"),
                    ("Ctrl-D / Ctrl-U", "page diff"),
                    ("mouse wheel", "scroll hovered pane"),
                    ("drag in diff", "select and copy code text"),
                ],
            },
            Section {
                title: "Actions",
                keys: vec![
                    ("enter / o / e", "open in editor"),
                    ("enter on gap", "expand hidden lines"),
                    ("enter on lockfile", "show collapsed generated diff"),
                    ("click gap row", "↓ top row, ↑ bottom row"),
                    ("Ctrl-C", "copy selection, or quit"),
                    ("space", "stage / unstage file"),
                    ("x / X", "toggle viewed / viewed + next"),
                    ("A", "toggle stage all files"),
                    ("1 / 2 / 3", "resolve conflict: shown sides / both"),
                    ("d", "discard selected file"),
                    ("c", "commit staged changes"),
                ],
            },
            Section {
                title: "Reviewing a pull request",
                keys: vec![
                    ("c", "comment on line or selection"),
                    ("c on overview / C", "comment on the conversation"),
                    ("R", "reply to the thread on this line"),
                    ("T", "resolve / reopen that thread"),
                    ("D", "drafts: edit or delete"),
                    ("S", "submit review"),
                    ("M", "merge, or merge when ready"),
                    ("A", "close / reopen, draft / ready"),
                    ("composer: ctrl-s", "save the draft, or post"),
                    ("composer: ⏎", "new line"),
                    ("composer: ctrl-e", "write in $EDITOR"),
                    ("composer: ctrl-g", "insert a suggestion block"),
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
